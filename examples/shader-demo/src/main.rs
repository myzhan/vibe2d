//! Shader demo — a minimal, runnable showcase of vibe2d's custom-shader
//! support (the `dev.shader` branch feature).
//!
//! It draws three things, each through a *different* render pipeline, to prove
//! the pieces compose inside one frame and one sprite batcher:
//!
//! 1. A fullscreen animated **plasma** background, drawn through a custom
//!    fragment shader that ignores its texture and synthesises colour from the
//!    quad's UV + the auto-advanced `uniforms.time`.
//! 2. A bouncing ball distorted by a second custom shader (**ripple**) that
//!    samples the ball's own circle texture with a time-varying UV offset.
//! 3. A top HUD bar through the **built-in** sprite pipeline (a tinted quad),
//!    showing custom and default shaders coexisting and batching normally.
//!
//! Both custom shaders read `uniforms.params.x` (a game-supplied strength),
//! wired up via `Screen::set_shader_uniforms`. `time` needs no setup — the
//! renderer advances it every frame automatically (like Love2D's `love_Time`).

use vibe2d::prelude::*;

const VW: f32 = 640.0;
const VH: f32 = 360.0;

/// Animated colour plasma. The preamble (`shader_common.wgsl`) already provides
/// `VertexOutput`, `t_texture`/`t_sampler` and `uniforms`; a game only writes the
/// fragment function (plus whatever helpers it needs).
const PLASMA_FS: &str = r#"
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.tex_coords;
    let t = uniforms.time;
    let v = sin(uv.x * 12.0 + t)
          + sin(uv.y * 9.0 - t * 1.3)
          + sin((uv.x + uv.y) * 7.0 + t * 0.7);
    let phase = v * 1.04719;
    let rgb = vec3<f32>(
        0.5 + 0.5 * sin(phase),
        0.5 + 0.5 * sin(phase + 2.0944),
        0.5 + 0.5 * sin(phase + 4.1888)
    );
    let intensity = uniforms.params.x;
    return vec4<f32>(rgb * intensity, 1.0);
}
"#;

/// UV-distorting "ripple" applied to whatever texture the sprite uses.
const RIPPLE_FS: &str = r#"
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.tex_coords;
    let t = uniforms.time;
    let amp = uniforms.params.x * 0.02;
    let offset = vec2<f32>(
        sin(uv.y * 30.0 + t * 5.0) * amp,
        sin(uv.x * 30.0 - t * 4.0) * amp
    );
    let col = textureSample(t_texture, t_sampler, uv + offset);
    return col * in.color;
}
"#;

struct ShaderDemo {
    white: TextureId,
    ball: TextureId,
    plasma: ShaderId,
    ripple: ShaderId,
    ball_pos: Vec2,
    ball_vel: Vec2,
    radius: f32,
}

impl Game for ShaderDemo {
    fn new(ctx: &mut Context, renderer: &Renderer) -> Self {
        // Procedural textures — no PNG assets needed for this demo.
        let white = ctx
            .assets
            .register_texture("white", renderer.create_white_pixel_texture());
        let ball = ctx
            .assets
            .register_texture("ball", renderer.create_filled_circle_texture("ball", 256));

        // Compile the two custom fragment shaders. `create_fragment_shader`
        // takes the `&Renderer` that `Game::new` is handed and returns a
        // `ShaderId` to select while drawing.
        let plasma = renderer
            .create_fragment_shader("plasma", PLASMA_FS)
            .expect("plasma shader failed to compile");
        let ripple = renderer
            .create_fragment_shader("ripple", RIPPLE_FS)
            .expect("ripple shader failed to compile");

        Self {
            white,
            ball,
            plasma,
            ripple,
            ball_pos: Vec2::new(VW * 0.5, VH * 0.5),
            ball_vel: Vec2::new(170.0, 120.0),
            radius: 60.0,
        }
    }

    fn update(&mut self, _ctx: &mut Context, dt: f32, _input: &InputState) {
        self.ball_pos += self.ball_vel * dt;
        if self.ball_pos.x < self.radius || self.ball_pos.x > VW - self.radius {
            self.ball_vel.x = -self.ball_vel.x;
            self.ball_pos.x = self.ball_pos.x.clamp(self.radius, VW - self.radius);
        }
        if self.ball_pos.y < self.radius || self.ball_pos.y > VH - self.radius {
            self.ball_vel.y = -self.ball_vel.y;
            self.ball_pos.y = self.ball_pos.y.clamp(self.radius, VH - self.radius);
        }
    }

    fn draw(&self, _ctx: &Context, screen: &mut Screen) {
        // 1. Plasma background.
        screen.set_shader(self.plasma);
        screen.set_shader_uniforms(
            self.plasma,
            ShaderUniforms::new().with_params([0.9, 0.0, 0.0, 0.0]),
        );
        screen.draw_sprite(self.white, 0.0, 0.0, VW, VH);

        // 2. Bouncing ball through the ripple shader.
        screen.set_shader(self.ripple);
        screen.set_shader_uniforms(
            self.ripple,
            ShaderUniforms::new().with_params([3.0, 0.0, 0.0, 0.0]),
        );
        let d = self.radius * 2.0;
        screen.draw_sprite(
            self.ball,
            self.ball_pos.x - self.radius,
            self.ball_pos.y - self.radius,
            d,
            d,
        );

        // 3. HUD bar with the built-in pipeline (custom + default coexist).
        screen.clear_shader();
        screen.draw_sprite_tinted(
            self.white,
            0.0,
            0.0,
            VW,
            8.0,
            Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.85,
            },
        );
    }

    fn clear_color(&self) -> Color {
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

fn main() {
    vibe2d::run::<ShaderDemo>("game.yaml");
}
