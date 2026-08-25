use std::sync::Arc;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::texture::Texture;

// Tracy zone marker: a scoped span guard under `profiling` (native only),
// otherwise a no-op. `render` compiles for wasm too, where `tracy_client` is
// unavailable, so the active arms also exclude wasm. With `profiling-callstacks`
// each zone also captures a Rust call stack (extra per-frame overhead), which is
// the only way to see per-frame Rust stacks on macOS (no sampling backend there).
#[cfg(all(
    feature = "profiling-callstacks",
    feature = "profiling",
    not(target_arch = "wasm32")
))]
macro_rules! zone {
    ($name:expr) => {
        let _tracy_zone = tracy_client::span!($name, 32);
    };
}
#[cfg(all(
    not(feature = "profiling-callstacks"),
    feature = "profiling",
    not(target_arch = "wasm32")
))]
macro_rules! zone {
    ($name:expr) => {
        let _tracy_zone = tracy_client::span!($name);
    };
}
#[cfg(not(all(feature = "profiling", not(target_arch = "wasm32"))))]
macro_rules! zone {
    ($name:expr) => {};
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpriteVertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
    color: [f32; 4],
}

/// Draw command queued for the current frame.
///
/// Implements [`Default`] so call sites can use `..Default::default()` and stay
/// source-compatible when fields are added.
#[derive(Clone, Copy, Default)]
pub struct DrawCommand {
    pub texture_id: crate::TextureId,
    pub src_rect: [f32; 4], // x, y, w, h in UV coordinates (0..1)
    pub dst_rect: [f32; 4], // x, y, w, h in virtual pixels
    pub color: [f32; 4],
    pub flip_x: bool,
    pub flip_y: bool,
    /// Painter's-algorithm layer. Lower layers draw first (behind).
    ///
    /// Within a single layer, **submission order is preserved** — the sort is
    /// stable and keys on `layer` alone. That is deliberate: sorting by texture
    /// inside a layer would batch better but would silently reorder overlapping
    /// sprites, making "which one is on top" depend on which texture they happen
    /// to use. Group your draws by texture yourself if you want fewer batches.
    pub layer: i32,
    /// Optional scissor rectangle in **virtual** pixels (x, y, w, h).
    ///
    /// Draws are clipped to this rect. `None` means no clipping. A change of clip
    /// forces a batch break, so use it for the handful of things that need it
    /// (partially-grown sprites: vines, laser beams, rising platforms) rather
    /// than per-sprite.
    pub clip: Option<[f32; 4]>,
    /// Rotation in radians, clockwise, about the destination rect's centre.
    ///
    /// Costs four extra multiply-adds per vertex, and only when non-zero — the
    /// axis-aligned fast path is preserved for the overwhelming majority of
    /// sprites. Rotation does **not** affect batching.
    pub rotation: f32,
}

/// The 2D renderer. Batches sprite draws and submits to GPU each frame.
///
/// `device`, `queue`, and `texture_bind_group_layout` are wrapped in [`Arc`]
/// so that subsystems like [`crate::Font`]'s lazy glyph atlas can hold cheap
/// references and upload pixel data outside the render path.
pub struct Renderer {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub pipeline: wgpu::RenderPipeline,
    pub texture_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    pub projection_bind_group: wgpu::BindGroup,
    pub projection_buffer: wgpu::Buffer,
    draw_commands: Vec<DrawCommand>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    pub virtual_width: f32,
    pub virtual_height: f32,
    /// Latches so the per-frame overflow warning is emitted once, not 60×/second.
    sprite_overflow_warned: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pending_screenshot: Option<std::path::PathBuf>,
}

const MAX_SPRITES: usize = 10_000;
const VERTICES_PER_SPRITE: usize = 4;
const INDICES_PER_SPRITE: usize = 6;

impl Renderer {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        surface_config: wgpu::SurfaceConfiguration,
        virtual_width: f32,
        virtual_height: f32,
    ) -> Self {
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Projection uniform (orthographic matrix)
        let projection = orthographic_projection(virtual_width, virtual_height);
        let projection_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("projection_buffer"),
            contents: bytemuck::cast_slice(&projection),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let projection_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("projection_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let projection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &projection_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: projection_buffer.as_entire_binding(),
            }],
            label: Some("projection_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite_pipeline_layout"),
            bind_group_layouts: &[&projection_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite_vertex_buffer"),
            size: (MAX_SPRITES * VERTICES_PER_SPRITE * std::mem::size_of::<SpriteVertex>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Pre-generate index buffer (0,1,2, 2,3,0 pattern for each quad)
        let mut indices = Vec::with_capacity(MAX_SPRITES * INDICES_PER_SPRITE);
        for i in 0..MAX_SPRITES as u16 {
            let base = i * 4;
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
        }
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite_index_buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            device,
            queue,
            surface,
            surface_config,
            pipeline,
            texture_bind_group_layout: Arc::new(texture_bind_group_layout),
            projection_bind_group,
            projection_buffer,
            draw_commands: Vec::with_capacity(256),
            sprite_overflow_warned: false,
            vertex_buffer,
            index_buffer,
            virtual_width,
            virtual_height,
            #[cfg(not(target_arch = "wasm32"))]
            pending_screenshot: None,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    /// Returns the maximum supported texture dimension for this device.
    pub fn max_texture_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Queue a sprite draw command for this frame.
    pub fn draw_sprite(&mut self, cmd: DrawCommand) {
        self.draw_commands.push(cmd);
    }

    /// Enforce the sprite cap and apply layer ordering.
    ///
    /// Must run before vertices are built and before `execute_draw_commands`, and
    /// both must see the *same* list — the batch ranges computed there index into
    /// the vertex buffer built here.
    fn prepare_draw_commands(&mut self) {
        enforce_sprite_cap(&mut self.draw_commands, &mut self.sprite_overflow_warned);
        sort_by_layer(&mut self.draw_commands);
    }

    /// Request a screenshot to be captured on the next render.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_screenshot(&mut self, path: impl Into<std::path::PathBuf>) {
        self.pending_screenshot = Some(path.into());
    }

    /// Render all queued draw commands and present to screen.
    pub fn render(&mut self, clear_color: [f32; 4], textures: &[&Texture]) -> Result<()> {
        zone!("gpu_submit");
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        self.prepare_draw_commands();

        // Build vertex data from draw commands
        let mut vertices: Vec<SpriteVertex> = Vec::with_capacity(self.draw_commands.len() * 4);

        for cmd in &self.draw_commands {
            let [su, sv, sw, sh] = cmd.src_rect;

            let (tu_left, tu_right) = if cmd.flip_x {
                (su + sw, su)
            } else {
                (su, su + sw)
            };
            let (tv_top, tv_bottom) = if cmd.flip_y {
                (sv + sh, sv)
            } else {
                (sv, sv + sh)
            };

            let corners = quad_corners(cmd.dst_rect, cmd.rotation);
            let uvs = [
                [tu_left, tv_top],
                [tu_right, tv_top],
                [tu_right, tv_bottom],
                [tu_left, tv_bottom],
            ];
            for (position, tex_coords) in corners.into_iter().zip(uvs) {
                vertices.push(SpriteVertex {
                    position,
                    tex_coords,
                    color: cmd.color,
                });
            }
        }

        if !vertices.is_empty() {
            self.queue
                .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }

        // Main render pass to surface
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprite_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Main pass: the target is the swapchain surface.
            self.execute_draw_commands(
                &mut render_pass,
                textures,
                self.surface_config.width,
                self.surface_config.height,
            );
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Screenshot capture (after present, draw commands still available)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(screenshot_path) = self.pending_screenshot.take() {
            self.capture_screenshot(clear_color, textures, &screenshot_path);
        }

        self.draw_commands.clear();

        Ok(())
    }

    /// Execute batched draw commands on a render pass.
    /// `target_width`/`target_height` are the dimensions of the attachment being
    /// drawn into, in physical pixels.
    ///
    /// They are **not** always the surface size: the screenshot paths render into
    /// an offscreen texture sized to the virtual resolution. wgpu validates
    /// scissor rects against the attachment, so using the surface size there is a
    /// hard validation error, not a cosmetic mismatch.
    fn execute_draw_commands<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        textures: &'a [&'a Texture],
        target_width: u32,
        target_height: u32,
    ) {
        if self.draw_commands.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.projection_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for batch in compute_batches(&self.draw_commands) {
            match batch.clip {
                Some(clip) => {
                    let (x, y, w, h) = scissor_rect(
                        clip,
                        target_width,
                        target_height,
                        self.virtual_width,
                        self.virtual_height,
                    );
                    // wgpu rejects a zero-extent scissor *and* one that reaches past
                    // the attachment, so a clip that clamps to nothing is skipped
                    // outright — padding it to 1px puts the origin on the far edge and
                    // fails validation, which crashed the whole frame the first time a
                    // clipped sprite scrolled off the right of the screen.
                    if w == 0 || h == 0 {
                        continue;
                    }
                    render_pass.set_scissor_rect(x, y, w, h);
                }
                None => render_pass.set_scissor_rect(0, 0, target_width, target_height),
            }
            if batch.texture_index < textures.len() {
                render_pass.set_bind_group(1, &textures[batch.texture_index].bind_group, &[]);
            }
            let end = batch.start + batch.count;
            render_pass.draw_indexed((batch.start as u32 * 6)..(end as u32 * 6), 0, 0..1);
        }
    }

    /// Capture the current frame to a PNG file.
    #[cfg(not(target_arch = "wasm32"))]
    fn capture_screenshot(
        &self,
        clear_color: [f32; 4],
        textures: &[&Texture],
        path: &std::path::Path,
    ) {
        let vw = self.virtual_width as u32;
        let vh = self.virtual_height as u32;
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = vw * bytes_per_pixel;
        let align = 256u32;
        // Round up `unpadded_bytes_per_row` to the wgpu-required 256B alignment.
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let offscreen_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot_texture"),
            size: wgpu::Extent3d {
                width: vw,
                height: vh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let offscreen_view = offscreen_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let buffer_size = (padded_bytes_per_row * vh) as wgpu::BufferAddress;
        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screenshot_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Screenshot pass: the target is a virtual-resolution texture.
            self.execute_draw_commands(&mut render_pass, textures, vw, vh);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &offscreen_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(vh),
                },
            },
            wgpu::Extent3d {
                width: vw,
                height: vh,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);

        match rx.recv() {
            Ok(Ok(())) => {
                let data = buffer_slice.get_mapped_range();
                let is_bgra = matches!(
                    self.surface_config.format,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                );

                let mut pixels = Vec::with_capacity((vw * vh * 4) as usize);
                for row in 0..vh {
                    let offset = (row * padded_bytes_per_row) as usize;
                    let row_data = &data[offset..offset + unpadded_bytes_per_row as usize];
                    if is_bgra {
                        for pixel in row_data.as_chunks::<4>().0 {
                            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                        }
                    } else {
                        pixels.extend_from_slice(row_data);
                    }
                }

                drop(data);
                staging_buffer.unmap();

                if let Some(img) = image::RgbaImage::from_raw(vw, vh, pixels) {
                    if let Err(e) = img.save(path) {
                        tracing::error!("Failed to save screenshot: {}", e);
                    } else {
                        tracing::info!("Screenshot saved to {:?}", path);
                    }
                }
            }
            _ => {
                tracing::error!("Failed to map screenshot staging buffer");
            }
        }
    }

    /// Start an async screenshot capture (web/wasm32).
    /// Returns a `ScreenshotCapture` that can be resolved asynchronously to PNG bytes.
    #[cfg(target_arch = "wasm32")]
    pub fn start_screenshot_capture(
        &self,
        clear_color: [f32; 4],
        textures: &[&Texture],
    ) -> ScreenshotCapture {
        let vw = self.virtual_width as u32;
        let vh = self.virtual_height as u32;
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = vw * bytes_per_pixel;
        let align = 256u32;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let offscreen_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot_texture"),
            size: wgpu::Extent3d {
                width: vw,
                height: vh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let offscreen_view = offscreen_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let buffer_size = (padded_bytes_per_row * vh) as wgpu::BufferAddress;
        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screenshot_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Screenshot pass: the target is a virtual-resolution texture.
            self.execute_draw_commands(&mut render_pass, textures, vw, vh);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &offscreen_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(vh),
                },
            },
            wgpu::Extent3d {
                width: vw,
                height: vh,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        let is_bgra = matches!(
            self.surface_config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );

        ScreenshotCapture {
            buffer: staging_buffer,
            width: vw,
            height: vh,
            padded_bytes_per_row,
            unpadded_bytes_per_row,
            is_bgra,
            device: Arc::clone(&self.device),
        }
    }
}

/// Pending screenshot capture that can be resolved asynchronously (wasm32).
#[cfg(target_arch = "wasm32")]
pub struct ScreenshotCapture {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    unpadded_bytes_per_row: u32,
    is_bgra: bool,
    device: Arc<wgpu::Device>,
}

#[cfg(target_arch = "wasm32")]
impl ScreenshotCapture {
    /// Resolve the screenshot capture asynchronously.
    /// Returns PNG-encoded bytes on success.
    pub async fn resolve(self) -> Option<Vec<u8>> {
        let buffer_slice = self.buffer.slice(..);

        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        // On wasm, poll(Wait) triggers the browser event loop to process the map callback.
        self.device.poll(wgpu::Maintain::Wait);

        // The callback should have fired after poll. If not, yield once.
        let map_result = match rx.try_recv() {
            Ok(r) => r,
            Err(_) => {
                // Yield to browser event loop and retry
                wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(
                    &wasm_bindgen::JsValue::NULL,
                ))
                .await
                .ok();
                rx.try_recv().ok()?
            }
        };

        if map_result.is_err() {
            return None;
        }

        let data = buffer_slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
        for row in 0..self.height {
            let offset = (row * self.padded_bytes_per_row) as usize;
            let row_data = &data[offset..offset + self.unpadded_bytes_per_row as usize];
            if self.is_bgra {
                for pixel in row_data.chunks_exact(4) {
                    pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
            } else {
                pixels.extend_from_slice(row_data);
            }
        }
        drop(data);
        self.buffer.unmap();

        // Encode as PNG
        let img = image::RgbaImage::from_raw(self.width, self.height, pixels)?;
        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
        Some(png_bytes)
    }
}

fn orthographic_projection(width: f32, height: f32) -> [f32; 16] {
    // Maps (0..width, 0..height) to (-1..1, -1..1) clip space
    // Y=0 is top, Y=height is bottom (screen coordinates)
    let sx = 2.0 / width;
    let sy = -2.0 / height;
    [
        sx, 0.0, 0.0, 0.0, //
        0.0, sy, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        -1.0, 1.0, 0.0, 1.0, //
    ]
}

impl Renderer {
    /// Decode and upload an image-format file (PNG / JPG / etc.) into a
    /// new GPU texture. Returns the [`Texture`] for the caller to register
    /// into its asset registry.
    ///
    /// This is a high-level convenience that hides the renderer's
    /// `device` / `queue` / `bind_group_layout` from non-render crates,
    /// so asset code does not need to depend on `wgpu` directly.
    pub fn load_texture(&self, label: &str, bytes: &[u8]) -> Result<Texture> {
        Texture::from_bytes(
            &self.device,
            &self.queue,
            &self.texture_bind_group_layout,
            bytes,
            label,
        )
    }

    /// Parse font bytes and create the initial (ASCII-warmed, otherwise lazy)
    /// glyph atlas. Returns the [`crate::Font`] together with its initial
    /// atlas [`Texture`]; the caller must register the texture under
    /// `atlas_texture_id` in its asset registry.
    pub fn load_font(
        &self,
        bytes: &[u8],
        size: f32,
        atlas_texture_id: crate::TextureId,
    ) -> Result<(crate::Font, Texture)> {
        crate::Font::from_bytes(
            &self.device,
            &self.queue,
            &self.texture_bind_group_layout,
            bytes,
            size,
            atlas_texture_id,
        )
    }

    /// Ensure every character in `text` has a rasterized glyph in `font`'s
    /// atlas, allocating and uploading new pixels (or growing the atlas)
    /// as needed.
    ///
    /// `atlas_slot` is the [`Texture`] slot in the caller's asset registry
    /// that currently holds this font's atlas. If the atlas needs to grow
    /// past its current size, a fresh GPU texture is allocated and written
    /// into `atlas_slot` in place — the caller's `TextureId` stays valid
    /// because it indexes into the same slot.
    pub fn prepare_text(
        &self,
        font: &mut crate::Font,
        atlas_slot: &mut Texture,
        text: &str,
    ) -> Result<()> {
        match font.prepare_text(
            &self.device,
            &self.queue,
            &self.texture_bind_group_layout,
            &atlas_slot.texture,
            text,
        ) {
            crate::PrepareOutcome::NoChange | crate::PrepareOutcome::AtlasUpdated => {}
            crate::PrepareOutcome::AtlasResized(new_texture) => {
                *atlas_slot = new_texture;
            }
        }
        Ok(())
    }
}

// ── Note: procedural texture creation lives in `procedural.rs` ─────
// `create_white_pixel_texture` / `create_filled_circle_texture` /
// `create_ring_texture` / `create_rgba_texture` are implemented in
// `procedural.rs` as additional `impl Renderer` blocks, alongside the
// pure-CPU pixel rasterizers (`build_filled_circle_pixels`,
// `build_ring_pixels`) they use. Keeping them out of this file keeps
// `renderer.rs` focused on the GPU pipeline (init, batching, draw,
// screenshot) and lets the rasterizers be unit-tested without spinning
// up a `Renderer`.

// ─────────────────────────────────────────────────────────────────────
// Batching / ordering / clipping — extracted as free functions
//
// These carry all the frame-preparation logic that used to be inline in
// `Renderer`. Pulling them out means they can be unit-tested without a GPU,
// which matters because they're where the sprite-cap crash guard and the draw
// ordering rules live.
// ─────────────────────────────────────────────────────────────────────

/// One run of draw commands that share a texture and a clip rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Batch {
    /// Index of the first sprite, in sprite units (not vertices or indices).
    pub start: usize,
    pub count: usize,
    pub texture_index: usize,
    pub clip: Option<[f32; 4]>,
}

/// Drop draw commands beyond the fixed vertex-buffer capacity.
///
/// The vertex buffer is allocated once at init for `MAX_SPRITES`. Writing past it
/// is a wgpu buffer overrun (an outright crash), so excess sprites are discarded
/// instead. `warned` latches so the diagnostic is emitted once rather than every
/// frame — silently losing sprites is bad, but a 60 Hz log flood is worse.
fn enforce_sprite_cap(commands: &mut Vec<DrawCommand>, warned: &mut bool) {
    if commands.len() <= MAX_SPRITES {
        return;
    }
    let dropped = commands.len() - MAX_SPRITES;
    if !*warned {
        *warned = true;
        tracing::warn!(
            "Draw command count ({}) exceeds MAX_SPRITES ({}); dropping {} sprite(s) this \
             frame. Further overflows will not be reported. Reduce sprites per frame or raise \
             MAX_SPRITES.",
            commands.len(),
            MAX_SPRITES,
            dropped
        );
    }
    commands.truncate(MAX_SPRITES);
}

/// Order draw commands back-to-front by layer.
///
/// **Stable, and keyed on `layer` alone.** Texture is deliberately not part of the
/// key: including it would batch better but would reorder overlapping sprites
/// within a layer, making "which is on top" depend on which texture they happen
/// to use. Callers who want fewer batches should group their own draws.
///
/// Skipped entirely when every command is on layer 0, so the common case pays
/// nothing.
fn sort_by_layer(commands: &mut [DrawCommand]) {
    if commands.iter().any(|c| c.layer != 0) {
        commands.sort_by_key(|c| c.layer);
    }
}

/// Group consecutive commands into batches, breaking on texture or clip change.
fn compute_batches(commands: &[DrawCommand]) -> Vec<Batch> {
    let mut batches: Vec<Batch> = Vec::new();
    for (i, cmd) in commands.iter().enumerate() {
        let tex = cmd.texture_id.0;
        match batches.last_mut() {
            // Bit equality on the clip is the intended test: values are copied
            // verbatim from the caller and never recomputed.
            Some(last) if last.texture_index == tex && last.clip == cmd.clip => {
                last.count += 1;
            }
            _ => batches.push(Batch {
                start: i,
                count: 1,
                texture_index: tex,
                clip: cmd.clip,
            }),
        }
    }
    batches
}

/// The four corners of a destination rect, in TL, TR, BR, BL order, rotated
/// clockwise about the rect's centre.
///
/// `rotation == 0.0` takes an exact fast path (no trig, no float drift), which is
/// what almost every sprite hits.
fn quad_corners(dst_rect: [f32; 4], rotation: f32) -> [[f32; 2]; 4] {
    let [x, y, w, h] = dst_rect;
    if rotation == 0.0 {
        return [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    }
    // Screen space is y-down, so a positive angle reads as clockwise on screen.
    let (sin, cos) = rotation.sin_cos();
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let rotate = |px: f32, py: f32| {
        let ox = px - cx;
        let oy = py - cy;
        [cx + ox * cos - oy * sin, cy + ox * sin + oy * cos]
    };
    [
        rotate(x, y),
        rotate(x + w, y),
        rotate(x + w, y + h),
        rotate(x, y + h),
    ]
}

/// Convert a virtual-pixel clip rect into a physical scissor rect, clamped to the
/// surface. wgpu rejects a scissor rect that extends past the attachment.
fn scissor_rect(
    clip: [f32; 4],
    surface_width: u32,
    surface_height: u32,
    virtual_width: f32,
    virtual_height: f32,
) -> (u32, u32, u32, u32) {
    if virtual_width <= 0.0 || virtual_height <= 0.0 {
        return (0, 0, surface_width, surface_height);
    }
    let scale_x = surface_width as f32 / virtual_width;
    let scale_y = surface_height as f32 / virtual_height;

    // Floor the origin and ceil the far edge so a clip never crops a pixel the
    // caller meant to include.
    let x0 = (clip[0] * scale_x).floor().max(0.0) as u32;
    let y0 = (clip[1] * scale_y).floor().max(0.0) as u32;
    let x1 = ((clip[0] + clip[2]) * scale_x).ceil().max(0.0) as u32;
    let y1 = ((clip[1] + clip[3]) * scale_y).ceil().max(0.0) as u32;

    let x0 = x0.min(surface_width);
    let y0 = y0.min(surface_height);
    // `saturating_sub` keeps a fully-offscreen clip at zero extent instead of
    // wrapping into a huge rect.
    let w = x1.min(surface_width).saturating_sub(x0);
    let h = y1.min(surface_height).saturating_sub(y0);
    (x0, y0, w, h)
}

#[cfg(test)]
mod batching_tests {
    use super::*;

    fn cmd(texture: usize, layer: i32) -> DrawCommand {
        DrawCommand {
            texture_id: crate::TextureId(texture),
            layer,
            ..Default::default()
        }
    }

    fn clipped(texture: usize, clip: Option<[f32; 4]>) -> DrawCommand {
        DrawCommand {
            texture_id: crate::TextureId(texture),
            clip,
            ..Default::default()
        }
    }

    // ── Sprite cap (the crash guard) ────────────────────────────────

    #[test]
    fn sprite_cap_truncates_instead_of_overrunning_the_buffer() {
        // The whole point: MAX_SPRITES+1 commands used to be written straight
        // into a fixed-size vertex buffer with no check, which is a wgpu buffer
        // overrun rather than a graceful degrade.
        let mut commands: Vec<DrawCommand> = (0..MAX_SPRITES + 500).map(|_| cmd(0, 0)).collect();
        let mut warned = false;
        enforce_sprite_cap(&mut commands, &mut warned);
        assert_eq!(commands.len(), MAX_SPRITES);
        assert!(warned, "overflow should be reported at least once");
    }

    #[test]
    fn sprite_cap_warns_only_once() {
        let mut warned = false;
        for _ in 0..3 {
            let mut commands: Vec<DrawCommand> = (0..MAX_SPRITES + 1).map(|_| cmd(0, 0)).collect();
            enforce_sprite_cap(&mut commands, &mut warned);
        }
        // Latched: a 60 Hz log flood would be worse than the dropped sprites.
        assert!(warned);
    }

    #[test]
    fn sprite_cap_leaves_normal_frames_untouched() {
        let mut commands: Vec<DrawCommand> = (0..100).map(|_| cmd(0, 0)).collect();
        let mut warned = false;
        enforce_sprite_cap(&mut commands, &mut warned);
        assert_eq!(commands.len(), 100);
        assert!(!warned);
    }

    // ── Layer ordering ──────────────────────────────────────────────

    #[test]
    fn sort_orders_layers_back_to_front() {
        let mut commands = vec![cmd(0, 5), cmd(1, -1), cmd(2, 0)];
        sort_by_layer(&mut commands);
        assert_eq!(
            commands.iter().map(|c| c.layer).collect::<Vec<_>>(),
            vec![-1, 0, 5]
        );
    }

    #[test]
    fn sort_is_stable_within_a_layer() {
        // Submission order must survive inside one layer, or painter's-algorithm
        // draws (e.g. a shadow then the sprite on top of it) would flip.
        let mut commands = vec![cmd(7, 0), cmd(3, 0), cmd(9, 0), cmd(1, 0)];
        sort_by_layer(&mut commands);
        assert_eq!(
            commands.iter().map(|c| c.texture_id.0).collect::<Vec<_>>(),
            vec![7, 3, 9, 1],
            "texture must NOT influence ordering within a layer"
        );
    }

    #[test]
    fn sort_preserves_submission_order_across_mixed_layers() {
        let mut commands = vec![cmd(1, 1), cmd(2, 0), cmd(3, 1), cmd(4, 0)];
        sort_by_layer(&mut commands);
        // Layer 0 first (2 then 4), then layer 1 (1 then 3) — each in submission order.
        assert_eq!(
            commands.iter().map(|c| c.texture_id.0).collect::<Vec<_>>(),
            vec![2, 4, 1, 3]
        );
    }

    // ── Batching ────────────────────────────────────────────────────

    #[test]
    fn consecutive_same_texture_commands_form_one_batch() {
        let commands = vec![cmd(0, 0), cmd(0, 0), cmd(0, 0)];
        let batches = compute_batches(&commands);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].start, 0);
        assert_eq!(batches[0].count, 3);
    }

    #[test]
    fn texture_change_breaks_the_batch() {
        // A,B,A,B is the pathological interleave: 4 draw calls, not 2.
        let commands = vec![cmd(0, 0), cmd(1, 0), cmd(0, 0), cmd(1, 0)];
        let batches = compute_batches(&commands);
        assert_eq!(batches.len(), 4);
        assert_eq!(
            batches.iter().map(|b| b.texture_index).collect::<Vec<_>>(),
            vec![0, 1, 0, 1]
        );
    }

    #[test]
    fn clip_change_breaks_the_batch_even_with_one_texture() {
        let commands = vec![
            clipped(0, None),
            clipped(0, Some([0.0, 0.0, 10.0, 10.0])),
            clipped(0, Some([0.0, 0.0, 10.0, 10.0])),
            clipped(0, None),
        ];
        let batches = compute_batches(&commands);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].count, 1);
        assert_eq!(batches[1].count, 2, "identical clips should coalesce");
        assert_eq!(batches[2].count, 1);
    }

    #[test]
    fn batch_ranges_cover_every_command_exactly_once() {
        // The ranges index the vertex buffer, so a gap or overlap would render
        // the wrong sprites.
        let commands = vec![cmd(0, 0), cmd(0, 0), cmd(1, 0), cmd(2, 0), cmd(2, 0)];
        let batches = compute_batches(&commands);
        let mut covered = 0;
        let mut expected_start = 0;
        for b in &batches {
            assert_eq!(b.start, expected_start);
            expected_start += b.count;
            covered += b.count;
        }
        assert_eq!(covered, commands.len());
    }

    #[test]
    fn empty_command_list_produces_no_batches() {
        assert!(compute_batches(&[]).is_empty());
    }

    // ── Scissor conversion ──────────────────────────────────────────

    #[test]
    fn scissor_scales_virtual_to_physical() {
        // 512x480 virtual on a 1024x960 surface is exactly 2x.
        let (x, y, w, h) = scissor_rect([10.0, 20.0, 100.0, 50.0], 1024, 960, 512.0, 480.0);
        assert_eq!((x, y, w, h), (20, 40, 200, 100));
    }

    #[test]
    fn scissor_is_clamped_to_the_surface() {
        // wgpu rejects a rect that leaves the attachment, so an over-large clip
        // must be trimmed rather than passed through.
        let (x, y, w, h) = scissor_rect([0.0, 0.0, 10_000.0, 10_000.0], 800, 600, 400.0, 300.0);
        assert_eq!((x, y), (0, 0));
        assert!(x + w <= 800, "w={w} would exceed the surface");
        assert!(y + h <= 600, "h={h} would exceed the surface");
    }

    #[test]
    fn fully_offscreen_clip_yields_zero_extent_not_a_wrapped_rect() {
        // Naive subtraction here would underflow u32 into a huge rect.
        let (_, _, w, h) = scissor_rect([5000.0, 5000.0, 10.0, 10.0], 800, 600, 400.0, 300.0);
        assert_eq!((w, h), (0, 0));
    }

    /// A clip straddling the right edge must stay inside the attachment.
    ///
    /// wgpu validates `x + w <= width`, so the dangerous case is not the fully
    /// offscreen one (extent 0, skipped) but the *partly* offscreen one: the origin is
    /// still on screen and the far edge must be pulled back to it, not left hanging
    /// over.
    #[test]
    fn a_clip_crossing_the_right_edge_stays_inside_the_attachment() {
        let (x, y, w, h) = scissor_rect([390.0, 290.0, 100.0, 100.0], 800, 600, 400.0, 300.0);
        assert!(
            x + w <= 800 && y + h <= 600,
            "scissor {x},{y} {w}×{h} reaches past 800×600"
        );
        assert!(w > 0 && h > 0, "the on-screen part must survive");
    }

    #[test]
    fn negative_clip_origin_is_clamped_to_zero() {
        let (x, y, _, _) = scissor_rect([-50.0, -50.0, 100.0, 100.0], 800, 600, 400.0, 300.0);
        assert_eq!((x, y), (0, 0));
    }

    // ── Rotation ────────────────────────────────────────────────────

    #[test]
    fn unrotated_quad_uses_the_exact_fast_path() {
        // Must be bit-exact, not merely close: every existing sprite goes through
        // here and any float drift would soften pixel-art edges.
        let c = quad_corners([10.0, 20.0, 30.0, 40.0], 0.0);
        assert_eq!(c, [[10.0, 20.0], [40.0, 20.0], [40.0, 60.0], [10.0, 60.0]]);
    }

    #[test]
    fn quarter_turn_rotates_about_the_centre() {
        // A square rotated 90° maps onto itself, with corners cycled.
        let c = quad_corners([0.0, 0.0, 10.0, 10.0], std::f32::consts::FRAC_PI_2);
        let approx =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        // TL -> TR position (clockwise in y-down screen space).
        assert!(approx(c[0], [10.0, 0.0]), "got {:?}", c[0]);
        assert!(approx(c[1], [10.0, 10.0]), "got {:?}", c[1]);
        assert!(approx(c[2], [0.0, 10.0]), "got {:?}", c[2]);
        assert!(approx(c[3], [0.0, 0.0]), "got {:?}", c[3]);
    }

    #[test]
    fn rotation_preserves_the_centre_and_the_diagonal() {
        let rect = [5.0, 7.0, 20.0, 12.0];
        let (cx, cy) = (5.0 + 10.0, 7.0 + 6.0);
        for angle in [0.3_f32, 1.1, -2.0, 3.5] {
            let c = quad_corners(rect, angle);
            let mx = c.iter().map(|p| p[0]).sum::<f32>() / 4.0;
            let my = c.iter().map(|p| p[1]).sum::<f32>() / 4.0;
            assert!((mx - cx).abs() < 1e-3, "centre moved at angle {angle}");
            assert!((my - cy).abs() < 1e-3, "centre moved at angle {angle}");
            // Rigid rotation: the diagonal length is invariant.
            let d = ((c[2][0] - c[0][0]).powi(2) + (c[2][1] - c[0][1]).powi(2)).sqrt();
            let expected = (20.0_f32.powi(2) + 12.0_f32.powi(2)).sqrt();
            assert!(
                (d - expected).abs() < 1e-3,
                "diagonal changed at angle {angle}"
            );
        }
    }

    #[test]
    fn full_turn_returns_to_the_start() {
        let rect = [3.0, 4.0, 8.0, 6.0];
        let a = quad_corners(rect, 0.0);
        let b = quad_corners(rect, std::f32::consts::TAU);
        for i in 0..4 {
            assert!((a[i][0] - b[i][0]).abs() < 1e-3);
            assert!((a[i][1] - b[i][1]).abs() < 1e-3);
        }
    }

    #[test]
    fn degenerate_virtual_size_falls_back_to_full_surface() {
        // Guards against a divide-by-zero producing NaN scissor values.
        let (x, y, w, h) = scissor_rect([1.0, 1.0, 2.0, 2.0], 800, 600, 0.0, 0.0);
        assert_eq!((x, y, w, h), (0, 0, 800, 600));
    }
}
