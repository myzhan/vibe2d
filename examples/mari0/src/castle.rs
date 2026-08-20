//! The axe ending: the castle's second way to finish a level.
//!
//! Every castle has one (`1-4` … `7-4` and `8-4_4`), and it is not a flagpole. Touching
//! the axe hands control to a scripted sequence: the chain goes, then the bridge
//! disappears one tile at a time from right to left, and when it runs out Bowser drops
//! into the lava. Only then is Mario released to walk to the toad.
//!
//! Everything runs off one timer that is **reset once**, when Bowser starts falling
//! (`mario.lua:517`) — so the later beats are measured from his fall rather than from
//! the axe, which is why the constants read as small numbers despite the bridge taking
//! most of a second to collapse.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::enemies::EnemyType;
use crate::game::Mari0Game;

/// Which beat of the castle ending is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum CastlePhase {
    /// Axe taken. Everything is frozen while the chain hangs there for a moment.
    Chain,
    /// Bridge tiles vanishing right to left, one every [`CASTLE_BRIDGE_DELAY`].
    Bridge,
    /// Bowser is falling. The timer restarted here.
    BowserFalls,
    /// Mario, released, running towards the far wall.
    MarioRuns,
    /// Standing at the end. Waiting to move on.
    Done,
}

/// The state of an axe ending in progress.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CastleEnding {
    pub(crate) phase: CastlePhase,
    /// Seconds in the current run of the sequence. Reset once, at Bowser's fall.
    pub(crate) timer: f32,
    /// Accumulator for the per-tile bridge delay.
    pub(crate) bridge_timer: f32,
    /// The next bridge tile to remove. Walks **left**.
    pub(crate) bridge: (i32, i32),
}

impl Mari0Game {
    /// Has the player touched the axe? If so, start the ending.
    ///
    /// The axe is a marker rather than an object here, so the test is the same shape as
    /// the flagpole's: the player's leading edge crossing its column.
    pub(crate) fn check_axe(&mut self, ctx: &Context) {
        if self.castle.is_some() {
            return;
        }
        let Some((ax, ay)) = self.level.axe else {
            return;
        };
        let axe_x = ax as f32 * TILE_SIZE;
        if self.player.x + self.player.width <= axe_x {
            return;
        }

        // Taking the axe clears the board: portals go, and **every platform is
        // deleted** (`mario.lua:2978-2980`) — 4-4 and 7-4 have moving platforms right up
        // to the bridge, and leaving them running would carry Mario off during a
        // sequence he has no control over.
        self.portals = [None, None];
        self.projectiles.clear();
        self.refresh_portal_holes();
        self.platforms.clear();
        self.level.platform_rects.clear();
        self.player.vx = 0.0;
        self.player.vy = 0.0;

        self.castle = Some(CastleEnding {
            phase: CastlePhase::Chain,
            timer: 0.0,
            bridge_timer: CASTLE_BRIDGE_DELAY,
            // The sweep starts one column left of the axe and two rows below it
            // (`mario.lua:2987-2988`), which is the right-hand end of the bridge run.
            bridge: (ax as i32 - 1, ay as i32 + 2),
        });
        ctx.audio.play("levelend");
    }

    /// Advance the castle ending. Returns true while it owns the player.
    ///
    /// Like the pipe transit, this suspends the normal update — but only until Mario is
    /// released, after which he runs under his own physics with input still disabled.
    pub(crate) fn update_castle(&mut self, dt: f32, ctx: &mut Context) -> bool {
        let Some(mut c) = self.castle else {
            return false;
        };
        c.timer += dt;

        match c.phase {
            CastlePhase::Chain => {
                if c.timer >= CASTLE_CHAIN_DISAPPEAR {
                    c.phase = CastlePhase::Bridge;
                }
            }
            CastlePhase::Bridge => {
                c.bridge_timer += dt;
                while c.bridge_timer > CASTLE_BRIDGE_DELAY {
                    c.bridge_timer -= CASTLE_BRIDGE_DELAY;
                    if self.remove_bridge_tile(c.bridge, ctx) {
                        c.bridge.0 -= 1;
                    } else {
                        // The run has ended — the next tile isn't bridge. That is what
                        // drops Bowser, and the timer restarts from here so the beats
                        // after it are measured from the fall.
                        c.phase = CastlePhase::BowserFalls;
                        c.timer = 0.0;
                        self.drop_bowser(ctx);
                        break;
                    }
                }
            }
            CastlePhase::BowserFalls => {
                if c.timer >= CASTLE_MARIO_MOVE {
                    c.phase = CastlePhase::MarioRuns;
                    // Released, but not under player control: he runs right at a fixed
                    // speed until he reaches the wall (`mario.lua:521-529`).
                    self.player.vx = CASTLE_MARIO_SPEED;
                    ctx.audio.play("castleend");
                }
            }
            CastlePhase::MarioRuns => {
                // `mapwidth - 8` blocks from the left edge (`mario.lua:534`) — a fixed
                // stop, which is where the toad stands.
                let stop = (self.level.width as f32 - CASTLE_STOP_FROM_END) * TILE_SIZE;
                if self.player.x >= stop {
                    self.player.x = stop;
                    self.player.vx = 0.0;
                    c.phase = CastlePhase::Done;
                    // The timer is *not* reset again here: `CASTLE_NEXT_LEVEL` is
                    // measured from Bowser's fall, the same origin as everything after it.
                } else {
                    // Re-asserted every frame, because the collision resolver zeroes
                    // `vx` on any contact and he brushes the pillar as he runs off it.
                    self.player.vx = CASTLE_MARIO_SPEED;
                }
            }
            CastlePhase::Done => {
                // The original spends this time on two lines of text and a splash
                // screen; both belong with the end-of-level screen rather than here, so
                // for now the wait is kept and the level simply advances after it.
                if c.timer >= CASTLE_NEXT_LEVEL {
                    self.castle = None;
                    self.advance_level();
                    return true;
                }
            }
        }

        // Once released he still needs gravity and collision — he runs off the pillar
        // the axe stands on and lands on the castle floor — but **not** input handling.
        // The original keeps `controlsenabled = false` for the whole sequence
        // (`mario.lua:2989`); letting the normal pass run instead just decays his speed
        // to nothing, because no direction key is held.
        if matches!(c.phase, CastlePhase::MarioRuns | CastlePhase::Done) {
            self.step_player_without_input(dt);
        }

        self.castle = Some(c);
        // Owns the frame throughout: from the axe to the next level there is no input.
        true
    }

    /// Move the player under gravity and collision with no input at all.
    ///
    /// Used by the scripted walk at the end of a castle. Deliberately not the full
    /// player update: no friction, no jump, no direction — his velocity is whatever the
    /// sequence set it to.
    pub(crate) fn step_player_without_input(&mut self, dt: f32) {
        self.player.vy = (self.player.vy + GRAVITY * dt).min(MAX_Y_SPEED);
        self.player.vx = crate::physics::move_and_collide_x(
            &mut self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
            self.player.vx,
            &self.level,
            dt,
            crate::physics::Body::Normal,
        );
        let (vy, on_ground) = crate::physics::move_and_collide_y(
            self.player.x,
            &mut self.player.y,
            self.player.width,
            self.player.height,
            self.player.vy,
            &self.level,
            dt,
            crate::physics::Body::Normal,
        );
        self.player.vy = vy;
        self.player.on_ground = on_ground;
    }

    /// Remove one bridge tile, and the chain above it if there is one.
    ///
    /// Returns false when the cell isn't a bridge tile, which is how the sweep knows it
    /// has reached the end of the run.
    ///
    /// The two ids are the only hardcoded tile numbers in the sequence: **11 is bridge
    /// and 10 is chain** (`mario.lua:490-495`). The chain check looks one row *up* and
    /// only fires on the first step, since there is exactly one chain link, at the
    /// right-hand end under the axe.
    fn remove_bridge_tile(&mut self, cell: (i32, i32), ctx: &Context) -> bool {
        let (cx, cy) = cell;
        if cx < 0 || cy < 0 || cy as usize >= self.level.height || cx as usize >= self.level.width {
            return false;
        }
        if self.level.tiles[cy as usize][cx as usize] != CASTLE_BRIDGE_TILE {
            return false;
        }
        self.level.tiles[cy as usize][cx as usize] = SMB_EMPTY;
        if cy > 0 && self.level.tiles[cy as usize - 1][cx as usize] == CASTLE_CHAIN_TILE {
            self.level.tiles[cy as usize - 1][cx as usize] = SMB_EMPTY;
        }
        ctx.audio.play("bridgebreak");
        true
    }

    /// Send Bowser into the lava.
    ///
    /// He stops moving entirely and falls at 27.5 blocks/s² — a *different* gravity from
    /// his own 10.9 (`mario.lua:512`), so the drop is much heavier than his hops.
    fn drop_bowser(&mut self, ctx: &Context) {
        let mut fell = false;
        for e in &mut self.enemies {
            if e.enemy_type == EnemyType::Bowser {
                e.vx = 0.0;
                e.vy = 0.0;
                e.backing_off = false;
                e.falling_to_lava = true;
                fell = true;
            }
        }
        if fell {
            ctx.audio.play("bowserfall");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The beats after Bowser's fall are measured from the fall, not from the axe.
    ///
    /// The timer is reset once, which is the only reason `CASTLE_MARIO_MOVE = 1.07`
    /// makes sense: the bridge alone takes most of a second to collapse.
    #[test]
    fn the_later_beats_are_measured_from_bowsers_fall() {
        // 1-4's bridge is 13 tiles. Chain (0.38) plus 13 at 0.06 is 1.16s to the fall —
        // past `CASTLE_MARIO_MOVE` at 1.07, so without the reset Mario would be released
        // while the bridge was still going, and would run onto tiles that then vanish.
        let to_the_fall = CASTLE_CHAIN_DISAPPEAR + 13.0 * CASTLE_BRIDGE_DELAY;
        assert!(
            to_the_fall > CASTLE_MARIO_MOVE,
            "without the reset Mario would be released mid-collapse: {to_the_fall}"
        );
        const { assert!(CASTLE_CHAIN_DISAPPEAR < CASTLE_MARIO_MOVE) };
        const { assert!(CASTLE_NEXT_LEVEL > CASTLE_MARIO_MOVE) };
    }

    /// Bowser falls under a heavier gravity than he lives under.
    #[test]
    fn the_death_drop_is_heavier_than_his_hops() {
        const { assert!(CASTLE_BOWSER_FALL_GRAVITY > BOWSER_GRAVITY) };
    }
}
