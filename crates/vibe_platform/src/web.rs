use std::sync::Arc;

use anyhow::Result;
use wasm_bindgen::JsCast;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::{Window, WindowId};

use vibe_input::InputState;
use vibe_render::Renderer;

use crate::common::{PlatformCallbacks, PlatformConfig};

struct App<C: PlatformCallbacks> {
    config: PlatformConfig,
    callbacks: C,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    input: InputState,
    last_frame: Option<Instant>,
    initialized: bool,
}

impl<C: PlatformCallbacks + 'static> ApplicationHandler for App<C> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Get the canvas element from the DOM
        let canvas: web_sys::HtmlCanvasElement = web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| doc.get_element_by_id("vibe2d-canvas"))
            .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("Failed to find <canvas id='vibe2d-canvas'>");

        let win_attrs = Window::default_attributes()
            .with_title(&self.config.window_title)
            .with_canvas(Some(canvas.clone()));

        let window = Arc::new(
            event_loop
                .create_window(win_attrs)
                .expect("Failed to create window"),
        );

        // Async wgpu init — spawn_local since we can't block on web
        let window_clone = window.clone();
        let virtual_width = self.config.virtual_width;
        let virtual_height = self.config.virtual_height;

        // For web, wgpu init must be async. We do it synchronously via
        // wasm_bindgen_futures inside resumed, but wgpu on web with
        // WebGPU backend requires async. We use pollster-like pattern
        // but web-compatible: the instance.request_adapter and
        // adapter.request_device are resolved via JS promises.
        // However, winit's resumed is not async, so we use a two-phase init:
        // Phase 1 (here): create instance + surface synchronously
        // Phase 2: request adapter/device via spawn_local, then init renderer

        // Use WebGL2 (GL backend) for maximum browser compatibility.
        // wgpu 24's WebGPU backend has canvas context type-cast issues with
        // certain browser versions, so we default to GL which works everywhere.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window_clone.clone())
            .expect("Failed to create surface");

        self.window = Some(window);
        self.last_frame = Some(Instant::now());

        // We need to store these temporarily for the async init
        // On web, we must do adapter/device request asynchronously
        let callbacks_ptr = &mut self.callbacks as *mut C;
        let renderer_ptr = &mut self.renderer as *mut Option<Renderer>;
        let initialized_ptr = &mut self.initialized as *mut bool;

        // SAFETY: This spawn_local runs on the same thread (WASM is single-threaded)
        // and completes before the next event loop iteration delivers RedrawRequested.
        // The pointers remain valid because App lives for the duration of the event loop.
        wasm_bindgen_futures::spawn_local(async move {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("Failed to find GPU adapter");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("vibe2d_device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: adapter.limits(),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .expect("Failed to create GPU device");

            let size = window_clone.inner_size();
            let max_dim = device.limits().max_texture_dimension_2d;
            let surface_caps = surface.get_capabilities(&adapter);
            let surface_format = surface_caps
                .formats
                .iter()
                .find(|f| f.is_srgb())
                .copied()
                .unwrap_or(surface_caps.formats[0]);

            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: size.width.max(1).min(max_dim),
                height: size.height.max(1).min(max_dim),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &surface_config);

            let renderer = Renderer::new(
                device,
                queue,
                surface,
                surface_config,
                virtual_width,
                virtual_height,
            );

            // SAFETY: single-threaded WASM, pointers still valid
            unsafe {
                let initialized = &mut *initialized_ptr;
                let renderer_slot = &mut *renderer_ptr;
                let callbacks = &mut *callbacks_ptr;

                if !*initialized {
                    callbacks.on_init(&renderer);
                    *initialized = true;
                }
                *renderer_slot = Some(renderer);
            }

            // Request first redraw
            window_clone.request_redraw();
        });
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    let max_dim = renderer.max_texture_dimension();
                    renderer.resize(
                        new_size.width.max(1).min(max_dim),
                        new_size.height.max(1).min(max_dim),
                    );
                }
            }
            WindowEvent::KeyboardInput { event, .. } if !self.callbacks.should_suppress_input() => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    if event.state.is_pressed() {
                        self.input.on_key_pressed(keycode);
                    } else {
                        self.input.on_key_released(keycode);
                    }
                }
                if event.state.is_pressed()
                    && self.input.ime_preedit().is_none()
                    && let Some(ref text) = event.text
                {
                    for ch in text.chars() {
                        if !ch.is_control() {
                            self.input.on_char_received(ch);
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) if !self.callbacks.should_suppress_input() => match ime {
                Ime::Enabled | Ime::Disabled => {
                    self.input.clear_ime_preedit();
                }
                Ime::Preedit(text, cursor_range) => {
                    let cursor_byte = cursor_range.map(|(start, _end)| start);
                    self.input.on_ime_preedit(text, cursor_byte);
                }
                Ime::Commit(text) => {
                    self.input.on_ime_commit(&text);
                }
            },
            WindowEvent::CursorMoved { position, .. } => {
                if !self.callbacks.should_suppress_input()
                    && let Some(window) = &self.window
                {
                    let size = window.inner_size();
                    if size.width > 0 && size.height > 0 {
                        let vx =
                            (position.x as f32 / size.width as f32) * self.config.virtual_width;
                        let vy =
                            (position.y as f32 / size.height as f32) * self.config.virtual_height;
                        self.input.on_mouse_moved(vx, vy);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. }
                if !self.callbacks.should_suppress_input() =>
            {
                let mb = match button {
                    winit::event::MouseButton::Left => Some(vibe_input::MouseButton::Left),
                    winit::event::MouseButton::Right => Some(vibe_input::MouseButton::Right),
                    winit::event::MouseButton::Middle => Some(vibe_input::MouseButton::Middle),
                    _ => None,
                };
                if let Some(mb) = mb {
                    if state.is_pressed() {
                        self.input.on_mouse_button_pressed(mb);
                    } else {
                        self.input.on_mouse_button_released(mb);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !self.callbacks.should_suppress_input() => {
                let (scroll_x, scroll_y) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 20.0, y * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };
                self.input.on_mouse_scroll(scroll_x, scroll_y);
            }
            WindowEvent::RedrawRequested => {
                // Skip frames until async init completes
                if self.renderer.is_none() {
                    return;
                }

                let now = Instant::now();
                let dt = if let Some(last) = self.last_frame {
                    now.duration_since(last).as_secs_f32()
                } else {
                    1.0 / 60.0
                };
                self.last_frame = Some(now);

                // Update
                self.callbacks.on_update(dt, &mut self.input);

                // Render
                if self.callbacks.should_render()
                    && let Some(renderer) = &mut self.renderer
                {
                    self.callbacks.on_render(renderer);
                    let clear_color = self.callbacks.clear_color();
                    let textures = self.callbacks.get_textures();
                    if let Err(e) = renderer.render(clear_color, &textures) {
                        tracing::error!("Render error: {}", e);
                    }
                }

                // Clear per-frame input after update
                self.input.begin_frame();

                // Request next frame (triggers requestAnimationFrame on web)
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Run the game on the web platform (winit web backend + wgpu WebGPU/WebGL2).
///
/// This function spawns the winit event loop on web using `spawn()`,
/// which integrates with the browser's `requestAnimationFrame` and never returns.
pub fn run_web<C: PlatformCallbacks + 'static>(
    config: PlatformConfig,
    callbacks: C,
    input: InputState,
) -> Result<()> {
    let event_loop = EventLoop::new()?;

    let app = App {
        config,
        callbacks,
        window: None,
        renderer: None,
        input,
        last_frame: None,
        initialized: false,
    };

    // On web, spawn() integrates with the browser event loop (rAF).
    // Unlike run_app(), it doesn't block — it moves the closure into JS.
    event_loop.spawn_app(app);
    Ok(())
}

/// Fetch a file from the server via HTTP.
pub async fn fetch_file(url: &str) -> anyhow::Result<Vec<u8>> {
    use js_sys::Uint8Array;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::Response;

    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window"))?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| anyhow::anyhow!("Fetch failed for '{}': {:?}", url, e))?;
    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| anyhow::anyhow!("Not a Response"))?;
    if !resp.ok() {
        return Err(anyhow::anyhow!("HTTP {} for '{}'", resp.status(), url));
    }
    let array_buffer = JsFuture::from(
        resp.array_buffer()
            .map_err(|e| anyhow::anyhow!("array_buffer() failed: {:?}", e))?,
    )
    .await
    .map_err(|e| anyhow::anyhow!("await array_buffer failed: {:?}", e))?;
    let uint8_array = Uint8Array::new(&array_buffer);
    Ok(uint8_array.to_vec())
}
