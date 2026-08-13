use crate::Color;
use vibe_render::{Font, Renderer, TextureId};

/// The render target for the current frame. Users draw to this.
///
/// Carries two pieces of *ambient* state — the current layer and clip rect —
/// which are stamped onto every draw issued afterwards. This mirrors how
/// immediate-mode 2D APIs conventionally work (set state, draw, restore) and
/// keeps the per-draw signatures from growing two more parameters.
pub struct Screen<'a> {
    renderer: &'a mut Renderer,
    pub virtual_width: f32,
    pub virtual_height: f32,
    layer: i32,
    clip: Option<[f32; 4]>,
    camera: [f32; 2],
    shake: [f32; 2],
    rotation: f32,
}

impl<'a> Screen<'a> {
    pub fn new(renderer: &'a mut Renderer, virtual_width: f32, virtual_height: f32) -> Self {
        Self {
            renderer,
            virtual_width,
            virtual_height,
            layer: 0,
            clip: None,
            camera: [0.0, 0.0],
            shake: [0.0, 0.0],
            rotation: 0.0,
        }
    }

    // ── Ambient draw state ──────────────────────────────────────────

    /// Set the layer for subsequent draws. Lower layers render behind higher
    /// ones; within one layer, draw order is preserved.
    ///
    /// Use this instead of carefully ordering your draw calls when things need to
    /// composite in a fixed back-to-front order (background, tiles, entities,
    /// foreground, HUD).
    pub fn set_layer(&mut self, layer: i32) {
        self.layer = layer;
    }

    pub fn layer(&self) -> i32 {
        self.layer
    }

    /// Clip subsequent draws to a rectangle in virtual pixels.
    ///
    /// The classic use is a sprite that is only partly "grown": a vine rising out
    /// of a block, a laser beam extending to its current length, a platform
    /// emerging from a pipe. Changing the clip forces a batch break, so set it
    /// around a small group of draws rather than per-sprite.
    pub fn set_clip_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.clip = Some([x, y, width, height]);
    }

    /// Remove any clip rect, restoring full-screen drawing.
    pub fn clear_clip(&mut self) {
        self.clip = None;
    }

    pub fn clip_rect(&self) -> Option<[f32; 4]> {
        self.clip
    }

    /// Run `f` with a clip rect applied, restoring the previous clip afterwards.
    ///
    /// Preferred over the raw setters because it can't leak the clip into
    /// unrelated draws if you return early.
    pub fn clipped(&mut self, x: f32, y: f32, width: f32, height: f32, f: impl FnOnce(&mut Self)) {
        let previous = self.clip;
        self.set_clip_rect(x, y, width, height);
        f(self);
        self.clip = previous;
    }

    /// Run `f` on a given layer, restoring the previous layer afterwards.
    pub fn on_layer(&mut self, layer: i32, f: impl FnOnce(&mut Self)) {
        let previous = self.layer;
        self.set_layer(layer);
        f(self);
        self.layer = previous;
    }

    // ── Camera ──────────────────────────────────────────────────────

    /// Set the camera offset (world-space scroll) for subsequent draws.
    ///
    /// Positions passed to `draw_*` are treated as world coordinates and shifted
    /// by `-camera` at submission time. This replaces subtracting the scroll
    /// offset by hand at every call site, which is both noisy and easy to forget
    /// on one sprite out of thirty.
    ///
    /// Applied on the CPU rather than via the projection matrix, so screen-space
    /// content (HUD, UI) is simply drawn inside [`Screen::screen_space`] — the UI
    /// layer never goes through here at all and is unaffected either way.
    pub fn set_camera(&mut self, x: f32, y: f32) {
        self.camera = [x, y];
    }

    pub fn camera(&self) -> [f32; 2] {
        self.camera
    }

    /// Add a transient offset on top of the camera — screen shake.
    ///
    /// Kept separate from [`Screen::set_camera`] so gameplay scroll and impact
    /// feedback don't have to be combined by the caller (and so shake can be
    /// cleared without disturbing the scroll position).
    pub fn set_shake(&mut self, x: f32, y: f32) {
        self.shake = [x, y];
    }

    pub fn shake(&self) -> [f32; 2] {
        self.shake
    }

    /// Draw `f`'s contents in screen space, ignoring camera and shake.
    ///
    /// Use for HUD and any overlay that must stay put while the world scrolls.
    pub fn screen_space(&mut self, f: impl FnOnce(&mut Self)) {
        let camera = self.camera;
        let shake = self.shake;
        self.camera = [0.0, 0.0];
        self.shake = [0.0, 0.0];
        f(self);
        self.camera = camera;
        self.shake = shake;
    }

    /// Total offset subtracted from world positions this draw.
    fn view_offset(&self) -> [f32; 2] {
        [
            self.camera[0] - self.shake[0],
            self.camera[1] - self.shake[1],
        ]
    }

    // ── Rotation ────────────────────────────────────────────────────

    /// Set the rotation (radians, clockwise) applied to subsequent sprites about
    /// their own centre. Zero restores the axis-aligned fast path.
    pub fn set_rotation(&mut self, radians: f32) {
        self.rotation = radians;
    }

    pub fn rotation(&self) -> f32 {
        self.rotation
    }

    /// Run `f` with a rotation applied, restoring the previous value afterwards.
    pub fn rotated(&mut self, radians: f32, f: impl FnOnce(&mut Self)) {
        let previous = self.rotation;
        self.set_rotation(radians);
        f(self);
        self.rotation = previous;
    }

    /// Apply the camera/shake offset and hand the command to the renderer.
    ///
    /// Every `draw_*` funnels through here, so world-to-screen translation lives
    /// in exactly one place. Only the position is shifted — width, height,
    /// rotation and UVs are untouched.
    fn submit(&mut self, mut cmd: vibe_render::DrawCommand) {
        let [ox, oy] = self.view_offset();
        cmd.dst_rect[0] -= ox;
        cmd.dst_rect[1] -= oy;
        self.renderer.draw_sprite(cmd);
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
        self.submit(vibe_render::DrawCommand {
            texture_id: texture,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [cx - radius, cy - radius, d, d],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id: texture,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [cx - radius, cy - radius, d, d],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
        });
    }

    /// Draw a sprite at position (x, y) using the full texture.
    pub fn draw_sprite(&mut self, texture_id: TextureId, x: f32, y: f32, width: f32, height: f32) {
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: true,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: true,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: true,
            flip_y: true,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
        });
    }

    /// Draw a sub-region of a sprite (for sprite sheets, scrolling textures, etc.).
    pub fn draw_sprite_region(
        &mut self,
        texture_id: TextureId,
        src_rect: [f32; 4],
        dst_rect: [f32; 4],
    ) {
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: [1.0, 1.0, 1.0, 1.0],
            flip_x,
            flip_y,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
        });
    }

    /// Draw text using a loaded font at position (x, y).
    pub fn draw_text(&mut self, font: &Font, text: &str, x: f32, y: f32) {
        for (tex_id, src_rect, dst_rect) in font.layout_text(text, x, y) {
            self.submit(vibe_render::DrawCommand {
                texture_id: tex_id,
                src_rect,
                dst_rect,
                color: [1.0, 1.0, 1.0, 1.0],
                flip_x: false,
                flip_y: false,
                layer: self.layer,
                clip: self.clip,
                rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, width, height],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
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
        self.submit(vibe_render::DrawCommand {
            texture_id,
            src_rect,
            dst_rect,
            color: color.to_array(),
            flip_x,
            flip_y,
            layer: self.layer,
            clip: self.clip,
            rotation: self.rotation,
        });
    }
}
