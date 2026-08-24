//! Finishing a level: the flagpole ending, what the pole is worth, and the fireworks.
//!
//! The two payout rules are lookup tables with a quirk each, and both are pure functions
//! of one number, so they sit at the top away from the sequence below them.
//!
//! The sequence itself is a six-beat cut-scene that owns the player from the grab to the
//! next level — a cousin of `castle.rs`, and the two are the only ways a level ends.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::player::PlayerAnim;

/// Points for grabbing the pole, by height (`flagscores`, `variables.lua:342`).
const FLAG_SCORES: [u32; 5] = [100, 400, 800, 2000, 5000];

/// The heights that separate them, as the **top edge of Mario in blocks**
/// (`flagvalues`). Lower numbers are higher up the pole, so the test is `y < value`.
///
/// Four thresholds for five bands, and the walk up is not linear: 100 at the bottom,
/// then 400, 800, 2000, and 5000 only for the very top of the pole.
const FLAG_HEIGHTS: [f32; 4] = [9.8125, 7.3125, 5.8125, 2.9375];

/// Each firework is worth **200**.
///
/// `mario.lua:462` says "500 points per firework" and it is *wrong* — it is a comment
/// sitting above the loop that spawns them, while the spawn itself does
/// `marioscore = marioscore + 200` (`firework.lua:7`). This port believed the comment.
/// NES SMB does pay 500, so the original's note is presumably a half-finished intention;
/// what matters here is matching Mari0, and Mari0 pays 200.
pub(crate) const FIREWORK_SCORE: u32 = 200;

/// What the pole pays out for a grab with the player's top edge at `top_blocks`.
///
/// The loop **stops at the first threshold you fail** rather than scanning them all
/// (`mario.lua:2942-2950`). With an ascending-height table that comes to the same
/// thing, and it is worth keeping because it says the bands are ordered on purpose.
pub(crate) fn flagpole_score(top_blocks: f32) -> u32 {
    let mut score = FLAG_SCORES[0];
    for (i, threshold) in FLAG_HEIGHTS.iter().enumerate() {
        if top_blocks < *threshold {
            score = FLAG_SCORES[i + 1];
        } else {
            break;
        }
    }
    score
}

/// How many fireworks go up, from the clock reading.
///
/// The rule is genuinely this odd: take the **last digit** of the rounded-up remaining
/// time, and keep it only if it is 1, 3 or 6 — anything else means no fireworks at all
/// (`mario.lua:2953-2957`, whose own comment reads "Who came up with this?").
///
/// Lab levels never get any: they have no clock, and the original suppresses them
/// whenever `portalbackground` is set.
pub(crate) fn firework_count(time_remaining: f32, portal_pack: bool) -> u32 {
    if portal_pack {
        return 0;
    }
    let digit = (time_remaining.ceil().max(0.0) as u32) % 10;
    if matches!(digit, 1 | 3 | 6) { digit } else { 0 }
}

// ── The sequence ────────────────────────────────────────────────────

/// Which beat of the flagpole ending is running.
///
/// One timer drives all of them and it is **never reset** — unlike the axe ending, whose
/// later beats are measured from Bowser's fall. That matters for [`CASTLE_MIN_TIME`],
/// which is an absolute floor on the whole sequence rather than a delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum FlagPhase {
    /// Sliding down the pole, with the flag coming down alongside.
    Slide,
    /// Hanging at the bottom, having swapped to the far side of the pole.
    Hang,
    /// Released, running for the castle at a fixed speed.
    Run,
    /// Inside. The clock is being cashed in at 50 points a unit.
    Countdown,
    /// The castle's own flag going up.
    CastleFlag,
    /// Fireworks, then the next level.
    Fireworks,
}

/// A flagpole ending in progress. While one exists the player has no control.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FlagSequence {
    pub(crate) phase: FlagPhase,
    /// Seconds since the pole was grabbed. Never reset.
    pub(crate) timer: f32,
    /// Where the flag sprite is, in world pixels — it descends with Mario.
    pub(crate) flag_y: f32,
    /// Accumulator for the one-unit-per-frame clock payout.
    pub(crate) subtract_timer: f32,
    /// The castle flag's offset above its final position, counting down to 0.
    pub(crate) castle_flag_y: f32,
    /// When the castle flag finished going up; the fireworks are timed from here.
    pub(crate) fireworks_from: f32,
    /// How many have gone off, and how many there will be.
    pub(crate) fired: u32,
    pub(crate) total: u32,
}

impl crate::game::Mari0Game {
    /// Grab the pole, if the player has reached it.
    ///
    /// The trigger is unchanged — the player's leading edge crossing the pole's column —
    /// but everything after it used to be a single lump of scoring. The height bonus is
    /// still banked here, because it depends on where on the pole he caught it and that
    /// is only true on this frame.
    pub(crate) fn check_flagpole(&mut self, ctx: &Context) {
        if self.flag.is_some() || self.level.flag_x <= 0.0 {
            return;
        }
        // The original's line is `x + width >= flagx + 6/16` (`mario.lua:968`), but that
        // 6/16 is sized for its 12/16-wide Mario. This port's is a full block, and the
        // pole's *base* — tile 78, the only solid cell in the pole — stops him with his
        // leading edge exactly on `flag_x`. Keeping the original's offset would mean the
        // trigger sat 12px beyond anywhere he can reach, and the level would never end.
        if self.player.x + self.player.width < self.level.flag_x {
            return;
        }
        // `y > 2.2`: the original refuses the grab above the pole's tip, so coming down
        // on it from a great height is not a shortcut past the animation.
        if self.player.y <= 2.2 * TILE_SIZE {
            return;
        }
        self.player.set_ducking(false);
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        // `x = flagx - 2/16`: on the near side of the pole, overlapping it slightly.
        self.player.x = self.level.flag_x - 2.0 / 16.0 * TILE_SIZE;
        self.player.anim_state = PlayerAnim::Climb;
        self.player.climb_frame = 2;
        self.score += flagpole_score(self.player.y / TILE_SIZE);
        let total = firework_count(self.time_remaining, self.current.pack == "portal");
        self.fireworks = total;
        self.flag = Some(FlagSequence {
            phase: FlagPhase::Slide,
            timer: 0.0,
            flag_y: FLAG_IMG_START,
            subtract_timer: 0.0,
            castle_flag_y: CASTLE_FLAG_START_Y,
            fireworks_from: 0.0,
            fired: 0,
            total,
        });
        ctx.audio.play("levelend");
    }

    /// Drive the ending. Returns true while it owns the player.
    pub(crate) fn update_flagpole(&mut self, ctx: &Context, dt: f32) -> bool {
        let Some(mut f) = self.flag else {
            return false;
        };
        f.timer += dt;

        match f.phase {
            FlagPhase::Slide => {
                // Mario and the flag descend over the same span in the same time, which
                // is what makes it read as him pulling it down.
                let progress = (f.timer / FLAG_DESCEND_TIME).min(1.0);
                f.flag_y = FLAG_IMG_START + FLAG_Y_DISTANCE * progress;
                self.player.y += FLAG_Y_DISTANCE / FLAG_DESCEND_TIME * dt;
                let bottom = FLAG_BOTTOM - self.player.height;
                if self.player.y > bottom {
                    self.player.y = bottom;
                    self.player.climb_frame = 2;
                } else {
                    // Twice the vine's rate, and it is the *timer* that is folded, so the
                    // flicker keeps going even once he has bottomed out mid-slide.
                    let phase = f.timer % (FLAG_CLIMB_FRAME_DELAY * 2.0);
                    self.player.climb_frame = if phase >= FLAG_CLIMB_FRAME_DELAY {
                        1
                    } else {
                        2
                    };
                }
                self.player.anim_state = PlayerAnim::Climb;
                if f.timer >= FLAG_DESCEND_TIME {
                    f.flag_y = FLAG_IMG_START + FLAG_Y_DISTANCE;
                    // Round to the far side of the pole, ready to run.
                    self.player.x = self.level.flag_x + 6.0 / 16.0 * TILE_SIZE;
                    self.player.facing_right = true;
                    f.phase = FlagPhase::Hang;
                }
            }

            FlagPhase::Hang => {
                if f.timer >= FLAG_DESCEND_TIME + FLAG_ANIM_DELAY {
                    self.player.anim_state = PlayerAnim::Run;
                    self.player.vx = FLAG_RUN_SPEED;
                    f.phase = FlagPhase::Run;
                }
            }

            FlagPhase::Run => {
                // Run, but still under gravity and still colliding — a flagpole sits on
                // a staircase in several levels and he has to come down it.
                // Re-asserted every frame: the resolver zeroes `vx` on any contact,
                // and several flagpoles sit at the foot of a staircase he brushes on the
                // way past. Gravity and collision still apply — just not input.
                self.player.vx = FLAG_RUN_SPEED;
                self.player.anim_state = PlayerAnim::Run;
                self.player.run_frame += self.player.vx.abs() * dt * 0.05;
                self.step_player_without_input(dt);
                if self.player.x >= self.level.flag_x + FLAG_CASTLE_DIST {
                    // Through the door. He is not drawn from here on.
                    self.player.vx = 0.0;
                    if self.time_remaining > 0.0 {
                        ctx.audio.play("scorering");
                        f.phase = FlagPhase::Countdown;
                    } else {
                        f.phase = FlagPhase::CastleFlag;
                    }
                }
            }

            FlagPhase::Countdown => {
                // One unit of clock per frame, 50 points each. Deliberately a *rate*
                // rather than an instant payout: the ticking is the reward.
                f.subtract_timer += dt;
                while f.subtract_timer > SCORE_SUBTRACT_SPEED {
                    f.subtract_timer -= SCORE_SUBTRACT_SPEED;
                    if self.time_remaining > 0.0 {
                        self.time_remaining = (self.time_remaining - 1.0).ceil().max(0.0);
                        self.score += 50;
                    }
                    if self.time_remaining <= 0.0 {
                        self.time_remaining = 0.0;
                        f.phase = FlagPhase::CastleFlag;
                        break;
                    }
                }
            }

            FlagPhase::CastleFlag => {
                // Floored at `CASTLE_MIN_TIME` from the *grab*, so a level finished with
                // a nearly empty clock still gets the same beat before the flag moves.
                if f.timer >= CASTLE_MIN_TIME {
                    f.castle_flag_y = (f.castle_flag_y - CASTLE_FLAG_SPEED * dt).max(0.0);
                    if f.castle_flag_y <= 0.0 {
                        f.fireworks_from = f.timer;
                        f.phase = FlagPhase::Fireworks;
                    }
                }
            }

            FlagPhase::Fireworks => {
                let since = f.timer - f.fireworks_from;
                while f.fired < f.total && since >= (f.fired + 1) as f32 * FIREWORK_DELAY {
                    f.fired += 1;
                    // Scattered around the castle, and each is worth 200 — see
                    // `FIREWORK_SCORE`, which the original's own comment gets wrong.
                    let spread = ((f.fired * 37) % 9) as f32 - 4.0;
                    self.fireworks_shown.push(Firework {
                        x: self.level.flag_x + FLAG_CASTLE_DIST + spread * TILE_SIZE,
                        y: (((f.fired * 53) % 5) + 2) as f32 * TILE_SIZE,
                        timer: 0.0,
                    });
                    self.score += FIREWORK_SCORE;
                }
                if since > f.total as f32 * FIREWORK_DELAY + FLAG_END_TIME {
                    self.flag = None;
                    self.state = crate::game::GameState::LevelComplete;
                    return true;
                }
            }
        }

        // The bang lands partway into a burst's life rather than on its first frame
        // (`firework.lua:13-15`), so the flash reads as leading the sound.
        for fw in &mut self.fireworks_shown {
            let before = fw.timer;
            fw.timer += dt;
            if before < FIREWORK_SOUND_TIME && fw.timer >= FIREWORK_SOUND_TIME {
                ctx.audio.play("boom");
            }
        }
        self.fireworks_shown.retain(|fw| fw.timer < FIREWORK_DELAY);

        self.flag = Some(f);
        true
    }
}

/// One firework burst, drawn from the fireball sheet's explosion frames.
///
/// `fireworkboom:draw` reaches for `fireballquad[5..7]` rather than any art of its own
/// (`firework.lua:22-32`), which is why there is no firework sprite in the assets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Firework {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) timer: f32,
}

impl Firework {
    /// Which of the three explosion frames is showing.
    pub(crate) fn frame(&self) -> u32 {
        let step = FIREWORK_DELAY / 3.0;
        if self.timer > step * 2.0 {
            2
        } else if self.timer > step {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five bands, checked at their edges. Sliding down to the bottom is worth 100;
    /// only the very top of the pole pays 5000.
    #[test]
    fn the_pole_pays_by_height() {
        assert_eq!(flagpole_score(12.0), 100, "at the foot of the pole");
        assert_eq!(
            flagpole_score(9.8125),
            100,
            "exactly on a threshold is below it"
        );
        assert_eq!(flagpole_score(9.8), 400);
        assert_eq!(flagpole_score(7.0), 800);
        assert_eq!(flagpole_score(5.0), 2000);
        assert_eq!(flagpole_score(2.0), 5000, "the top of the pole");
        assert_eq!(flagpole_score(0.0), 5000, "and anything above it");
    }

    /// Only three digits produce fireworks, and it is the *last* digit that counts —
    /// so 41 seconds left is one firework and 40 is none.
    #[test]
    fn fireworks_come_from_the_last_digit_of_the_clock() {
        assert_eq!(firework_count(41.0, false), 1);
        assert_eq!(firework_count(43.0, false), 3);
        assert_eq!(firework_count(46.0, false), 6);
        for none in [40.0, 42.0, 44.0, 45.0, 47.0, 48.0, 49.0] {
            assert_eq!(firework_count(none, false), 0, "{none} should give none");
        }
        // The clock is rounded *up*: 40.2 counts as 41.
        assert_eq!(firework_count(40.2, false), 1);
        assert_eq!(firework_count(0.0, false), 0);
    }

    #[test]
    fn the_lab_never_gets_fireworks() {
        assert_eq!(firework_count(41.0, true), 0);
    }
}
