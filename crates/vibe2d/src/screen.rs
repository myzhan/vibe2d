use crate::Color;
use vibe_render::{Font, Renderer, TextureId};

/// The render target for the current frame. Users draw to this.
pub struct Screen<'a> {
    renderer: &'a mut Renderer,
    pub virtual_width: f32,
    pub virtual_height: f32,
}

impl<'a> Screen<'a> {
    pub fn new(renderer: &'a mut Renderer, virtual_width: f32, virtual_height: f32) -> Self {
        Self {
            renderer,
            virtual_width,
            virtual_height,
        }
    }

    /// Draw a filled, antialiased circle centered at `(cx, cy)` with
    /// the given `radius` (in virtual pixels) and `color`, using
    /// `texture` as the disc image.
    ///
    /// `texture` should be an alpha-AA filled-circle sprite — typically
    /// produced once during `Game::new` via
    /// [`Renderer::create_filled_circle_texture`] and registered into
    /// [`vibe_asset::AssetManager`]. The engine no longer ships a
    /// built-in circle texture; games that want one own its lifecycle.
    ///
    /// Repeated calls with the same `texture` batch into one GPU draw
    /// call.
    pub fn draw_circle(&mut self, texture: TextureId, cx: f32, cy: f32, radius: f32, color: Color) {
        let d = radius * 2.0;
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id: texture,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [cx - radius, cy - radius, d, d],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw an antialiased circle outline (ring) centered at
    /// `(cx, cy)` with the given `radius` (in virtual pixels) and
    /// `color`, using `texture` as the ring image.
    ///
    /// `texture` should be an alpha-AA hollow ring sprite — typically
    /// produced via [`Renderer::create_ring_texture`] in `Game::new`.
    /// The ring's stroke thickness is whatever was baked into
    /// `texture` (see `Renderer::create_ring_texture`'s
    /// `thickness_ratio` parameter); for several different stroke
    /// widths, register multiple ring textures and pick one per call.
    pub fn draw_circle_outline(
        &mut self,
        texture: TextureId,
        cx: f32,
        cy: f32,
        radius: f32,
        color: Color,
    ) {
        let d = radius * 2.0;
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id: texture,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [cx - radius, cy - radius, d, d],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw a sprite at position (x, y) using the full texture.
    pub fn draw_sprite(&mut self, texture_id: TextureId, x: f32, y: f32, width: f32, height: f32) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw a sprite flipped vertically (used for upside-down pipes, etc.).
    pub fn draw_sprite_flipped(
        &mut self,
        texture_id: TextureId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: true,
        });
    }

    /// Draw a sprite flipped horizontally (used for left-facing characters, etc.).
    pub fn draw_sprite_flipped_h(
        &mut self,
        texture_id: TextureId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: true,
            flip_y: false,
        });
    }

    /// Draw a sprite flipped on both axes.
    pub fn draw_sprite_flipped_both(
        &mut self,
        texture_id: TextureId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: true,
            flip_y: true,
        });
    }

    /// Draw a sub-region of a sprite (for sprite sheets, scrolling textures, etc.).
    pub fn draw_sprite_region(
        &mut self,
        texture_id: TextureId,
        src_rect: [f32; 4],
        dst_rect: [f32; 4],
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw a sub-region of a sprite with flip control.
    pub fn draw_sprite_region_flipped(
        &mut self,
        texture_id: TextureId,
        src_rect: [f32; 4],
        dst_rect: [f32; 4],
        flip_x: bool,
        flip_y: bool,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x,
            flip_y,
        });
    }

    /// Draw text using a loaded font at position (x, y).
    pub fn draw_text(&mut self, font: &Font, text: &str, x: f32, y: f32) {
        for (tex_id, src_rect, dst_rect) in font.layout_text(text, x, y) {
            self.renderer.draw_sprite(vibe_render::DrawCommand {
                texture_id: tex_id,
                src_rect,
                dst_rect,
                color: [1.0, 1.0, 1.0, 1.0],
                flip_x: false,
                flip_y: false,
            });
        }
    }

    /// Draw text centered horizontally at the given y position.
    pub fn draw_text_centered(&mut self, font: &Font, text: &str, y: f32) {
        let text_w = font.text_width(text);
        let x = (self.virtual_width - text_w) / 2.0;
        self.draw_text(font, text, x, y);
    }

    /// Draw a sprite with color tinting (color is multiplied with texture color).
    pub fn draw_sprite_tinted(
        &mut self,
        texture_id: TextureId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw a sub-region of a sprite with color tinting.
    pub fn draw_sprite_region_tinted(
        &mut self,
        texture_id: TextureId,
        src_rect: [f32; 4],
        dst_rect: [f32; 4],
        color: Color,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draw a sub-region of a sprite with flip control and color tinting.
    pub fn draw_sprite_region_flipped_tinted(
        &mut self,
        texture_id: TextureId,
        src_rect: [f32; 4],
        dst_rect: [f32; 4],
        flip_x: bool,
        flip_y: bool,
        color: Color,
    ) {
        self.renderer.draw_sprite(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: color.to_array(),
            flip_x,
            flip_y,
        });
    }
}
