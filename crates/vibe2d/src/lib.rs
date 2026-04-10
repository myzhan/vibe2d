mod config;
mod context;
mod game;
mod screen;

pub mod prelude {
    pub use crate::config::GameConfig;
    pub use crate::context::Context;
    pub use crate::game::Game;
    pub use crate::screen::Screen;
    pub use crate::{run, Color};
    pub use glam::Vec2;
    pub use vibe_input::InputState;
    pub use vibe_render::TextureId;
}

pub use config::GameConfig;
pub use context::Context;
pub use game::Game;
pub use screen::Screen;

/// RGBA color.
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub fn from_hex(hex: u32) -> Self {
        let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let b = (hex & 0xFF) as f32 / 255.0;
        Self { r, g, b, a: 1.0 }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

// ── VDP: Simulated input types ──────────────────────────────────────

#[cfg(feature = "vdp")]
enum SimulatedInput {
    // Keyboard
    KeyPress(vibe_input::KeyCode),
    KeyRelease(vibe_input::KeyCode),
    KeyTap(vibe_input::KeyCode),
    // Mouse
    MouseMove(f32, f32),
    MouseButtonPress(vibe_input::MouseButton),
    MouseButtonRelease(vibe_input::MouseButton),
    MouseButtonClick(vibe_input::MouseButton),
}

#[cfg(feature = "vdp")]
struct PendingStepInspect {
    id: serde_json::Value,
    frames: u32,
}

// ── Main entry point ────────────────────────────────────────────────

/// Main entry point. Loads config from YAML and starts the game loop.
pub fn run<G: Game + 'static>(config_path: &str) {
    tracing_subscriber::fmt::init();

    let config = GameConfig::load(config_path).expect("Failed to load game config");

    let virtual_width = config.virtual_resolution.as_ref().map_or(
        config.window.width as f32,
        |vr| vr.width as f32,
    );
    let virtual_height = config.virtual_resolution.as_ref().map_or(
        config.window.height as f32,
        |vr| vr.height as f32,
    );

    let platform_config = vibe_platform::PlatformConfig {
        window_width: config.window.width,
        window_height: config.window.height,
        window_title: config.window.title.clone(),
        vsync: config.window.vsync.unwrap_or(true),
        virtual_width,
        virtual_height,
    };

    let mut input_state = vibe_input::InputState::new();
    if let Some(ref input_cfg) = config.input {
        input_state.load_actions(&input_cfg.actions);
    }

    // Start VDP server if configured
    #[cfg(feature = "vdp")]
    let vdp_channel = if config
        .debug
        .as_ref()
        .and_then(|d| d.vdp.as_ref())
        .and_then(|v| v.enabled)
        .unwrap_or(false)
    {
        let port = config
            .debug
            .as_ref()
            .and_then(|d| d.vdp.as_ref())
            .and_then(|v| v.port)
            .unwrap_or(9229);

        let (game_channel, server_channel) = vibe_debug::create_channel();
        if let Err(e) = vibe_debug::VdpServer::start(port, server_channel) {
            tracing::error!("Failed to start VDP server: {}", e);
            None
        } else {
            Some(game_channel)
        }
    } else {
        None
    };

    let bridge = GameBridge::<G> {
        game: None,
        assets: vibe_asset::AssetManager::new(),
        audio: vibe_audio::AudioEngine::new(),
        config,
        base_path: std::path::PathBuf::from(
            std::path::Path::new(config_path)
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        ),
        virtual_width,
        virtual_height,
        pending_screenshot: None,
        #[cfg(feature = "vdp")]
        vdp: vdp_channel,
        #[cfg(feature = "vdp")]
        paused: false,
        #[cfg(feature = "vdp")]
        step_frames: 0,
        #[cfg(feature = "vdp")]
        frame_count: 0,
        #[cfg(feature = "vdp")]
        elapsed_time: 0.0,
        #[cfg(feature = "vdp")]
        last_dt: 0.0,
        #[cfg(feature = "vdp")]
        resume_next_frame: false,
        #[cfg(feature = "vdp")]
        pending_simulated: Vec::new(),
        #[cfg(feature = "vdp")]
        pending_key_auto_releases: Vec::new(),
        #[cfg(feature = "vdp")]
        pending_mouse_auto_releases: Vec::new(),
        #[cfg(feature = "vdp")]
        pending_step_inspect: None,
        #[cfg(feature = "vdp")]
        vdp_skip_render: false,
    };

    vibe_platform::run_desktop(platform_config, bridge, input_state)
        .expect("Game loop failed");
}

use std::path::PathBuf;

/// Bridges the user's Game implementation to the platform callbacks.
struct GameBridge<G: Game> {
    game: Option<G>,
    assets: vibe_asset::AssetManager,
    audio: vibe_audio::AudioEngine,
    config: GameConfig,
    base_path: PathBuf,
    virtual_width: f32,
    virtual_height: f32,
    pending_screenshot: Option<PathBuf>,

    // ── VDP fields ──
    #[cfg(feature = "vdp")]
    vdp: Option<vibe_debug::VdpChannel>,
    #[cfg(feature = "vdp")]
    paused: bool,
    #[cfg(feature = "vdp")]
    step_frames: u32,
    #[cfg(feature = "vdp")]
    frame_count: u64,
    #[cfg(feature = "vdp")]
    elapsed_time: f32,
    #[cfg(feature = "vdp")]
    last_dt: f32,
    #[cfg(feature = "vdp")]
    resume_next_frame: bool,
    #[cfg(feature = "vdp")]
    pending_simulated: Vec<SimulatedInput>,
    #[cfg(feature = "vdp")]
    pending_key_auto_releases: Vec<vibe_input::KeyCode>,
    #[cfg(feature = "vdp")]
    pending_mouse_auto_releases: Vec<vibe_input::MouseButton>,
    #[cfg(feature = "vdp")]
    pending_step_inspect: Option<PendingStepInspect>,
    #[cfg(feature = "vdp")]
    vdp_skip_render: bool,
}

impl<G: Game> vibe_platform::PlatformCallbacks for GameBridge<G> {
    fn on_init(&mut self, renderer: &vibe_render::Renderer) {
        if let Some(ref tex_configs) = self.config.assets.as_ref().and_then(|a| a.textures.as_ref()) {
            if let Err(e) = self.assets.load_textures(renderer, &self.base_path, tex_configs) {
                tracing::error!("Failed to load textures: {}", e);
            }
        }

        if let Some(ref font_configs) = self.config.assets.as_ref().and_then(|a| a.fonts.as_ref()) {
            if let Err(e) = self.assets.load_fonts(renderer, &self.base_path, font_configs) {
                tracing::error!("Failed to load fonts: {}", e);
            }
        }

        if let Some(ref audio_configs) = self.config.assets.as_ref().and_then(|a| a.audio.as_ref()) {
            if let Err(e) = self.audio.load_sounds(&self.base_path, audio_configs) {
                tracing::error!("Failed to load audio: {}", e);
            }
        }

        let mut ctx = Context {
            assets: std::mem::take(&mut self.assets),
            audio: std::mem::take(&mut self.audio),
            virtual_width: self.virtual_width,
            virtual_height: self.virtual_height,
        };

        self.game = Some(G::new(&mut ctx));

        self.assets = ctx.assets;
        self.audio = ctx.audio;
    }

    fn on_input_event(&mut self, _input: &mut vibe_input::InputState) {}

    #[cfg(feature = "vdp")]
    fn should_suppress_input(&self) -> bool {
        self.vdp
            .as_ref()
            .map_or(false, |vdp| vdp.is_client_connected())
    }

    fn on_update(&mut self, dt: f32, input: &mut vibe_input::InputState) {
        // ── VDP: auto-releases, request processing, pause/step logic ──
        #[cfg(feature = "vdp")]
        let (will_update, effective_dt) = {
            // 1. Clean up previous frame's tap/click auto-releases
            for key in self.pending_key_auto_releases.drain(..) {
                input.on_key_released(key);
            }
            for btn in self.pending_mouse_auto_releases.drain(..) {
                input.on_mouse_button_released(btn);
            }

            // 2. Process VDP requests (may queue simulated inputs, modify paused/step_frames)
            self.process_vdp_requests();

            // 2.5 Fast-forward: stepAndInspect tight loop (skip rendering)
            if let Some(pending) = self.pending_step_inspect.take() {
                // Inject all pending simulated inputs
                self.inject_simulated_inputs(input);

                let dt_step = 1.0 / 60.0;
                for i in 0..pending.frames {
                    if i > 0 {
                        // Release tap keys from previous iteration
                        for key in self.pending_key_auto_releases.drain(..) {
                            input.on_key_released(key);
                        }
                        for btn in self.pending_mouse_auto_releases.drain(..) {
                            input.on_mouse_button_released(btn);
                        }
                        input.begin_frame();
                    }

                    if let Some(game) = &mut self.game {
                        let mut ctx = Context {
                            assets: std::mem::take(&mut self.assets),
                            audio: std::mem::take(&mut self.audio),
                            virtual_width: self.virtual_width,
                            virtual_height: self.virtual_height,
                        };
                        game.update(&mut ctx, dt_step, input);
                        self.assets = ctx.assets;
                        self.audio = ctx.audio;
                    }
                    self.frame_count += 1;
                    self.elapsed_time += dt_step;
                }

                // Send inspect result as response
                let result = if let Some(game) = &self.game {
                    game.inspect()
                } else {
                    serde_json::Value::Null
                };
                if let Some(vdp) = &self.vdp {
                    let _ = vdp.sender.send(vibe_debug::VdpResponse::success(pending.id, result));
                }

                self.last_dt = dt;
                (false, 0.0) // skip normal update
            } else {

            // 3. Determine if game.update will run this frame
            let will_update = !self.paused || self.step_frames > 0;

            // 4. If updating, inject simulated inputs
            if will_update {
                for sim in self.pending_simulated.drain(..) {
                    match sim {
                        SimulatedInput::KeyPress(k) => input.on_key_pressed(k),
                        SimulatedInput::KeyRelease(k) => input.on_key_released(k),
                        SimulatedInput::KeyTap(k) => {
                            input.on_key_pressed(k);
                            self.pending_key_auto_releases.push(k);
                        }
                        SimulatedInput::MouseMove(x, y) => input.on_mouse_moved(x, y),
                        SimulatedInput::MouseButtonPress(b) => input.on_mouse_button_pressed(b),
                        SimulatedInput::MouseButtonRelease(b) => input.on_mouse_button_released(b),
                        SimulatedInput::MouseButtonClick(b) => {
                            input.on_mouse_button_pressed(b);
                            self.pending_mouse_auto_releases.push(b);
                        }
                    }
                }
            }

            // 5. Compute effective dt
            let effective_dt = if will_update {
                if self.paused {
                    self.step_frames -= 1;
                    1.0 / 60.0
                } else if self.resume_next_frame {
                    self.resume_next_frame = false;
                    1.0 / 60.0
                } else {
                    dt
                }
            } else {
                0.0
            };

            self.last_dt = dt;

            (will_update, effective_dt)
            } // else (normal path)
        };

        #[cfg(not(feature = "vdp"))]
        let (will_update, effective_dt) = (true, dt);

        if will_update {
            if let Some(game) = &mut self.game {
                let mut ctx = Context {
                    assets: std::mem::take(&mut self.assets),
                    audio: std::mem::take(&mut self.audio),
                    virtual_width: self.virtual_width,
                    virtual_height: self.virtual_height,
                };
                game.update(&mut ctx, effective_dt, input);
                self.assets = ctx.assets;
                self.audio = ctx.audio;
            }

            #[cfg(feature = "vdp")]
            {
                self.frame_count += 1;
                self.elapsed_time += effective_dt;
            }
        }
    }

    fn on_render(&mut self, renderer: &mut vibe_render::Renderer) {
        if let Some(path) = self.pending_screenshot.take() {
            renderer.request_screenshot(path);
        }

        if let Some(game) = &mut self.game {
            let ctx = Context {
                assets: std::mem::take(&mut self.assets),
                audio: std::mem::take(&mut self.audio),
                virtual_width: self.virtual_width,
                virtual_height: self.virtual_height,
            };
            let mut screen = Screen::new(renderer, self.virtual_width, self.virtual_height);
            game.draw(&ctx, &mut screen);
            self.assets = ctx.assets;
            self.audio = ctx.audio;
        }
    }

    fn clear_color(&self) -> [f32; 4] {
        if let Some(game) = &self.game {
            game.clear_color().to_array()
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }

    fn get_textures(&self) -> Vec<&vibe_render::Texture> {
        self.assets.all_textures()
    }

    #[cfg(feature = "vdp")]
    fn should_render(&self) -> bool {
        !self.vdp_skip_render
    }
}

// ── VDP request handling ────────────────────────────────────────────

#[cfg(feature = "vdp")]
impl<G: Game> GameBridge<G> {
    fn process_vdp_requests(&mut self) {
        let vdp = match &self.vdp {
            Some(v) => v,
            None => return,
        };

        let requests: Vec<_> = std::iter::from_fn(|| vdp.receiver.try_recv().ok()).collect();

        for req in requests {
            // stepAndInspect is deferred — response sent after tight loop in on_update
            if req.method == "engine.stepAndInspect" {
                if !self.paused {
                    if let Some(vdp) = &self.vdp {
                        let _ = vdp.sender.send(vibe_debug::VdpResponse::error(
                            req.id.clone(), -32000, "Game is not paused",
                        ));
                    }
                    continue;
                }
                let frames = req.params.get("frames")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;
                // Parse optional embedded inputs
                if let Some(inputs) = req.params.get("inputs").and_then(|v| v.as_array()) {
                    for input_val in inputs {
                        self.parse_and_queue_input(input_val);
                    }
                }
                self.pending_step_inspect = Some(PendingStepInspect {
                    id: req.id.clone(),
                    frames,
                });
                continue;
            }

            let response = self.handle_vdp_request(&req);
            if let Some(vdp) = &self.vdp {
                let _ = vdp.sender.send(response);
            }
        }
    }

    fn handle_vdp_request(&mut self, req: &vibe_debug::VdpRequest) -> vibe_debug::VdpResponse {
        match req.method.as_str() {
            // ── Engine built-in methods ──
            "engine.info" => {
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "engine": "vibe2d",
                        "version": env!("CARGO_PKG_VERSION"),
                        "virtual_width": self.virtual_width,
                        "virtual_height": self.virtual_height,
                    }),
                )
            }

            "engine.pause" => {
                self.paused = true;
                self.step_frames = 0;
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "paused": true,
                        "frame_count": self.frame_count,
                    }),
                )
            }

            "engine.resume" => {
                self.paused = false;
                self.step_frames = 0;
                self.resume_next_frame = true;
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "paused": false,
                        "frame_count": self.frame_count,
                    }),
                )
            }

            "engine.step" => {
                if !self.paused {
                    return vibe_debug::VdpResponse::error(
                        req.id.clone(), -32000, "Game is not paused",
                    );
                }
                let frames = req.params.get("frames")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1) as u32;
                self.step_frames = frames;
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "frames": frames,
                        "frame_count": self.frame_count,
                    }),
                )
            }

            "engine.getTime" => {
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "frame_count": self.frame_count,
                        "elapsed_time": self.elapsed_time,
                        "dt": self.last_dt,
                        "paused": self.paused,
                        "step_frames_remaining": self.step_frames,
                    }),
                )
            }

            "engine.simulateInput" => {
                self.handle_simulate_input(req)
            }

            "engine.simulateInputBatch" => {
                let inputs = match req.params.get("inputs").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => return vibe_debug::VdpResponse::error(
                        req.id.clone(), -32602, "Missing 'inputs' array parameter",
                    ),
                };
                let count = inputs.len();
                for input_val in inputs {
                    self.parse_and_queue_input(input_val);
                }
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": count }),
                )
            }

            "engine.setRendering" => {
                let enabled = req.params.get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.vdp_skip_render = !enabled;
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "rendering": enabled }),
                )
            }

            // ── Game inspection ──
            "game.inspect" => {
                if let Some(game) = &self.game {
                    vibe_debug::VdpResponse::success(req.id.clone(), game.inspect())
                } else {
                    vibe_debug::VdpResponse::error(req.id.clone(), -32000, "Game not initialized")
                }
            }

            "game.screenshot" => {
                let path = req.params.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("screenshot.png")
                    .to_string();
                self.pending_screenshot = Some(PathBuf::from(&path));
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "path": path, "status": "queued" }),
                )
            }

            // ── Game-specific methods ──
            _ => {
                if let Some(game) = &mut self.game {
                    match game.handle_vdp(&req.method, &req.params) {
                        Ok(result) => vibe_debug::VdpResponse::success(req.id.clone(), result),
                        Err(msg) => vibe_debug::VdpResponse::error(req.id.clone(), -32000, msg),
                    }
                } else {
                    vibe_debug::VdpResponse::method_not_found(req.id.clone(), &req.method)
                }
            }
        }
    }

    fn handle_simulate_input(&mut self, req: &vibe_debug::VdpRequest) -> vibe_debug::VdpResponse {
        let device = req.params.get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("keyboard");
        let action = match req.params.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return vibe_debug::VdpResponse::error(
                req.id.clone(), -32602, "Missing 'action' parameter",
            ),
        };

        match device {
            "keyboard" => {
                let key_name = match req.params.get("key").and_then(|v| v.as_str()) {
                    Some(k) => k,
                    None => return vibe_debug::VdpResponse::error(
                        req.id.clone(), -32602, "Missing 'key' parameter",
                    ),
                };
                let keycode = match vibe_input::string_to_keycode(key_name) {
                    Some(k) => k,
                    None => return vibe_debug::VdpResponse::error(
                        req.id.clone(), -32602, format!("Unknown key: {}", key_name),
                    ),
                };
                let sim = match action {
                    "press" => SimulatedInput::KeyPress(keycode),
                    "release" => SimulatedInput::KeyRelease(keycode),
                    "tap" => SimulatedInput::KeyTap(keycode),
                    _ => return vibe_debug::VdpResponse::error(
                        req.id.clone(), -32602,
                        format!("Unknown keyboard action: {}", action),
                    ),
                };
                self.pending_simulated.push(sim);
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "device": "keyboard", "action": action,
                        "key": key_name, "queued": true,
                    }),
                )
            }

            "mouse" => {
                match action {
                    "move" => {
                        let x = match req.params.get("x").and_then(|v| v.as_f64()) {
                            Some(v) => v as f32,
                            None => return vibe_debug::VdpResponse::error(
                                req.id.clone(), -32602, "Missing 'x' parameter",
                            ),
                        };
                        let y = match req.params.get("y").and_then(|v| v.as_f64()) {
                            Some(v) => v as f32,
                            None => return vibe_debug::VdpResponse::error(
                                req.id.clone(), -32602, "Missing 'y' parameter",
                            ),
                        };
                        self.pending_simulated.push(SimulatedInput::MouseMove(x, y));
                        vibe_debug::VdpResponse::success(
                            req.id.clone(),
                            serde_json::json!({
                                "device": "mouse", "action": "move",
                                "x": x, "y": y, "queued": true,
                            }),
                        )
                    }
                    "press" | "release" | "click" => {
                        let btn_name = match req.params.get("button").and_then(|v| v.as_str()) {
                            Some(b) => b,
                            None => return vibe_debug::VdpResponse::error(
                                req.id.clone(), -32602, "Missing 'button' parameter",
                            ),
                        };
                        let button = match vibe_input::string_to_mouse_button(btn_name) {
                            Some(b) => b,
                            None => return vibe_debug::VdpResponse::error(
                                req.id.clone(), -32602,
                                format!("Unknown mouse button: {}", btn_name),
                            ),
                        };
                        let sim = match action {
                            "press" => SimulatedInput::MouseButtonPress(button),
                            "release" => SimulatedInput::MouseButtonRelease(button),
                            "click" => SimulatedInput::MouseButtonClick(button),
                            _ => unreachable!(),
                        };
                        self.pending_simulated.push(sim);
                        vibe_debug::VdpResponse::success(
                            req.id.clone(),
                            serde_json::json!({
                                "device": "mouse", "action": action,
                                "button": btn_name, "queued": true,
                            }),
                        )
                    }
                    _ => vibe_debug::VdpResponse::error(
                        req.id.clone(), -32602,
                        format!("Unknown mouse action: {}", action),
                    ),
                }
            }

            "gamepad" => {
                vibe_debug::VdpResponse::error(
                    req.id.clone(), -32000,
                    "Gamepad simulation not yet supported",
                )
            }

            _ => {
                vibe_debug::VdpResponse::error(
                    req.id.clone(), -32602,
                    format!("Unknown device: {}", device),
                )
            }
        }
    }

    /// Inject all pending simulated inputs into the InputState.
    fn inject_simulated_inputs(&mut self, input: &mut vibe_input::InputState) {
        for sim in self.pending_simulated.drain(..) {
            match sim {
                SimulatedInput::KeyPress(k) => input.on_key_pressed(k),
                SimulatedInput::KeyRelease(k) => input.on_key_released(k),
                SimulatedInput::KeyTap(k) => {
                    input.on_key_pressed(k);
                    self.pending_key_auto_releases.push(k);
                }
                SimulatedInput::MouseMove(x, y) => input.on_mouse_moved(x, y),
                SimulatedInput::MouseButtonPress(b) => input.on_mouse_button_pressed(b),
                SimulatedInput::MouseButtonRelease(b) => input.on_mouse_button_released(b),
                SimulatedInput::MouseButtonClick(b) => {
                    input.on_mouse_button_pressed(b);
                    self.pending_mouse_auto_releases.push(b);
                }
            }
        }
    }

    /// Parse a single input JSON object and queue it as a SimulatedInput.
    fn parse_and_queue_input(&mut self, val: &serde_json::Value) {
        let device = val.get("device").and_then(|v| v.as_str()).unwrap_or("keyboard");
        let action = match val.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return,
        };
        match device {
            "keyboard" => {
                let key_name = match val.get("key").and_then(|v| v.as_str()) {
                    Some(k) => k,
                    None => return,
                };
                let keycode = match vibe_input::string_to_keycode(key_name) {
                    Some(k) => k,
                    None => return,
                };
                let sim = match action {
                    "press" => SimulatedInput::KeyPress(keycode),
                    "release" => SimulatedInput::KeyRelease(keycode),
                    "tap" => SimulatedInput::KeyTap(keycode),
                    _ => return,
                };
                self.pending_simulated.push(sim);
            }
            "mouse" => {
                match action {
                    "move" => {
                        if let (Some(x), Some(y)) = (
                            val.get("x").and_then(|v| v.as_f64()),
                            val.get("y").and_then(|v| v.as_f64()),
                        ) {
                            self.pending_simulated.push(SimulatedInput::MouseMove(x as f32, y as f32));
                        }
                    }
                    "press" | "release" | "click" => {
                        if let Some(btn_name) = val.get("button").and_then(|v| v.as_str()) {
                            if let Some(button) = vibe_input::string_to_mouse_button(btn_name) {
                                let sim = match action {
                                    "press" => SimulatedInput::MouseButtonPress(button),
                                    "release" => SimulatedInput::MouseButtonRelease(button),
                                    "click" => SimulatedInput::MouseButtonClick(button),
                                    _ => return,
                                };
                                self.pending_simulated.push(sim);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
