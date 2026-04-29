use std::collections::HashMap;

use vibe_asset::AssetManager;
use vibe_render::{DrawCommand, Renderer, TextureId};

use crate::id::WidgetId;
use crate::vdp::{VdpUiAction, WidgetSnapshot};

/// Persistent state for TextInput widgets, stored across frames.
#[derive(Debug, Clone, Default)]
pub struct TextInputState {
    pub text: String,
    pub cursor_position: usize,
    pub selection_start: Option<usize>,
}

/// Which scrollbar is currently being dragged (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbarDrag {
    #[default]
    None,
    Vertical,
    Horizontal,
}

/// Persistent state for ScrollList widgets, stored across frames.
#[derive(Debug, Clone, Default)]
pub struct ScrollListState {
    pub scroll_offset: f32,
    pub horizontal_offset: f32,
    pub total_content_height: f32,
    pub total_content_width: f32,

    /// Active scrollbar drag state.
    pub dragging: ScrollbarDrag,
    /// Mouse position at drag start (y for vertical, x for horizontal).
    pub drag_start_mouse: f32,
    /// Scroll offset at drag start.
    pub drag_start_offset: f32,
}

/// Internal name under which the UI registers its 1×1 white pixel
/// texture into [`AssetManager`]. Used as a debug label only — game
/// code never looks this up by name (it goes through
/// [`UiContext`](crate::UiContext), which reads the `TextureId`
/// straight out of [`UiState`]). Prefixed with `__vibe_ui_` so a
/// game's own `game.yaml` texture names won't collide.
const UI_WHITE_TEXTURE_NAME: &str = "__vibe_ui_white";

/// Cross-frame persistent UI state, stored in the engine Context.
///
/// Manages focus, text input buffers, scroll positions, VDP snapshots,
/// pending VDP actions, and the UI system's own private GPU resources
/// (e.g. the 1×1 white pixel texture used to draw filled rectangles).
pub struct UiState {
    /// Currently focused widget (receives keyboard input).
    pub focused: Option<WidgetId>,

    /// TextInput persistent state indexed by widget ID.
    pub text_inputs: HashMap<WidgetId, TextInputState>,

    /// ScrollList persistent state indexed by widget ID.
    pub scroll_lists: HashMap<WidgetId, ScrollListState>,

    /// Widget tree snapshot from the last frame (for VDP inspection).
    pub last_frame_widgets: Vec<WidgetSnapshot>,

    /// VDP-injected actions to be consumed in the next frame's ui() call.
    pub pending_vdp_actions: Vec<VdpUiAction>,

    /// Cached draw commands from the last `draw_ui()` call, replayed during `on_render`.
    pub cached_draw_commands: Vec<DrawCommand>,

    /// Auto-ID counter, reset each frame.
    pub auto_id_counter: usize,

    /// Elapsed time in seconds (for cursor blink animation).
    pub elapsed_time: f64,

    /// 1×1 white pixel texture used to draw filled rectangles via
    /// tinting. Allocated once in [`UiState::init`] using the public
    /// [`Renderer::create_white_pixel_texture`] +
    /// [`AssetManager::register_texture`] flow — the same path game
    /// code uses for its own procedural textures. `None` until `init`
    /// has been called (the engine does this at the end of `on_init`,
    /// before the user's `Game::new`).
    pub white_texture_id: Option<TextureId>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            focused: None,
            text_inputs: HashMap::new(),
            scroll_lists: HashMap::new(),
            last_frame_widgets: Vec::new(),
            pending_vdp_actions: Vec::new(),
            cached_draw_commands: Vec::new(),
            auto_id_counter: 0,
            elapsed_time: 0.0,
            white_texture_id: None,
        }
    }
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// One-shot initialization: allocate the UI's private GPU
    /// textures (currently just the 1×1 white pixel for filled
    /// rects) and register them into `assets` so the renderer can
    /// reach them by [`TextureId`] during sprite batching.
    ///
    /// Called once by the engine in its `on_init`, before the user's
    /// `Game::new` runs. Re-calling is a no-op (the white texture is
    /// only created the first time) so test harnesses that build
    /// `UiState` by hand can call it idempotently.
    ///
    /// The UI deliberately uses the same public
    /// [`Renderer::create_white_pixel_texture`] +
    /// [`AssetManager::register_texture`] APIs that game code uses —
    /// there is no separate "engine-internal" path. The only thing
    /// that distinguishes the UI's white texture from a
    /// game-registered one is the `__vibe_ui_` name prefix used as a
    /// debug label.
    pub fn init(&mut self, renderer: &Renderer, assets: &mut AssetManager) {
        if self.white_texture_id.is_some() {
            return;
        }
        let tex = renderer.create_white_pixel_texture();
        let id = assets.register_texture(UI_WHITE_TEXTURE_NAME, tex);
        self.white_texture_id = Some(id);
    }

    /// Generate the next auto-ID for this frame.
    pub fn next_auto_id(&mut self) -> WidgetId {
        let id = WidgetId::auto(self.auto_id_counter);
        self.auto_id_counter += 1;
        id
    }

    /// Reset per-frame counters. Called at the start of each ui() invocation.
    pub fn begin_frame(&mut self) {
        self.auto_id_counter = 0;
    }

    /// Update elapsed time (called from engine update loop).
    pub fn update_time(&mut self, dt: f64) {
        self.elapsed_time += dt;
    }

    /// Get or create TextInput state for the given ID.
    pub fn text_input_state(&mut self, id: &WidgetId) -> &mut TextInputState {
        self.text_inputs.entry(id.clone()).or_default()
    }

    /// Get or create ScrollList state for the given ID.
    pub fn scroll_list_state(&mut self, id: &WidgetId) -> &mut ScrollListState {
        self.scroll_lists.entry(id.clone()).or_default()
    }

    /// Drain all pending VDP actions for consumption.
    pub fn drain_vdp_actions(&mut self) -> Vec<VdpUiAction> {
        std::mem::take(&mut self.pending_vdp_actions)
    }

    /// Push a VDP action into the pending queue (called from VDP request handlers).
    pub fn push_vdp_action(&mut self, action: VdpUiAction) {
        self.pending_vdp_actions.push(action);
    }
}
