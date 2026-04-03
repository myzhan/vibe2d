use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use vibe_input::InputState;
use vibe_render::Renderer;

/// Configuration needed to create the platform window and renderer.
pub struct PlatformConfig {
    pub window_width: u32,
    pub window_height: u32,
    pub window_title: String,
    pub vsync: bool,
    pub virtual_width: f32,
    pub virtual_height: f32,
}

/// Callbacks that the game provides to the platform runner.
pub trait PlatformCallbacks {
    fn on_init(&mut self, renderer: &Renderer);
    fn on_input_event(&mut self, input: &mut InputState);
    fn on_update(&mut self, dt: f32, input: &mut InputState);
    fn on_render(&mut self, renderer: &mut Renderer);
    fn clear_color(&self) -> [f32; 4];
    fn get_textures(&self) -> Vec<&vibe_render::Texture>;
}

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

        let win_attrs = Window::default_attributes()
            .with_title(&self.config.window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window_width,
                self.config.window_height,
            ));

        let window = Arc::new(event_loop.create_window(win_attrs).expect("Failed to create window"));

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
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    if event.state.is_pressed() {
                        self.input.on_key_pressed(keycode);
                    } else {
                        self.input.on_key_released(keycode);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    if size.width > 0 && size.height > 0 {
                        let vx = (position.x as f32 / size.width as f32) * self.config.virtual_width;
                        let vy = (position.y as f32 / size.height as f32) * self.config.virtual_height;
                        self.input.on_mouse_moved(vx, vy);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
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

                // Render
                if let Some(renderer) = &mut self.renderer {
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
