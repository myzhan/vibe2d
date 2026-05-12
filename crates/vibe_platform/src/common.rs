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
    fn should_render(&self) -> bool {
        true
    }
    /// Returns `true` when real keyboard/mouse input should be suppressed
    /// (e.g. a VDP client is connected and providing simulated input).
    fn should_suppress_input(&self) -> bool {
        false
    }
}
