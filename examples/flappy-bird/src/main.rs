use vibe2d::prelude::*;

// ── Constants (matching Love2D reference) ──────────────────────────
const GAME_SPEED: f32 = 200.0;
const GRAVITY: f32 = 500.0;
const JUMP_VELOCITY: f32 = -200.0;
const PIPE_WIDTH: f32 = 50.0;
const PIPE_GAP: f32 = 70.0;
const PIPE_SPAWN_INTERVAL: f32 = 1.5;
const COUNTDOWN_DURATION: f32 = 3.0;

// ── Game state machine ────────────────────────────────────────────
#[derive(PartialEq)]
enum GameState {
    Idle,
    Countdown,
    Playing,
    Dead,
}

// ── Background layer ──────────────────────────────────────────────
struct BgLayer {
    texture: TextureId,
    width: f32,
    height: f32,
    scroll_speed: f32,
    scroll_offset: f32,
}

impl BgLayer {
    fn new(texture: TextureId, width: f32, height: f32, layer_index: usize) -> Self {
        Self {
            texture,
            width,
            height,
            scroll_speed: 150.0 / layer_index as f32,
            scroll_offset: 0.0,
        }
    }

    fn update(&mut self, dt: f32) {
        self.scroll_offset = (self.scroll_offset + self.scroll_speed * dt) % self.width;
    }

    fn draw(&self, screen: &mut Screen) {
        let w = self.width;
        let h = self.height;
        screen.draw_sprite(self.texture, -self.scroll_offset, 0.0, w, h);
        screen.draw_sprite(self.texture, w - self.scroll_offset, 0.0, w, h);
    }
}

// ── Pipe pair ─────────────────────────────────────────────────────
struct PipePair {
    x: f32,
    gap_y: f32,
    scored: bool,
}

// ── Main game ─────────────────────────────────────────────────────
struct FlappyBirdGame {
    ground_tex: TextureId,
    bird_tex: TextureId,
    pipe_tex: TextureId,

    bg_layers: Vec<BgLayer>,

    ground_scroll: f32,
    ground_width: f32,
    ground_height: f32,

    bird_x: f32,
    bird_y: f32,
    bird_w: f32,
    bird_h: f32,
    bird_vy: f32,

    pipes: Vec<PipePair>,
    pipe_timer: f32,

    state: GameState,
    countdown_timer: f32,
    score: u32,
    best_score: u32,

    vw: f32,
    vh: f32,
}

impl FlappyBirdGame {
    fn reset_bird(&mut self) {
        self.bird_y = self.vh / 2.0 - self.bird_h / 2.0;
        self.bird_vy = 0.0;
    }

    fn reset_game(&mut self) {
        self.reset_bird();
        self.pipes.clear();
        self.pipe_timer = 0.0;
        self.score = 0;
    }

    fn ground_top(&self) -> f32 {
        self.vh - self.ground_height
    }
}

impl Game for FlappyBirdGame {
    fn new(ctx: &mut Context, _renderer: &Renderer) -> Self {
        let tex = |name: &str| -> TextureId {
            ctx.assets
                .texture_id(name)
                .unwrap_or_else(|| panic!("Missing texture: {}", name))
        };

        let vw = ctx.virtual_width;
        let vh = ctx.virtual_height;

        let bg_names = [
            ("background", 10),
            ("distant_clouds1", 9),
            ("distant_clouds2", 8),
            ("clouds", 7),
            ("huge_clouds", 6),
            ("hill2", 5),
            ("hill1", 4),
            ("bushes", 3),
            ("distant_trees", 2),
            ("trees_and_bushes", 1),
        ];

        let bg_layers: Vec<BgLayer> = bg_names
            .iter()
            .map(|(name, idx)| {
                let id = tex(name);
                let (w, h) = ctx.assets.texture_size(id);
                BgLayer::new(id, w as f32, h as f32, *idx)
            })
            .collect();

        let ground_tex = tex("ground");
        let (gw, gh) = ctx.assets.texture_size(ground_tex);

        let bird_tex = tex("bird");
        let (bw, bh) = ctx.assets.texture_size(bird_tex);

        Self {
            ground_tex,
            bird_tex,
            pipe_tex: tex("pipe"),
            bg_layers,
            ground_scroll: 0.0,
            ground_width: gw as f32,
            ground_height: gh as f32,
            bird_x: vw * 0.25,
            bird_y: vh / 2.0 - bh as f32 / 2.0,
            bird_w: bw as f32,
            bird_h: bh as f32,
            bird_vy: 0.0,
            pipes: Vec::new(),
            pipe_timer: 0.0,
            state: GameState::Idle,
            countdown_timer: 0.0,
            score: 0,
            best_score: 0,
            vw,
            vh,
        }
    }

    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        // Background and ground always scroll
        for layer in &mut self.bg_layers {
            layer.update(dt);
        }
        self.ground_scroll = (self.ground_scroll + GAME_SPEED * dt) % self.ground_width;

        match self.state {
            GameState::Idle => {
                // Bird gentle hover (sine wave)
                self.bird_y =
                    self.vh / 2.0 - self.bird_h / 2.0 + (self.ground_scroll * 0.05).sin() * 5.0;

                if input.is_action_just_pressed("flap") {
                    self.reset_game();
                    self.countdown_timer = COUNTDOWN_DURATION;
                    self.state = GameState::Countdown;
                    ctx.audio.play("flap");
                }
            }

            GameState::Countdown => {
                self.countdown_timer -= dt;
                // Bird gentle hover during countdown
                self.bird_y =
                    self.vh / 2.0 - self.bird_h / 2.0 + (self.ground_scroll * 0.05).sin() * 5.0;

                if self.countdown_timer <= 0.0 {
                    self.bird_vy = JUMP_VELOCITY;
                    self.state = GameState::Playing;
                }
            }

            GameState::Playing => {
                // Bird physics
                if input.is_action_just_pressed("flap") {
                    self.bird_vy = JUMP_VELOCITY;
                    ctx.audio.play("flap");
                }
                self.bird_vy += GRAVITY * dt;
                self.bird_y += self.bird_vy * dt;

                // Ground collision
                let ground_top = self.ground_top();
                if self.bird_y + self.bird_h > ground_top {
                    self.bird_y = ground_top - self.bird_h;
                    self.bird_vy = 0.0;
                    if self.score > self.best_score {
                        self.best_score = self.score;
                    }
                    self.state = GameState::Dead;
                    ctx.audio.play("hurt");
                    return;
                }
                // Ceiling
                if self.bird_y < 0.0 {
                    self.bird_y = 0.0;
                    self.bird_vy = 0.0;
                }

                // Pipe spawning
                self.pipe_timer += dt;
                if self.pipe_timer >= PIPE_SPAWN_INTERVAL {
                    self.pipe_timer -= PIPE_SPAWN_INTERVAL;
                    let min_y = 50.0;
                    let max_y = ground_top - 50.0;
                    let gap_y = min_y + rand::random::<f32>() * (max_y - min_y);
                    self.pipes.push(PipePair {
                        x: self.vw + PIPE_WIDTH,
                        gap_y,
                        scored: false,
                    });
                }

                // Move pipes
                for pipe in &mut self.pipes {
                    pipe.x -= GAME_SPEED * dt;
                }

                // Scoring
                for pipe in &mut self.pipes {
                    if !pipe.scored && pipe.x + PIPE_WIDTH < self.bird_x - self.bird_w / 2.0 {
                        pipe.scored = true;
                        self.score += 1;
                    }
                }

                // Remove off-screen pipes
                self.pipes.retain(|p| p.x + PIPE_WIDTH > -10.0);

                // Collision detection
                let bx = self.bird_x - self.bird_w / 2.0;
                let bird_rect = (bx, self.bird_y, self.bird_w, self.bird_h);
                for pipe in &self.pipes {
                    let top_h = pipe.gap_y - PIPE_GAP / 2.0;
                    if top_h > 0.0 && aabb_overlap(bird_rect, (pipe.x, 0.0, PIPE_WIDTH, top_h)) {
                        if self.score > self.best_score {
                            self.best_score = self.score;
                        }
                        self.state = GameState::Dead;
                        ctx.audio.play("hurt");
                        return;
                    }
                    let bot_y = pipe.gap_y + PIPE_GAP / 2.0;
                    let bot_h = ground_top - bot_y;
                    if bot_h > 0.0 && aabb_overlap(bird_rect, (pipe.x, bot_y, PIPE_WIDTH, bot_h)) {
                        if self.score > self.best_score {
                            self.best_score = self.score;
                        }
                        self.state = GameState::Dead;
                        ctx.audio.play("hurt");
                        return;
                    }
                }
            }

            GameState::Dead => {
                // Bird falls to ground
                self.bird_vy += GRAVITY * dt;
                self.bird_y += self.bird_vy * dt;
                let ground_top = self.ground_top();
                if self.bird_y + self.bird_h > ground_top {
                    self.bird_y = ground_top - self.bird_h;
                    self.bird_vy = 0.0;
                }

                if input.is_action_just_pressed("flap") {
                    self.reset_game();
                    self.countdown_timer = COUNTDOWN_DURATION;
                    self.state = GameState::Countdown;
                    ctx.audio.play("flap");
                }
            }
        }
    }

    fn update_ui(&mut self, ctx: &mut Context, input: &InputState) {
        let vw = self.vw;
        let vh = self.vh;
        let score = self.score;
        let best_score = self.best_score;
        let countdown_timer = self.countdown_timer;

        // Take ui_state out so we can borrow ctx.assets independently
        let mut ui_state = std::mem::take(&mut ctx.ui_state);

        let mut ui = UiContext::new(&mut ui_state, input, vw, vh);
        ui.set_anchor(Anchor::TopCenter);

        match self.state {
            GameState::Idle => {
                if let Some(font) = ctx.assets.font("score") {
                    ui.set_cursor(0.0, 40.0);
                    ui.label(font, "Flappy Bird");
                }
                if let Some(font) = ctx.assets.font("ui") {
                    ui.set_cursor(0.0, vh / 2.0 + 30.0);
                    ui.label(font, "Press SPACE to start");
                    if best_score > 0 {
                        let best = format!("Best: {}", best_score);
                        ui.label(font, &best);
                    }
                }
            }
            GameState::Countdown => {
                if let Some(font) = ctx.assets.font("score") {
                    let count = countdown_timer.ceil() as u32;
                    ui.set_cursor(0.0, vh / 3.0);
                    ui.label(font, &count.to_string());
                }
            }
            GameState::Playing => {
                if let Some(font) = ctx.assets.font("score") {
                    ui.set_cursor(0.0, 10.0);
                    ui.label(font, &score.to_string());
                }
            }
            GameState::Dead => {
                if let Some(font) = ctx.assets.font("score") {
                    ui.set_cursor(0.0, 50.0);
                    ui.label(font, "Game Over");
                    let text = format!("Score: {}", score);
                    ui.label(font, &text);
                }
                if let Some(font) = ctx.assets.font("ui") {
                    if best_score > 0 {
                        let best = format!("Best: {}", best_score);
                        ui.label(font, &best);
                    }
                    ui.set_cursor(0.0, vh / 2.0 + 40.0);
                    ui.label(font, "Press SPACE to retry");
                }
            }
        }

        ui.finish();
        ctx.ui_state = ui_state;
    }

    fn draw(&self, _ctx: &Context, screen: &mut Screen) {
        let ground_top = self.ground_top();

        // Background layers (back to front)
        for layer in &self.bg_layers {
            layer.draw(screen);
        }

        // Pipes (only during playing/dead)
        if self.state == GameState::Playing || self.state == GameState::Dead {
            for pipe in &self.pipes {
                let top_h = pipe.gap_y - PIPE_GAP / 2.0;
                if top_h > 0.0 {
                    screen.draw_sprite_flipped(self.pipe_tex, pipe.x, 0.0, PIPE_WIDTH, top_h);
                }
                let bot_y = pipe.gap_y + PIPE_GAP / 2.0;
                let bot_h = ground_top - bot_y;
                if bot_h > 0.0 {
                    screen.draw_sprite(self.pipe_tex, pipe.x, bot_y, PIPE_WIDTH, bot_h);
                }
            }
        }

        // Ground
        let gw = self.ground_width;
        let gh = self.ground_height;
        screen.draw_sprite(self.ground_tex, -self.ground_scroll, ground_top, gw, gh);
        screen.draw_sprite(self.ground_tex, gw - self.ground_scroll, ground_top, gw, gh);

        // Bird
        screen.draw_sprite(
            self.bird_tex,
            self.bird_x - self.bird_w / 2.0,
            self.bird_y,
            self.bird_w,
            self.bird_h,
        );
    }

    fn clear_color(&self) -> Color {
        Color::from_hex(0x4EC0CA)
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        let state_str = match self.state {
            GameState::Idle => "idle",
            GameState::Countdown => "countdown",
            GameState::Playing => "playing",
            GameState::Dead => "dead",
        };

        let pipes: Vec<serde_json::Value> = self
            .pipes
            .iter()
            .map(|p| {
                serde_json::json!({
                    "x": p.x,
                    "gap_y": p.gap_y,
                    "scored": p.scored,
                })
            })
            .collect();

        serde_json::json!({
            "state": state_str,
            "score": self.score,
            "best_score": self.best_score,
            "bird": {
                "x": self.bird_x,
                "y": self.bird_y,
                "vy": self.bird_vy,
                "width": self.bird_w,
                "height": self.bird_h,
            },
            "pipes": pipes,
            "countdown_timer": self.countdown_timer,
        })
    }

    #[cfg(feature = "vdp")]
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match method {
            "game.setBirdY" => {
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'y' parameter")?;
                self.bird_y = y as f32;
                if let Some(vy) = params.get("vy").and_then(|v| v.as_f64()) {
                    self.bird_vy = vy as f32;
                }
                Ok(serde_json::json!({"bird_y": self.bird_y, "bird_vy": self.bird_vy}))
            }
            "game.setScore" => {
                let score = params
                    .get("score")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'score' parameter")?;
                self.score = score as u32;
                Ok(serde_json::json!({"score": self.score}))
            }
            "game.setState" => {
                let state = params
                    .get("state")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'state' parameter")?;
                match state {
                    "idle" => {
                        self.state = GameState::Idle;
                        self.reset_bird();
                    }
                    "countdown" => {
                        self.reset_game();
                        self.countdown_timer = COUNTDOWN_DURATION;
                        self.state = GameState::Countdown;
                    }
                    "playing" => {
                        self.state = GameState::Playing;
                    }
                    "dead" => {
                        if self.score > self.best_score {
                            self.best_score = self.score;
                        }
                        self.state = GameState::Dead;
                    }
                    _ => return Err(format!("Unknown state: {}", state)),
                }
                Ok(serde_json::json!({"state": state}))
            }
            _ => Err(format!("Unknown method: {}", method)),
        }
    }
}

fn aabb_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    vibe2d::run::<FlappyBirdGame>("game.yaml");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // No-op on web; the real entry point is web_main below.
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    wasm_bindgen_futures::spawn_local(async {
        vibe2d::run_web::<FlappyBirdGame>("game.yaml").await;
    });
}
