use vibe_input::InputState;
use vibe_render::{DrawCommand, Font, TextureId};

use crate::id::WidgetId;
use crate::layout::{Anchor, LayoutDirection, anchor_origin};
use crate::response::{Response, ScrollListResponse, TextInputResponse};
use crate::state::{ScrollbarDrag, UiState};
use crate::style::{ButtonStyle, PanelStyle, Style, UiColor};
use crate::vdp::{VdpUiAction, WidgetProperties, WidgetSnapshot, WidgetType};

/// Output returned after a UI frame, indicating what the UI consumed.
#[derive(Debug, Clone, Default)]
pub struct UiOutput {
    /// UI consumed the mouse click this frame.
    pub consumed_mouse: bool,
    /// UI consumed keyboard input this frame (a TextInput has focus).
    pub consumed_keyboard: bool,
}

/// Immediate-mode UI context.
///
/// Does **not** require a Renderer reference — all draw commands are collected
/// into an internal buffer and cached in `UiState` when `finish()` is called.
/// The engine replays these cached commands during the render phase.
pub struct UiContext<'a> {
    ui_state: &'a mut UiState,
    input: &'a InputState,
    white_texture_id: TextureId,

    // Input snapshot
    mouse_x: f32,
    mouse_y: f32,
    mouse_just_clicked: bool,
    mouse_pressed: bool,
    scroll_delta: f32,
    scroll_delta_x: f32,
    chars_this_frame: Vec<char>,
    // ── IME state for the focused TextInput ──
    /// Multi-character text committed by the IME this frame (e.g. a finalized Chinese word).
    /// Inserted atomically into the focused text input.
    ime_commit: String,
    /// In-flight composition text (and caret byte offset). Rendered as an
    /// underlined overlay at the cursor without being written into the buffer.
    ime_preedit: Option<(String, Option<usize>)>,

    // Screen dimensions
    virtual_width: f32,
    virtual_height: f32,

    // Layout state
    cursor_x: f32,
    cursor_y: f32,
    anchor: Anchor,
    layout_direction: LayoutDirection,
    style: Style,

    // Frame tracking
    frame_widgets: Vec<WidgetSnapshot>,
    consumed_mouse: bool,

    // VDP actions for this frame
    vdp_actions: Vec<VdpUiAction>,

    // Draw command buffer (replaces direct Renderer calls)
    draw_commands: Vec<DrawCommand>,

    // Deferred draws: (background_commands, text_commands) for batch optimization
    deferred_bg: Vec<DrawCommand>,
    deferred_text: Vec<DrawCommand>,
    use_deferred: bool,
}

impl<'a> UiContext<'a> {
    /// Create a new UiContext for this frame.
    ///
    /// Does not require a Renderer — draw commands are buffered
    /// internally and stored into `UiState` on `finish()`.
    ///
    /// The 1×1 white pixel texture used to draw filled rectangles
    /// is read straight out of `ui_state` (allocated once via
    /// [`UiState::init`] during engine startup), so callers no
    /// longer need to thread a `TextureId` through here. If `init`
    /// hasn't been called yet (test/headless paths), filled-rect
    /// commands fall back to [`TextureId(0)`], which is harmless as
    /// long as no actual rendering is performed against this UI
    /// state.
    pub fn new(
        ui_state: &'a mut UiState,
        input: &'a InputState,
        virtual_width: f32,
        virtual_height: f32,
    ) -> Self {
        ui_state.begin_frame();
        let (mouse_x, mouse_y) = input.mouse_position();
        let vdp_actions = ui_state.drain_vdp_actions();
        let white_texture_id = ui_state.white_texture_id.unwrap_or(TextureId(0));

        Self {
            ui_state,
            input,
            white_texture_id,
            mouse_x,
            mouse_y,
            mouse_just_clicked: input.is_mouse_button_just_pressed(vibe_input::MouseButton::Left),
            mouse_pressed: input.is_mouse_button_pressed(vibe_input::MouseButton::Left),
            scroll_delta: input.mouse_scroll_delta(),
            scroll_delta_x: input.mouse_scroll_delta_x(),
            chars_this_frame: input.chars_this_frame().to_vec(),
            ime_commit: input.ime_commit().to_string(),
            ime_preedit: input.ime_preedit().map(|p| (p.text.clone(), p.cursor_byte)),
            virtual_width,
            virtual_height,
            cursor_x: 0.0,
            cursor_y: 0.0,
            anchor: Anchor::TopLeft,
            layout_direction: LayoutDirection::Vertical,
            style: Style::default(),
            frame_widgets: Vec::new(),
            consumed_mouse: false,
            vdp_actions,
            draw_commands: Vec::new(),
            deferred_bg: Vec::new(),
            deferred_text: Vec::new(),
            use_deferred: false,
        }
    }

    /// Finalize the UI frame: store widget snapshots and cached draw commands.
    pub fn finish(self) -> UiOutput {
        let has_focus = self.ui_state.focused.is_some();
        self.ui_state.last_frame_widgets = self.frame_widgets;
        self.ui_state.cached_draw_commands = self.draw_commands;
        UiOutput {
            consumed_mouse: self.consumed_mouse,
            consumed_keyboard: has_focus,
        }
    }

    // ── Layout setters ──────────────────────────────────────────

    /// Set the anchor point for subsequent widgets.
    pub fn set_anchor(&mut self, anchor: Anchor) {
        self.anchor = anchor;
    }

    /// Set the layout direction (Vertical or Horizontal).
    pub fn set_layout(&mut self, direction: LayoutDirection) {
        self.layout_direction = direction;
    }

    /// Set the spacing between widgets.
    pub fn set_spacing(&mut self, spacing: f32) {
        self.style.spacing = spacing;
    }

    /// Set the padding from the anchor edge.
    pub fn set_padding(&mut self, padding: f32) {
        self.style.padding = padding;
    }

    /// Set the global style.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Manually set the cursor position (offset from anchor origin).
    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    // ── Helpers ─────────────────────────────────────────────────

    fn hit_test(&self, rect: [f32; 4]) -> bool {
        let [x, y, w, h] = rect;
        self.mouse_x >= x && self.mouse_x <= x + w && self.mouse_y >= y && self.mouse_y <= y + h
    }

    fn has_vdp_click(&self, id: &WidgetId) -> bool {
        self.vdp_actions
            .iter()
            .any(|a| matches!(a, VdpUiAction::Click { id: click_id } if click_id == id))
    }

    fn has_vdp_submit(&self, id: &WidgetId) -> bool {
        self.vdp_actions
            .iter()
            .any(|a| matches!(a, VdpUiAction::Submit { id: submit_id } if submit_id == id))
    }

    fn advance_cursor(&mut self, width: f32, height: f32) {
        match self.layout_direction {
            LayoutDirection::Vertical => {
                self.cursor_y += height + self.style.spacing;
            }
            LayoutDirection::Horizontal => {
                self.cursor_x += width + self.style.spacing;
            }
        }
    }

    fn draw_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: UiColor) {
        let cmd = DrawCommand {
            texture_id: self.white_texture_id,
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_rect: [x, y, w, h],
            color: color.to_array(),
            flip_x: false,
            flip_y: false,
        };
        if self.use_deferred {
            self.deferred_bg.push(cmd);
        } else {
            self.draw_commands.push(cmd);
        }
    }

    fn draw_text_internal(&mut self, font: &Font, text: &str, x: f32, y: f32, color: UiColor) {
        for (tex_id, src_rect, dst_rect) in font.layout_text(text, x, y) {
            let cmd = DrawCommand {
                texture_id: tex_id,
                src_rect,
                dst_rect,
                color: color.to_array(),
                flip_x: false,
                flip_y: false,
            };
            if self.use_deferred {
                self.deferred_text.push(cmd);
            } else {
                self.draw_commands.push(cmd);
            }
        }
    }

    fn flush_deferred(&mut self) {
        for cmd in self.deferred_bg.drain(..) {
            self.draw_commands.push(cmd);
        }
        for cmd in self.deferred_text.drain(..) {
            self.draw_commands.push(cmd);
        }
    }

    // ── Label ───────────────────────────────────────────────────

    /// Draw a text label with auto-generated ID.
    pub fn label(&mut self, font: &Font, text: &str) {
        let id = self.ui_state.next_auto_id();
        self.label_impl(id, font, text, self.style.text_color);
    }

    /// Draw a text label with an explicit ID (for VDP targeting).
    pub fn label_with_id(&mut self, id_str: &str, font: &Font, text: &str) {
        let id = WidgetId::new(id_str);
        self.label_impl(id, font, text, self.style.text_color);
    }

    /// Draw a colored text label.
    pub fn label_colored(&mut self, font: &Font, text: &str, color: UiColor) {
        let id = self.ui_state.next_auto_id();
        self.label_impl(id, font, text, color);
    }

    fn label_impl(&mut self, id: WidgetId, font: &Font, text: &str, color: UiColor) {
        let text_w = font.text_width(text);
        let text_h = font.line_height;

        let (draw_x, draw_y) = self.resolve_position(text_w, text_h);

        self.draw_text_internal(font, text, draw_x, draw_y, color);

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::Label,
            rect: [draw_x, draw_y, text_w, text_h],
            visible: true,
            properties: WidgetProperties::Label {
                text: text.to_string(),
                color: color.to_array(),
            },
        });

        self.advance_cursor(text_w, text_h);
    }

    // ── Button ──────────────────────────────────────────────────

    /// Draw a button with auto-generated ID.
    pub fn button(&mut self, font: &Font, text: &str) -> Response {
        let id = self.ui_state.next_auto_id();
        self.button_impl(id, font, text, &self.style.button.clone())
    }

    /// Draw a button with an explicit ID.
    pub fn button_with_id(&mut self, id_str: &str, font: &Font, text: &str) -> Response {
        let id = WidgetId::new(id_str);
        self.button_impl(id, font, text, &self.style.button.clone())
    }

    /// Draw a button with custom style.
    pub fn button_styled(&mut self, font: &Font, text: &str, style: ButtonStyle) -> Response {
        let id = self.ui_state.next_auto_id();
        self.button_impl(id, font, text, &style)
    }

    fn button_impl(
        &mut self,
        id: WidgetId,
        font: &Font,
        text: &str,
        style: &ButtonStyle,
    ) -> Response {
        let text_w = font.text_width(text);
        let text_h = font.line_height;
        let btn_w = text_w + style.padding * 2.0;
        let btn_h = text_h + style.padding * 2.0;

        let (draw_x, draw_y) = self.resolve_position(btn_w, btn_h);
        let rect = [draw_x, draw_y, btn_w, btn_h];

        let hovered = self.hit_test(rect);
        let pressed = hovered && self.mouse_pressed;
        let vdp_clicked = self.has_vdp_click(&id);
        let clicked = (hovered && self.mouse_just_clicked) || vdp_clicked;

        if clicked {
            self.consumed_mouse = true;
        }

        // Choose background color based on state
        let bg_color = if pressed {
            style.pressed_color
        } else if hovered {
            style.hover_color
        } else {
            style.bg_color
        };

        self.draw_rect(draw_x, draw_y, btn_w, btn_h, bg_color);
        self.draw_text_internal(
            font,
            text,
            draw_x + style.padding,
            draw_y + style.padding,
            style.text_color,
        );

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::Button,
            rect,
            visible: true,
            properties: WidgetProperties::Button {
                text: text.to_string(),
                hovered,
                pressed,
            },
        });

        self.advance_cursor(btn_w, btn_h);

        Response {
            hovered,
            pressed,
            clicked,
        }
    }

    // ── Panel ───────────────────────────────────────────────────

    /// Draw a panel container with default style.
    pub fn panel(&mut self, style: PanelStyle, content: impl FnOnce(&mut UiContext)) {
        let id = self.ui_state.next_auto_id();
        self.panel_impl(id, style, content);
    }

    /// Draw a panel container with explicit ID.
    pub fn panel_with_id(
        &mut self,
        id_str: &str,
        style: PanelStyle,
        content: impl FnOnce(&mut UiContext),
    ) {
        let id = WidgetId::new(id_str);
        self.panel_impl(id, style, content);
    }

    fn panel_impl(
        &mut self,
        id: WidgetId,
        style: PanelStyle,
        content: impl FnOnce(&mut UiContext),
    ) {
        // Save layout state before panel
        let saved_cursor_x = self.cursor_x;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.layout_direction;
        let saved_anchor = self.anchor;
        let saved_spacing = self.style.spacing;

        // ── Measure pass: calculate content size ──
        // We'll use deferred rendering to collect draw commands, then position them.
        let prev_deferred = self.use_deferred;
        self.use_deferred = true;

        // Reset cursor for content measurement
        self.cursor_x = 0.0;
        self.cursor_y = 0.0;
        self.layout_direction = LayoutDirection::Vertical;

        let widgets_before = self.frame_widgets.len();
        content(self);

        let content_w = self.cursor_x; // For horizontal layout
        let content_h = self.cursor_y - self.style.spacing; // Remove trailing spacing
        let content_h = content_h.max(0.0);

        // Calculate actual content dimensions based on deferred commands
        let (measured_w, measured_h) = self.measure_deferred_bounds();
        let actual_content_w = measured_w.max(content_w);
        let actual_content_h = measured_h.max(content_h);

        let panel_w = actual_content_w + style.padding * 2.0;
        let panel_h = actual_content_h + style.padding * 2.0;

        // Restore anchor and cursor for panel positioning
        self.anchor = saved_anchor;
        self.cursor_x = saved_cursor_x;
        self.cursor_y = saved_cursor_y;
        let (panel_x, panel_y) = self.resolve_position(panel_w, panel_h);

        // ── Render pass: draw background, then offset and flush deferred ──
        self.use_deferred = prev_deferred;

        // Draw panel background
        self.draw_rect(panel_x, panel_y, panel_w, panel_h, style.bg_color);

        // Offset all deferred draw commands by panel position + padding
        let offset_x = panel_x + style.padding;
        let offset_y = panel_y + style.padding;
        for cmd in &mut self.deferred_bg {
            cmd.dst_rect[0] += offset_x;
            cmd.dst_rect[1] += offset_y;
        }
        for cmd in &mut self.deferred_text {
            cmd.dst_rect[0] += offset_x;
            cmd.dst_rect[1] += offset_y;
        }

        // Also offset widget snapshot rects
        for snapshot in &mut self.frame_widgets[widgets_before..] {
            snapshot.rect[0] += offset_x;
            snapshot.rect[1] += offset_y;
        }

        self.flush_deferred();

        // Collect children IDs
        let children: Vec<WidgetId> = self.frame_widgets[widgets_before..]
            .iter()
            .map(|w| w.id.clone())
            .collect();

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::Panel,
            rect: [panel_x, panel_y, panel_w, panel_h],
            visible: true,
            properties: WidgetProperties::Panel { children },
        });

        // Restore layout state
        self.cursor_x = saved_cursor_x;
        self.cursor_y = saved_cursor_y;
        self.layout_direction = saved_direction;
        self.style.spacing = saved_spacing;

        self.advance_cursor(panel_w, panel_h);
    }

    fn measure_deferred_bounds(&self) -> (f32, f32) {
        let mut max_x: f32 = 0.0;
        let mut max_y: f32 = 0.0;
        for cmd in &self.deferred_bg {
            max_x = max_x.max(cmd.dst_rect[0] + cmd.dst_rect[2]);
            max_y = max_y.max(cmd.dst_rect[1] + cmd.dst_rect[3]);
        }
        for cmd in &self.deferred_text {
            max_x = max_x.max(cmd.dst_rect[0] + cmd.dst_rect[2]);
            max_y = max_y.max(cmd.dst_rect[1] + cmd.dst_rect[3]);
        }
        (max_x, max_y)
    }

    // ── ProgressBar ─────────────────────────────────────────────

    /// Draw a progress bar (0.0 to 1.0).
    pub fn progress_bar(&mut self, progress: f32, width: f32, height: f32) {
        let fill_color = UiColor::new(0.3, 0.8, 0.3, 0.9);
        let bg_color = UiColor::new(0.2, 0.2, 0.2, 0.8);
        let id = self.ui_state.next_auto_id();
        self.progress_bar_impl(id, progress, width, height, fill_color, bg_color);
    }

    /// Draw a progress bar with explicit ID and colors.
    pub fn progress_bar_with_id(
        &mut self,
        id_str: &str,
        progress: f32,
        width: f32,
        height: f32,
        fill_color: UiColor,
        bg_color: UiColor,
    ) {
        let id = WidgetId::new(id_str);
        self.progress_bar_impl(id, progress, width, height, fill_color, bg_color);
    }

    fn progress_bar_impl(
        &mut self,
        id: WidgetId,
        progress: f32,
        width: f32,
        height: f32,
        fill_color: UiColor,
        bg_color: UiColor,
    ) {
        let progress = progress.clamp(0.0, 1.0);
        let (draw_x, draw_y) = self.resolve_position(width, height);

        // Background
        self.draw_rect(draw_x, draw_y, width, height, bg_color);

        // Fill
        let fill_width = width * progress;
        if fill_width > 0.0 {
            self.draw_rect(draw_x, draw_y, fill_width, height, fill_color);
        }

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::ProgressBar,
            rect: [draw_x, draw_y, width, height],
            visible: true,
            properties: WidgetProperties::ProgressBar { progress },
        });

        self.advance_cursor(width, height);
    }

    // ── TextInput ───────────────────────────────────────────────

    /// Draw a text input field.
    pub fn text_input(&mut self, id_str: &str, font: &Font, width: f32) -> TextInputResponse {
        self.text_input_impl(id_str, font, width, "")
    }

    /// Draw a text input field with placeholder text.
    pub fn text_input_with_placeholder(
        &mut self,
        id_str: &str,
        font: &Font,
        width: f32,
        placeholder: &str,
    ) -> TextInputResponse {
        self.text_input_impl(id_str, font, width, placeholder)
    }

    fn text_input_impl(
        &mut self,
        id_str: &str,
        font: &Font,
        width: f32,
        placeholder: &str,
    ) -> TextInputResponse {
        let id = WidgetId::new(id_str);
        let style = self.style.text_input.clone();
        let height = font.line_height + style.padding * 2.0;

        let (draw_x, draw_y) = self.resolve_position(width, height);
        let rect = [draw_x, draw_y, width, height];

        let is_focused = self.ui_state.focused.as_ref() == Some(&id);
        let hovered = self.hit_test(rect);
        let clicked = hovered && self.mouse_just_clicked;

        // Handle focus changes
        if clicked {
            self.ui_state.focused = Some(id.clone());
            self.consumed_mouse = true;
        } else if self.mouse_just_clicked && !hovered && is_focused {
            self.ui_state.focused = None;
        }

        let is_focused = self.ui_state.focused.as_ref() == Some(&id);

        // Apply VDP SetText action
        for action in &self.vdp_actions {
            if let VdpUiAction::SetText {
                id: action_id,
                text,
            } = action
                && action_id == &id
            {
                let state = self.ui_state.text_input_state(&id);
                state.text = text.clone();
                state.cursor_position = text.len();
            }
        }

        // Handle keyboard input when focused
        let mut changed = false;
        let mut submitted = false;

        if is_focused {
            // Snapshot key states before borrowing ui_state mutably
            let chars: Vec<char> = self.chars_this_frame.clone();
            let ime_commit = self.ime_commit.clone();
            let has_active_preedit = self.ime_preedit.is_some();
            let key_backspace = self.is_key_just_pressed(vibe_input::KeyCode::Backspace);
            let key_delete = self.is_key_just_pressed(vibe_input::KeyCode::Delete);
            let key_left = self.is_key_just_pressed(vibe_input::KeyCode::ArrowLeft);
            let key_right = self.is_key_just_pressed(vibe_input::KeyCode::ArrowRight);
            let key_home = self.is_key_just_pressed(vibe_input::KeyCode::Home);
            let key_end = self.is_key_just_pressed(vibe_input::KeyCode::End);
            let key_enter = self.is_key_just_pressed(vibe_input::KeyCode::Enter);
            let key_escape = self.is_key_just_pressed(vibe_input::KeyCode::Escape);

            let state = self.ui_state.text_input_state(&id);

            // Character input from regular keystrokes.
            // Skipped while an IME composition is active — the platform layer
            // already filters those, but we double-check defensively.
            if !has_active_preedit {
                for &ch in &chars {
                    state.text.insert(state.cursor_position, ch);
                    state.cursor_position += ch.len_utf8();
                    changed = true;
                }
            }

            // IME commit: insert the entire committed string atomically.
            // This is what makes Chinese / Japanese / emoji input work — the
            // IME hands us a finalized multi-character string in one event.
            if !ime_commit.is_empty() {
                state.text.insert_str(state.cursor_position, &ime_commit);
                state.cursor_position += ime_commit.len();
                changed = true;
            }

            // Functional keys
            if key_backspace && state.cursor_position > 0 {
                let prev = state.text[..state.cursor_position]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.text.drain(prev..state.cursor_position);
                state.cursor_position = prev;
                changed = true;
            }

            if key_delete && state.cursor_position < state.text.len() {
                let next = state.text[state.cursor_position..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.cursor_position + i)
                    .unwrap_or(state.text.len());
                state.text.drain(state.cursor_position..next);
                changed = true;
            }

            if key_left && state.cursor_position > 0 {
                let prev = state.text[..state.cursor_position]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.cursor_position = prev;
            }

            if key_right && state.cursor_position < state.text.len() {
                let next = state.text[state.cursor_position..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.cursor_position + i)
                    .unwrap_or(state.text.len());
                state.cursor_position = next;
            }

            if key_home {
                state.cursor_position = 0;
            }

            if key_end {
                state.cursor_position = state.text.len();
            }

            if key_enter {
                submitted = true;
            }

            if key_escape {
                self.ui_state.focused = None;
            }
        }

        // Check VDP submit
        if self.has_vdp_submit(&id) {
            submitted = true;
        }

        // Check VDP focus
        for action in &self.vdp_actions {
            if let VdpUiAction::SetFocus { id: focus_id } = action
                && focus_id == &id
            {
                self.ui_state.focused = Some(id.clone());
            }
        }

        let is_focused = self.ui_state.focused.as_ref() == Some(&id);

        // ── Render ──
        let bg_color = if is_focused {
            style.focused_bg_color
        } else {
            style.bg_color
        };
        let border_color = if is_focused {
            style.focused_border_color
        } else {
            style.border_color
        };

        // Border (1px outline)
        self.draw_rect(draw_x, draw_y, width, height, border_color);
        self.draw_rect(
            draw_x + 1.0,
            draw_y + 1.0,
            width - 2.0,
            height - 2.0,
            bg_color,
        );

        let text_x = draw_x + style.padding;
        let text_y = draw_y + style.padding;

        // Read state values, then drop the mutable borrow before drawing
        let display_text: String;
        let is_empty: bool;
        let cursor_position: usize;
        {
            let state = self.ui_state.text_input_state(&id);
            display_text = state.text.clone();
            is_empty = state.text.is_empty();
            cursor_position = state.cursor_position;
        }

        if is_empty && !placeholder.is_empty() {
            self.draw_text_internal(font, placeholder, text_x, text_y, style.placeholder_color);
        } else {
            self.draw_text_internal(font, &display_text, text_x, text_y, style.text_color);
        }

        // Draw cursor + IME preedit overlay when focused
        if is_focused {
            let cursor_text = &display_text[..cursor_position];
            let cursor_x_offset = font.text_width(cursor_text);

            // IME preedit overlay (rendered above the regular text but not
            // written into the buffer; it's pending composition).
            // Drawn before the cursor so the cursor sits on top of it.
            // Cloned out to release the immutable borrow on `self.ime_preedit`
            // before calling the `&mut self` draw helpers.
            let preedit_snapshot = self.ime_preedit.clone();
            if let Some((preedit_text, preedit_caret)) = preedit_snapshot {
                let preedit_x = text_x + cursor_x_offset;
                self.draw_text_internal(font, &preedit_text, preedit_x, text_y, style.text_color);
                // Underline the preedit so users can distinguish it from
                // committed text (matches OS-native IME conventions).
                let preedit_w = font.text_width(&preedit_text);
                self.draw_rect(
                    preedit_x,
                    text_y + font.line_height - 1.0,
                    preedit_w,
                    1.0,
                    style.text_color,
                );
                // Caret inside the preedit (no blink — IME owns the caret).
                let caret_byte = preedit_caret.unwrap_or(preedit_text.len());
                let caret_offset =
                    font.text_width(&preedit_text[..caret_byte.min(preedit_text.len())]);
                self.draw_rect(
                    preedit_x + caret_offset,
                    text_y,
                    1.0,
                    font.line_height,
                    style.cursor_color,
                );
            } else {
                // Regular blinking cursor when no IME composition is active.
                let elapsed = self.ui_state.elapsed_time;
                // 2 Hz cursor blink: on for even half-seconds, off for odd.
                let blink = ((elapsed * 2.0) as u64).is_multiple_of(2);
                if blink {
                    self.draw_rect(
                        text_x + cursor_x_offset,
                        text_y,
                        1.0,
                        font.line_height,
                        style.cursor_color,
                    );
                }
            }
        }

        let current_text = display_text;

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::TextInput,
            rect,
            visible: true,
            properties: WidgetProperties::TextInput {
                text: current_text,
                placeholder: placeholder.to_string(),
                focused: is_focused,
                cursor_position,
            },
        });

        self.advance_cursor(width, height);

        TextInputResponse {
            response: Response {
                hovered,
                pressed: hovered && self.mouse_pressed,
                clicked,
            },
            changed,
            submitted,
        }
    }

    // ── TextInput accessors ─────────────────────────────────────

    /// Get the current text content of a TextInput.
    pub fn text_input_value(&self, id_str: &str) -> String {
        let id = WidgetId::new(id_str);
        self.ui_state
            .text_inputs
            .get(&id)
            .map(|s| s.text.clone())
            .unwrap_or_default()
    }

    /// Programmatically set the text content of a TextInput.
    pub fn text_input_set_value(&mut self, id_str: &str, text: &str) {
        let id = WidgetId::new(id_str);
        let state = self.ui_state.text_input_state(&id);
        state.text = text.to_string();
        state.cursor_position = text.len();
    }

    /// Clear a TextInput.
    pub fn text_input_clear(&mut self, id_str: &str) {
        self.text_input_set_value(id_str, "");
    }

    // ── ScrollList ──────────────────────────────────────────────

    /// Draw a scrollable list container.
    pub fn scroll_list(
        &mut self,
        id_str: &str,
        width: f32,
        height: f32,
        content: impl FnOnce(&mut UiContext),
    ) -> ScrollListResponse {
        self.scroll_list_impl(id_str, width, height, content)
    }

    fn scroll_list_impl(
        &mut self,
        id_str: &str,
        width: f32,
        height: f32,
        content: impl FnOnce(&mut UiContext),
    ) -> ScrollListResponse {
        let id = WidgetId::new(id_str);
        let list_style = self.style.scroll_list.clone();

        let (draw_x, draw_y) = self.resolve_position(width, height);
        let rect = [draw_x, draw_y, width, height];
        let hovered = self.hit_test(rect);

        // Apply VDP scroll actions
        for action in &self.vdp_actions {
            match action {
                VdpUiAction::Scroll {
                    id: scroll_id,
                    offset,
                } if scroll_id == &id => {
                    let state = self.ui_state.scroll_list_state(&id);
                    state.scroll_offset = *offset;
                }
                VdpUiAction::ScrollHorizontal {
                    id: scroll_id,
                    offset,
                } if scroll_id == &id => {
                    let state = self.ui_state.scroll_list_state(&id);
                    state.horizontal_offset = *offset;
                }
                VdpUiAction::ScrollToBottom { id: scroll_id } if scroll_id == &id => {
                    let state = self.ui_state.scroll_list_state(&id);
                    let max_scroll =
                        (state.total_content_height - height + list_style.padding * 2.0).max(0.0);
                    state.scroll_offset = max_scroll;
                }
                _ => {}
            }
        }

        // Handle scrollbar drag (must be processed before mouse scroll to take priority)
        let current_drag = self.ui_state.scroll_list_state(&id).dragging;
        if current_drag != ScrollbarDrag::None {
            if self.mouse_pressed {
                // Continue dragging
                let state = self.ui_state.scroll_list_state(&id);
                match current_drag {
                    ScrollbarDrag::Vertical => {
                        let inner_h = height - list_style.padding * 2.0;
                        let scrollbar_h = (inner_h / state.total_content_height * inner_h).max(8.0);
                        let track_range = inner_h - scrollbar_h;
                        if track_range > 0.0 {
                            let mouse_delta = self.mouse_y - state.drag_start_mouse;
                            let max_scroll = (state.total_content_height - inner_h).max(0.0);
                            state.scroll_offset =
                                state.drag_start_offset + (mouse_delta / track_range) * max_scroll;
                        }
                    }
                    ScrollbarDrag::Horizontal => {
                        let inner_w = width - list_style.padding * 2.0 - list_style.scrollbar_width;
                        let scrollbar_w = (inner_w / state.total_content_width * inner_w).max(8.0);
                        let track_range = inner_w - scrollbar_w;
                        if track_range > 0.0 {
                            let mouse_delta = self.mouse_x - state.drag_start_mouse;
                            let max_scroll = (state.total_content_width - inner_w).max(0.0);
                            state.horizontal_offset =
                                state.drag_start_offset + (mouse_delta / track_range) * max_scroll;
                        }
                    }
                    ScrollbarDrag::None => {}
                }
                self.consumed_mouse = true;
            } else {
                // Mouse released — end drag
                let state = self.ui_state.scroll_list_state(&id);
                state.dragging = ScrollbarDrag::None;
            }
        }

        // Handle mouse scroll (vertical and horizontal)
        if hovered && current_drag == ScrollbarDrag::None {
            if self.scroll_delta != 0.0 {
                let state = self.ui_state.scroll_list_state(&id);
                state.scroll_offset -= self.scroll_delta;
                self.consumed_mouse = true;
            }
            if self.scroll_delta_x != 0.0 {
                let state = self.ui_state.scroll_list_state(&id);
                state.horizontal_offset += self.scroll_delta_x;
                self.consumed_mouse = true;
            }
        }

        // Draw background
        self.draw_rect(draw_x, draw_y, width, height, list_style.bg_color);

        // Save state
        let saved_cursor_x = self.cursor_x;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.layout_direction;

        // Render content into deferred buffer
        let prev_deferred = self.use_deferred;
        self.use_deferred = true;
        self.cursor_x = 0.0;
        self.cursor_y = 0.0;
        self.layout_direction = LayoutDirection::Vertical;

        let widgets_before = self.frame_widgets.len();
        content(self);

        let content_total_height = (self.cursor_y - self.style.spacing).max(0.0);

        // Measure content width from deferred draw commands
        let mut content_total_width: f32 = 0.0;
        for cmd in &self.deferred_bg {
            content_total_width = content_total_width.max(cmd.dst_rect[0] + cmd.dst_rect[2]);
        }
        for cmd in &self.deferred_text {
            content_total_width = content_total_width.max(cmd.dst_rect[0] + cmd.dst_rect[2]);
        }

        let inner_w = width - list_style.padding * 2.0 - list_style.scrollbar_width;
        let inner_h = height - list_style.padding * 2.0;

        // Update persistent state
        {
            let state = self.ui_state.scroll_list_state(&id);
            state.total_content_height = content_total_height;
            state.total_content_width = content_total_width;

            // Clamp vertical scroll
            let max_v_scroll = (content_total_height - inner_h).max(0.0);
            state.scroll_offset = state.scroll_offset.clamp(0.0, max_v_scroll);

            // Clamp horizontal scroll
            let max_h_scroll = (content_total_width - inner_w).max(0.0);
            state.horizontal_offset = state.horizontal_offset.clamp(0.0, max_h_scroll);
        }

        let scroll_offset = self.ui_state.scroll_list_state(&id).scroll_offset;
        let horizontal_offset = self.ui_state.scroll_list_state(&id).horizontal_offset;

        // Offset deferred commands by list position - scroll offset, then clip
        let inner_x = draw_x + list_style.padding;
        let inner_y = draw_y + list_style.padding;
        let clip_rect = [inner_x, inner_y, inner_w, inner_h];

        let offset_x = inner_x - horizontal_offset;
        let offset_y = inner_y - scroll_offset;

        // Collect and clip deferred commands
        let bg_cmds: Vec<DrawCommand> = self.deferred_bg.drain(..).collect();
        let text_cmds: Vec<DrawCommand> = self.deferred_text.drain(..).collect();

        self.use_deferred = prev_deferred;

        for mut cmd in bg_cmds {
            cmd.dst_rect[0] += offset_x;
            cmd.dst_rect[1] += offset_y;
            if let Some(clipped) = clip_draw_command(&cmd, clip_rect) {
                self.draw_commands.push(clipped);
            }
        }
        for mut cmd in text_cmds {
            cmd.dst_rect[0] += offset_x;
            cmd.dst_rect[1] += offset_y;
            if let Some(clipped) = clip_draw_command(&cmd, clip_rect) {
                self.draw_commands.push(clipped);
            }
        }

        // Offset widget snapshots
        for snapshot in &mut self.frame_widgets[widgets_before..] {
            snapshot.rect[0] += offset_x;
            snapshot.rect[1] += offset_y;
        }

        // ── Scrollbar rendering + drag initiation ───────────────

        // Vertical scrollbar
        if content_total_height > inner_h {
            let max_v_scroll = content_total_height - inner_h;
            let scrollbar_h = (inner_h / content_total_height * inner_h).max(8.0);
            let scrollbar_x = draw_x + width - list_style.scrollbar_width - 1.0;
            let scrollbar_y = inner_y + (scroll_offset / max_v_scroll) * (inner_h - scrollbar_h);

            // Detect click on vertical scrollbar to start drag
            if self.mouse_just_clicked && current_drag == ScrollbarDrag::None {
                let sb_rect = [
                    scrollbar_x,
                    scrollbar_y,
                    list_style.scrollbar_width,
                    scrollbar_h,
                ];
                if self.hit_test(sb_rect) {
                    let state = self.ui_state.scroll_list_state(&id);
                    state.dragging = ScrollbarDrag::Vertical;
                    state.drag_start_mouse = self.mouse_y;
                    state.drag_start_offset = scroll_offset;
                    self.consumed_mouse = true;
                }
            }

            // Highlight scrollbar when hovered or dragging
            let is_dragging_v =
                self.ui_state.scroll_list_state(&id).dragging == ScrollbarDrag::Vertical;
            let sb_hovered = self.hit_test([
                scrollbar_x,
                scrollbar_y,
                list_style.scrollbar_width,
                scrollbar_h,
            ]);
            let sb_color = if is_dragging_v || sb_hovered {
                UiColor::new(0.7, 0.7, 0.7, 0.8)
            } else {
                list_style.scrollbar_color
            };

            self.draw_rect(
                scrollbar_x,
                scrollbar_y,
                list_style.scrollbar_width,
                scrollbar_h,
                sb_color,
            );
        }

        // Horizontal scrollbar
        if content_total_width > inner_w {
            let max_h_scroll = content_total_width - inner_w;
            let scrollbar_w = (inner_w / content_total_width * inner_w).max(8.0);
            let scrollbar_y = draw_y + height - list_style.scrollbar_width - 1.0;
            let scrollbar_x =
                inner_x + (horizontal_offset / max_h_scroll) * (inner_w - scrollbar_w);

            // Detect click on horizontal scrollbar to start drag
            if self.mouse_just_clicked && current_drag == ScrollbarDrag::None {
                let sb_rect = [
                    scrollbar_x,
                    scrollbar_y,
                    scrollbar_w,
                    list_style.scrollbar_width,
                ];
                if self.hit_test(sb_rect) {
                    let state = self.ui_state.scroll_list_state(&id);
                    state.dragging = ScrollbarDrag::Horizontal;
                    state.drag_start_mouse = self.mouse_x;
                    state.drag_start_offset = horizontal_offset;
                    self.consumed_mouse = true;
                }
            }

            // Highlight scrollbar when hovered or dragging
            let is_dragging_h =
                self.ui_state.scroll_list_state(&id).dragging == ScrollbarDrag::Horizontal;
            let sb_hovered = self.hit_test([
                scrollbar_x,
                scrollbar_y,
                scrollbar_w,
                list_style.scrollbar_width,
            ]);
            let sb_color = if is_dragging_h || sb_hovered {
                UiColor::new(0.7, 0.7, 0.7, 0.8)
            } else {
                list_style.scrollbar_color
            };

            self.draw_rect(
                scrollbar_x,
                scrollbar_y,
                scrollbar_w,
                list_style.scrollbar_width,
                sb_color,
            );
        }

        let children: Vec<WidgetId> = self.frame_widgets[widgets_before..]
            .iter()
            .map(|w| w.id.clone())
            .collect();

        self.frame_widgets.push(WidgetSnapshot {
            id,
            widget_type: WidgetType::ScrollList,
            rect,
            visible: true,
            properties: WidgetProperties::ScrollList {
                scroll_offset,
                horizontal_offset,
                content_height: content_total_height,
                content_width: content_total_width,
                visible_height: height,
                visible_width: width,
                children,
            },
        });

        // Restore layout
        self.cursor_x = saved_cursor_x;
        self.cursor_y = saved_cursor_y;
        self.layout_direction = saved_direction;

        self.advance_cursor(width, height);

        ScrollListResponse {
            response: Response {
                hovered,
                pressed: hovered && self.mouse_pressed,
                clicked: hovered && self.mouse_just_clicked,
            },
            scroll_offset,
            horizontal_offset,
            content_height: content_total_height,
            content_width: content_total_width,
            visible_height: height,
            visible_width: width,
        }
    }

    // ── ScrollList accessors ────────────────────────────────────

    /// Programmatically set vertical scroll offset.
    pub fn scroll_list_set_offset(&mut self, id_str: &str, offset: f32) {
        let id = WidgetId::new(id_str);
        let state = self.ui_state.scroll_list_state(&id);
        state.scroll_offset = offset;
    }

    /// Programmatically set horizontal scroll offset.
    pub fn scroll_list_set_horizontal_offset(&mut self, id_str: &str, offset: f32) {
        let id = WidgetId::new(id_str);
        let state = self.ui_state.scroll_list_state(&id);
        state.horizontal_offset = offset;
    }

    /// Scroll a list to the bottom.
    pub fn scroll_list_scroll_to_bottom(&mut self, id_str: &str) {
        let id = WidgetId::new(id_str);
        let state = self.ui_state.scroll_list_state(&id);
        let max_scroll = (state.total_content_height - state.scroll_offset).max(0.0);
        state.scroll_offset = max_scroll;
    }

    /// Get the current vertical scroll offset.
    pub fn scroll_list_offset(&self, id_str: &str) -> f32 {
        let id = WidgetId::new(id_str);
        self.ui_state
            .scroll_lists
            .get(&id)
            .map(|s| s.scroll_offset)
            .unwrap_or(0.0)
    }

    // ── Position resolution ─────────────────────────────────────

    fn resolve_position(&mut self, content_width: f32, content_height: f32) -> (f32, f32) {
        if self.use_deferred {
            // Inside Panel or ScrollList: use relative cursor position
            let x = self.cursor_x;
            let y = self.cursor_y;
            (x, y)
        } else {
            // Top-level: use anchor-based positioning
            let base = anchor_origin(
                self.anchor,
                self.virtual_width,
                self.virtual_height,
                content_width,
                content_height,
                self.style.padding,
            );
            let x = base.0 + self.cursor_x;
            let y = base.1 + self.cursor_y;
            (x, y)
        }
    }

    // ── Key helper ──────────────────────────────────────────────

    fn is_key_just_pressed(&self, key: vibe_input::KeyCode) -> bool {
        self.input.is_key_just_pressed(key)
    }
}

/// Clip a DrawCommand to a rectangular region. Returns None if fully outside.
fn clip_draw_command(cmd: &DrawCommand, clip_rect: [f32; 4]) -> Option<DrawCommand> {
    let [dx, dy, dw, dh] = cmd.dst_rect;
    let [cx, cy, cw, ch] = clip_rect;

    // Fully outside
    if dx + dw <= cx || dx >= cx + cw || dy + dh <= cy || dy >= cy + ch {
        return None;
    }

    // Fully inside
    if dx >= cx && dy >= cy && dx + dw <= cx + cw && dy + dh <= cy + ch {
        return Some(DrawCommand {
            texture_id: cmd.texture_id,
            src_rect: cmd.src_rect,
            dst_rect: cmd.dst_rect,
            color: cmd.color,
            flip_x: cmd.flip_x,
            flip_y: cmd.flip_y,
        });
    }

    // Partial overlap — clip both dst and src rects
    let new_x = dx.max(cx);
    let new_y = dy.max(cy);
    let new_right = (dx + dw).min(cx + cw);
    let new_bottom = (dy + dh).min(cy + ch);
    let new_w = new_right - new_x;
    let new_h = new_bottom - new_y;

    if new_w <= 0.0 || new_h <= 0.0 {
        return None;
    }

    // Adjust UV coordinates proportionally
    let [su, sv, sw, sh] = cmd.src_rect;
    let left_clip = (new_x - dx) / dw;
    let top_clip = (new_y - dy) / dh;
    let right_clip = (new_right - dx) / dw;
    let bottom_clip = (new_bottom - dy) / dh;

    let new_su = su + sw * left_clip;
    let new_sv = sv + sh * top_clip;
    let new_sw = sw * (right_clip - left_clip);
    let new_sh = sh * (bottom_clip - top_clip);

    Some(DrawCommand {
        texture_id: cmd.texture_id,
        src_rect: [new_su, new_sv, new_sw, new_sh],
        dst_rect: [new_x, new_y, new_w, new_h],
        color: cmd.color,
        flip_x: cmd.flip_x,
        flip_y: cmd.flip_y,
    })
}
