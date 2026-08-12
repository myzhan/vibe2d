use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
pub use winit::keyboard::KeyCode;

/// Mouse button identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

// ─────────────────────────────────────────────────────────────────────
// Gamepad
// ─────────────────────────────────────────────────────────────────────

/// Gamepad buttons, named by *position* rather than by printed label.
///
/// On an Xbox-layout pad: `South` = A, `East` = B, `West` = X, `North` = Y.
/// Positional naming is what controller databases and other engines use, and
/// it's the only naming that stays correct across layouts — on a
/// Nintendo-style pad the button *printed* "A" sits in the `East` position.
///
/// We deliberately diverge from gilrs here: gilrs calls the shoulder button
/// `LeftTrigger` and the analog trigger `LeftTrigger2`, which reads backwards
/// to anyone writing a `game.yaml`. We use `LeftShoulder` / `LeftTrigger`, and
/// the platform layer does the renaming (see `vibe_platform`'s gamepad module).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadButton {
    // ── Action pad (positional) ──
    South,
    East,
    North,
    West,
    // ── Shoulders (gilrs `LeftTrigger` / `RightTrigger`) ──
    LeftShoulder,
    RightShoulder,
    // ── Analog triggers (gilrs `LeftTrigger2` / `RightTrigger2`) ──
    // Digital view here; use `gamepad_button_value` for the 0.0..=1.0 reading.
    LeftTrigger,
    RightTrigger,
    // ── Menu cluster ──
    Select,
    Start,
    Mode,
    // ── Stick clicks ──
    LeftThumb,
    RightThumb,
    // ── D-pad ──
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl GamepadButton {
    /// Every variant in a stable order. Used by the gamepad-tester example to
    /// lay out its button grid, and by tests to sweep the whole enum.
    pub const ALL: [GamepadButton; 17] = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::North,
        GamepadButton::West,
        GamepadButton::LeftShoulder,
        GamepadButton::RightShoulder,
        GamepadButton::LeftTrigger,
        GamepadButton::RightTrigger,
        GamepadButton::Select,
        GamepadButton::Start,
        GamepadButton::Mode,
        GamepadButton::LeftThumb,
        GamepadButton::RightThumb,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ];

    /// Canonical name, matching what `string_to_gamepad_button` accepts.
    pub fn name(self) -> &'static str {
        match self {
            GamepadButton::South => "South",
            GamepadButton::East => "East",
            GamepadButton::North => "North",
            GamepadButton::West => "West",
            GamepadButton::LeftShoulder => "LeftShoulder",
            GamepadButton::RightShoulder => "RightShoulder",
            GamepadButton::LeftTrigger => "LeftTrigger",
            GamepadButton::RightTrigger => "RightTrigger",
            GamepadButton::Select => "Select",
            GamepadButton::Start => "Start",
            GamepadButton::Mode => "Mode",
            GamepadButton::LeftThumb => "LeftThumb",
            GamepadButton::RightThumb => "RightThumb",
            GamepadButton::DPadUp => "DPadUp",
            GamepadButton::DPadDown => "DPadDown",
            GamepadButton::DPadLeft => "DPadLeft",
            GamepadButton::DPadRight => "DPadRight",
        }
    }
}

/// Analog stick axes, each reading -1.0..=1.0.
///
/// **Y is up-positive** (the SDL / gilrs convention), which is the *opposite*
/// of vibe2d's y-down screen space. Code converting a stick reading into a
/// screen delta must negate Y. We keep the industry convention because that's
/// what every controller doc and mapping database uses.
///
/// There is deliberately no `DPadX` / `DPadY`: gilrs's default
/// `axis_dpad_to_button` filter already converts axis-reported d-pads into
/// `DPad*` button events, so a second (sometimes-empty) path for the same
/// input would only create ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
}

impl GamepadAxis {
    pub const ALL: [GamepadAxis; 4] = [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
    ];

    /// Dense index into the per-pad `[f32; 4]` axis arrays.
    pub const fn index(self) -> usize {
        match self {
            GamepadAxis::LeftStickX => 0,
            GamepadAxis::LeftStickY => 1,
            GamepadAxis::RightStickX => 2,
            GamepadAxis::RightStickY => 3,
        }
    }

    /// The other axis of the same physical stick. Radial deadzoning needs both
    /// axes of a stick at once, so every read has to be able to find its pair.
    pub const fn stick_partner(self) -> GamepadAxis {
        match self {
            GamepadAxis::LeftStickX => GamepadAxis::LeftStickY,
            GamepadAxis::LeftStickY => GamepadAxis::LeftStickX,
            GamepadAxis::RightStickX => GamepadAxis::RightStickY,
            GamepadAxis::RightStickY => GamepadAxis::RightStickX,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GamepadAxis::LeftStickX => "LeftStickX",
            GamepadAxis::LeftStickY => "LeftStickY",
            GamepadAxis::RightStickX => "RightStickX",
            GamepadAxis::RightStickY => "RightStickY",
        }
    }
}

/// Stable per-pad handle.
///
/// On desktop this mirrors `usize::from(gilrs::GamepadId)`; VDP-simulated pads
/// use small integers starting at 0. Ordering is meaningful: "player 1" is the
/// lowest connected id (see [`InputState::primary_gamepad`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GamepadId(usize);

impl GamepadId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for GamepadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which half of an axis an axis-as-button binding refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisDir {
    Positive,
    Negative,
}

/// One `gamepad_axes` binding: an axis plus the half of it that counts as
/// "pressed". Produced by [`parse_gamepad_axis_spec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisSpec {
    pub axis: GamepadAxis,
    pub dir: AxisDir,
}

/// A queued rumble (force-feedback) request.
///
/// This is an *output*, yet it lives in `vibe_input` because it is keyed by
/// [`GamepadId`] and because both `vibe2d` and `vibe_platform` already depend
/// on this crate — keeping the platform callback signature free of any new
/// dependency edge, mirroring `on_update(&mut InputState)`.
///
/// Games queue these via `Context::rumble`; the platform layer drains them
/// through `PlatformCallbacks::take_rumble_requests`. Desktop-only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RumbleRequest {
    /// `None` targets every connected pad (the single-player convenience).
    pub pad: Option<GamepadId>,
    /// Low-frequency ("strong") motor, clamped 0.0..=1.0.
    pub strong: f32,
    /// High-frequency ("weak") motor, clamped 0.0..=1.0.
    pub weak: f32,
    pub duration_ms: u32,
}

/// State of one physical (or VDP-simulated) gamepad.
pub struct GamepadState {
    name: String,
    connected: bool,
    pressed: HashMap<GamepadButton, bool>,
    just_pressed: HashMap<GamepadButton, bool>,
    just_released: HashMap<GamepadButton, bool>,
    /// Analog value per button. Triggers report 0.0..=1.0; digital buttons
    /// land here as 0.0 / 1.0 so a single accessor covers both.
    values: HashMap<GamepadButton, f32>,
    /// RAW axis values as reported, indexed by [`GamepadAxis::index`].
    ///
    /// Deadzone is applied at *read* time rather than on ingest, so the
    /// deadzone stays runtime-tunable and a diagnostic UI can show raw and
    /// processed values side by side.
    axes_raw: [f32; 4],
    /// `axes_raw` as of the end of the previous frame. Snapshotted in
    /// `begin_frame` and nowhere else — it is the *only* mechanism behind
    /// axis-derived `just_pressed` / `just_released`.
    prev_axes_raw: [f32; 4],
}

impl GamepadState {
    fn new(name: String) -> Self {
        Self {
            name,
            connected: true,
            pressed: HashMap::new(),
            just_pressed: HashMap::new(),
            just_released: HashMap::new(),
            values: HashMap::new(),
            axes_raw: [0.0; 4],
            prev_axes_raw: [0.0; 4],
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn is_pressed(&self, button: GamepadButton) -> bool {
        self.pressed.get(&button).copied().unwrap_or(false)
    }

    pub fn is_just_pressed(&self, button: GamepadButton) -> bool {
        self.just_pressed.get(&button).copied().unwrap_or(false)
    }

    pub fn is_just_released(&self, button: GamepadButton) -> bool {
        self.just_released.get(&button).copied().unwrap_or(false)
    }

    /// Analog value for a button, 0.0..=1.0. Meaningful for the triggers;
    /// digital buttons read 1.0 while held and 0.0 otherwise.
    pub fn value(&self, button: GamepadButton) -> f32 {
        self.values.get(&button).copied().unwrap_or(0.0)
    }

    /// Axis value as reported by the driver, with no deadzone applied.
    pub fn axis_raw(&self, axis: GamepadAxis) -> f32 {
        self.axes_raw[axis.index()]
    }

    /// Axis value with a radial deadzone applied against its stick partner.
    pub fn axis(&self, axis: GamepadAxis, deadzone: f32) -> f32 {
        Self::deadzoned(&self.axes_raw, axis, deadzone)
    }

    /// Same as [`Self::axis`], but reading the previous frame's snapshot.
    /// Used only for axis-as-action edge detection.
    fn prev_axis(&self, axis: GamepadAxis, deadzone: f32) -> f32 {
        Self::deadzoned(&self.prev_axes_raw, axis, deadzone)
    }

    fn deadzoned(axes: &[f32; 4], axis: GamepadAxis, deadzone: f32) -> f32 {
        let partner = axis.stick_partner();
        let (v, _) = apply_radial_deadzone(axes[axis.index()], axes[partner.index()], deadzone);
        v
    }

    /// Drop all held state, keeping the entry itself. Used on disconnect.
    ///
    /// Anything that was held records a `just_released` edge, so a game
    /// watching `is_action_just_released` gets a clean "input ended" signal on
    /// unplug rather than silently losing the button. This mirrors what the
    /// axes do naturally: zeroing `axes_raw` while `prev_axes_raw` still holds
    /// the deflected value produces an axis release edge on the same frame.
    fn clear_held(&mut self) {
        for (button, held) in self.pressed.iter_mut() {
            if *held {
                self.just_released.insert(*button, true);
            }
            *held = false;
        }
        self.values.clear();
        self.axes_raw = [0.0; 4];
    }
}

/// Tracks keyboard, mouse and gamepad state per frame.
pub struct InputState {
    // ── Keyboard ──
    pressed: HashMap<KeyCode, bool>,
    just_pressed: HashMap<KeyCode, bool>,
    just_released: HashMap<KeyCode, bool>,
    actions: HashMap<String, Vec<KeyCode>>,

    // ── Mouse ──
    mouse_x: f32,
    mouse_y: f32,
    mouse_pressed: HashMap<MouseButton, bool>,
    mouse_just_pressed: HashMap<MouseButton, bool>,
    mouse_just_released: HashMap<MouseButton, bool>,
    mouse_actions: HashMap<String, Vec<MouseButton>>,

    // ── Character input (for UI text input) ──
    chars_received: Vec<char>,

    // ── IME (Input Method Editor) ──
    /// Text committed by the IME this frame (the result of finalizing a
    /// composition, e.g. selecting a Chinese candidate). Multi-character
    /// strings are inserted as one atomic unit, unlike `chars_received`.
    ime_commit: String,
    /// In-flight composition text being edited by the IME, with the cursor
    /// position (byte offset within the preedit). `None` when no IME
    /// composition is active. The preedit must be rendered as a hint above
    /// the focused widget but **not** appended to the widget's text buffer.
    ime_preedit: Option<ImePreedit>,

    // ── Mouse scroll ──
    scroll_delta: f32,
    scroll_delta_x: f32,

    // ── Gamepad ──
    /// A `BTreeMap`, not a `HashMap`: deterministic iteration makes
    /// "player 1 = the lowest connected id" stable across runs, and keeps a
    /// diagnostic pad list from reshuffling frame to frame.
    gamepads: BTreeMap<GamepadId, GamepadState>,
    gamepad_actions: HashMap<String, Vec<GamepadButton>>,
    gamepad_axis_actions: HashMap<String, Vec<AxisSpec>>,
    /// Radial stick deadzone, 0.0..1.0.
    gamepad_deadzone: f32,
    /// |axis| past which an axis-as-action counts as pressed.
    gamepad_axis_threshold: f32,
    gamepads_connected_this_frame: Vec<GamepadId>,
    gamepads_disconnected_this_frame: Vec<GamepadId>,
}

/// Default radial stick deadzone. Small enough not to eat deliberate nudges,
/// large enough to swallow the resting drift of a worn analog stick.
pub const DEFAULT_GAMEPAD_DEADZONE: f32 = 0.15;
/// Default |axis| threshold for treating an axis-as-action as pressed.
pub const DEFAULT_GAMEPAD_AXIS_THRESHOLD: f32 = 0.5;

fn default_deadzone() -> f32 {
    DEFAULT_GAMEPAD_DEADZONE
}

fn default_axis_threshold() -> f32 {
    DEFAULT_GAMEPAD_AXIS_THRESHOLD
}

/// The `input.gamepad` block in `game.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct GamepadConfig {
    #[serde(default = "default_deadzone")]
    pub deadzone: f32,
    #[serde(default = "default_axis_threshold")]
    pub axis_threshold: f32,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            deadzone: DEFAULT_GAMEPAD_DEADZONE,
            axis_threshold: DEFAULT_GAMEPAD_AXIS_THRESHOLD,
        }
    }
}

/// In-progress IME composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImePreedit {
    /// The text currently being composed (e.g. "ni hao" / "你好" candidates).
    pub text: String,
    /// Caret byte offset inside `text`. `None` when the cursor is hidden.
    pub cursor_byte: Option<usize>,
}

/// Input action mapping from game.yaml.
///
/// All four binding lists are OR-ed together: an action fires if *any* bound
/// key, mouse button, gamepad button or stick direction is active. That's what
/// lets one action name serve keyboard, d-pad and analog stick at once.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActionConfig {
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub mouse_buttons: Vec<String>,
    /// Names accepted by [`string_to_gamepad_button`], e.g. `"South"`, `"DPadLeft"`.
    #[serde(default)]
    pub gamepad_buttons: Vec<String>,
    /// Directional axis bindings accepted by [`parse_gamepad_axis_spec`],
    /// e.g. `"LeftStickLeft"` or `"LeftStickX-"`.
    #[serde(default)]
    pub gamepad_axes: Vec<String>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            pressed: HashMap::new(),
            just_pressed: HashMap::new(),
            just_released: HashMap::new(),
            actions: HashMap::new(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_pressed: HashMap::new(),
            mouse_just_pressed: HashMap::new(),
            mouse_just_released: HashMap::new(),
            mouse_actions: HashMap::new(),
            chars_received: Vec::new(),
            ime_commit: String::new(),
            ime_preedit: None,
            scroll_delta: 0.0,
            scroll_delta_x: 0.0,
            gamepads: BTreeMap::new(),
            gamepad_actions: HashMap::new(),
            gamepad_axis_actions: HashMap::new(),
            gamepad_deadzone: DEFAULT_GAMEPAD_DEADZONE,
            gamepad_axis_threshold: DEFAULT_GAMEPAD_AXIS_THRESHOLD,
            gamepads_connected_this_frame: Vec::new(),
            gamepads_disconnected_this_frame: Vec::new(),
        }
    }

    /// Load action mappings from config.
    pub fn load_actions(&mut self, actions: &HashMap<String, ActionConfig>) {
        for (name, config) in actions {
            let keycodes: Vec<KeyCode> = config
                .keys
                .iter()
                .filter_map(|s| string_to_keycode(s))
                .collect();
            if !keycodes.is_empty() {
                self.actions.insert(name.clone(), keycodes);
            }

            let buttons: Vec<MouseButton> = config
                .mouse_buttons
                .iter()
                .filter_map(|s| string_to_mouse_button(s))
                .collect();
            if !buttons.is_empty() {
                self.mouse_actions.insert(name.clone(), buttons);
            }

            let pad_buttons: Vec<GamepadButton> = config
                .gamepad_buttons
                .iter()
                .filter_map(|s| string_to_gamepad_button(s))
                .collect();
            if !pad_buttons.is_empty() {
                self.gamepad_actions.insert(name.clone(), pad_buttons);
            }

            let pad_axes: Vec<AxisSpec> = config
                .gamepad_axes
                .iter()
                .filter_map(|s| parse_gamepad_axis_spec(s))
                .collect();
            if !pad_axes.is_empty() {
                self.gamepad_axis_actions.insert(name.clone(), pad_axes);
            }
        }
    }

    /// Apply the `input.gamepad` block from `game.yaml`.
    pub fn configure_gamepad(&mut self, cfg: &GamepadConfig) {
        self.set_gamepad_deadzone(cfg.deadzone);
        self.set_gamepad_axis_threshold(cfg.axis_threshold);
    }

    /// Called at the start of each frame to clear per-frame state.
    ///
    /// Note: `ime_preedit` persists across frames — it represents IME
    /// composition state, which is cleared by the platform layer via
    /// `clear_ime_preedit()` when the IME explicitly ends/cancels.
    ///
    /// This runs at the **end** of a frame (after `game.update`), which is what
    /// makes the gamepad axis snapshot below correct — see `prev_axes_raw`.
    pub fn begin_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
        self.chars_received.clear();
        self.ime_commit.clear();
        self.scroll_delta = 0.0;
        self.scroll_delta_x = 0.0;

        self.gamepads_connected_this_frame.clear();
        self.gamepads_disconnected_this_frame.clear();
        for pad in self.gamepads.values_mut() {
            pad.just_pressed.clear();
            pad.just_released.clear();
            // Snapshot for the NEXT frame's axis-as-action edge detection.
            // This is the only place `prev_axes_raw` is ever written.
            //
            // Because `begin_frame` runs at the end of frame N, at the top of
            // frame N+1 this holds exactly the values the game saw during
            // frame N's `update`. Gamepad events for frame N+1 are drained
            // after this point and only touch `axes_raw`, so an edge
            // comparison is always "what the game sees now" vs "what the game
            // saw last frame".
            pad.prev_axes_raw = pad.axes_raw;
        }
    }

    // ── Keyboard events ──

    /// Called when a key is pressed.
    pub fn on_key_pressed(&mut self, key: KeyCode) {
        if !self.pressed.get(&key).copied().unwrap_or(false) {
            self.just_pressed.insert(key, true);
        }
        self.pressed.insert(key, true);
    }

    /// Called when a key is released.
    pub fn on_key_released(&mut self, key: KeyCode) {
        self.pressed.insert(key, false);
        self.just_released.insert(key, true);
    }

    pub fn is_key_pressed(&self, key: KeyCode) -> bool {
        self.pressed.get(&key).copied().unwrap_or(false)
    }

    pub fn is_key_just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed.get(&key).copied().unwrap_or(false)
    }

    /// True on the single frame a key went up.
    pub fn is_key_just_released(&self, key: KeyCode) -> bool {
        self.just_released.get(&key).copied().unwrap_or(false)
    }

    // ── Mouse events ──

    /// Called when the mouse cursor moves (coordinates in virtual resolution).
    pub fn on_mouse_moved(&mut self, x: f32, y: f32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Called when a mouse button is pressed.
    pub fn on_mouse_button_pressed(&mut self, button: MouseButton) {
        if !self.mouse_pressed.get(&button).copied().unwrap_or(false) {
            self.mouse_just_pressed.insert(button, true);
        }
        self.mouse_pressed.insert(button, true);
    }

    /// Called when a mouse button is released.
    pub fn on_mouse_button_released(&mut self, button: MouseButton) {
        self.mouse_pressed.insert(button, false);
        self.mouse_just_released.insert(button, true);
    }

    /// Get the current mouse position in virtual coordinates.
    pub fn mouse_position(&self) -> (f32, f32) {
        (self.mouse_x, self.mouse_y)
    }

    /// Current mouse X in virtual coordinates.
    pub fn mouse_x(&self) -> f32 {
        self.mouse_x
    }

    /// Current mouse Y in virtual coordinates.
    pub fn mouse_y(&self) -> f32 {
        self.mouse_y
    }

    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.get(&button).copied().unwrap_or(false)
    }

    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed
            .get(&button)
            .copied()
            .unwrap_or(false)
    }

    /// True on the single frame a mouse button went up.
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released
            .get(&button)
            .copied()
            .unwrap_or(false)
    }

    // ── Gamepad events ──
    //
    // Every ingestion method auto-vivifies the pad entry (marked connected,
    // with a generic name). That way a button event arriving before the
    // platform's `Connected` event — or a VDP-simulated pad that never sends
    // one — still shows up in `connected_gamepads()`.

    fn pad_entry(&mut self, id: GamepadId) -> &mut GamepadState {
        self.gamepads
            .entry(id)
            .or_insert_with(|| GamepadState::new("Gamepad".to_string()))
    }

    /// Called when a gamepad is connected (or discovered at startup).
    pub fn on_gamepad_connected(&mut self, id: GamepadId, name: String) {
        let pad = self.pad_entry(id);
        pad.name = name;
        pad.connected = true;
        self.gamepads_connected_this_frame.push(id);
    }

    /// Called when a gamepad is disconnected.
    ///
    /// The entry is kept (gilrs reuses ids by device UUID, so preserving the
    /// slot keeps the player↔pad association across a replug) but all held
    /// state is cleared. Clearing matters: without it, unplugging a pad while
    /// holding "right" leaves the action stuck on forever.
    ///
    /// `prev_axes_raw` is deliberately NOT cleared — leaving it lets the next
    /// `begin_frame` snapshot naturally, so a stick that was deflected at
    /// unplug time still produces a proper `just_released` edge.
    pub fn on_gamepad_disconnected(&mut self, id: GamepadId) {
        if let Some(pad) = self.gamepads.get_mut(&id) {
            pad.clear_held();
            pad.connected = false;
            self.gamepads_disconnected_this_frame.push(id);
        }
    }

    pub fn on_gamepad_button_pressed(&mut self, id: GamepadId, button: GamepadButton) {
        let pad = self.pad_entry(id);
        if !pad.pressed.get(&button).copied().unwrap_or(false) {
            pad.just_pressed.insert(button, true);
        }
        pad.pressed.insert(button, true);
        pad.values.insert(button, 1.0);
    }

    pub fn on_gamepad_button_released(&mut self, id: GamepadId, button: GamepadButton) {
        let pad = self.pad_entry(id);
        pad.pressed.insert(button, false);
        pad.just_released.insert(button, true);
        pad.values.insert(button, 0.0);
    }

    /// Analog value update for a button (the triggers, 0.0..=1.0).
    ///
    /// This records the value only; it does NOT synthesize press/release
    /// edges. gilrs emits `ButtonPressed`/`ButtonReleased` alongside
    /// `ButtonChanged` for the same physical trigger, so deriving edges here
    /// too would double-fire `just_pressed`.
    pub fn on_gamepad_button_value(&mut self, id: GamepadId, button: GamepadButton, value: f32) {
        let pad = self.pad_entry(id);
        pad.values.insert(button, value.clamp(0.0, 1.0));
    }

    pub fn on_gamepad_axis_changed(&mut self, id: GamepadId, axis: GamepadAxis, value: f32) {
        let pad = self.pad_entry(id);
        pad.axes_raw[axis.index()] = value.clamp(-1.0, 1.0);
    }

    // ── Gamepad configuration ──

    /// Set the radial stick deadzone. Clamped to 0.0..=0.95 — a deadzone at or
    /// near 1.0 would make the stick unreadable.
    pub fn set_gamepad_deadzone(&mut self, deadzone: f32) {
        self.gamepad_deadzone = deadzone.clamp(0.0, 0.95);
    }

    /// Set the |axis| threshold for axis-as-action bindings. Clamped to
    /// 0.05..=1.0 — a threshold of 0 would latch the action on permanently.
    pub fn set_gamepad_axis_threshold(&mut self, threshold: f32) {
        self.gamepad_axis_threshold = threshold.clamp(0.05, 1.0);
    }

    pub fn gamepad_deadzone(&self) -> f32 {
        self.gamepad_deadzone
    }

    pub fn gamepad_axis_threshold(&self) -> f32 {
        self.gamepad_axis_threshold
    }

    // ── Gamepad queries: per-pad ──

    pub fn gamepad(&self, id: GamepadId) -> Option<&GamepadState> {
        self.gamepads.get(&id)
    }

    /// Number of currently connected pads (disconnected entries are retained
    /// internally but not counted).
    pub fn gamepad_count(&self) -> usize {
        self.gamepads.values().filter(|p| p.connected).count()
    }

    /// Connected pad ids in ascending order.
    pub fn connected_gamepads(&self) -> Vec<GamepadId> {
        self.gamepads
            .iter()
            .filter(|(_, p)| p.connected)
            .map(|(id, _)| *id)
            .collect()
    }

    /// The lowest connected pad id — "player 1" for single-player games.
    pub fn primary_gamepad(&self) -> Option<GamepadId> {
        self.gamepads
            .iter()
            .find(|(_, p)| p.connected)
            .map(|(id, _)| *id)
    }

    pub fn is_gamepad_connected(&self, id: GamepadId) -> bool {
        self.gamepads.get(&id).is_some_and(|p| p.connected)
    }

    pub fn gamepad_name(&self, id: GamepadId) -> Option<&str> {
        self.gamepads.get(&id).map(|p| p.name())
    }

    /// Pads connected this frame. Cleared by `begin_frame`.
    pub fn gamepads_connected_this_frame(&self) -> &[GamepadId] {
        &self.gamepads_connected_this_frame
    }

    /// Pads disconnected this frame. Cleared by `begin_frame`.
    pub fn gamepads_disconnected_this_frame(&self) -> &[GamepadId] {
        &self.gamepads_disconnected_this_frame
    }

    pub fn is_gamepad_button_pressed_on(&self, pad: GamepadId, button: GamepadButton) -> bool {
        self.gamepads
            .get(&pad)
            .is_some_and(|p| p.connected && p.is_pressed(button))
    }

    pub fn is_gamepad_button_just_pressed_on(&self, pad: GamepadId, button: GamepadButton) -> bool {
        self.gamepads
            .get(&pad)
            .is_some_and(|p| p.is_just_pressed(button))
    }

    pub fn is_gamepad_button_just_released_on(
        &self,
        pad: GamepadId,
        button: GamepadButton,
    ) -> bool {
        self.gamepads
            .get(&pad)
            .is_some_and(|p| p.is_just_released(button))
    }

    pub fn gamepad_button_value_on(&self, pad: GamepadId, button: GamepadButton) -> f32 {
        self.gamepads
            .get(&pad)
            .filter(|p| p.connected)
            .map_or(0.0, |p| p.value(button))
    }

    pub fn gamepad_axis_on(&self, pad: GamepadId, axis: GamepadAxis) -> f32 {
        self.gamepads
            .get(&pad)
            .filter(|p| p.connected)
            .map_or(0.0, |p| p.axis(axis, self.gamepad_deadzone))
    }

    pub fn gamepad_axis_raw_on(&self, pad: GamepadId, axis: GamepadAxis) -> f32 {
        self.gamepads
            .get(&pad)
            .filter(|p| p.connected)
            .map_or(0.0, |p| p.axis_raw(axis))
    }

    // ── Gamepad queries: merged across all connected pads ──
    //
    // The single-player convenience path: "did *any* pad do this". Games that
    // care which pad it was use the `_on` variants above.

    fn connected_pads(&self) -> impl Iterator<Item = &GamepadState> {
        self.gamepads.values().filter(|p| p.connected)
    }

    pub fn is_gamepad_button_pressed(&self, button: GamepadButton) -> bool {
        self.connected_pads().any(|p| p.is_pressed(button))
    }

    pub fn is_gamepad_button_just_pressed(&self, button: GamepadButton) -> bool {
        self.connected_pads().any(|p| p.is_just_pressed(button))
    }

    /// True on the single frame a button went up on any pad.
    ///
    /// Unlike the pressed/just-pressed queries this does NOT filter on
    /// `connected`: a pad unplugged mid-hold records release edges as it
    /// clears, and those edges are real — the input genuinely ended.
    pub fn is_gamepad_button_just_released(&self, button: GamepadButton) -> bool {
        self.gamepads.values().any(|p| p.is_just_released(button))
    }

    /// Largest analog value across connected pads.
    ///
    /// Max rather than sum: summing two half-pulled triggers would report
    /// 1.0 ("fully pulled") when neither is.
    pub fn gamepad_button_value(&self, button: GamepadButton) -> f32 {
        self.connected_pads()
            .map(|p| p.value(button))
            .fold(0.0, f32::max)
    }

    /// Deadzoned axis value with the largest magnitude across connected pads.
    pub fn gamepad_axis(&self, axis: GamepadAxis) -> f32 {
        let dz = self.gamepad_deadzone;
        self.connected_pads()
            .map(|p| p.axis(axis, dz))
            .fold(0.0, |acc, v| if v.abs() > acc.abs() { v } else { acc })
    }

    /// Raw axis value with the largest magnitude across connected pads.
    pub fn gamepad_axis_raw(&self, axis: GamepadAxis) -> f32 {
        self.connected_pads()
            .map(|p| p.axis_raw(axis))
            .fold(0.0, |acc, v| if v.abs() > acc.abs() { v } else { acc })
    }

    // ── Action queries (keyboard + mouse) ──

    /// Check if an action (defined in game.yaml) was just pressed this frame.
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        let key_match = self
            .actions
            .get(action)
            .is_some_and(|keys| keys.iter().any(|k| self.is_key_just_pressed(*k)));
        let mouse_match = self
            .mouse_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| self.is_mouse_button_just_pressed(*b)));
        key_match || mouse_match || self.gamepad_action_just_pressed(action)
    }

    // ── Character input ──

    /// Characters received this frame (for text input widgets).
    pub fn chars_this_frame(&self) -> &[char] {
        &self.chars_received
    }

    /// Called by the platform layer when a printable character is received.
    pub fn on_char_received(&mut self, ch: char) {
        self.chars_received.push(ch);
    }

    // ── IME ──

    /// Text committed by the IME this frame, if any (e.g. a finalized Chinese word).
    /// Empty when no commit happened this frame.
    pub fn ime_commit(&self) -> &str {
        &self.ime_commit
    }

    /// Current in-progress IME composition, if any.
    /// Returns `None` when no IME composition is active.
    pub fn ime_preedit(&self) -> Option<&ImePreedit> {
        self.ime_preedit.as_ref()
    }

    /// Called by the platform layer when the IME commits a composition.
    /// Multiple commits within the same frame are concatenated (rare in practice).
    pub fn on_ime_commit(&mut self, text: &str) {
        self.ime_commit.push_str(text);
        // A commit always ends the composition.
        self.ime_preedit = None;
    }

    /// Called by the platform layer for IME preedit updates.
    /// Pass an empty `text` to clear the preedit.
    pub fn on_ime_preedit(&mut self, text: String, cursor_byte: Option<usize>) {
        if text.is_empty() {
            self.ime_preedit = None;
        } else {
            self.ime_preedit = Some(ImePreedit { text, cursor_byte });
        }
    }

    /// Explicitly clear any in-progress IME composition (e.g. on focus loss).
    pub fn clear_ime_preedit(&mut self) {
        self.ime_preedit = None;
    }

    // ── Mouse scroll ──

    /// Vertical mouse scroll wheel delta this frame (positive = scroll up).
    pub fn mouse_scroll_delta(&self) -> f32 {
        self.scroll_delta
    }

    /// Horizontal mouse scroll wheel delta this frame (positive = scroll right).
    pub fn mouse_scroll_delta_x(&self) -> f32 {
        self.scroll_delta_x
    }

    /// Called by the platform layer when a scroll event is received.
    pub fn on_mouse_scroll(&mut self, delta_x: f32, delta_y: f32) {
        self.scroll_delta += delta_y;
        self.scroll_delta_x += delta_x;
    }

    /// Check if an action is currently held down.
    pub fn is_action_pressed(&self, action: &str) -> bool {
        let key_match = self
            .actions
            .get(action)
            .is_some_and(|keys| keys.iter().any(|k| self.is_key_pressed(*k)));
        let mouse_match = self
            .mouse_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| self.is_mouse_button_pressed(*b)));
        key_match || mouse_match || self.gamepad_action_pressed(action)
    }

    /// Check if an action was released this frame.
    pub fn is_action_just_released(&self, action: &str) -> bool {
        let key_match = self
            .actions
            .get(action)
            .is_some_and(|keys| keys.iter().any(|k| self.is_key_just_released(*k)));
        let mouse_match = self
            .mouse_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| self.is_mouse_button_just_released(*b)));
        key_match || mouse_match || self.gamepad_action_just_released(action)
    }

    // ── Action queries: gamepad contribution ──
    //
    // Split out as helpers so the three public action queries above each stay
    // a readable OR of four binding sources.

    fn gamepad_action_pressed(&self, action: &str) -> bool {
        let button_match = self
            .gamepad_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| self.is_gamepad_button_pressed(*b)));
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs.iter().any(|s| {
                self.connected_pads()
                    .any(|p| self.axis_spec_active(p, *s, false))
            })
        });
        button_match || axis_match
    }

    fn gamepad_action_just_pressed(&self, action: &str) -> bool {
        let button_match = self
            .gamepad_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| self.is_gamepad_button_just_pressed(*b)));
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs.iter().any(|s| {
                self.connected_pads().any(|p| {
                    self.axis_spec_active(p, *s, false) && !self.axis_spec_active(p, *s, true)
                })
            })
        });
        button_match || axis_match
    }

    fn gamepad_action_just_released(&self, action: &str) -> bool {
        let button_match = self.gamepad_actions.get(action).is_some_and(|btns| {
            btns.iter()
                .any(|b| self.is_gamepad_button_just_released(*b))
        });
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs.iter().any(|s| {
                self.gamepads.values().any(|p| {
                    self.axis_spec_active(p, *s, true) && !self.axis_spec_active(p, *s, false)
                })
            })
        });
        button_match || axis_match
    }

    /// Is an axis-as-button binding active on this pad?
    ///
    /// `prev` selects the previous frame's snapshot instead of the current
    /// value — that comparison is the whole of axis edge detection.
    fn axis_spec_active(&self, pad: &GamepadState, spec: AxisSpec, prev: bool) -> bool {
        let value = if prev {
            pad.prev_axis(spec.axis, self.gamepad_deadzone)
        } else {
            pad.axis(spec.axis, self.gamepad_deadzone)
        };
        axis_dir_active(value, spec.dir, self.gamepad_axis_threshold)
    }

    // ── Action queries: scoped to one pad (local multiplayer) ──
    //
    // These consider ONLY the gamepad bindings. Keyboard and mouse bindings are
    // deliberately ignored: they don't belong to any pad, and in a split-screen
    // game player 1's keyboard must not drive player 2.

    pub fn is_action_pressed_on(&self, pad: GamepadId, action: &str) -> bool {
        let Some(state) = self.gamepads.get(&pad).filter(|p| p.connected) else {
            return false;
        };
        let button_match = self
            .gamepad_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| state.is_pressed(*b)));
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs
                .iter()
                .any(|s| self.axis_spec_active(state, *s, false))
        });
        button_match || axis_match
    }

    pub fn is_action_just_pressed_on(&self, pad: GamepadId, action: &str) -> bool {
        let Some(state) = self.gamepads.get(&pad).filter(|p| p.connected) else {
            return false;
        };
        let button_match = self
            .gamepad_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| state.is_just_pressed(*b)));
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs.iter().any(|s| {
                self.axis_spec_active(state, *s, false) && !self.axis_spec_active(state, *s, true)
            })
        });
        button_match || axis_match
    }

    pub fn is_action_just_released_on(&self, pad: GamepadId, action: &str) -> bool {
        let Some(state) = self.gamepads.get(&pad) else {
            return false;
        };
        let button_match = self
            .gamepad_actions
            .get(action)
            .is_some_and(|btns| btns.iter().any(|b| state.is_just_released(*b)));
        let axis_match = self.gamepad_axis_actions.get(action).is_some_and(|specs| {
            specs.iter().any(|s| {
                self.axis_spec_active(state, *s, true) && !self.axis_spec_active(state, *s, false)
            })
        });
        button_match || axis_match
    }

    // ── Binding introspection ──
    //
    // Lets a diagnostic UI render the *actual* game.yaml bindings instead of a
    // hardcoded list that silently drifts out of sync.

    /// Every configured action name, sorted and de-duplicated across all four
    /// binding maps.
    pub fn action_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .actions
            .keys()
            .chain(self.mouse_actions.keys())
            .chain(self.gamepad_actions.keys())
            .chain(self.gamepad_axis_actions.keys())
            .map(|s| s.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    pub fn action_keys(&self, action: &str) -> &[KeyCode] {
        self.actions.get(action).map_or(&[], |v| v.as_slice())
    }

    pub fn action_mouse_buttons(&self, action: &str) -> &[MouseButton] {
        self.mouse_actions.get(action).map_or(&[], |v| v.as_slice())
    }

    pub fn gamepad_action_buttons(&self, action: &str) -> &[GamepadButton] {
        self.gamepad_actions
            .get(action)
            .map_or(&[], |v| v.as_slice())
    }

    pub fn gamepad_action_axes(&self, action: &str) -> &[AxisSpec] {
        self.gamepad_axis_actions
            .get(action)
            .map_or(&[], |v| v.as_slice())
    }
}

/// Is a deadzoned axis reading past the threshold in the given direction?
pub fn axis_dir_active(value: f32, dir: AxisDir, threshold: f32) -> bool {
    match dir {
        AxisDir::Positive => value >= threshold,
        AxisDir::Negative => value <= -threshold,
    }
}

/// Radial (per-stick) deadzone with rescaling.
///
/// Inside `deadzone` the stick reads exactly `(0, 0)`; outside it, the
/// remaining range is rescaled to ramp 0→1 rather than jumping from `deadzone`
/// to 1. Returns the deadzoned `(x, y)`.
///
/// Radial rather than per-axis, for two reasons:
///
/// 1. **It preserves direction.** Both components are scaled by the same
///    factor, so the angle the player is pushing survives exactly. A per-axis
///    deadzone subtracts the threshold from each axis independently, which
///    bends the angle — pushing `(0.9, 0.2)` through a 0.15 per-axis deadzone
///    yields `(0.75, 0.05)`, swinging the direction from ~12.5° to ~3.8°.
/// 2. **The dead region is direction-independent.** A per-axis deadzone carves
///    out a *square*, so a diagonal push has to travel `deadzone * √2` before
///    it registers while a straight push only needs `deadzone` — the stick
///    feels stickier on the diagonals.
pub fn apply_radial_deadzone(x: f32, y: f32, deadzone: f32) -> (f32, f32) {
    // Clamped defensively: this is a public free function, and a deadzone of
    // exactly 1.0 would make the rescale below divide by zero. (`InputState`
    // already clamps at the setter, so this only guards direct callers.)
    let deadzone = deadzone.clamp(0.0, 0.95);
    let magnitude = (x * x + y * y).sqrt();
    if magnitude <= deadzone || magnitude == 0.0 {
        return (0.0, 0.0);
    }
    // Rescale the live band [deadzone, 1] onto [0, 1].
    let scaled = ((magnitude - deadzone) / (1.0 - deadzone)).min(1.0);
    let factor = scaled / magnitude;
    (x * factor, y * factor)
}

/// Map a `game.yaml` key name to a winit `KeyCode`.
///
/// Names use the short, "ergonomic" form (e.g. `"L"`, `"7"`, `"Space"`) —
/// **not** winit's internal identifiers like `"KeyL"` or `"Digit7"`. We
/// surface the short form because game configs are written by humans and
/// the winit names are an implementation detail.
///
/// Coverage:
/// - All 26 letters: `"A".."Z"` → `KeyCode::KeyA..KeyZ`
/// - All 10 digits: `"0".."9"` → `KeyCode::Digit0..Digit9`
/// - Arrows: `"Up" / "Down" / "Left" / "Right"`
/// - Common controls: `Space`, `Enter`/`Return`, `Escape`, `Tab`, `Backspace`,
///   `Delete`, `Home`, `End`, `PageUp`, `PageDown`
/// - Modifiers: `ShiftLeft`, `ShiftRight`, `ControlLeft`, `ControlRight`,
///   `AltLeft`, `AltRight`
/// - Function keys: `F1".."F12"`
///
/// Unknown names (including lowercase like `"a"` or winit-style `"KeyA"`)
/// return `None` and are silently dropped at action-load time.
pub fn string_to_keycode(s: &str) -> Option<KeyCode> {
    match s {
        // ── Letters ──
        "A" => Some(KeyCode::KeyA),
        "B" => Some(KeyCode::KeyB),
        "C" => Some(KeyCode::KeyC),
        "D" => Some(KeyCode::KeyD),
        "E" => Some(KeyCode::KeyE),
        "F" => Some(KeyCode::KeyF),
        "G" => Some(KeyCode::KeyG),
        "H" => Some(KeyCode::KeyH),
        "I" => Some(KeyCode::KeyI),
        "J" => Some(KeyCode::KeyJ),
        "K" => Some(KeyCode::KeyK),
        "L" => Some(KeyCode::KeyL),
        "M" => Some(KeyCode::KeyM),
        "N" => Some(KeyCode::KeyN),
        "O" => Some(KeyCode::KeyO),
        "P" => Some(KeyCode::KeyP),
        "Q" => Some(KeyCode::KeyQ),
        "R" => Some(KeyCode::KeyR),
        "S" => Some(KeyCode::KeyS),
        "T" => Some(KeyCode::KeyT),
        "U" => Some(KeyCode::KeyU),
        "V" => Some(KeyCode::KeyV),
        "W" => Some(KeyCode::KeyW),
        "X" => Some(KeyCode::KeyX),
        "Y" => Some(KeyCode::KeyY),
        "Z" => Some(KeyCode::KeyZ),

        // ── Digits (top row, not numpad) ──
        "0" => Some(KeyCode::Digit0),
        "1" => Some(KeyCode::Digit1),
        "2" => Some(KeyCode::Digit2),
        "3" => Some(KeyCode::Digit3),
        "4" => Some(KeyCode::Digit4),
        "5" => Some(KeyCode::Digit5),
        "6" => Some(KeyCode::Digit6),
        "7" => Some(KeyCode::Digit7),
        "8" => Some(KeyCode::Digit8),
        "9" => Some(KeyCode::Digit9),

        // ── Arrows ──
        "Up" => Some(KeyCode::ArrowUp),
        "Down" => Some(KeyCode::ArrowDown),
        "Left" => Some(KeyCode::ArrowLeft),
        "Right" => Some(KeyCode::ArrowRight),

        // ── Common controls ──
        "Space" => Some(KeyCode::Space),
        "Enter" | "Return" => Some(KeyCode::Enter),
        "Escape" => Some(KeyCode::Escape),
        "Tab" => Some(KeyCode::Tab),
        "Backspace" => Some(KeyCode::Backspace),
        "Delete" => Some(KeyCode::Delete),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "PageUp" => Some(KeyCode::PageUp),
        "PageDown" => Some(KeyCode::PageDown),

        // ── Modifiers ──
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),

        // ── Function keys ──
        "F1" => Some(KeyCode::F1),
        "F2" => Some(KeyCode::F2),
        "F3" => Some(KeyCode::F3),
        "F4" => Some(KeyCode::F4),
        "F5" => Some(KeyCode::F5),
        "F6" => Some(KeyCode::F6),
        "F7" => Some(KeyCode::F7),
        "F8" => Some(KeyCode::F8),
        "F9" => Some(KeyCode::F9),
        "F10" => Some(KeyCode::F10),
        "F11" => Some(KeyCode::F11),
        "F12" => Some(KeyCode::F12),

        _ => None,
    }
}

pub fn string_to_mouse_button(s: &str) -> Option<MouseButton> {
    match s {
        "Left" => Some(MouseButton::Left),
        "Right" => Some(MouseButton::Right),
        "Middle" => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Map a `game.yaml` `gamepad_buttons` entry to a [`GamepadButton`].
///
/// Canonical names are **positional** (`"South"`, `"LeftShoulder"`, `"DPadUp"`).
/// Xbox-style labels are accepted as aliases for convenience — but note they
/// describe the *Xbox layout*, so `"A"` always means the SOUTH position even on
/// a Nintendo-style pad whose south button is printed "B".
///
/// | Aliases | Button |
/// |---|---|
/// | `A` | `South` |
/// | `B` | `East` |
/// | `X` | `West` |
/// | `Y` | `North` |
/// | `LB`, `L1` | `LeftShoulder` |
/// | `RB`, `R1` | `RightShoulder` |
/// | `LT`, `L2` | `LeftTrigger` |
/// | `RT`, `R2` | `RightTrigger` |
/// | `Back` | `Select` |
/// | `Guide`, `Home` | `Mode` |
/// | `LeftStick` | `LeftThumb` |
/// | `RightStick` | `RightThumb` |
///
/// Unknown names return `None` and are silently dropped at action-load time —
/// the same contract as [`string_to_keycode`].
pub fn string_to_gamepad_button(s: &str) -> Option<GamepadButton> {
    match s {
        // ── Action pad ──
        "South" | "A" => Some(GamepadButton::South),
        "East" | "B" => Some(GamepadButton::East),
        "West" | "X" => Some(GamepadButton::West),
        "North" | "Y" => Some(GamepadButton::North),

        // ── Shoulders ──
        "LeftShoulder" | "LB" | "L1" => Some(GamepadButton::LeftShoulder),
        "RightShoulder" | "RB" | "R1" => Some(GamepadButton::RightShoulder),

        // ── Analog triggers ──
        "LeftTrigger" | "LT" | "L2" => Some(GamepadButton::LeftTrigger),
        "RightTrigger" | "RT" | "R2" => Some(GamepadButton::RightTrigger),

        // ── Menu cluster ──
        "Select" | "Back" => Some(GamepadButton::Select),
        "Start" => Some(GamepadButton::Start),
        "Mode" | "Guide" | "Home" => Some(GamepadButton::Mode),

        // ── Stick clicks ──
        "LeftThumb" | "LeftStick" => Some(GamepadButton::LeftThumb),
        "RightThumb" | "RightStick" => Some(GamepadButton::RightThumb),

        // ── D-pad ──
        "DPadUp" => Some(GamepadButton::DPadUp),
        "DPadDown" => Some(GamepadButton::DPadDown),
        "DPadLeft" => Some(GamepadButton::DPadLeft),
        "DPadRight" => Some(GamepadButton::DPadRight),

        _ => None,
    }
}

/// Map a bare axis name to a [`GamepadAxis`], with no direction.
/// Used by VDP's `{"action": "axis", "axis": …}` payloads.
pub fn string_to_gamepad_axis(s: &str) -> Option<GamepadAxis> {
    match s {
        "LeftStickX" => Some(GamepadAxis::LeftStickX),
        "LeftStickY" => Some(GamepadAxis::LeftStickY),
        "RightStickX" => Some(GamepadAxis::RightStickX),
        "RightStickY" => Some(GamepadAxis::RightStickY),
        _ => None,
    }
}

/// Parse a `game.yaml` `gamepad_axes` binding into an [`AxisSpec`].
///
/// Two forms are accepted:
/// - **Named** (recommended in YAML): `"LeftStickUp"`, `"LeftStickDown"`,
///   `"LeftStickLeft"`, `"LeftStickRight"`, and the `RightStick*` equivalents.
/// - **Suffix**: `"LeftStickX-"`, `"LeftStickY+"`.
///
/// Prefer the named form when writing configs — `"LeftStickY+"` requires the
/// reader to remember that Y is up-positive, `"LeftStickUp"` does not.
///
/// A bare axis name (`"LeftStickX"`) is deliberately **rejected**: a whole axis
/// isn't a boolean, and silently guessing a direction would be worse than a
/// dropped binding.
pub fn parse_gamepad_axis_spec(s: &str) -> Option<AxisSpec> {
    // Named form first — it's the documented recommendation.
    let named = match s {
        // Y is up-positive, so Up => Positive.
        "LeftStickUp" => Some((GamepadAxis::LeftStickY, AxisDir::Positive)),
        "LeftStickDown" => Some((GamepadAxis::LeftStickY, AxisDir::Negative)),
        "LeftStickRight" => Some((GamepadAxis::LeftStickX, AxisDir::Positive)),
        "LeftStickLeft" => Some((GamepadAxis::LeftStickX, AxisDir::Negative)),
        "RightStickUp" => Some((GamepadAxis::RightStickY, AxisDir::Positive)),
        "RightStickDown" => Some((GamepadAxis::RightStickY, AxisDir::Negative)),
        "RightStickRight" => Some((GamepadAxis::RightStickX, AxisDir::Positive)),
        "RightStickLeft" => Some((GamepadAxis::RightStickX, AxisDir::Negative)),
        _ => None,
    };
    if let Some((axis, dir)) = named {
        return Some(AxisSpec { axis, dir });
    }

    // Suffix form: strip a trailing '+' / '-' and resolve the bare axis name.
    let (base, dir) = match s.strip_suffix('+') {
        Some(base) => (base, AxisDir::Positive),
        None => (s.strip_suffix('-')?, AxisDir::Negative),
    };
    Some(AxisSpec {
        axis: string_to_gamepad_axis(base)?,
        dir,
    })
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Unit tests — pure logic, no winit event loop required
// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_action_config(keys: &[&str], mouse_buttons: &[&str]) -> ActionConfig {
        ActionConfig {
            keys: keys.iter().map(|s| s.to_string()).collect(),
            mouse_buttons: mouse_buttons.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Gamepad sibling of `make_action_config`. Kept separate rather than
    /// widening that helper to four positional slices, which reads as noise at
    /// every call site that only cares about keys.
    fn make_gamepad_action_config(gamepad_buttons: &[&str], gamepad_axes: &[&str]) -> ActionConfig {
        ActionConfig {
            gamepad_buttons: gamepad_buttons.iter().map(|s| s.to_string()).collect(),
            gamepad_axes: gamepad_axes.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// An `InputState` with one action bound to the given gamepad bindings.
    fn input_with_gamepad_action(action: &str, buttons: &[&str], axes: &[&str]) -> InputState {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            action.to_string(),
            make_gamepad_action_config(buttons, axes),
        );
        input.load_actions(&actions);
        input
    }

    const PAD0: GamepadId = GamepadId::new(0);
    const PAD1: GamepadId = GamepadId::new(1);

    #[test]
    fn string_to_keycode_known_keys() {
        assert_eq!(string_to_keycode("Space"), Some(KeyCode::Space));
        assert_eq!(string_to_keycode("Enter"), Some(KeyCode::Enter));
        assert_eq!(string_to_keycode("Return"), Some(KeyCode::Enter));
        assert_eq!(string_to_keycode("Escape"), Some(KeyCode::Escape));
        assert_eq!(string_to_keycode("Up"), Some(KeyCode::ArrowUp));
        assert_eq!(string_to_keycode("Down"), Some(KeyCode::ArrowDown));
        assert_eq!(string_to_keycode("Left"), Some(KeyCode::ArrowLeft));
        assert_eq!(string_to_keycode("Right"), Some(KeyCode::ArrowRight));
        assert_eq!(string_to_keycode("A"), Some(KeyCode::KeyA));
        assert_eq!(string_to_keycode("W"), Some(KeyCode::KeyW));
        assert_eq!(string_to_keycode("ShiftLeft"), Some(KeyCode::ShiftLeft));
    }

    #[test]
    fn string_to_keycode_all_letters() {
        // The L key was historically missing from the whitelist, which silently
        // broke `aoi-demo`'s `[L] toggle_lod` action. Make sure the full alphabet
        // is wired up so similar regressions are caught at unit-test time.
        assert_eq!(string_to_keycode("L"), Some(KeyCode::KeyL));
        assert_eq!(string_to_keycode("M"), Some(KeyCode::KeyM));
        assert_eq!(string_to_keycode("Q"), Some(KeyCode::KeyQ));
        assert_eq!(string_to_keycode("Y"), Some(KeyCode::KeyY));
    }

    #[test]
    fn string_to_keycode_digits_and_function_keys() {
        assert_eq!(string_to_keycode("0"), Some(KeyCode::Digit0));
        assert_eq!(string_to_keycode("7"), Some(KeyCode::Digit7));
        assert_eq!(string_to_keycode("F1"), Some(KeyCode::F1));
        assert_eq!(string_to_keycode("F12"), Some(KeyCode::F12));
    }

    #[test]
    fn string_to_keycode_unknown_returns_none() {
        assert_eq!(string_to_keycode(""), None);
        assert_eq!(string_to_keycode("space"), None); // case-sensitive
        // We deliberately don't accept winit's internal "KeyX" form — game.yaml
        // uses the short "X" form. If users write "KeyL" they get None and the
        // action silently has no keys; that's a config error we surface at
        // action-load time (empty action vector).
        assert_eq!(string_to_keycode("KeyL"), None);
        assert_eq!(string_to_keycode("Digit3"), None);
    }

    #[test]
    fn string_to_mouse_button_known() {
        assert_eq!(string_to_mouse_button("Left"), Some(MouseButton::Left));
        assert_eq!(string_to_mouse_button("Right"), Some(MouseButton::Right));
        assert_eq!(string_to_mouse_button("Middle"), Some(MouseButton::Middle));
        assert_eq!(string_to_mouse_button("X"), None);
    }

    #[test]
    fn key_press_sets_pressed_and_just_pressed() {
        let mut input = InputState::new();
        input.on_key_pressed(KeyCode::Space);
        assert!(input.is_key_pressed(KeyCode::Space));
        assert!(input.is_key_just_pressed(KeyCode::Space));
    }

    #[test]
    fn key_just_pressed_clears_after_begin_frame() {
        let mut input = InputState::new();
        input.on_key_pressed(KeyCode::Space);
        assert!(input.is_key_just_pressed(KeyCode::Space));
        input.begin_frame();
        // Still held, but no longer "just" pressed
        assert!(input.is_key_pressed(KeyCode::Space));
        assert!(!input.is_key_just_pressed(KeyCode::Space));
    }

    #[test]
    fn key_release_clears_pressed() {
        let mut input = InputState::new();
        input.on_key_pressed(KeyCode::KeyA);
        input.begin_frame();
        input.on_key_released(KeyCode::KeyA);
        assert!(!input.is_key_pressed(KeyCode::KeyA));
    }

    #[test]
    fn key_repeated_press_does_not_retrigger_just_pressed() {
        let mut input = InputState::new();
        input.on_key_pressed(KeyCode::Space);
        input.begin_frame();
        // Already held — pressing again on the same key should NOT mark just_pressed
        input.on_key_pressed(KeyCode::Space);
        assert!(!input.is_key_just_pressed(KeyCode::Space));
    }

    #[test]
    fn mouse_position_tracks_movement() {
        let mut input = InputState::new();
        input.on_mouse_moved(123.0, 456.0);
        assert_eq!(input.mouse_position(), (123.0, 456.0));
    }

    #[test]
    fn mouse_button_state_machine() {
        let mut input = InputState::new();
        input.on_mouse_button_pressed(MouseButton::Left);
        assert!(input.is_mouse_button_pressed(MouseButton::Left));
        assert!(input.is_mouse_button_just_pressed(MouseButton::Left));
        input.begin_frame();
        assert!(input.is_mouse_button_pressed(MouseButton::Left));
        assert!(!input.is_mouse_button_just_pressed(MouseButton::Left));
        input.on_mouse_button_released(MouseButton::Left);
        assert!(!input.is_mouse_button_pressed(MouseButton::Left));
    }

    #[test]
    fn action_mapping_keyboard() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert("jump".to_string(), make_action_config(&["Space"], &[]));
        input.load_actions(&actions);

        assert!(!input.is_action_just_pressed("jump"));
        input.on_key_pressed(KeyCode::Space);
        assert!(input.is_action_just_pressed("jump"));
        assert!(input.is_action_pressed("jump"));
    }

    #[test]
    fn action_mapping_mouse() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert("attack".to_string(), make_action_config(&[], &["Left"]));
        input.load_actions(&actions);

        input.on_mouse_button_pressed(MouseButton::Left);
        assert!(input.is_action_just_pressed("attack"));
    }

    #[test]
    fn action_mapping_mixed_keyboard_and_mouse() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            "fire".to_string(),
            make_action_config(&["Space", "Enter"], &["Left", "Right"]),
        );
        input.load_actions(&actions);

        input.on_mouse_button_pressed(MouseButton::Right);
        assert!(input.is_action_just_pressed("fire"));
        input.begin_frame();

        input.on_key_pressed(KeyCode::Enter);
        assert!(input.is_action_just_pressed("fire"));
    }

    #[test]
    fn action_with_invalid_keys_filters_them_out() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            "jump".to_string(),
            make_action_config(&["BogusKey", "Space"], &[]),
        );
        input.load_actions(&actions);

        input.on_key_pressed(KeyCode::Space);
        assert!(input.is_action_just_pressed("jump"));
    }

    #[test]
    fn unknown_action_returns_false() {
        let input = InputState::new();
        assert!(!input.is_action_just_pressed("nonexistent"));
        assert!(!input.is_action_pressed("nonexistent"));
    }

    #[test]
    fn chars_received_buffered_and_cleared_each_frame() {
        let mut input = InputState::new();
        input.on_char_received('a');
        input.on_char_received('b');
        assert_eq!(input.chars_this_frame(), &['a', 'b']);
        input.begin_frame();
        assert!(input.chars_this_frame().is_empty());
    }

    #[test]
    fn ime_commit_buffered_and_cleared_each_frame() {
        let mut input = InputState::new();
        assert_eq!(input.ime_commit(), "");
        input.on_ime_commit("你好");
        assert_eq!(input.ime_commit(), "你好");
        // Multiple commits in the same frame concatenate.
        input.on_ime_commit("世界");
        assert_eq!(input.ime_commit(), "你好世界");
        input.begin_frame();
        assert_eq!(input.ime_commit(), "");
    }

    #[test]
    fn ime_commit_clears_active_preedit() {
        let mut input = InputState::new();
        input.on_ime_preedit("nih".to_string(), Some(3));
        assert!(input.ime_preedit().is_some());
        input.on_ime_commit("你");
        assert!(input.ime_preedit().is_none());
    }

    #[test]
    fn ime_preedit_persists_across_frames_until_cleared() {
        let mut input = InputState::new();
        input.on_ime_preedit("ni".to_string(), Some(2));
        let pe = input.ime_preedit().expect("preedit set");
        assert_eq!(pe.text, "ni");
        assert_eq!(pe.cursor_byte, Some(2));

        // begin_frame must NOT clear the preedit (it's a stateful IME composition).
        input.begin_frame();
        assert!(input.ime_preedit().is_some());

        // Empty preedit text clears it.
        input.on_ime_preedit(String::new(), None);
        assert!(input.ime_preedit().is_none());
    }

    #[test]
    fn ime_preedit_explicit_clear() {
        let mut input = InputState::new();
        input.on_ime_preedit("x".to_string(), Some(1));
        input.clear_ime_preedit();
        assert!(input.ime_preedit().is_none());
    }

    #[test]
    fn scroll_delta_accumulates_within_frame() {
        let mut input = InputState::new();
        input.on_mouse_scroll(0.0, 1.0);
        input.on_mouse_scroll(2.0, 3.0);
        assert_eq!(input.mouse_scroll_delta(), 4.0);
        assert_eq!(input.mouse_scroll_delta_x(), 2.0);
        input.begin_frame();
        assert_eq!(input.mouse_scroll_delta(), 0.0);
        assert_eq!(input.mouse_scroll_delta_x(), 0.0);
    }

    // ─────────────────────────────────────────────────────────────────
    // Previously-missing keyboard/mouse getters
    //
    // `docs/api.md` documented all five of these long before they existed;
    // the internal `just_released` state was always tracked, just unreachable.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn key_just_released_getter() {
        let mut input = InputState::new();
        input.on_key_pressed(KeyCode::Space);
        assert!(!input.is_key_just_released(KeyCode::Space));
        input.begin_frame();
        input.on_key_released(KeyCode::Space);
        assert!(input.is_key_just_released(KeyCode::Space));
        input.begin_frame();
        assert!(!input.is_key_just_released(KeyCode::Space));
    }

    #[test]
    fn mouse_button_just_released_getter() {
        let mut input = InputState::new();
        input.on_mouse_button_pressed(MouseButton::Left);
        assert!(!input.is_mouse_button_just_released(MouseButton::Left));
        input.begin_frame();
        input.on_mouse_button_released(MouseButton::Left);
        assert!(input.is_mouse_button_just_released(MouseButton::Left));
        input.begin_frame();
        assert!(!input.is_mouse_button_just_released(MouseButton::Left));
    }

    #[test]
    fn mouse_x_y_match_mouse_position() {
        let mut input = InputState::new();
        input.on_mouse_moved(12.0, 34.0);
        assert_eq!((input.mouse_x(), input.mouse_y()), input.mouse_position());
        assert_eq!(input.mouse_x(), 12.0);
        assert_eq!(input.mouse_y(), 34.0);
    }

    #[test]
    fn action_just_released_for_keyboard_and_mouse() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            "fire".to_string(),
            make_action_config(&["Space"], &["Left"]),
        );
        input.load_actions(&actions);

        input.on_key_pressed(KeyCode::Space);
        input.begin_frame();
        input.on_key_released(KeyCode::Space);
        assert!(input.is_action_just_released("fire"));

        input.begin_frame();
        input.on_mouse_button_pressed(MouseButton::Left);
        input.begin_frame();
        input.on_mouse_button_released(MouseButton::Left);
        assert!(input.is_action_just_released("fire"));
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: name parsing
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn string_to_gamepad_button_canonical_names() {
        assert_eq!(
            string_to_gamepad_button("South"),
            Some(GamepadButton::South)
        );
        assert_eq!(string_to_gamepad_button("East"), Some(GamepadButton::East));
        assert_eq!(
            string_to_gamepad_button("North"),
            Some(GamepadButton::North)
        );
        assert_eq!(string_to_gamepad_button("West"), Some(GamepadButton::West));
        assert_eq!(
            string_to_gamepad_button("DPadUp"),
            Some(GamepadButton::DPadUp)
        );
        assert_eq!(
            string_to_gamepad_button("Start"),
            Some(GamepadButton::Start)
        );
        assert_eq!(
            string_to_gamepad_button("Select"),
            Some(GamepadButton::Select)
        );
        assert_eq!(string_to_gamepad_button("Mode"), Some(GamepadButton::Mode));
        assert_eq!(
            string_to_gamepad_button("LeftThumb"),
            Some(GamepadButton::LeftThumb)
        );
    }

    #[test]
    fn string_to_gamepad_button_shoulder_vs_trigger_are_not_swapped() {
        // gilrs names the SHOULDER `LeftTrigger` and the ANALOG TRIGGER
        // `LeftTrigger2`. We rename at the platform boundary; this test pins
        // the vibe_input side of that contract so a future refactor of the
        // translation layer can't quietly swap them.
        assert_eq!(
            string_to_gamepad_button("LeftShoulder"),
            Some(GamepadButton::LeftShoulder)
        );
        assert_eq!(
            string_to_gamepad_button("LB"),
            Some(GamepadButton::LeftShoulder)
        );
        assert_eq!(
            string_to_gamepad_button("L1"),
            Some(GamepadButton::LeftShoulder)
        );
        assert_eq!(
            string_to_gamepad_button("LeftTrigger"),
            Some(GamepadButton::LeftTrigger)
        );
        assert_eq!(
            string_to_gamepad_button("LT"),
            Some(GamepadButton::LeftTrigger)
        );
        assert_eq!(
            string_to_gamepad_button("L2"),
            Some(GamepadButton::LeftTrigger)
        );
        assert_ne!(
            string_to_gamepad_button("LeftShoulder"),
            string_to_gamepad_button("LeftTrigger")
        );
    }

    #[test]
    fn string_to_gamepad_button_xbox_aliases() {
        // Xbox-LAYOUT aliases: "A" is the SOUTH position regardless of what a
        // given pad prints there.
        assert_eq!(string_to_gamepad_button("A"), Some(GamepadButton::South));
        assert_eq!(string_to_gamepad_button("B"), Some(GamepadButton::East));
        assert_eq!(string_to_gamepad_button("X"), Some(GamepadButton::West));
        assert_eq!(string_to_gamepad_button("Y"), Some(GamepadButton::North));
        assert_eq!(
            string_to_gamepad_button("Back"),
            Some(GamepadButton::Select)
        );
        assert_eq!(string_to_gamepad_button("Guide"), Some(GamepadButton::Mode));
        assert_eq!(
            string_to_gamepad_button("LeftStick"),
            Some(GamepadButton::LeftThumb)
        );
    }

    #[test]
    fn string_to_gamepad_button_unknown_returns_none() {
        assert_eq!(string_to_gamepad_button(""), None);
        assert_eq!(string_to_gamepad_button("south"), None); // case-sensitive
        // gilrs's internal name is deliberately NOT accepted — config authors
        // write our names, and silently taking gilrs's would entrench the
        // shoulder/trigger confusion we renamed away from.
        assert_eq!(string_to_gamepad_button("LeftTrigger2"), None);
        assert_eq!(string_to_gamepad_button("Bogus"), None);
    }

    #[test]
    fn gamepad_button_all_and_name_round_trip() {
        // Every variant in ALL must survive name() -> string_to_gamepad_button().
        for button in GamepadButton::ALL {
            assert_eq!(
                string_to_gamepad_button(button.name()),
                Some(button),
                "{} did not round-trip",
                button.name()
            );
        }
    }

    #[test]
    fn string_to_gamepad_axis_known_and_unknown() {
        assert_eq!(
            string_to_gamepad_axis("LeftStickX"),
            Some(GamepadAxis::LeftStickX)
        );
        assert_eq!(
            string_to_gamepad_axis("RightStickY"),
            Some(GamepadAxis::RightStickY)
        );
        assert_eq!(string_to_gamepad_axis("DPadX"), None);
        assert_eq!(string_to_gamepad_axis(""), None);
    }

    #[test]
    fn parse_gamepad_axis_spec_suffix_and_named_forms_agree() {
        // Y is up-positive, so "Up" == "Y+". If these two ever disagree the
        // named form has silently become a different binding.
        assert_eq!(
            parse_gamepad_axis_spec("LeftStickUp"),
            parse_gamepad_axis_spec("LeftStickY+")
        );
        assert_eq!(
            parse_gamepad_axis_spec("LeftStickDown"),
            parse_gamepad_axis_spec("LeftStickY-")
        );
        assert_eq!(
            parse_gamepad_axis_spec("LeftStickLeft"),
            parse_gamepad_axis_spec("LeftStickX-")
        );
        assert_eq!(
            parse_gamepad_axis_spec("RightStickRight"),
            parse_gamepad_axis_spec("RightStickX+")
        );
        assert_eq!(
            parse_gamepad_axis_spec("LeftStickUp"),
            Some(AxisSpec {
                axis: GamepadAxis::LeftStickY,
                dir: AxisDir::Positive
            })
        );
    }

    #[test]
    fn parse_gamepad_axis_spec_rejects_bare_axis_and_garbage() {
        // A whole axis isn't a boolean — guessing a direction would be worse
        // than dropping the binding.
        assert_eq!(parse_gamepad_axis_spec("LeftStickX"), None);
        assert_eq!(parse_gamepad_axis_spec(""), None);
        assert_eq!(parse_gamepad_axis_spec("+"), None);
        assert_eq!(parse_gamepad_axis_spec("Bogus+"), None);
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: button state machine
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn gamepad_button_state_machine() {
        let mut input = InputState::new();
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        assert!(input.is_gamepad_button_pressed(GamepadButton::South));
        assert!(input.is_gamepad_button_just_pressed(GamepadButton::South));
        assert_eq!(input.gamepad_button_value(GamepadButton::South), 1.0);

        input.begin_frame();
        assert!(input.is_gamepad_button_pressed(GamepadButton::South));
        assert!(!input.is_gamepad_button_just_pressed(GamepadButton::South));

        input.on_gamepad_button_released(PAD0, GamepadButton::South);
        assert!(!input.is_gamepad_button_pressed(GamepadButton::South));
        assert!(input.is_gamepad_button_just_released(GamepadButton::South));
        assert_eq!(input.gamepad_button_value(GamepadButton::South), 0.0);
    }

    #[test]
    fn gamepad_repeated_press_does_not_retrigger_just_pressed() {
        // Mirrors `key_repeated_press_does_not_retrigger_just_pressed`.
        let mut input = InputState::new();
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        input.begin_frame();
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        assert!(!input.is_gamepad_button_just_pressed(GamepadButton::South));
    }

    #[test]
    fn gamepad_auto_vivifies_pad_and_marks_it_connected() {
        // A button event for an unknown pad must create it, so VDP-simulated
        // pads (and events racing ahead of `Connected`) still work.
        let mut input = InputState::new();
        assert_eq!(input.gamepad_count(), 0);
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        assert_eq!(input.gamepad_count(), 1);
        assert!(input.is_gamepad_connected(PAD0));
        assert_eq!(input.connected_gamepads(), vec![PAD0]);
    }

    #[test]
    fn gamepad_connect_sets_name_and_is_reported_for_one_frame() {
        let mut input = InputState::new();
        input.on_gamepad_connected(PAD0, "Test Pad".to_string());
        assert_eq!(input.gamepad_name(PAD0), Some("Test Pad"));
        assert_eq!(input.gamepads_connected_this_frame(), &[PAD0]);
        input.begin_frame();
        assert!(input.gamepads_connected_this_frame().is_empty());
        // Still connected — only the per-frame edge list was cleared.
        assert!(input.is_gamepad_connected(PAD0));
    }

    #[test]
    fn gamepad_disconnect_clears_held_state_and_reports_release_edges() {
        // The "unplugged while holding right, character walks into a wall
        // forever" regression.
        let mut input = InputState::new();
        input.on_gamepad_connected(PAD0, "Pad".to_string());
        input.on_gamepad_button_pressed(PAD0, GamepadButton::DPadRight);
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, 1.0);
        input.begin_frame();
        assert!(input.is_gamepad_button_pressed(GamepadButton::DPadRight));

        input.on_gamepad_disconnected(PAD0);
        assert!(!input.is_gamepad_connected(PAD0));
        assert_eq!(input.gamepad_count(), 0);
        assert!(!input.is_gamepad_button_pressed(GamepadButton::DPadRight));
        assert_eq!(input.gamepad_axis(GamepadAxis::LeftStickX), 0.0);
        assert_eq!(input.gamepads_disconnected_this_frame(), &[PAD0]);
        // Held input ended, so a release edge is reported.
        assert!(input.is_gamepad_button_just_released(GamepadButton::DPadRight));
    }

    #[test]
    fn gamepad_entry_survives_disconnect_so_ids_stay_stable() {
        // gilrs reuses ids by device UUID; keeping the entry preserves the
        // player <-> pad association across a replug.
        let mut input = InputState::new();
        input.on_gamepad_connected(PAD0, "Pad".to_string());
        input.on_gamepad_disconnected(PAD0);
        assert!(input.gamepad(PAD0).is_some());
        assert_eq!(input.gamepad_name(PAD0), Some("Pad"));
        input.begin_frame();
        input.on_gamepad_connected(PAD0, "Pad".to_string());
        assert!(input.is_gamepad_connected(PAD0));
    }

    #[test]
    fn gamepad_analog_trigger_value_is_tracked_without_faking_edges() {
        let mut input = InputState::new();
        input.on_gamepad_button_value(PAD0, GamepadButton::RightTrigger, 0.75);
        assert!((input.gamepad_button_value(GamepadButton::RightTrigger) - 0.75).abs() < 1e-6);
        // A value update alone must NOT synthesize a press edge — gilrs sends
        // ButtonPressed separately, and doing both would double-fire.
        assert!(!input.is_gamepad_button_just_pressed(GamepadButton::RightTrigger));
        assert!(!input.is_gamepad_button_pressed(GamepadButton::RightTrigger));
    }

    #[test]
    fn gamepad_button_value_is_clamped() {
        let mut input = InputState::new();
        input.on_gamepad_button_value(PAD0, GamepadButton::LeftTrigger, 5.0);
        assert_eq!(input.gamepad_button_value(GamepadButton::LeftTrigger), 1.0);
        input.on_gamepad_button_value(PAD0, GamepadButton::LeftTrigger, -3.0);
        assert_eq!(input.gamepad_button_value(GamepadButton::LeftTrigger), 0.0);
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: deadzone
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn apply_radial_deadzone_zeroes_center_and_rescales_edge() {
        // Inside the deadzone: exactly zero on both axes.
        assert_eq!(apply_radial_deadzone(0.1, 0.1, 0.15), (0.0, 0.0));
        assert_eq!(apply_radial_deadzone(0.0, 0.0, 0.15), (0.0, 0.0));

        // Full deflection stays full.
        let (x, y) = apply_radial_deadzone(1.0, 0.0, 0.15);
        assert!((x - 1.0).abs() < 1e-6, "x was {x}");
        assert_eq!(y, 0.0);

        // Halfway: rescaled, NOT passed through. (0.5 - 0.15) / (1 - 0.15)
        // = 0.35 / 0.85 = 0.41176…  Asserting the actual formula rather than
        // merely "non-zero" — a broken rescale still produces non-zero.
        let (x, _) = apply_radial_deadzone(0.5, 0.0, 0.15);
        assert!((x - 0.411_764_7).abs() < 1e-5, "x was {x}");
    }

    #[test]
    fn apply_radial_deadzone_preserves_direction() {
        // The main reason we're radial: both components scale by the same
        // factor, so the angle the player is pushing survives. A per-axis
        // deadzone would subtract 0.15 from each independently, turning
        // (0.9, 0.2) into (0.75, 0.05) and swinging the angle from ~12.5° to
        // ~3.8°.
        let (x, y) = apply_radial_deadzone(0.9, 0.2, 0.15);
        let original_ratio = 0.9 / 0.2;
        let deadzoned_ratio = x / y;
        assert!(
            (original_ratio - deadzoned_ratio).abs() < 1e-4,
            "direction changed: {original_ratio} vs {deadzoned_ratio}"
        );
    }

    #[test]
    fn apply_radial_deadzone_region_is_direction_independent() {
        // The dead region is a circle, so what matters is magnitude alone —
        // never which way the stick is pointing. With a per-axis (square)
        // deadzone a diagonal push would need `dz * sqrt(2)` of travel to
        // register while a straight push needs only `dz`.
        let dz = 0.15;
        // Just inside the radius, along three different directions: all dead.
        for (x, y) in [(0.14, 0.0), (0.0, -0.14), (0.09, 0.09)] {
            assert_eq!(
                apply_radial_deadzone(x, y, dz),
                (0.0, 0.0),
                "({x}, {y}) should be inside the deadzone"
            );
        }
        // Just outside the radius, same three directions: all live.
        for (x, y) in [(0.2, 0.0), (0.0, -0.2), (0.15, 0.15)] {
            let (ox, oy) = apply_radial_deadzone(x, y, dz);
            assert!(
                ox != 0.0 || oy != 0.0,
                "({x}, {y}) should be outside the deadzone"
            );
        }
    }

    #[test]
    fn deadzoned_axis_reads_zero_while_raw_still_drifts() {
        // This split is why the deadzone is applied at read time: a tester can
        // show both, and games see clean zeros.
        let mut input = InputState::new();
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -0.05);
        assert_eq!(input.gamepad_axis(GamepadAxis::LeftStickX), 0.0);
        assert!((input.gamepad_axis_raw(GamepadAxis::LeftStickX) + 0.05).abs() < 1e-6);
    }

    #[test]
    fn gamepad_axis_value_is_clamped() {
        let mut input = InputState::new();
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -9.0);
        assert_eq!(input.gamepad_axis_raw(GamepadAxis::LeftStickX), -1.0);
    }

    #[test]
    fn configure_gamepad_clamps_out_of_range_values() {
        let mut input = InputState::new();
        // Defaults first.
        assert_eq!(input.gamepad_deadzone(), DEFAULT_GAMEPAD_DEADZONE);
        assert_eq!(
            input.gamepad_axis_threshold(),
            DEFAULT_GAMEPAD_AXIS_THRESHOLD
        );

        input.configure_gamepad(&GamepadConfig {
            deadzone: 5.0,
            axis_threshold: 0.0,
        });
        // A deadzone of 1.0+ would make the stick unreadable; a threshold of 0
        // would latch axis actions on permanently.
        assert_eq!(input.gamepad_deadzone(), 0.95);
        assert_eq!(input.gamepad_axis_threshold(), 0.05);

        input.configure_gamepad(&GamepadConfig {
            deadzone: -1.0,
            axis_threshold: 9.0,
        });
        assert_eq!(input.gamepad_deadzone(), 0.0);
        assert_eq!(input.gamepad_axis_threshold(), 1.0);
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: action mapping
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn gamepad_button_action_mapping() {
        let mut input = input_with_gamepad_action("jump", &["South"], &[]);
        assert!(!input.is_action_pressed("jump"));
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        assert!(input.is_action_pressed("jump"));
        assert!(input.is_action_just_pressed("jump"));
        input.begin_frame();
        input.on_gamepad_button_released(PAD0, GamepadButton::South);
        assert!(input.is_action_just_released("jump"));
    }

    #[test]
    fn gamepad_actions_with_invalid_names_are_filtered_out() {
        // Mirrors `action_with_invalid_keys_filters_them_out`.
        let mut input = input_with_gamepad_action("jump", &["Bogus", "South"], &["Nope", "Bad+"]);
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        assert!(input.is_action_just_pressed("jump"));
        // The garbage axis specs were dropped, leaving no axis bindings at all.
        assert!(input.gamepad_action_axes("jump").is_empty());
    }

    #[test]
    fn action_unions_keyboard_and_gamepad_bindings() {
        // The point of the whole design: one action, many input devices.
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            "move_left".to_string(),
            ActionConfig {
                keys: vec!["A".to_string()],
                gamepad_buttons: vec!["DPadLeft".to_string()],
                gamepad_axes: vec!["LeftStickLeft".to_string()],
                ..Default::default()
            },
        );
        input.load_actions(&actions);

        input.on_key_pressed(KeyCode::KeyA);
        assert!(input.is_action_just_pressed("move_left"));
        input.begin_frame();
        input.on_key_released(KeyCode::KeyA);
        input.begin_frame();

        input.on_gamepad_button_pressed(PAD0, GamepadButton::DPadLeft);
        assert!(input.is_action_just_pressed("move_left"));
        input.begin_frame();
        input.on_gamepad_button_released(PAD0, GamepadButton::DPadLeft);
        input.begin_frame();

        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -1.0);
        assert!(input.is_action_just_pressed("move_left"));
    }

    #[test]
    fn axis_action_pressed_past_threshold_only() {
        let mut input = input_with_gamepad_action("move_left", &[], &["LeftStickLeft"]);
        // Below the 0.5 default threshold (post-deadzone) — inert.
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -0.2);
        assert!(!input.is_action_pressed("move_left"));
        // Full deflection — active.
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -1.0);
        assert!(input.is_action_pressed("move_left"));
        // Wrong direction — the other half of the axis must not fire.
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, 1.0);
        assert!(!input.is_action_pressed("move_left"));
    }

    #[test]
    fn axis_action_just_pressed_only_on_the_crossing_frame() {
        // Pins the `prev_axes_raw` contract: the edge comes from the
        // begin_frame snapshot, not from recomputation.
        let mut input = input_with_gamepad_action("move_left", &[], &["LeftStickLeft"]);
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -1.0);
        assert!(input.is_action_pressed("move_left"));
        assert!(input.is_action_just_pressed("move_left"));

        // Same value, next frame: still held, no longer a fresh edge.
        input.begin_frame();
        assert!(input.is_action_pressed("move_left"));
        assert!(!input.is_action_just_pressed("move_left"));
    }

    #[test]
    fn axis_action_just_released_when_returning_to_center() {
        let mut input = input_with_gamepad_action("move_left", &[], &["LeftStickLeft"]);
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, -1.0);
        input.begin_frame();
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, 0.0);
        assert!(!input.is_action_pressed("move_left"));
        assert!(input.is_action_just_released("move_left"));
        input.begin_frame();
        assert!(!input.is_action_just_released("move_left"));
    }

    #[test]
    fn axis_action_respects_up_positive_y_convention() {
        // Y is up-positive, so pushing the stick "up" is +1.0.
        let mut input = input_with_gamepad_action("aim_up", &[], &["RightStickUp"]);
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::RightStickY, 1.0);
        assert!(input.is_action_pressed("aim_up"));
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::RightStickY, -1.0);
        assert!(!input.is_action_pressed("aim_up"));
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: multiple pads
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn connected_gamepads_is_sorted_ascending() {
        // The BTreeMap guarantee behind "player 1 = lowest id".
        let mut input = InputState::new();
        for idx in [3usize, 1, 2] {
            input.on_gamepad_connected(GamepadId::new(idx), format!("Pad {idx}"));
        }
        assert_eq!(
            input.connected_gamepads(),
            vec![GamepadId::new(1), GamepadId::new(2), GamepadId::new(3)]
        );
        assert_eq!(input.primary_gamepad(), Some(GamepadId::new(1)));
    }

    #[test]
    fn primary_gamepad_skips_disconnected_pads() {
        let mut input = InputState::new();
        input.on_gamepad_connected(PAD0, "First".to_string());
        input.on_gamepad_connected(PAD1, "Second".to_string());
        assert_eq!(input.primary_gamepad(), Some(PAD0));
        input.on_gamepad_disconnected(PAD0);
        assert_eq!(input.primary_gamepad(), Some(PAD1));
    }

    #[test]
    fn merged_queries_span_pads_and_axis_picks_largest_magnitude() {
        let mut input = InputState::new();
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickX, 0.6);
        input.on_gamepad_axis_changed(PAD1, GamepadAxis::LeftStickX, -0.9);
        // -0.9 has the larger magnitude, and the sign must be preserved.
        let merged = input.gamepad_axis(GamepadAxis::LeftStickX);
        assert!(merged < 0.0, "merged was {merged}");
        assert!(merged.abs() > 0.5, "merged was {merged}");

        // Buttons merge as "any pad".
        input.on_gamepad_button_pressed(PAD1, GamepadButton::North);
        assert!(input.is_gamepad_button_pressed(GamepadButton::North));
    }

    #[test]
    fn merged_button_value_takes_max_not_sum() {
        // Two half-pulled triggers must not read as fully pulled.
        let mut input = InputState::new();
        input.on_gamepad_button_value(PAD0, GamepadButton::RightTrigger, 0.5);
        input.on_gamepad_button_value(PAD1, GamepadButton::RightTrigger, 0.5);
        assert!((input.gamepad_button_value(GamepadButton::RightTrigger) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn per_pad_queries_are_independent() {
        let mut input = InputState::new();
        input.on_gamepad_button_pressed(PAD0, GamepadButton::South);
        input.on_gamepad_button_pressed(PAD1, GamepadButton::North);

        assert!(input.is_gamepad_button_pressed_on(PAD0, GamepadButton::South));
        assert!(!input.is_gamepad_button_pressed_on(PAD0, GamepadButton::North));
        assert!(input.is_gamepad_button_pressed_on(PAD1, GamepadButton::North));
        assert!(!input.is_gamepad_button_pressed_on(PAD1, GamepadButton::South));
        assert_eq!(input.gamepad_count(), 2);
    }

    #[test]
    fn per_pad_action_ignores_other_pads() {
        let mut input = input_with_gamepad_action("jump", &["South"], &[]);
        input.on_gamepad_button_pressed(PAD1, GamepadButton::South);
        // Merged query sees it; pad 0's scoped query must not.
        assert!(input.is_action_just_pressed("jump"));
        assert!(input.is_action_just_pressed_on(PAD1, "jump"));
        assert!(!input.is_action_just_pressed_on(PAD0, "jump"));
        assert!(!input.is_action_pressed_on(PAD0, "jump"));
    }

    #[test]
    fn per_pad_action_ignores_keyboard_bindings() {
        // A split-screen game must not let player 1's keyboard drive player 2.
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert(
            "jump".to_string(),
            ActionConfig {
                keys: vec!["Space".to_string()],
                gamepad_buttons: vec!["South".to_string()],
                ..Default::default()
            },
        );
        input.load_actions(&actions);
        input.on_gamepad_connected(PAD0, "Pad".to_string());
        input.on_key_pressed(KeyCode::Space);

        assert!(input.is_action_just_pressed("jump")); // merged: yes
        assert!(!input.is_action_just_pressed_on(PAD0, "jump")); // pad-scoped: no
    }

    #[test]
    fn per_pad_axis_action_is_independent() {
        let mut input = input_with_gamepad_action("move_left", &[], &["LeftStickLeft"]);
        input.on_gamepad_axis_changed(PAD1, GamepadAxis::LeftStickX, -1.0);
        assert!(input.is_action_pressed_on(PAD1, "move_left"));
        assert!(!input.is_action_pressed_on(PAD0, "move_left"));
        assert!(input.is_action_just_pressed_on(PAD1, "move_left"));
    }

    #[test]
    fn disconnected_pad_stops_driving_actions() {
        let mut input = input_with_gamepad_action("move_right", &["DPadRight"], &[]);
        input.on_gamepad_button_pressed(PAD0, GamepadButton::DPadRight);
        input.begin_frame();
        assert!(input.is_action_pressed("move_right"));
        input.on_gamepad_disconnected(PAD0);
        assert!(!input.is_action_pressed("move_right"));
        assert!(!input.is_action_pressed_on(PAD0, "move_right"));
    }

    // ─────────────────────────────────────────────────────────────────
    // Gamepad: binding introspection
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn action_names_unions_all_four_binding_maps_sorted() {
        let mut input = InputState::new();
        let mut actions = HashMap::new();
        actions.insert("zoom".to_string(), make_action_config(&["Z"], &[]));
        actions.insert("click".to_string(), make_action_config(&[], &["Left"]));
        actions.insert(
            "jump".to_string(),
            make_gamepad_action_config(&["South"], &[]),
        );
        actions.insert(
            "aim".to_string(),
            make_gamepad_action_config(&[], &["RightStickUp"]),
        );
        input.load_actions(&actions);

        assert_eq!(input.action_names(), vec!["aim", "click", "jump", "zoom"]);
    }

    #[test]
    fn binding_introspection_returns_parsed_bindings() {
        let input = input_with_gamepad_action("jump", &["South", "A"], &["LeftStickUp"]);
        // "South" and "A" are aliases for the same button, so both parse to it.
        assert_eq!(
            input.gamepad_action_buttons("jump"),
            &[GamepadButton::South, GamepadButton::South]
        );
        assert_eq!(
            input.gamepad_action_axes("jump"),
            &[AxisSpec {
                axis: GamepadAxis::LeftStickY,
                dir: AxisDir::Positive
            }]
        );
        // Unknown actions return empty slices rather than panicking.
        assert!(input.gamepad_action_buttons("nope").is_empty());
        assert!(input.gamepad_action_axes("nope").is_empty());
        assert!(input.action_keys("nope").is_empty());
        assert!(input.action_mouse_buttons("nope").is_empty());
    }

    #[test]
    fn unknown_gamepad_queries_are_inert() {
        let input = InputState::new();
        assert!(!input.is_gamepad_button_pressed(GamepadButton::South));
        assert!(!input.is_gamepad_button_pressed_on(PAD0, GamepadButton::South));
        assert_eq!(input.gamepad_axis(GamepadAxis::LeftStickX), 0.0);
        assert_eq!(input.gamepad_axis_on(PAD0, GamepadAxis::LeftStickX), 0.0);
        assert_eq!(
            input.gamepad_axis_raw_on(PAD0, GamepadAxis::LeftStickX),
            0.0
        );
        assert_eq!(
            input.gamepad_button_value_on(PAD0, GamepadButton::South),
            0.0
        );
        assert_eq!(input.gamepad_count(), 0);
        assert_eq!(input.primary_gamepad(), None);
        assert_eq!(input.gamepad_name(PAD0), None);
        assert!(input.gamepad(PAD0).is_none());
    }

    #[test]
    fn gamepad_state_object_path_matches_flat_queries() {
        // `input.gamepad(id)` is the ergonomic path for `for pad in players`
        // loops; it must agree with the flat `_on` accessors.
        let mut input = InputState::new();
        input.on_gamepad_connected(PAD0, "Pad".to_string());
        input.on_gamepad_button_pressed(PAD0, GamepadButton::West);
        input.on_gamepad_axis_changed(PAD0, GamepadAxis::LeftStickY, 1.0);

        let pad = input.gamepad(PAD0).expect("pad exists");
        assert!(pad.is_connected());
        assert_eq!(pad.name(), "Pad");
        assert_eq!(
            pad.is_pressed(GamepadButton::West),
            input.is_gamepad_button_pressed_on(PAD0, GamepadButton::West)
        );
        assert_eq!(
            pad.is_just_pressed(GamepadButton::West),
            input.is_gamepad_button_just_pressed_on(PAD0, GamepadButton::West)
        );
        assert_eq!(
            pad.axis_raw(GamepadAxis::LeftStickY),
            input.gamepad_axis_raw_on(PAD0, GamepadAxis::LeftStickY)
        );
        assert_eq!(
            pad.axis(GamepadAxis::LeftStickY, input.gamepad_deadzone()),
            input.gamepad_axis_on(PAD0, GamepadAxis::LeftStickY)
        );
    }
}
