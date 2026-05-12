use std::sync::Arc;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::texture::Texture;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpriteVertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
    color: [f32; 4],
}

/// Draw command queued for the current frame.
#[derive(Clone, Copy)]
pub struct DrawCommand {
    pub texture_id: crate::TextureId,
    pub src_rect: [f32; 4], // x, y, w, h in UV coordinates (0..1)
    pub dst_rect: [f32; 4], // x, y, w, h in virtual pixels
    pub color: [f32; 4],
    pub flip_x: bool,
    pub flip_y: bool,
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

    /// Request a screenshot to be captured on the next render.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_screenshot(&mut self, path: impl Into<std::path::PathBuf>) {
        self.pending_screenshot = Some(path.into());
    }

    /// Render all queued draw commands and present to screen.
    pub fn render(&mut self, clear_color: [f32; 4], textures: &[&Texture]) -> Result<()> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        // Build vertex data from draw commands
        let mut vertices: Vec<SpriteVertex> = Vec::with_capacity(self.draw_commands.len() * 4);

        for cmd in &self.draw_commands {
            let [dx, dy, dw, dh] = cmd.dst_rect;
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

            vertices.push(SpriteVertex {
                position: [dx, dy],
                tex_coords: [tu_left, tv_top],
                color: cmd.color,
            });
            vertices.push(SpriteVertex {
                position: [dx + dw, dy],
                tex_coords: [tu_right, tv_top],
                color: cmd.color,
            });
            vertices.push(SpriteVertex {
                position: [dx + dw, dy + dh],
                tex_coords: [tu_right, tv_bottom],
                color: cmd.color,
            });
            vertices.push(SpriteVertex {
                position: [dx, dy + dh],
                tex_coords: [tu_left, tv_bottom],
                color: cmd.color,
            });
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

            self.execute_draw_commands(&mut render_pass, textures);
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
    fn execute_draw_commands<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        textures: &'a [&'a Texture],
    ) {
        if self.draw_commands.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.projection_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut current_texture: Option<usize> = None;
        let mut batch_start: u32 = 0;

        for (i, cmd) in self.draw_commands.iter().enumerate() {
            let tex_idx = cmd.texture_id.0;
            if current_texture != Some(tex_idx) {
                if let Some(_prev) = current_texture {
                    let sprite_count = i as u32 - batch_start;
                    if sprite_count > 0 {
                        render_pass.draw_indexed(
                            (batch_start * 6)..((batch_start + sprite_count) * 6),
                            0,
                            0..1,
                        );
                    }
                }
                if tex_idx < textures.len() {
                    render_pass.set_bind_group(1, &textures[tex_idx].bind_group, &[]);
                }
                current_texture = Some(tex_idx);
                batch_start = i as u32;
            }
        }

        if current_texture.is_some() {
            let sprite_count = self.draw_commands.len() as u32 - batch_start;
            if sprite_count > 0 {
                render_pass.draw_indexed(
                    (batch_start * 6)..((batch_start + sprite_count) * 6),
                    0,
                    0..1,
                );
            }
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

            self.execute_draw_commands(&mut render_pass, textures);
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
                        for pixel in row_data.chunks_exact(4) {
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

            self.execute_draw_commands(&mut render_pass, textures);
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
