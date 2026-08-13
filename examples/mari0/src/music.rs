//! Level music: which track plays, and the low-time switch to the fast variant.
//!
//! Mari0 keeps two parallel arrays of track names — the ordinary themes and their
//! `-fast` counterparts, index-aligned — and a level's `music` field selects one
//! by number. The switch to the fast variant when time runs low is **two discrete
//! events, not a crossfade**, and reproducing that timing exactly is the whole
//! reason this lives in its own module rather than inline in the update pass.

use vibe2d::prelude::*;

use crate::game::Mari0Game;

/// The five level themes, indexed by a level's `music` field minus 2.
///
/// The field's own encoding (`editor.lua:29`) is `1 = silent, 2 = overworld,
/// 3 = underground, 4 = castle, 5 = underwater, 6 = star, 7 = mappack-supplied`;
/// the original indexes its array with `musici - 1` because the silent case never
/// reaches the lookup. Track 7 has no bundled asset, so it falls back to silence.
const THEMES: [&str; 5] = [
    "overworld",
    "underground",
    "castle",
    "underwater",
    "starmusic",
];

/// Seconds of *game time* (not real time) between the low-time warning and the
/// music switching to its fast variant.
///
/// The original counts 7.5 time units here, and its clock runs at 2.5 units per
/// real second — so this is 3 real seconds. Expressed in the clock's own units so
/// it stays correct if the clock rate is ever revisited.
const FAST_MUSIC_DELAY: f32 = 7.5;

/// The clock reading that triggers the low-time warning.
pub(crate) const LOW_TIME: f32 = 99.0;

/// How fast the on-screen clock runs, in units per real second.
///
/// Not 1.0: Mari0 does `mariotime = mariotime - 2.5*dt` (`game.lua`), so a "400"
/// level lasts 160 real seconds. Using 1.0 here made every level two and a half
/// times as long as the original.
pub(crate) const TIME_RATE: f32 = 2.5;

/// Where the level's music is in its low-time sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum MusicPhase {
    /// Normal theme playing (or silence, for `music = 1`).
    #[default]
    Normal,
    /// Past 99: everything is stopped and the warning beep is sounding. The theme
    /// does *not* continue underneath — the original calls a blanket
    /// `love.audio.stop()` here.
    Warning,
    /// The `-fast` variant is playing, restarted from zero.
    Fast,
}

impl Mari0Game {
    /// Track name for this level, or `None` when the level asks for silence.
    fn theme(&self) -> Option<&'static str> {
        let index = self.level.music.checked_sub(2)? as usize;
        THEMES.get(index).copied()
    }

    /// Start this level's music from the beginning. Called on level entry and on
    /// respawn, both of which reset the low-time sequence.
    pub(crate) fn start_music(&mut self, ctx: &mut Context) {
        self.music_phase = MusicPhase::Normal;
        self.warning_started_at = None;
        match self.theme() {
            Some(track) => ctx.audio.play_music(track),
            None => ctx.audio.stop_music(),
        }
    }

    /// Advance the clock and drive the low-time music sequence.
    ///
    /// Returns `true` when time has run out and the player should die.
    #[must_use]
    pub(crate) fn tick_clock(&mut self, ctx: &mut Context, dt: f32) -> bool {
        // Levels with `timelimit = 0` (the intermission stubs, and the lab
        // mappack) are untimed; the clock must not run at all or they'd kill the
        // player on entry.
        if self.level.time_limit <= 0.0 {
            return false;
        }

        let before = self.time_remaining;
        self.time_remaining = (self.time_remaining - TIME_RATE * dt).max(0.0);

        // Crossing 99 stops *all* audio and sounds the warning. Deliberately not
        // a fade or a layer: the theme is gone until the fast variant starts.
        if before > LOW_TIME && self.time_remaining <= LOW_TIME {
            ctx.audio.stop_all();
            ctx.audio.play("lowtime");
            self.music_phase = MusicPhase::Warning;
            self.warning_started_at = Some(self.time_remaining);
        }

        // Then, 7.5 time units later, the fast variant starts from zero.
        if self.music_phase == MusicPhase::Warning
            && let Some(started) = self.warning_started_at
            && started - self.time_remaining >= FAST_MUSIC_DELAY
        {
            // A star grabbed during the warning wins, and it plays the *ordinary*
            // star theme — the original does not use `starmusic-fast` here.
            let track = if self.star_timer > 0.0 {
                "starmusic".to_string()
            } else {
                match self.theme() {
                    Some(theme) => format!("{theme}-fast"),
                    None => {
                        self.music_phase = MusicPhase::Fast;
                        return self.time_remaining <= 0.0;
                    }
                }
            };
            ctx.audio.play_music(&track);
            self.music_phase = MusicPhase::Fast;
        }

        self.time_remaining <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clock is 2.5x, so a 400-unit level is 160 real seconds — not 400.
    #[test]
    fn a_400_unit_level_lasts_160_real_seconds() {
        assert_eq!(400.0 / TIME_RATE, 160.0);
    }

    /// The gap between the warning and the fast music is 3 real seconds.
    #[test]
    fn fast_music_delay_is_three_real_seconds() {
        assert_eq!(FAST_MUSIC_DELAY / TIME_RATE, 3.0);
    }

    /// `music` field values map onto the theme list as `value - 2`, leaving 1 as
    /// silence and 7 (mappack-supplied) with no bundled track.
    #[test]
    fn music_field_maps_to_themes() {
        let theme_for = |field: u8| {
            field
                .checked_sub(2)
                .and_then(|i| THEMES.get(i as usize))
                .copied()
        };
        assert_eq!(theme_for(1), None, "1 means silent");
        assert_eq!(theme_for(2), Some("overworld"));
        assert_eq!(theme_for(3), Some("underground"));
        assert_eq!(theme_for(4), Some("castle"));
        assert_eq!(theme_for(5), Some("underwater"));
        assert_eq!(theme_for(6), Some("starmusic"));
        assert_eq!(theme_for(7), None, "mappack-supplied; no bundled asset");
    }

    /// Every theme must have a `-fast` counterpart, since the low-time switch
    /// composes the name rather than looking it up in a second table.
    #[test]
    fn every_theme_has_a_fast_variant_declared() {
        let declared = include_str!("../game.yaml");
        for theme in THEMES {
            assert!(
                declared.contains(&format!("{theme}-fast:")),
                "{theme}-fast is not declared in game.yaml"
            );
        }
    }
}
