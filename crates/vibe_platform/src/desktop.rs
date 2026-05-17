use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::{Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
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

impl<C: PlatformCallbacks> ApplicationHandler for App<C> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let mut win_attrs = Window::default_attributes()
            .with_title(&self.config.window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window_width,
                self.config.window_height,
            ));
        // VIBE_HEADLESS=1 → still create the window (wgpu surface, screenshot,
        // etc. all keep working) but mark it invisible so `--ignored` test runs
        // don't steal focus on macOS. The window is required by current wgpu;
        // a true surfaceless path is tracked separately (C 方案).
        if std::env::var_os("VIBE_HEADLESS").is_some() {
            win_attrs = win_attrs.with_visible(false);
        }

        let window = Arc::new(
            event_loop
                .create_window(win_attrs)
                .expect("Failed to create window"),
        );

        // Enable IME so winit emits WindowEvent::Ime{Enabled,Preedit,Commit,Disabled}.
        // This is what makes Chinese / Japanese / Korean / emoji-picker input work.
        // No-op on platforms that don't support it.
        window.set_ime_allowed(true);

        // Create wgpu instance + surface + device
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to find GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("vibe2d_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
            None,
        ))
        .expect("Failed to create GPU device");

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let present_mode = if self.config.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode,
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
            self.config.virtual_width,
            self.config.virtual_height,
        );

        if !self.initialized {
            self.callbacks.on_init(&renderer);
            self.initialized = true;
        }

        self.renderer = Some(renderer);
        self.window = Some(window);
        self.last_frame = Some(Instant::now());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);
                }
            }
            // `if` guard lets us skip the whole arm when VDP is driving input,
            // which is cleaner than an inner `if` per clippy::collapsible_match.
            WindowEvent::KeyboardInput { event, .. } if !self.callbacks.should_suppress_input() => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    if event.state.is_pressed() {
                        self.input.on_key_pressed(keycode);
                    } else {
                        self.input.on_key_released(keycode);
                    }
                }
                // Forward printable characters for UI text input.
                // Skip when an IME composition is active — those keystrokes belong
                // to the IME, and the resulting text will arrive via WindowEvent::Ime.
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
                    // Disabled means the IME composition was abandoned; clear any
                    // leftover preedit so widgets stop showing stale composition text.
                    self.input.clear_ime_preedit();
                }
                Ime::Preedit(text, cursor_range) => {
                    // winit reports the cursor as a (start, end) byte range; we only
                    // use the start as the caret position. Empty `text` ends preedit.
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
                let now = Instant::now();
                let dt = if let Some(last) = self.last_frame {
                    now.duration_since(last).as_secs_f32()
                } else {
                    1.0 / 60.0
                };
                self.last_frame = Some(now);

                // Update
                self.callbacks.on_update(dt, &mut self.input);

                // Render (skip when VDP fast-forward is active)
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

                // Request next frame
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Run the game on the desktop platform (winit + wgpu).
pub fn run_desktop<C: PlatformCallbacks + 'static>(
    config: PlatformConfig,
    callbacks: C,
    input: InputState,
) -> Result<()> {
    let event_loop = EventLoop::new()?;

    let mut app = App {
        config,
        callbacks,
        window: None,
        renderer: None,
        input,
        last_frame: None,
        initialized: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Read a file from the local filesystem.
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", path, e))
}
