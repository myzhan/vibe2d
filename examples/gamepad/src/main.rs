//! Gamepad tester — a diagnostic harness for vibe2d's gamepad support.
//!
//! Everything on screen is a direct readout of [`InputState`] with no smoothing
//! or interpretation, which makes this the manual test for the gilrs →
//! `vibe_input` translation layer:
//!
//! - Pad list with driver-reported names, plus a connect/disconnect log
//! - All 17 `GamepadButton`s as a grid — dim = up, bright = held,
//!   ringed = `just_pressed` this frame
//! - Both sticks as a dot in a box, showing RAW *and* deadzoned values side by
//!   side, with the deadzone drawn as a visible circle
//! - Analog trigger bars driven by `gamepad_button_value`
//! - A live action readout built from `input.action_names()` — i.e. from
//!   `game.yaml`, not from a hardcoded list that could drift out of sync
//! - Rumble test buttons, one per motor (desktop only)
//!
//! ## Rumble: silence is a valid result
//!
//! `strong` and `weak` are two **independent physical motors** (evdev's
//! `strong_magnitude` / `weak_magnitude`), and many pads only wire up one. An
//! 8BitDo Ultimate Wired on Linux, for instance, responds to `weak` only — the
//! ioctl succeeds either way, the motor just isn't there. So one of these
//! buttons doing nothing tells you about your hardware, not about a bug. Games
//! that just want "definitely felt" rumble should set both values.
//!
//! ## Two conventions worth knowing
//!
//! **Y is up-positive.** `GamepadAxis::LeftStickY` follows the SDL convention
//! where pushing the stick up reads `+1.0`, while vibe2d's screen space is
//! y-down. `stick_dot_pos` negates Y when plotting; if the dot ever moves the
//! wrong way, that's the bug.
//!
//! **Shoulders are not triggers.** gilrs names the shoulder button
//! `LeftTrigger` and the analog trigger `LeftTrigger2`. vibe2d renames these to
//! `LeftShoulder` / `LeftTrigger` at the platform boundary, so pressing LB must
//! light the `LeftShoulder` cell and pulling LT must light `LeftTrigger`.

use vibe2d::prelude::*;

// ── Layout ────────────────────────────────────────────────────────
//
// All coordinates are in the virtual resolution, which `game.yaml` pins 1:1 to
// the window (1280x720). Most examples render at half resolution and let the
// engine upscale, but this one is almost entirely small text — at 2x upscale
// every glyph gets resampled and turns mushy. 1:1 costs nothing here (no pixel
// art to keep chunky) and keeps the readout sharp.
const MARGIN: f32 = 16.0;
/// Gutter reserved for the left panel. UI panels auto-size to their content and
/// composite *on top* of the `draw` layer, so anything wider than this would
/// silently hide the button grid — hence `MAX_PANEL_CHARS` below.
const PAD_PANEL_W: f32 = 320.0;
/// Hard character cap for left-panel lines, chosen so `body` (20 px) text stays
/// inside `PAD_PANEL_W`. Driver-reported pad names are long enough to matter:
/// the 8BitDo reports "8BitDo Ultimate Wireless / Pro 2 Wired Controller".
const MAX_PANEL_CHARS: usize = 22;

// Button grid: 17 cells laid out in a fixed-width grid on the left.
const GRID_X: f32 = MARGIN + PAD_PANEL_W + 20.0;
const GRID_Y: f32 = 88.0;
const GRID_COLS: usize = 3;
const CELL_W: f32 = 148.0;
const CELL_H: f32 = 40.0;
const CELL_GAP: f32 = 6.0;

// Stick boxes, to the right of the grid.
const STICK_BOX: f32 = 184.0;
const STICK_X: f32 = GRID_X + (CELL_W + CELL_GAP) * GRID_COLS as f32 + 28.0;
const STICK_Y: f32 = GRID_Y;
const STICK_DOT_R: f32 = 8.0;
/// Radius of the hollow "raw value" reference ring. Deliberately *larger* than
/// the live dot: deadzone rescaling keeps the two within a few pixels of each
/// other most of the time, and a smaller ring would simply vanish behind the
/// dot. As a halo it stays readable whether the two coincide or diverge.
const STICK_RAW_R: f32 = 13.0;

// Trigger bars, below the stick boxes.
const TRIG_W: f32 = STICK_BOX * 2.0 + 24.0;
const TRIG_H: f32 = 22.0;
const TRIG_Y: f32 = STICK_Y + STICK_BOX + 68.0;

// ── Colors ────────────────────────────────────────────────────────
const C_BG: u32 = 0x14161A;
const C_CELL_IDLE: u32 = 0x2A2F38;
const C_CELL_HELD: u32 = 0x4CD964;
const C_EDGE_RING: u32 = 0xFFD24C;
const C_BOX: u32 = 0x333A45;
const C_DEADZONE: u32 = 0x5A4020;
const C_DOT_RAW: u32 = 0x7A7F8A;
const C_DOT_LIVE: u32 = 0x4CA6FF;
const C_TRIG_FILL: u32 = 0xFF8C42;
const C_LABEL: u32 = 0x9AA3B0;

/// How many connect/disconnect lines to keep.
const EVENT_LOG_LEN: usize = 5;

/// Truncate to `max` chars, marking elision with a trailing `~`.
///
/// Char-based rather than byte-based so it can't split a UTF-8 sequence; all the
/// strings here are ASCII in practice, but pad names come from the driver.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('~');
    out
}

// ── Per-frame snapshot ────────────────────────────────────────────
//
// `draw(&self, ..)` gets no `&InputState`, so `update` snapshots everything the
// frame will render. The snapshot is also exactly what `inspect()` serializes,
// which keeps the screen and the integration tests reading one source of truth.

#[derive(Default)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
struct PadView {
    id: usize,
    name: String,
    connected: bool,
    /// Canonical names of every currently-held button.
    pressed: Vec<&'static str>,
    /// Names of buttons that went down this frame.
    just_pressed: Vec<&'static str>,
    /// Deadzoned stick values.
    axes: AxesView,
    /// Raw stick values, before the deadzone.
    axes_raw: AxesView,
    /// Analog trigger readings, 0.0..=1.0.
    lt: f32,
    rt: f32,
}

#[derive(Default, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
struct AxesView {
    lx: f32,
    ly: f32,
    rx: f32,
    ry: f32,
}

#[derive(Default)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
struct ActionView {
    name: String,
    pressed: bool,
    just_pressed: bool,
    just_released: bool,
    /// Human-readable binding summary straight out of `game.yaml`.
    bindings: String,
}

#[derive(Default)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
struct Snapshot {
    pad_count: usize,
    pads: Vec<PadView>,
    actions: Vec<ActionView>,
    deadzone: f32,
    axis_threshold: f32,
}

impl Snapshot {
    /// The pad the detail panes are showing, if any.
    fn selected(&self, index: usize) -> Option<&PadView> {
        self.pads.iter().filter(|p| p.connected).nth(index)
    }
}

// ── Game ──────────────────────────────────────────────────────────

struct GamepadTester {
    /// Index into the *connected* pad list, cycled by the `cycle_pad` action.
    selected: usize,
    /// Rolling connect/disconnect log, newest last.
    event_log: Vec<String>,
    /// Incremented every time a rumble button fires. The VDP test asserts on
    /// this: a headless run can't observe the gilrs call, but it can prove the
    /// `Context` → `GameBridge` → platform queue is wired up.
    rumble_sent: u32,
    snapshot: Snapshot,
    white_tex: TextureId,
    disc_tex: TextureId,
    ring_tex: TextureId,
}

impl GamepadTester {
    /// Fill a solid rectangle using the 1×1 white pixel texture.
    fn rect(&self, screen: &mut Screen, x: f32, y: f32, w: f32, h: f32, color: Color) {
        screen.draw_sprite_tinted(self.white_tex, x, y, w, h, color);
    }

    /// Draw a 1 px outline as four thin rects (the UI layer has no stroke
    /// primitive, and four batched quads are cheaper than a shader).
    fn outline(&self, screen: &mut Screen, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.rect(screen, x, y, w, 1.0, color);
        self.rect(screen, x, y + h - 1.0, w, 1.0, color);
        self.rect(screen, x, y, 1.0, h, color);
        self.rect(screen, x + w - 1.0, y, 1.0, h, color);
    }

    /// Top-left corner of a button grid cell.
    fn cell_origin(index: usize) -> (f32, f32) {
        let col = index % GRID_COLS;
        let row = index / GRID_COLS;
        (
            GRID_X + col as f32 * (CELL_W + CELL_GAP),
            GRID_Y + row as f32 * (CELL_H + CELL_GAP),
        )
    }

    /// Map a stick reading to a screen position inside its box.
    ///
    /// **Y is negated here**: the axis is up-positive, the screen is y-down.
    fn stick_dot_pos(box_x: f32, box_y: f32, x: f32, y: f32) -> (f32, f32) {
        let half = STICK_BOX / 2.0 - STICK_DOT_R - 1.0;
        let cx = box_x + STICK_BOX / 2.0;
        let cy = box_y + STICK_BOX / 2.0;
        (cx + x * half, cy - y * half)
    }

    /// One-line binding summary for an action, e.g.
    /// `keys[Left,A] pad[DPadLeft] axes[LeftStickX-]`.
    fn describe_bindings(input: &InputState, action: &str) -> String {
        let mut parts: Vec<String> = Vec::new();

        let keys = input.action_keys(action);
        if !keys.is_empty() {
            // KeyCode's Debug is winit's internal name ("KeyA"); good enough
            // for a diagnostic readout and it avoids a reverse name table.
            let names: Vec<String> = keys.iter().map(|k| format!("{k:?}")).collect();
            parts.push(format!("keys[{}]", names.join(",")));
        }
        let mice = input.action_mouse_buttons(action);
        if !mice.is_empty() {
            let names: Vec<String> = mice.iter().map(|b| format!("{b:?}")).collect();
            parts.push(format!("mouse[{}]", names.join(",")));
        }
        let buttons = input.gamepad_action_buttons(action);
        if !buttons.is_empty() {
            let names: Vec<&str> = buttons.iter().map(|b| b.name()).collect();
            parts.push(format!("pad[{}]", names.join(",")));
        }
        let axes = input.gamepad_action_axes(action);
        if !axes.is_empty() {
            let names: Vec<String> = axes
                .iter()
                .map(|s| {
                    let sign = match s.dir {
                        AxisDir::Positive => '+',
                        AxisDir::Negative => '-',
                    };
                    format!("{}{}", s.axis.name(), sign)
                })
                .collect();
            parts.push(format!("axes[{}]", names.join(",")));
        }

        if parts.is_empty() {
            "(unbound)".to_string()
        } else {
            parts.join(" ")
        }
    }

    fn build_snapshot(&self, input: &InputState) -> Snapshot {
        let pads = input
            .connected_gamepads()
            .into_iter()
            .filter_map(|id| {
                let pad = input.gamepad(id)?;
                Some(PadView {
                    id: id.index(),
                    name: pad.name().to_string(),
                    connected: pad.is_connected(),
                    pressed: GamepadButton::ALL
                        .iter()
                        .filter(|b| pad.is_pressed(**b))
                        .map(|b| b.name())
                        .collect(),
                    just_pressed: GamepadButton::ALL
                        .iter()
                        .filter(|b| pad.is_just_pressed(**b))
                        .map(|b| b.name())
                        .collect(),
                    axes: AxesView {
                        lx: input.gamepad_axis_on(id, GamepadAxis::LeftStickX),
                        ly: input.gamepad_axis_on(id, GamepadAxis::LeftStickY),
                        rx: input.gamepad_axis_on(id, GamepadAxis::RightStickX),
                        ry: input.gamepad_axis_on(id, GamepadAxis::RightStickY),
                    },
                    axes_raw: AxesView {
                        lx: pad.axis_raw(GamepadAxis::LeftStickX),
                        ly: pad.axis_raw(GamepadAxis::LeftStickY),
                        rx: pad.axis_raw(GamepadAxis::RightStickX),
                        ry: pad.axis_raw(GamepadAxis::RightStickY),
                    },
                    lt: pad.value(GamepadButton::LeftTrigger),
                    rt: pad.value(GamepadButton::RightTrigger),
                })
            })
            .collect();

        let actions = input
            .action_names()
            .into_iter()
            .map(|name| ActionView {
                pressed: input.is_action_pressed(name),
                just_pressed: input.is_action_just_pressed(name),
                just_released: input.is_action_just_released(name),
                bindings: Self::describe_bindings(input, name),
                name: name.to_string(),
            })
            .collect();

        Snapshot {
            pad_count: input.gamepad_count(),
            pads,
            actions,
            deadzone: input.gamepad_deadzone(),
            axis_threshold: input.gamepad_axis_threshold(),
        }
    }

    fn log_event(&mut self, line: String) {
        self.event_log.push(clip(&line, MAX_PANEL_CHARS));
        if self.event_log.len() > EVENT_LOG_LEN {
            self.event_log.remove(0);
        }
    }
}

impl Game for GamepadTester {
    fn new(ctx: &mut Context, renderer: &Renderer) -> Self {
        // Games own their procedural textures — the engine ships none.
        let white_tex = ctx
            .assets
            .register_texture("gamepad_white", renderer.create_white_pixel_texture());
        let disc_tex = ctx.assets.register_texture(
            "gamepad_disc",
            renderer.create_filled_circle_texture("gamepad_disc", 128),
        );
        let ring_tex = ctx.assets.register_texture(
            "gamepad_ring",
            renderer.create_ring_texture("gamepad_ring", 128, 0.10),
        );

        Self {
            selected: 0,
            event_log: Vec::new(),
            rumble_sent: 0,
            snapshot: Snapshot::default(),
            white_tex,
            disc_tex,
            ring_tex,
        }
    }

    fn update(&mut self, _ctx: &mut Context, _dt: f32, input: &InputState) {
        // Connect / disconnect log. Both lists are one-frame-lived, so reading
        // them here catches every event exactly once.
        for id in input.gamepads_connected_this_frame() {
            let name = input.gamepad_name(*id).unwrap_or("?").to_string();
            self.log_event(format!("+ [{}] {}", id.index(), name));
        }
        for id in input.gamepads_disconnected_this_frame() {
            self.log_event(format!("- [{}] disconnected", id.index()));
        }

        // Keep the selection inside the connected pad list — it shrinks when a
        // pad is unplugged.
        let connected = input.gamepad_count();
        if input.is_action_just_pressed("cycle_pad") && connected > 0 {
            self.selected = (self.selected + 1) % connected;
        }
        if connected > 0 && self.selected >= connected {
            self.selected = 0;
        }

        self.snapshot = self.build_snapshot(input);
    }

    fn update_ui(&mut self, ctx: &mut Context, input: &InputState) {
        let vw = ctx.virtual_width;
        let vh = ctx.virtual_height;

        // The UI closure only receives `&mut UiContext`, so click results have
        // to be collected into locals here and acted on after `ui.finish()`.
        let mut rumble_weak = false;
        let mut rumble_strong = false;
        let mut rumble_both = false;

        let mut ui_state = std::mem::take(&mut ctx.ui_state);
        let mut ui = UiContext::new(&mut ui_state, input, vw, vh);

        let panel_style = PanelStyle {
            bg_color: UiColor::new(0.08, 0.10, 0.13, 0.95),
            padding: 6.0,
        };

        // ── Left panel: pad list + event log + rumble buttons ──
        if let Some(font) = ctx.assets.font("body") {
            ui.set_anchor(Anchor::TopLeft);
            ui.set_padding(0.0);
            ui.set_cursor(MARGIN, GRID_Y);
            ui.set_spacing(2.0);

            ui.panel(panel_style.clone(), |ui| {
                ui.label_colored(font, "Gamepads", UiColor::from_hex(0x55BBFF));
                if self.snapshot.pad_count == 0 {
                    ui.label(font, "none detected");
                } else {
                    for (i, pad) in self
                        .snapshot
                        .pads
                        .iter()
                        .filter(|p| p.connected)
                        .enumerate()
                    {
                        let marker = if i == self.selected { ">" } else { " " };
                        let row = format!("{marker} [{}] {}", pad.id, pad.name);
                        ui.label(font, &clip(&row, MAX_PANEL_CHARS));
                    }
                }

                ui.label_colored(font, "Events", UiColor::from_hex(0x55BBFF));
                if self.event_log.is_empty() {
                    ui.label(font, "(none yet)");
                } else {
                    for line in &self.event_log {
                        ui.label(font, line);
                    }
                }

                // `strong` and `weak` are two independent physical motors and
                // plenty of pads only wire up one of them, so the labels name
                // which motor each button drives — "nothing happened" is a
                // legitimate (and useful) result here, not a failure.
                ui.label_colored(font, "Rumble", UiColor::from_hex(0x55BBFF));
                rumble_weak = ui
                    .button_with_id("rumble_weak", font, "weak = light motor")
                    .clicked;
                rumble_strong = ui
                    .button_with_id("rumble_strong", font, "strong = heavy motor")
                    .clicked;
                rumble_both = ui.button_with_id("rumble_both", font, "both").clicked;
                ui.label_colored(font, "silence = this pad", UiColor::from_hex(C_LABEL));
                ui.label_colored(font, "lacks that motor", UiColor::from_hex(C_LABEL));
            });
        }

        // ── Right panel: live action readout, straight from game.yaml ──
        if let Some(font) = ctx.assets.font("small") {
            ui.set_anchor(Anchor::TopLeft);
            ui.set_padding(0.0);
            ui.set_cursor(MARGIN, TRIG_Y + 92.0);
            ui.set_spacing(1.0);

            ui.panel(panel_style.clone(), |ui| {
                ui.label_colored(
                    font,
                    "Actions (from game.yaml)",
                    UiColor::from_hex(0x55BBFF),
                );
                for action in &self.snapshot.actions {
                    // Mark the frame-edge states so tap-vs-hold is visible.
                    let state = if action.just_pressed {
                        "DOWN"
                    } else if action.pressed {
                        "held"
                    } else if action.just_released {
                        "UP  "
                    } else {
                        "  . "
                    };
                    let color = if action.pressed {
                        UiColor::from_hex(C_CELL_HELD)
                    } else {
                        UiColor::from_hex(C_LABEL)
                    };
                    ui.label_colored(
                        font,
                        &format!("{state} {:<11} {}", action.name, action.bindings),
                        color,
                    );
                }
            });
        }

        ui.finish();
        ctx.ui_state = ui_state;

        // Now that `ui_state` is back in `ctx`, the rumble requests can be
        // queued. Short, punchy durations — this is a test, not an effect.
        if rumble_weak {
            ctx.rumble(0.0, 1.0, 250);
            self.rumble_sent += 1;
        }
        if rumble_strong {
            ctx.rumble(1.0, 0.0, 250);
            self.rumble_sent += 1;
        }
        if rumble_both {
            ctx.rumble(1.0, 1.0, 400);
            self.rumble_sent += 1;
        }
    }

    fn draw(&self, ctx: &Context, screen: &mut Screen) {
        let Some(body) = ctx.assets.font("body") else {
            return;
        };

        if let Some(title) = ctx.assets.font("title") {
            screen.draw_text(title, "Vibe2D Gamepad Tester", MARGIN, MARGIN + 8.0);
        }

        // No pad at all: say so plainly and explain both ways to get one.
        if self.snapshot.pad_count == 0 {
            let cx = ctx.virtual_width / 2.0;
            let cy = ctx.virtual_height / 2.0;
            const BOX_W: f32 = 520.0;
            const BOX_H: f32 = 84.0;
            let (bx, by) = (cx - BOX_W / 2.0, cy - BOX_H / 2.0);
            self.rect(screen, bx, by, BOX_W, BOX_H, Color::from_hex(0x1E2229));
            self.outline(screen, bx, by, BOX_W, BOX_H, Color::from_hex(C_BOX));
            screen.draw_text_centered(body, "No gamepad connected", cy - 24.0);
            screen.draw_text_centered(body, "Plug one in, or drive it over VDP", cy + 4.0);
            return;
        }

        let Some(pad) = self.snapshot.selected(self.selected) else {
            return;
        };

        // ── Button grid ──
        // Cell labels use `small`: the longest canonical name is
        // "RightShoulder" (13 chars), which overflows CELL_W at `body` size.
        let cell_font = ctx.assets.font("small").unwrap_or(body);
        for (i, button) in GamepadButton::ALL.iter().enumerate() {
            let (x, y) = Self::cell_origin(i);
            let name = button.name();
            let held = pad.pressed.contains(&name);
            let edge = pad.just_pressed.contains(&name);

            let fill = if held { C_CELL_HELD } else { C_CELL_IDLE };
            self.rect(screen, x, y, CELL_W, CELL_H, Color::from_hex(fill));
            // A `just_pressed` frame gets a bright border, so a single tap is
            // still visible even though `held` also becomes true.
            if edge {
                self.outline(screen, x, y, CELL_W, CELL_H, Color::from_hex(C_EDGE_RING));
            }
            screen.draw_text(cell_font, name, x + 8.0, y + 10.0);
        }

        // ── Stick boxes ──
        for (label, x, y, rx, ry, raw_x, raw_y) in [
            (
                "Left stick",
                STICK_X,
                STICK_Y,
                pad.axes.lx,
                pad.axes.ly,
                pad.axes_raw.lx,
                pad.axes_raw.ly,
            ),
            (
                "Right stick",
                STICK_X + STICK_BOX + 24.0,
                STICK_Y,
                pad.axes.rx,
                pad.axes.ry,
                pad.axes_raw.rx,
                pad.axes_raw.ry,
            ),
        ] {
            screen.draw_text(body, label, x, y - 26.0);
            self.rect(
                screen,
                x,
                y,
                STICK_BOX,
                STICK_BOX,
                Color::from_hex(0x1B1F26),
            );
            self.outline(screen, x, y, STICK_BOX, STICK_BOX, Color::from_hex(C_BOX));

            // Crosshair through the centre.
            let cx = x + STICK_BOX / 2.0;
            let cy = y + STICK_BOX / 2.0;
            self.rect(
                screen,
                x + 1.0,
                cy,
                STICK_BOX - 2.0,
                1.0,
                Color::from_hex(C_BOX),
            );
            self.rect(
                screen,
                cx,
                y + 1.0,
                1.0,
                STICK_BOX - 2.0,
                Color::from_hex(C_BOX),
            );

            // Deadzone made visible: everything inside this ring reads as zero.
            let half = STICK_BOX / 2.0 - STICK_DOT_R - 1.0;
            screen.draw_circle_outline(
                self.ring_tex,
                cx,
                cy,
                self.snapshot.deadzone * half,
                Color::from_hex(C_DEADZONE),
            );

            // Two indicators, with a deliberate visual hierarchy:
            //
            //   • raw  = small DIM HOLLOW ring — a reference marker
            //   • live = solid BRIGHT dot      — the value the game actually sees
            //
            // Both are worth showing (the gap between them is exactly what the
            // deadzone is doing, and deadzone rescaling means they only coincide
            // at full deflection). But drawing both as equal filled discs read as
            // "two cursors fighting each other" rather than "value + reference",
            // so the raw one is now unmistakably secondary.
            let (rawx, rawy) = Self::stick_dot_pos(x, y, raw_x, raw_y);
            screen.draw_circle_outline(
                self.ring_tex,
                rawx,
                rawy,
                STICK_RAW_R,
                Color::from_hex(C_DOT_RAW),
            );
            let (dx, dy) = Self::stick_dot_pos(x, y, rx, ry);
            screen.draw_circle(
                self.disc_tex,
                dx,
                dy,
                STICK_DOT_R,
                Color::from_hex(C_DOT_LIVE),
            );

            if let Some(small) = ctx.assets.font("small") {
                // Legend doubles as the value readout, so the two markers in the
                // box never need explaining separately.
                screen.draw_text(
                    small,
                    &format!("* live {rx:+.2},{ry:+.2}"),
                    x,
                    y + STICK_BOX + 6.0,
                );
                screen.draw_text(
                    small,
                    &format!("o raw  {raw_x:+.2},{raw_y:+.2}"),
                    x,
                    y + STICK_BOX + 24.0,
                );
            }
        }

        // ── Analog trigger bars ──
        for (i, (label, value)) in [("LT", pad.lt), ("RT", pad.rt)].into_iter().enumerate() {
            let y = TRIG_Y + i as f32 * (TRIG_H + 24.0);
            screen.draw_text(body, &format!("{label} {value:.2}"), STICK_X, y - 2.0);
            let bar_x = STICK_X + 104.0;
            let bar_w = TRIG_W - 104.0;
            self.rect(screen, bar_x, y, bar_w, TRIG_H, Color::from_hex(0x1B1F26));
            self.rect(
                screen,
                bar_x,
                y,
                bar_w * value.clamp(0.0, 1.0),
                TRIG_H,
                Color::from_hex(C_TRIG_FILL),
            );
            self.outline(screen, bar_x, y, bar_w, TRIG_H, Color::from_hex(C_BOX));
        }

        // ── Footer ──
        if let Some(small) = ctx.assets.font("small") {
            screen.draw_text(
                small,
                &format!(
                    "pad {}  deadzone {:.2}  axis_threshold {:.2}  |  Tab/Select: cycle pad",
                    pad.id, self.snapshot.deadzone, self.snapshot.axis_threshold
                ),
                MARGIN,
                ctx.virtual_height - 26.0,
            );
        }
    }

    fn clear_color(&self) -> Color {
        Color::from_hex(C_BG)
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        // The per-frame snapshot IS the inspect payload, so the assertions in
        // tests/vdp_gamepad.rs check exactly what the screen shows.
        let view = TesterInspect {
            snapshot: &self.snapshot,
            selected: self.selected,
            rumble_sent: self.rumble_sent,
            event_log: &self.event_log,
        };
        serde_json::to_value(&view).unwrap_or(serde_json::Value::Null)
    }

    #[cfg(feature = "vdp")]
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.dispatch_vdp(method, params)
            .unwrap_or_else(|| Err(format!("Unknown method: {method}")))
    }
}

// ── VDP inspect snapshot + methods ────────────────────────────────
//
// `TextureId`s aren't serializable, so `inspect` projects a borrowed view
// rather than deriving `Serialize` on the game struct itself.

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
struct TesterInspect<'a> {
    #[serde(flatten)]
    snapshot: &'a Snapshot,
    selected: usize,
    rumble_sent: u32,
    event_log: &'a [String],
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
struct SelectPad {
    index: u64,
}

#[cfg(feature = "vdp")]
#[vibe2d::vdp::vdp_methods]
impl GamepadTester {
    /// Choose which connected pad the detail panes show.
    ///
    /// (There is deliberately no `tester.setDeadzone`: the deadzone lives in
    /// `InputState`, which game code only ever holds immutably, so a game-level
    /// setter is impossible. Configure it via `input.gamepad.deadzone` in
    /// `game.yaml`.)
    #[vdp("tester.selectPad")]
    fn vdp_select_pad(&mut self, p: SelectPad) -> Result<serde_json::Value, String> {
        let index = p.index as usize;
        let connected = self.snapshot.pad_count;
        if index >= connected {
            return Err(format!(
                "pad index {index} out of range (have {connected} connected)"
            ));
        }
        self.selected = index;
        Ok(serde_json::json!({ "selected": self.selected }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    vibe2d::run::<GamepadTester>("game.yaml");
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    wasm_bindgen_futures::spawn_local(async {
        vibe2d::run_web::<GamepadTester>("game.yaml").await;
    });
}
