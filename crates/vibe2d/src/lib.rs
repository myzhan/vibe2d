mod config;
mod context;
mod game;
mod screen;

pub mod prelude {
    pub use crate::Color;
    pub use crate::config::GameConfig;
    pub use crate::context::Context;
    pub use crate::game::Game;
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::run;
    #[cfg(target_arch = "wasm32")]
    pub use crate::run_web;
    pub use crate::screen::Screen;
    pub use glam::Vec2;
    pub use vibe_input::InputState;
    pub use vibe_render::{Renderer, TextureId};
    pub use vibe_ui::{
        Anchor, ButtonStyle, LayoutDirection, PanelStyle, ScrollListStyle, Style, TextInputStyle,
        UiColor, UiContext, UiOutput, WidgetId,
    };
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
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

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

/// Main entry point (desktop). Loads config from YAML and starts the game loop.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<G: Game + 'static>(config_path: &str) {
    tracing_subscriber::fmt::init();

    // Resolve config path: falls back to CARGO_MANIFEST_DIR when running
    // from the workspace root (e.g. `cargo run -p mari0`).
    let resolved_config_path = GameConfig::resolve_config_path(config_path);
    let config =
        GameConfig::load_from_path(&resolved_config_path).expect("Failed to load game config");

    let virtual_width = config
        .virtual_resolution
        .as_ref()
        .map_or(config.window.width as f32, |vr| vr.width as f32);
    let virtual_height = config
        .virtual_resolution
        .as_ref()
        .map_or(config.window.height as f32, |vr| vr.height as f32);

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
        ui_state: vibe_ui::UiState::new(),
        config,
        base_path: resolved_config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf(),
        virtual_width,
        virtual_height,
        pending_screenshot: None,
        pending_text_prep: Vec::new(),
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

    vibe_platform::run_desktop(platform_config, bridge, input_state).expect("Game loop failed");
}

// ── Web entry point ─────────────────────────────────────────────────

/// Main entry point (WASM). Fetches config and assets via HTTP, then starts the game loop.
#[cfg(target_arch = "wasm32")]
pub async fn run_web<G: Game + 'static>(config_yaml_url: &str) {
    console_error_panic_hook::set_once();
    // Minimal tracing setup for web (logs to browser console)
    tracing_subscriber::fmt()
        .with_writer(
            tracing_subscriber::fmt::writer::MakeWriterExt::with_max_level(
                std::io::stderr,
                tracing::Level::INFO,
            ),
        )
        .without_time()
        .init();

    // 1. Fetch game.yaml
    let config_bytes = vibe_platform::fetch_file(config_yaml_url)
        .await
        .expect("Failed to fetch game.yaml");
    let config = GameConfig::load_from_bytes(&config_bytes).expect("Failed to parse game config");

    let virtual_width = config
        .virtual_resolution
        .as_ref()
        .map_or(config.window.width as f32, |vr| vr.width as f32);
    let virtual_height = config
        .virtual_resolution
        .as_ref()
        .map_or(config.window.height as f32, |vr| vr.height as f32);

    // 2. Fetch all assets in parallel
    let bundle = fetch_all_assets(&config).await;

    // 3. Create input state + action mappings
    let mut input_state = vibe_input::InputState::new();
    if let Some(ref input_cfg) = config.input {
        input_state.load_actions(&input_cfg.actions);
    }

    let platform_config = vibe_platform::PlatformConfig {
        window_width: config.window.width,
        window_height: config.window.height,
        window_title: config.window.title.clone(),
        vsync: config.window.vsync.unwrap_or(true),
        virtual_width,
        virtual_height,
    };

    let bridge = GameBridge::<G> {
        game: None,
        assets: vibe_asset::AssetManager::new(),
        audio: vibe_audio::AudioEngine::new(),
        ui_state: vibe_ui::UiState::new(),
        config,
        virtual_width,
        virtual_height,
        pending_text_prep: Vec::new(),
        asset_bundle: Some(bundle),
    };

    vibe_platform::run_web(platform_config, bridge, input_state).expect("Game loop failed");
}

#[cfg(target_arch = "wasm32")]
async fn fetch_all_assets(config: &GameConfig) -> vibe_asset::AssetBundle {
    use std::collections::HashMap;

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();

    // Collect all asset paths
    let mut paths: Vec<String> = Vec::new();
    if let Some(ref assets) = config.assets {
        if let Some(ref textures) = assets.textures {
            for rel_path in textures.values() {
                paths.push(rel_path.clone());
            }
        }
        if let Some(ref fonts) = assets.fonts {
            for config_str in fonts.values() {
                // Font format is "path:size" — extract path part
                if let Some((rel_path, _)) = config_str.rsplit_once(':') {
                    paths.push(rel_path.to_string());
                }
            }
        }
        if let Some(ref audio) = assets.audio {
            for rel_path in audio.values() {
                paths.push(rel_path.clone());
            }
        }
    }

    // Fetch all assets via the platform I/O layer
    for path in paths {
        match vibe_platform::fetch_file(&path).await {
            Ok(bytes) => {
                files.insert(path, bytes);
            }
            Err(e) => {
                tracing::error!("Failed to fetch asset '{}': {}", path, e);
            }
        }
    }

    vibe_asset::AssetBundle { files }
}

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// Drains [`Context::pending_text_prep`] and uploads the requested glyphs
/// into each font's atlas via the renderer.
///
/// Called both at the end of `on_init` (so prep done in `Game::new` lands
/// in time for the very first frame) and at the start of `on_render`
/// (the normal per-frame path).
fn flush_text_prep(ctx: &mut Context, renderer: &vibe_render::Renderer) {
    for (font_name, text) in ctx.pending_text_prep.drain(..) {
        if let Err(e) = ctx.assets.prepare_text(renderer, &font_name, &text) {
            tracing::warn!("prepare_text(font={}) failed: {}", font_name, e);
        }
    }
}

/// Bridges the user's Game implementation to the platform callbacks.
struct GameBridge<G: Game> {
    game: Option<G>,
    assets: vibe_asset::AssetManager,
    audio: vibe_audio::AudioEngine,
    ui_state: vibe_ui::UiState,
    config: GameConfig,
    #[cfg(not(target_arch = "wasm32"))]
    base_path: PathBuf,
    #[cfg(target_arch = "wasm32")]
    asset_bundle: Option<vibe_asset::AssetBundle>,
    virtual_width: f32,
    virtual_height: f32,
    #[cfg(not(target_arch = "wasm32"))]
    pending_screenshot: Option<PathBuf>,

    /// Carries `(font_name, text)` glyph-prep requests from `update` (where
    /// the renderer is not borrowed) into `on_render` (where it is). See
    /// [`Context::prepare_text`] and [`flush_text_prep`].
    pending_text_prep: Vec<(String, String)>,

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

impl<G: Game> GameBridge<G> {
    /// Build or take the asset bundle.
    /// - Desktop: reads all asset files from the filesystem into memory.
    /// - Web: takes the pre-fetched bundle that was populated via HTTP.
    fn build_asset_bundle(&mut self) -> vibe_asset::AssetBundle {
        #[cfg(target_arch = "wasm32")]
        {
            self.asset_bundle
                .take()
                .unwrap_or_else(|| vibe_asset::AssetBundle {
                    files: std::collections::HashMap::new(),
                })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::collections::HashMap;
            let mut files: HashMap<String, Vec<u8>> = HashMap::new();

            if let Some(ref assets) = self.config.assets {
                // Textures
                if let Some(ref textures) = assets.textures {
                    for rel_path in textures.values() {
                        let full_path = self.base_path.join(rel_path);
                        match vibe_platform::read_file(&full_path.to_string_lossy()) {
                            Ok(bytes) => {
                                files.insert(rel_path.clone(), bytes);
                            }
                            Err(e) => {
                                tracing::error!("Failed to read {:?}: {}", full_path, e);
                            }
                        }
                    }
                }
                // Fonts (path is "rel_path:size", extract just the path portion)
                if let Some(ref fonts) = assets.fonts {
                    for config_str in fonts.values() {
                        if let Some((rel_path, _)) = config_str.rsplit_once(':') {
                            let full_path = self.base_path.join(rel_path);
                            match vibe_platform::read_file(&full_path.to_string_lossy()) {
                                Ok(bytes) => {
                                    files.insert(rel_path.to_string(), bytes);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to read {:?}: {}", full_path, e);
                                }
                            }
                        }
                    }
                }
                // Audio
                if let Some(ref audio) = assets.audio {
                    for rel_path in audio.values() {
                        let full_path = self.base_path.join(rel_path);
                        match vibe_platform::read_file(&full_path.to_string_lossy()) {
                            Ok(bytes) => {
                                files.insert(rel_path.clone(), bytes);
                            }
                            Err(e) => {
                                tracing::error!("Failed to read {:?}: {}", full_path, e);
                            }
                        }
                    }
                }
            }

            vibe_asset::AssetBundle { files }
        }
    }
}

impl<G: Game> vibe_platform::PlatformCallbacks for GameBridge<G> {
    fn on_init(&mut self, renderer: &vibe_render::Renderer) {
        // Build or take the asset bundle (desktop reads from fs, web already has it)
        let bundle = self.build_asset_bundle();

        // Load all assets from the bundle (unified path for all platforms)
        if let Some(tex_configs) = self
            .config
            .assets
            .as_ref()
            .and_then(|a| a.textures.as_ref())
            && let Err(e) = self
                .assets
                .load_textures_from_bundle(renderer, tex_configs, &bundle)
        {
            tracing::error!("Failed to load textures: {}", e);
        }

        if let Some(font_configs) = self.config.assets.as_ref().and_then(|a| a.fonts.as_ref())
            && let Err(e) = self
                .assets
                .load_fonts_from_bundle(renderer, font_configs, &bundle)
        {
            tracing::error!("Failed to load fonts: {}", e);
        }

        if let Some(audio_configs) = self.config.assets.as_ref().and_then(|a| a.audio.as_ref())
            && let Err(e) = self.audio.load_sounds_from_bundle(audio_configs, &bundle)
        {
            tracing::error!("Failed to load audio: {}", e);
        }

        // Give the UI system a chance to allocate its private textures
        // (e.g. the 1×1 white pixel for filled rectangles) via the
        // public renderer API. UI sits on the same dynamic-texture
        // pipeline as game code — see `Renderer::create_*_texture` and
        // `AssetManager::register_texture`.
        self.ui_state.init(renderer, &mut self.assets);

        let mut ctx = Context {
            assets: std::mem::take(&mut self.assets),
            audio: std::mem::take(&mut self.audio),
            ui_state: std::mem::take(&mut self.ui_state),
            virtual_width: self.virtual_width,
            virtual_height: self.virtual_height,
            pending_text_prep: Vec::new(),
        };

        // Hand the renderer to `Game::new` so it can build its own
        // procedural textures (circles, white pixels, gradients, …)
        // and register them via `ctx.assets.register_texture`. The
        // engine no longer pre-creates any textures of its own.
        self.game = Some(G::new(&mut ctx, renderer));

        // Flush any text prep requested during Game::new immediately, since
        // the renderer is in scope right here. (Most games queue all their
        // text prep from `update`, but a few may pre-warm in `new`.)
        flush_text_prep(&mut ctx, renderer);

        self.assets = ctx.assets;
        self.audio = ctx.audio;
        self.ui_state = ctx.ui_state;
    }

    fn on_input_event(&mut self, _input: &mut vibe_input::InputState) {}

    #[cfg(feature = "vdp")]
    fn should_suppress_input(&self) -> bool {
        self.vdp
            .as_ref()
            .is_some_and(|vdp| vdp.is_client_connected())
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
                            ui_state: std::mem::take(&mut self.ui_state),
                            virtual_width: self.virtual_width,
                            virtual_height: self.virtual_height,
                            pending_text_prep: std::mem::take(&mut self.pending_text_prep),
                        };
                        game.update(&mut ctx, dt_step, input);
                        // Carry queued text prep over to the render phase
                        // (we don't have a Renderer here in `on_update`).
                        self.pending_text_prep = ctx.pending_text_prep;
                        self.assets = ctx.assets;
                        self.audio = ctx.audio;
                        self.ui_state = ctx.ui_state;
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
                    let _ = vdp
                        .sender
                        .send(vibe_debug::VdpResponse::success(pending.id, result));
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
                            SimulatedInput::MouseButtonRelease(b) => {
                                input.on_mouse_button_released(b)
                            }
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
            self.ui_state.update_time(effective_dt as f64);

            if let Some(game) = &mut self.game {
                let mut ctx = Context {
                    assets: std::mem::take(&mut self.assets),
                    audio: std::mem::take(&mut self.audio),
                    ui_state: std::mem::take(&mut self.ui_state),
                    virtual_width: self.virtual_width,
                    virtual_height: self.virtual_height,
                    pending_text_prep: std::mem::take(&mut self.pending_text_prep),
                };
                game.update(&mut ctx, effective_dt, input);
                game.update_ui(&mut ctx, input);
                // Carry queued text prep over to the render phase.
                self.pending_text_prep = ctx.pending_text_prep;
                self.assets = ctx.assets;
                self.audio = ctx.audio;
                self.ui_state = ctx.ui_state;
            }

            #[cfg(feature = "vdp")]
            {
                self.frame_count += 1;
                self.elapsed_time += effective_dt;
            }
        }
    }

    fn on_render(&mut self, renderer: &mut vibe_render::Renderer) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.pending_screenshot.take() {
            renderer.request_screenshot(path);
        }

        if let Some(game) = &self.game {
            let mut ctx = Context {
                assets: std::mem::take(&mut self.assets),
                audio: std::mem::take(&mut self.audio),
                ui_state: std::mem::take(&mut self.ui_state),
                virtual_width: self.virtual_width,
                virtual_height: self.virtual_height,
                pending_text_prep: std::mem::take(&mut self.pending_text_prep),
            };

            // Flush any pending font glyph preparation **before** drawing,
            // so text laid out this frame finds its glyphs in the atlas.
            flush_text_prep(&mut ctx, renderer);

            let mut screen = Screen::new(renderer, self.virtual_width, self.virtual_height);
            game.draw(&ctx, &mut screen);

            // Replay cached UI draw commands on top of game rendering
            for cmd in &ctx.ui_state.cached_draw_commands {
                renderer.draw_sprite(*cmd);
            }

            self.assets = ctx.assets;
            self.audio = ctx.audio;
            self.ui_state = ctx.ui_state;
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
                            req.id.clone(),
                            -32000,
                            "Game is not paused",
                        ));
                    }
                    continue;
                }
                let frames = req
                    .params
                    .get("frames")
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
            "engine.info" => vibe_debug::VdpResponse::success(
                req.id.clone(),
                serde_json::json!({
                    "engine": "vibe2d",
                    "version": env!("CARGO_PKG_VERSION"),
                    "virtual_width": self.virtual_width,
                    "virtual_height": self.virtual_height,
                }),
            ),

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
                        req.id.clone(),
                        -32000,
                        "Game is not paused",
                    );
                }
                let frames = req
                    .params
                    .get("frames")
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

            "engine.getTime" => vibe_debug::VdpResponse::success(
                req.id.clone(),
                serde_json::json!({
                    "frame_count": self.frame_count,
                    "elapsed_time": self.elapsed_time,
                    "dt": self.last_dt,
                    "paused": self.paused,
                    "step_frames_remaining": self.step_frames,
                }),
            ),

            "engine.simulateInput" => self.handle_simulate_input(req),

            "engine.simulateInputBatch" => {
                let inputs = match req.params.get("inputs").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'inputs' array parameter",
                        );
                    }
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
                let enabled = req
                    .params
                    .get("enabled")
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
                let path = req
                    .params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("screenshot.png")
                    .to_string();
                self.pending_screenshot = Some(PathBuf::from(&path));
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "path": path, "status": "queued" }),
                )
            }

            // ── UI methods ──
            "ui.listWidgets" => {
                let widgets = &self.ui_state.last_frame_widgets;
                let json_widgets: Vec<serde_json::Value> = widgets
                    .iter()
                    .map(|w| serde_json::to_value(w).unwrap_or(serde_json::Value::Null))
                    .collect();
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "widgets": json_widgets }),
                )
            }

            "ui.click" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                self.ui_state.push_vdp_action(vibe_ui::VdpUiAction::Click {
                    id: vibe_ui::WidgetId::new(widget_id),
                });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "click", "id": widget_id }),
                )
            }

            "ui.setText" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                let text = match req.params.get("text").and_then(|v| v.as_str()) {
                    Some(t) => t.to_string(),
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'text' parameter",
                        );
                    }
                };
                self.ui_state
                    .push_vdp_action(vibe_ui::VdpUiAction::SetText {
                        id: vibe_ui::WidgetId::new(widget_id),
                        text: text.clone(),
                    });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "setText", "id": widget_id, "text": text }),
                )
            }

            "ui.submit" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                self.ui_state.push_vdp_action(vibe_ui::VdpUiAction::Submit {
                    id: vibe_ui::WidgetId::new(widget_id),
                });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "submit", "id": widget_id }),
                )
            }

            "ui.setFocus" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                self.ui_state
                    .push_vdp_action(vibe_ui::VdpUiAction::SetFocus {
                        id: vibe_ui::WidgetId::new(widget_id),
                    });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "setFocus", "id": widget_id }),
                )
            }

            "ui.clearFocus" => {
                self.ui_state
                    .push_vdp_action(vibe_ui::VdpUiAction::ClearFocus);
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "clearFocus" }),
                )
            }

            "ui.scroll" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                let offset = req
                    .params
                    .get("offset")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                self.ui_state.push_vdp_action(vibe_ui::VdpUiAction::Scroll {
                    id: vibe_ui::WidgetId::new(widget_id),
                    offset,
                });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "scroll", "id": widget_id, "offset": offset }),
                )
            }

            "ui.scrollHorizontal" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                let offset = req
                    .params
                    .get("offset")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                self.ui_state
                    .push_vdp_action(vibe_ui::VdpUiAction::ScrollHorizontal {
                        id: vibe_ui::WidgetId::new(widget_id),
                        offset,
                    });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "scrollHorizontal", "id": widget_id, "offset": offset }),
                )
            }

            "ui.scrollToBottom" => {
                let widget_id = match req.params.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'id' parameter",
                        );
                    }
                };
                self.ui_state
                    .push_vdp_action(vibe_ui::VdpUiAction::ScrollToBottom {
                        id: vibe_ui::WidgetId::new(widget_id),
                    });
                vibe_debug::VdpResponse::success(
                    req.id.clone(),
                    serde_json::json!({ "queued": true, "action": "scrollToBottom", "id": widget_id }),
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
        let device = req
            .params
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("keyboard");
        let action = match req.params.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                return vibe_debug::VdpResponse::error(
                    req.id.clone(),
                    -32602,
                    "Missing 'action' parameter",
                );
            }
        };

        match device {
            "keyboard" => {
                let key_name = match req.params.get("key").and_then(|v| v.as_str()) {
                    Some(k) => k,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            "Missing 'key' parameter",
                        );
                    }
                };
                let keycode = match vibe_input::string_to_keycode(key_name) {
                    Some(k) => k,
                    None => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            format!("Unknown key: {}", key_name),
                        );
                    }
                };
                let sim = match action {
                    "press" => SimulatedInput::KeyPress(keycode),
                    "release" => SimulatedInput::KeyRelease(keycode),
                    "tap" => SimulatedInput::KeyTap(keycode),
                    _ => {
                        return vibe_debug::VdpResponse::error(
                            req.id.clone(),
                            -32602,
                            format!("Unknown keyboard action: {}", action),
                        );
                    }
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

            "mouse" => match action {
                "move" => {
                    let x = match req.params.get("x").and_then(|v| v.as_f64()) {
                        Some(v) => v as f32,
                        None => {
                            return vibe_debug::VdpResponse::error(
                                req.id.clone(),
                                -32602,
                                "Missing 'x' parameter",
                            );
                        }
                    };
                    let y = match req.params.get("y").and_then(|v| v.as_f64()) {
                        Some(v) => v as f32,
                        None => {
                            return vibe_debug::VdpResponse::error(
                                req.id.clone(),
                                -32602,
                                "Missing 'y' parameter",
                            );
                        }
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
                        None => {
                            return vibe_debug::VdpResponse::error(
                                req.id.clone(),
                                -32602,
                                "Missing 'button' parameter",
                            );
                        }
                    };
                    let button = match vibe_input::string_to_mouse_button(btn_name) {
                        Some(b) => b,
                        None => {
                            return vibe_debug::VdpResponse::error(
                                req.id.clone(),
                                -32602,
                                format!("Unknown mouse button: {}", btn_name),
                            );
                        }
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
                    req.id.clone(),
                    -32602,
                    format!("Unknown mouse action: {}", action),
                ),
            },

            "gamepad" => vibe_debug::VdpResponse::error(
                req.id.clone(),
                -32000,
                "Gamepad simulation not yet supported",
            ),

            _ => vibe_debug::VdpResponse::error(
                req.id.clone(),
                -32602,
                format!("Unknown device: {}", device),
            ),
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
        let device = val
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("keyboard");
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
            "mouse" => match action {
                "move" => {
                    if let (Some(x), Some(y)) = (
                        val.get("x").and_then(|v| v.as_f64()),
                        val.get("y").and_then(|v| v.as_f64()),
                    ) {
                        self.pending_simulated
                            .push(SimulatedInput::MouseMove(x as f32, y as f32));
                    }
                }
                "press" | "release" | "click" => {
                    if let Some(btn_name) = val.get("button").and_then(|v| v.as_str())
                        && let Some(button) = vibe_input::string_to_mouse_button(btn_name)
                    {
                        let sim = match action {
                            "press" => SimulatedInput::MouseButtonPress(button),
                            "release" => SimulatedInput::MouseButtonRelease(button),
                            "click" => SimulatedInput::MouseButtonClick(button),
                            _ => return,
                        };
                        self.pending_simulated.push(sim);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}
