//! Procedurally generated textures.
//!
//! These are runtime-generated images (a 1×1 white pixel, an antialiased
//! filled disc, a hollow ring, …) that the engine and games use as
//! drawable atoms — typically tinted at draw time. Kept in their own
//! module rather than mixed into `renderer.rs` so the renderer file
//! stays focused on the render-pipeline / batching / screenshot path,
//! and so the pure CPU pixel rasterizers can be unit-tested without
//! standing up a `Renderer`.
//!
//! Layering:
//! - `build_*_pixels`  — pure CPU; produce row-major RGBA8 buffers.
//! - `Renderer::create_*_texture` (in this file's `impl Renderer` block)
//!   — glue the CPU buffers to a GPU upload via `create_rgba_texture`.
//! - `create_rgba_texture` — the single point of contact with wgpu for
//!   "give me a sampled texture from this RGBA buffer". Reused by every
//!   procedural-texture helper here, and exposed publicly so games can
//!   roll their own (gradients, particle masks, …) without re-deriving
//!   the bind-group plumbing.

use crate::{Renderer, Texture};

impl Renderer {
    /// Create a 1×1 white pixel texture for UI rectangle rendering.
    ///
    /// This is a runtime-generated internal texture, not a user asset.
    /// The caller is responsible for registering the returned `Texture`
    /// into `AssetManager`.
    pub fn create_white_pixel_texture(&self) -> Texture {
        let white_pixel_data: [u8; 4] = [255, 255, 255, 255];
        self.create_rgba_texture("vibe_white_pixel", 1, 1, &white_pixel_data)
    }

    /// Create a high-resolution filled-circle texture (square, RGBA, with
    /// alpha-based antialiasing on the perimeter). The disc fills the
    /// inscribed circle of the texture; pixels outside have alpha 0 so
    /// the result composites cleanly over arbitrary backgrounds when
    /// blitted at any scale.
    ///
    /// `size` is the side length in pixels. 256 is a good default — large
    /// enough that downscaling to typical sprite sizes (10–80 px) stays
    /// visually round, small enough that the upload cost is trivial
    /// (256 KB).
    pub fn create_filled_circle_texture(&self, label: &str, size: u32) -> Texture {
        let pixels = build_filled_circle_pixels(size);
        self.create_rgba_texture(label, size, size, &pixels)
    }

    /// Create a high-resolution ring (hollow circle outline) texture.
    /// Same conventions as [`Renderer::create_filled_circle_texture`]:
    /// square, RGBA, alpha-AA on both inner and outer perimeters.
    ///
    /// `thickness_ratio` is the ring width expressed as a fraction of
    /// the texture's radius (so `0.1` means the ring band occupies 10 %
    /// of the radius — i.e. on a 256 px texture, ~12.8 px wide). Values
    /// in the 0.05–0.20 range read well at typical sprite sizes; very
    /// thin rings (<0.03) start to alias when downscaled because the
    /// band collapses to under 1 px.
    pub fn create_ring_texture(&self, label: &str, size: u32, thickness_ratio: f32) -> Texture {
        let pixels = build_ring_pixels(size, thickness_ratio);
        self.create_rgba_texture(label, size, size, &pixels)
    }

    /// Upload an arbitrary RGBA8 image as a sampled texture. Useful for
    /// games that want to generate textures procedurally at runtime
    /// (e.g. circles, gradients, particle masks) without going through
    /// the PNG encode/decode round-trip that `Texture::from_bytes`
    /// requires.
    ///
    /// `pixels` must be exactly `width * height * 4` bytes in row-major
    /// RGBA order (top-left first). Sampling uses Nearest filtering and
    /// ClampToEdge addressing — same as the loaded-from-PNG path — so
    /// the produced texture composites identically with the rest of
    /// the engine's sprite batches.
    pub fn create_rgba_texture(
        &self,
        label: &str,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Texture {
        assert_eq!(
            pixels.len(),
            (width as usize) * (height as usize) * 4,
            "create_rgba_texture: pixel buffer size mismatch"
        );

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: Some(&format!("{label}_bind_group")),
        });

        Texture {
            texture,
            view,
            bind_group,
            width,
            height,
        }
    }
}

// ── Procedural circle / ring rasterizers ────────────────────────────
//
// These are kept as free functions (not methods on Renderer) because
// they're pure CPU work — no GPU handles needed — and because they're
// also useful from unit tests, which can't easily spin up a Renderer.
// The Renderer's `create_filled_circle_texture` / `create_ring_texture`
// just glue these into a GPU upload.
//
// Antialiasing strategy: classic distance-from-center thresholding with
// a 1.5 px smoothstep band around each edge. At 256² resolution the AA
// band is ~0.6 % of the diameter; smaller textures use the same 1.5 px
// width because alias width in *pixels* is what matters for the eye, not
// alias width in % of the disc. Both pixel buffers are returned in
// row-major RGBA8 (top-left origin, 4 bytes per pixel) so they slot
// straight into `create_rgba_texture` / `wgpu::write_texture`.

fn smoothstep_alpha(distance: f32, edge: f32, half_band: f32) -> f32 {
    // Returns 1.0 well inside `edge`, 0.0 well outside, and a smooth
    // ramp across a `2 * half_band` wide region centered on `edge`.
    // Formula matches GLSL's `1.0 - smoothstep(edge - h, edge + h, d)`.
    let t = ((distance - (edge - half_band)) / (2.0 * half_band)).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    1.0 - s
}

/// Rasterize a filled circle into an RGBA8 buffer. The disc is white
/// (so it tints cleanly) and uses alpha for soft edges; pixels strictly
/// outside the inscribed circle are fully transparent.
pub fn build_filled_circle_pixels(size: u32) -> Vec<u8> {
    let s = size as f32;
    let center = s * 0.5;
    // Leave a 1 px margin so the AA ramp at the disc edge isn't clipped
    // by the texture boundary.
    let radius = center - 1.0;
    let half_band = 0.75; // -> 1.5 px AA band

    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            // Sample the *pixel center* (offset by 0.5) for symmetric AA
            // around true geometric edges.
            let dx = (x as f32 + 0.5) - center;
            let dy = (y as f32 + 0.5) - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = smoothstep_alpha(dist, radius, half_band);
            let a8 = (alpha * 255.0).round() as u8;
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = 255;
            pixels[i + 1] = 255;
            pixels[i + 2] = 255;
            pixels[i + 3] = a8;
        }
    }
    pixels
}

/// Rasterize a ring outline into an RGBA8 buffer. `thickness_ratio` is
/// the band width expressed as a fraction of the texture radius — see
/// [`Renderer::create_ring_texture`] for usage notes.
pub fn build_ring_pixels(size: u32, thickness_ratio: f32) -> Vec<u8> {
    let s = size as f32;
    let center = s * 0.5;
    let outer = center - 1.0;
    // Clamp thickness so the inner radius can't go negative or eat the
    // entire disc. Anything thicker than the radius itself would be a
    // filled circle; anything thinner than 1 px aliases to nothing.
    let band_px = (outer * thickness_ratio).clamp(1.0, outer - 1.0);
    let inner = outer - band_px;
    let half_band = 0.75; // matches the filled-circle AA width

    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) - center;
            let dy = (y as f32 + 0.5) - center;
            let dist = (dx * dx + dy * dy).sqrt();
            // Inside the outer circle AND outside the inner circle. We
            // multiply the two masks instead of building a piecewise
            // function so the AA at the thin-ring crossover (where
            // outer and inner edges overlap) degrades gracefully into
            // a translucent band rather than a hard step.
            let outer_mask = smoothstep_alpha(dist, outer, half_band);
            let inner_mask = 1.0 - smoothstep_alpha(dist, inner, half_band);
            let alpha = outer_mask * inner_mask;
            let a8 = (alpha * 255.0).round() as u8;
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = 255;
            pixels[i + 1] = 255;
            pixels[i + 2] = 255;
            pixels[i + 3] = a8;
        }
    }
    pixels
}

#[cfg(test)]
mod circle_pixel_tests {
    use super::*;

    #[test]
    fn filled_circle_center_is_opaque_corner_is_transparent() {
        let p = build_filled_circle_pixels(64);
        // Center pixel (32, 32) — well inside.
        let center_alpha = p[((32 * 64 + 32) * 4 + 3) as usize];
        assert!(center_alpha >= 250, "center alpha = {center_alpha}");
        // Corner (0, 0) — well outside the inscribed circle.
        let corner_alpha = p[3];
        assert_eq!(corner_alpha, 0, "corner must be fully transparent");
    }

    #[test]
    fn ring_center_is_transparent_band_is_opaque() {
        // 64² texture, 40 % thick ring. Geometry:
        //   outer = 31 px (32 - 1 px AA margin)
        //   band  = 31 * 0.4 = 12.4 px wide
        //   inner = 31 - 12.4 = 18.6 px
        // So pixels at radius < 18.6 are in the hole, 18.6..31 are
        // opaque band, > 31 are outside.
        let p = build_ring_pixels(64, 0.4);
        let alpha = |x: u32, y: u32| -> u8 { p[((y * 64 + x) * 4 + 3) as usize] };
        // Geometric center (radius 0) — well inside the hole.
        assert_eq!(alpha(32, 32), 0, "ring center must be hollow");
        // (32, 7) is 25 px above center → squarely inside the
        // 18.6..31 band, so should be opaque.
        let band_alpha = alpha(32, 7);
        assert!(band_alpha >= 250, "band alpha = {band_alpha}");
        // (32, 12) is 20 px above center → just outside the inner
        // edge (18.6) but still well inside the outer (31), so still
        // opaque (allowing for the 1.5 px AA ramp).
        let band_alpha2 = alpha(32, 12);
        assert!(
            band_alpha2 >= 250,
            "band alpha (just inside inner edge) = {band_alpha2}"
        );
        // Far corner — outside the outer edge.
        assert_eq!(alpha(0, 0), 0, "corner must be transparent");
    }
}
