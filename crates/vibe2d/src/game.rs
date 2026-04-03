use crate::context::Context;
use crate::screen::Screen;
use crate::Color;
use vibe_input::InputState;

/// The main trait users implement to create a game.
///
/// Follows the Ebiten/Love2D pattern: new -> update -> draw loop.
pub trait Game {
    /// Create and initialize the game. Load assets, set up state.
    fn new(ctx: &mut Context) -> Self;

    /// Called every frame. Update game logic, handle input.
    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState);

    /// Called every frame after update. Draw everything to screen.
    fn draw(&mut self, ctx: &Context, screen: &mut Screen);

    /// Background clear color. Override to customize.
    fn clear_color(&self) -> Color {
        Color::BLACK
    }

    /// Return the game state as JSON for VDP inspection.
    /// Override this to let AI tools inspect your game state.
    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Handle a VDP command to modify game state.
    /// Returns Ok(Value) on success, or an error message.
    #[cfg(feature = "vdp")]
    fn handle_vdp(&mut self, _method: &str, _params: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("Not implemented".to_string())
    }
}
