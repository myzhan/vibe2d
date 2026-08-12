//! gilrs → `vibe_input` translation and rumble playback.
//!
//! Shared by both platform backends (desktop and web) — gilrs supports wasm32
//! natively via `navigator.getGamepads()`, so unlike the keyboard/mouse arms
//! there's no reason to duplicate this per platform.
//!
//! Gamepads are *polled*, not event-driven: winit emits no gamepad events at
//! all, so [`GamepadBackend::pump`] must be called explicitly once per frame.
//! See the call sites in `desktop.rs` / `web.rs` for why it has to run before
//! `on_update`.

use vibe_input::{GamepadAxis, GamepadButton, GamepadId, InputState};

// Rumble is desktop-only (see `apply_rumble`), so its imports are too.
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
#[cfg(not(target_arch = "wasm32"))]
use vibe_input::RumbleRequest;

/// Cap on simultaneously-live rumble effects, so mashing a rumble trigger can't
/// exhaust gilrs's force-feedback slots.
#[cfg(not(target_arch = "wasm32"))]
const MAX_ACTIVE_RUMBLE: usize = 16;

/// Extra time an effect handle is held beyond its nominal duration.
///
/// gilrs quantizes durations to 50 ms ticks and rounds *up*, so dropping the
/// handle at exactly `duration_ms` can cut the effect off mid-play — and
/// dropping an `Effect` stops it immediately.
#[cfg(not(target_arch = "wasm32"))]
const RUMBLE_SLACK_MS: u64 = 100;

/// A rumble effect that is currently playing.
///
/// The `Effect` handle must stay alive for the effect's duration: gilrs's
/// `Drop for Effect` sends `HandleDropped`, which stops playback. Holding it
/// forever would instead leak effect slots, hence the expiry sweep.
#[cfg(not(target_arch = "wasm32"))]
struct ActiveRumble {
    /// Never read — held purely so its `Drop` doesn't run early. Dropping the
    /// handle is what *stops* the effect, so the field being "unused" is
    /// precisely what makes it work.
    #[allow(dead_code)]
    effect: gilrs::ff::Effect,
    expires_at: Instant,
}

pub(crate) struct GamepadBackend {
    gilrs: gilrs::Gilrs,
    #[cfg(not(target_arch = "wasm32"))]
    active: Vec<ActiveRumble>,
}

impl GamepadBackend {
    /// Build the backend and seed already-connected pads into `input`.
    ///
    /// Returns `None` when the platform has no gamepad support at all, which is
    /// a normal outcome rather than an error.
    ///
    /// **The seeding is not optional.** `GilrsBuilder::build()` enumerates
    /// attached pads into its internal state but enqueues no `Connected`
    /// events, so a controller plugged in *before* launch generates no events
    /// and would otherwise stay invisible for the entire run.
    pub(crate) fn new(input: &mut InputState) -> Option<Self> {
        let gilrs = match gilrs::Gilrs::new() {
            Ok(gilrs) => gilrs,
            // `NotImplemented` carries a usable dummy instance; treat it as
            // "this platform has no gamepad support", not as a failure.
            Err(gilrs::Error::NotImplemented(_)) => {
                tracing::info!("gamepad: not supported on this platform");
                return None;
            }
            Err(e) => {
                tracing::warn!("gamepad: gilrs init failed: {e}");
                return None;
            }
        };

        for (id, pad) in gilrs.gamepads() {
            let vibe_id = GamepadId::new(usize::from(id));
            tracing::info!("gamepad: found {} ({})", pad.name(), vibe_id);
            input.on_gamepad_connected(vibe_id, pad.name().to_string());
        }

        Some(Self {
            gilrs,
            #[cfg(not(target_arch = "wasm32"))]
            active: Vec::new(),
        })
    }

    /// Drain every pending gilrs event into `input`.
    ///
    /// When `suppressed` (a VDP client is driving input) events are still
    /// drained but discarded. Skipping the poll entirely would let the OS event
    /// queue back up and then dump a burst of stale input the moment the VDP
    /// client disconnects.
    pub(crate) fn pump(&mut self, input: &mut InputState, suppressed: bool) {
        while let Some(event) = self.gilrs.next_event() {
            if suppressed {
                continue;
            }
            let pad = GamepadId::new(usize::from(event.id));
            match event.event {
                gilrs::EventType::Connected => {
                    let name = self.gilrs.gamepad(event.id).name().to_string();
                    input.on_gamepad_connected(pad, name);
                }
                gilrs::EventType::Disconnected => input.on_gamepad_disconnected(pad),
                gilrs::EventType::ButtonPressed(button, _) => {
                    if let Some(button) = map_button(button) {
                        // Logged post-translation on purpose: this is the line
                        // that proves the gilrs->vibe_input mapping, so it must
                        // show OUR name (e.g. LB -> "LeftShoulder"), not gilrs's.
                        tracing::debug!("gamepad: pad {pad} press {}", button.name());
                        input.on_gamepad_button_pressed(pad, button);
                    }
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    if let Some(button) = map_button(button) {
                        tracing::debug!("gamepad: pad {pad} release {}", button.name());
                        input.on_gamepad_button_released(pad, button);
                    }
                }
                gilrs::EventType::ButtonChanged(button, value, _) => {
                    if let Some(button) = map_button(button) {
                        tracing::debug!("gamepad: pad {pad} value {} {value:.3}", button.name());
                        input.on_gamepad_button_value(pad, button, value);
                    }
                }
                gilrs::EventType::AxisChanged(axis, value, _) => {
                    if let Some(axis) = map_axis(axis) {
                        tracing::debug!("gamepad: pad {pad} axis {} {value:+.3}", axis.name());
                        input.on_gamepad_axis_changed(pad, axis, value);
                    }
                }
                // `ButtonRepeated` only appears under the `Repeat` filter, which
                // we don't install. `Dropped` and `ForceFeedbackEffectCompleted`
                // are informational.
                _ => {}
            }
        }
    }

    /// Start playing the given rumble requests.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn apply_rumble(&mut self, requests: &[RumbleRequest]) {
        for request in requests {
            let targets: Vec<gilrs::GamepadId> = self
                .gilrs
                .gamepads()
                .filter(|(id, pad)| {
                    pad.is_ff_supported()
                        && request
                            .pad
                            .is_none_or(|want| usize::from(*id) == want.index())
                })
                .map(|(id, _)| id)
                .collect();
            if targets.is_empty() {
                continue;
            }

            let duration = gilrs::ff::Ticks::from_ms(request.duration_ms);
            let scheduling = gilrs::ff::Replay {
                play_for: duration,
                ..Default::default()
            };
            let magnitude = |v: f32| (v.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16;

            let mut builder = gilrs::ff::EffectBuilder::new();
            if request.strong > 0.0 {
                builder.add_effect(gilrs::ff::BaseEffect {
                    kind: gilrs::ff::BaseEffectType::Strong {
                        magnitude: magnitude(request.strong),
                    },
                    scheduling,
                    ..Default::default()
                });
            }
            if request.weak > 0.0 {
                builder.add_effect(gilrs::ff::BaseEffect {
                    kind: gilrs::ff::BaseEffectType::Weak {
                        magnitude: magnitude(request.weak),
                    },
                    scheduling,
                    ..Default::default()
                });
            }
            builder
                .gamepads(&targets)
                .repeat(gilrs::ff::Repeat::For(duration));

            match builder.finish(&mut self.gilrs) {
                Ok(effect) => {
                    if let Err(e) = effect.play() {
                        tracing::warn!("gamepad: rumble play failed: {e}");
                        continue;
                    }
                    // Logged so "I pressed the button and felt nothing" can be
                    // told apart from "the request never reached the driver".
                    tracing::debug!(
                        "gamepad: rumble strong={:.2} weak={:.2} for {}ms on {} pad(s)",
                        request.strong,
                        request.weak,
                        request.duration_ms,
                        targets.len(),
                    );
                    // Evict the oldest rather than refusing the newest — the most
                    // recent feedback is the one the player is waiting on.
                    if self.active.len() >= MAX_ACTIVE_RUMBLE {
                        self.active.remove(0);
                    }
                    self.active.push(ActiveRumble {
                        effect,
                        expires_at: Instant::now()
                            + Duration::from_millis(
                                u64::from(request.duration_ms) + RUMBLE_SLACK_MS,
                            ),
                    });
                }
                Err(e) => tracing::warn!("gamepad: rumble effect build failed: {e}"),
            }
        }
    }

    /// Reap finished rumble effects. Must run every frame, not only on frames
    /// that queued new requests.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn expire_rumble(&mut self) {
        let now = Instant::now();
        // Dropping the `Effect` sends `HandleDropped`, which is exactly the
        // "stop and free the slot" semantics we want once the scheduled
        // duration has elapsed.
        self.active.retain(|a| a.expires_at > now);
    }
}

/// gilrs → `vibe_input` button translation.
///
/// **This is where gilrs's confusing trigger naming is fixed.** gilrs calls the
/// shoulder button `LeftTrigger` and the analog trigger `LeftTrigger2`; we swap
/// them onto `LeftShoulder` / `LeftTrigger` so the trap dies in this one
/// function instead of in every `game.yaml` written against the engine.
///
/// Returns `None` for `C` / `Z` (Sega-era six-button pads, which we don't model)
/// and `Unknown`.
fn map_button(button: gilrs::Button) -> Option<GamepadButton> {
    use gilrs::Button as G;
    Some(match button {
        G::South => GamepadButton::South,
        G::East => GamepadButton::East,
        G::North => GamepadButton::North,
        G::West => GamepadButton::West,
        // Shoulders — gilrs's unsuffixed names.
        G::LeftTrigger => GamepadButton::LeftShoulder,
        G::RightTrigger => GamepadButton::RightShoulder,
        // Analog triggers — gilrs's "2"-suffixed names.
        G::LeftTrigger2 => GamepadButton::LeftTrigger,
        G::RightTrigger2 => GamepadButton::RightTrigger,
        G::Select => GamepadButton::Select,
        G::Start => GamepadButton::Start,
        G::Mode => GamepadButton::Mode,
        G::LeftThumb => GamepadButton::LeftThumb,
        G::RightThumb => GamepadButton::RightThumb,
        G::DPadUp => GamepadButton::DPadUp,
        G::DPadDown => GamepadButton::DPadDown,
        G::DPadLeft => GamepadButton::DPadLeft,
        G::DPadRight => GamepadButton::DPadRight,
        G::C | G::Z | G::Unknown => return None,
    })
}

/// gilrs → `vibe_input` axis translation.
///
/// `LeftZ` / `RightZ` are dropped: on the drivers that report them they're the
/// analog triggers, which already arrive as `ButtonChanged(LeftTrigger2, …)`.
/// Reporting them twice would make pulling a trigger look like stick movement.
///
/// `DPadX` / `DPadY` never reach us — gilrs's default `axis_dpad_to_button`
/// filter converts axis-reported d-pads into `DPad*` button events.
fn map_axis(axis: gilrs::Axis) -> Option<GamepadAxis> {
    use gilrs::Axis as A;
    Some(match axis {
        A::LeftStickX => GamepadAxis::LeftStickX,
        A::LeftStickY => GamepadAxis::LeftStickY,
        A::RightStickX => GamepadAxis::RightStickX,
        A::RightStickY => GamepadAxis::RightStickY,
        A::LeftZ | A::RightZ | A::DPadX | A::DPadY | A::Unknown => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shoulder_and_trigger_are_swapped_relative_to_gilrs_names() {
        // The whole point of `map_button`. gilrs's unsuffixed `LeftTrigger` is
        // the SHOULDER; its `LeftTrigger2` is the analog TRIGGER. If these two
        // assertions ever flip, every game's LB/LT bindings silently swap.
        assert_eq!(
            map_button(gilrs::Button::LeftTrigger),
            Some(GamepadButton::LeftShoulder)
        );
        assert_eq!(
            map_button(gilrs::Button::LeftTrigger2),
            Some(GamepadButton::LeftTrigger)
        );
        assert_eq!(
            map_button(gilrs::Button::RightTrigger),
            Some(GamepadButton::RightShoulder)
        );
        assert_eq!(
            map_button(gilrs::Button::RightTrigger2),
            Some(GamepadButton::RightTrigger)
        );
    }

    #[test]
    fn face_buttons_map_positionally() {
        assert_eq!(map_button(gilrs::Button::South), Some(GamepadButton::South));
        assert_eq!(map_button(gilrs::Button::East), Some(GamepadButton::East));
        assert_eq!(map_button(gilrs::Button::North), Some(GamepadButton::North));
        assert_eq!(map_button(gilrs::Button::West), Some(GamepadButton::West));
    }

    #[test]
    fn unmodelled_buttons_are_dropped() {
        assert_eq!(map_button(gilrs::Button::C), None);
        assert_eq!(map_button(gilrs::Button::Z), None);
        assert_eq!(map_button(gilrs::Button::Unknown), None);
    }

    #[test]
    fn every_mapped_button_is_distinct() {
        // A copy-paste slip in `map_button` that mapped two gilrs buttons onto
        // the same vibe button would be invisible in normal play.
        let all = [
            gilrs::Button::South,
            gilrs::Button::East,
            gilrs::Button::North,
            gilrs::Button::West,
            gilrs::Button::LeftTrigger,
            gilrs::Button::RightTrigger,
            gilrs::Button::LeftTrigger2,
            gilrs::Button::RightTrigger2,
            gilrs::Button::Select,
            gilrs::Button::Start,
            gilrs::Button::Mode,
            gilrs::Button::LeftThumb,
            gilrs::Button::RightThumb,
            gilrs::Button::DPadUp,
            gilrs::Button::DPadDown,
            gilrs::Button::DPadLeft,
            gilrs::Button::DPadRight,
        ];
        let mut mapped: Vec<GamepadButton> = all.iter().filter_map(|b| map_button(*b)).collect();
        assert_eq!(mapped.len(), all.len(), "some button failed to map");
        mapped.sort();
        let before = mapped.len();
        mapped.dedup();
        assert_eq!(before, mapped.len(), "two gilrs buttons mapped to the same");
        // And together they cover every variant the engine exposes.
        assert_eq!(mapped.len(), GamepadButton::ALL.len());
    }

    #[test]
    fn only_the_four_stick_axes_map() {
        assert_eq!(
            map_axis(gilrs::Axis::LeftStickX),
            Some(GamepadAxis::LeftStickX)
        );
        assert_eq!(
            map_axis(gilrs::Axis::RightStickY),
            Some(GamepadAxis::RightStickY)
        );
        // Analog triggers arrive as ButtonChanged, so the Z axes are dropped to
        // avoid double-reporting them as stick motion.
        assert_eq!(map_axis(gilrs::Axis::LeftZ), None);
        assert_eq!(map_axis(gilrs::Axis::RightZ), None);
        // gilrs's default filter converts these to DPad* buttons before we see
        // them; mapping them would create a second path for the same input.
        assert_eq!(map_axis(gilrs::Axis::DPadX), None);
        assert_eq!(map_axis(gilrs::Axis::DPadY), None);
        assert_eq!(map_axis(gilrs::Axis::Unknown), None);
    }
}
