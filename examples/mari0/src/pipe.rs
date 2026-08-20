//! Pipes: entering one, the slide animation, and arriving in the destination.
//!
//! Three trips share this code — down into a sublevel, sideways into one, and
//! rising out of the pipe at the far end. What differs is the axis and whether the
//! transition ends by loading a level or by handing control back.
//!
//! The load is deliberately *not* instant. The original slides Mario in over
//! `pipeanimationtime`, holds him out of sight for `pipeanimationdelay`, and only
//! then swaps levels (`mario.lua:298-313`); the arrival waits `pipeupdelay` before
//! sliding him out. Skipping the hold makes pipes feel like teleports.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::{GameState, Mari0Game};

/// How long the slide into or out of a pipe takes (`pipeanimationtime`).
const SLIDE_TIME: f32 = 0.7;
/// How long Mario stays hidden inside the pipe before the level swaps
/// (`pipeanimationdelay`).
const HOLD_TIME: f32 = 1.0;
/// How long the destination sits still before Mario rises out (`pipeupdelay`).
const EMERGE_DELAY: f32 = 1.0;
/// Distance travelled by a downward slide: `pipeanimationdistancedown = 32/16`
/// blocks, i.e. two tiles.
const SLIDE_DOWN_DIST: f32 = 2.0 * TILE_SIZE;
/// Distance travelled by a sideways slide: `pipeanimationdistanceright = 16/16`,
/// one tile.
const SLIDE_RIGHT_DIST: f32 = TILE_SIZE;

/// Which way Mario is moving through the pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum PipeDir {
    /// Sinking into a pipe mouth underfoot.
    Down,
    /// Walking into a pipe mouth to the right.
    Right,
    /// Rising out of the destination pipe.
    Up,
}

/// Where a pipe leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeTarget {
    /// A sublevel of the current level; 0 means back to the main level.
    Sublevel(u32),
    /// A warp pipe: the first level of another world.
    Warp(u32),
    /// Nothing left to do — this is the arrival half of a trip.
    None,
}

/// An in-progress pipe trip. While one exists the player has no control.
#[derive(Debug, Clone)]
pub(crate) struct PipeTransit {
    pub(crate) dir: PipeDir,
    pub(crate) target: PipeTarget,
    /// Seconds since the trip began.
    pub(crate) timer: f32,
    /// Where Mario was when the slide started, so the slide is a lerp rather than
    /// an accumulation (which would drift with the frame rate).
    pub(crate) from: (f32, f32),
}

impl Mari0Game {
    /// Is the player mid-pipe? Control and physics are suspended while so.
    pub(crate) fn in_pipe(&self) -> bool {
        self.pipe.is_some()
    }

    /// Start a trip into a pipe. Ignored if one is already running.
    fn enter_pipe(&mut self, ctx: &Context, dir: PipeDir, target: PipeTarget) {
        if self.pipe.is_some() {
            return;
        }
        ctx.audio.play("pipe");
        // Taking a pipe out of an intermission stub makes the destination the place
        // to respawn (`mario.lua:2891-2893`). Without this, dying in 1-2_1 drops the
        // player back into 1-2 — a 24-tile corridor whose only content is this pipe.
        if self.level.intermission
            && let PipeTarget::Sublevel(dest) = target
        {
            self.respawn_sublevel = dest;
        }
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.pipe = Some(PipeTransit {
            dir,
            target,
            timer: 0.0,
            from: (self.player.x, self.player.y),
        });
    }

    /// Look for a pipe the player is trying to enter, and start the trip.
    ///
    /// Two triggers, both from the original:
    ///
    /// - **Down** (`mario.lua:930-938`): holding down while standing on a pipe's
    ///   mouth. The entity sits on the mouth's **top-right** cell — 1-1's pipe is
    ///   tiles 16/17 across columns 57-58 of row 9, with entity `21-1` on (58, 9) —
    ///   so the probe row is the tile being stood *on*, and both cells under the
    ///   player's footprint are checked. The original probes a single point at a
    ///   fixed offset; probing the footprint is equivalent wherever a mouth is two
    ///   tiles wide, and doesn't depend on reproducing its half-tile body offsets.
    /// - **Right** (`mario.lua:1935-1946`): being *stopped* by a pipe cell. 1-2's
    ///   pipe is a sideways mouth (tile 80) at (10, 12), level with a standing
    ///   player. The original runs this check inside horizontal collision
    ///   resolution, so `blocked_right` must mean "tried to move right and was
    ///   stopped", not merely "holding right" — otherwise walking *across the top*
    ///   of 1-1's pipe swallows the player, since the entity sits on the mouth's
    ///   top-right cell and a standing player's midline is level with it.
    ///   The original checks the cell at Mario's midline and then the one below, so
    ///   a big Mario entering a two-tile mouth still triggers.
    pub(crate) fn check_pipe_entry(&mut self, ctx: &Context, down_held: bool, blocked_right: bool) {
        if self.pipe.is_some() {
            return;
        }

        if down_held && self.player.on_ground {
            let row = (self.player.bottom() / TILE_SIZE).floor() as i32;
            let left = (self.player.x / TILE_SIZE).floor() as i32;
            // `- 1.0` keeps a player flush against a tile boundary from claiming the
            // next column along.
            let right = ((self.player.x + self.player.width - 1.0) / TILE_SIZE).floor() as i32;
            for col in left..=right {
                if let Some(target) = self.pipe_target_at((col, row)) {
                    self.enter_pipe(ctx, PipeDir::Down, target);
                    return;
                }
            }
        }

        if blocked_right {
            let px = self.player.x + self.player.width + 2.0;
            let col = (px / TILE_SIZE).floor() as i32;
            let mid_row = ((self.player.y + self.player.height * 0.5) / TILE_SIZE).floor() as i32;
            for row in [mid_row, mid_row + 1] {
                if let Some(target) = self.pipe_target_at((col, row)) {
                    self.enter_pipe(ctx, PipeDir::Right, target);
                    return;
                }
            }
        }
    }

    /// The window Mario stays visible in while moving through a pipe.
    ///
    /// Without this he slides *over* the pipe instead of into it. The original
    /// clips him to a small box outside the mouth (`customscissor`, set in
    /// `mario:pipe` and cleared when `pipeup` finishes) — that's one of the 43
    /// `setScissor` calls that are gameplay-visible rather than decoration.
    ///
    /// Expressed as "outside the mouth" rather than by porting the original's
    /// literal rect: its numbers are relative to a body origin offset by `-6/16` of
    /// a block, so copying them across coordinate systems would be guesswork. The
    /// mouth edge comes from where Mario stood when the trip began, which is exact.
    ///
    /// Returned in screen space, matching the draw calls it wraps.
    pub(crate) fn pipe_clip_rect(&self, cam_x: f32, vw: f32, vh: f32) -> Option<[f32; 4]> {
        let transit = self.pipe.as_ref()?;
        match transit.dir {
            // Feet at the start of the trip == the mouth's top surface.
            PipeDir::Down => {
                let mouth_top = transit.from.1 + self.player.height;
                Some([0.0, 0.0, vw, mouth_top.max(0.0)])
            }
            // The arrival starts sunk by the slide distance, so undo it to recover
            // the rim.
            PipeDir::Up => {
                let mouth_top = transit.from.1 - SLIDE_DOWN_DIST + self.player.height;
                Some([0.0, 0.0, vw, mouth_top.max(0.0)])
            }
            // Right edge at the start of the trip == the mouth's near face.
            PipeDir::Right => {
                let mouth_left = transit.from.0 + self.player.width - cam_x;
                Some([0.0, 0.0, mouth_left.max(0.0), vh])
            }
        }
    }

    /// What the pipe at this cell leads to, if there's a pipe there at all.
    fn pipe_target_at(&self, cell: (i32, i32)) -> Option<PipeTarget> {
        if let Some(dest) = self.level.pipes.get(&cell) {
            return Some(PipeTarget::Sublevel(*dest));
        }
        if let Some(world) = self.level.warp_pipes.get(&cell) {
            return Some(PipeTarget::Warp(*world));
        }
        None
    }

    /// Drive the current pipe trip. Returns `true` while one is active, in which
    /// case the caller must skip normal player movement.
    pub(crate) fn update_pipe(&mut self, dt: f32) -> bool {
        let Some(mut transit) = self.pipe.take() else {
            return false;
        };
        transit.timer += dt;

        match transit.dir {
            PipeDir::Down | PipeDir::Right => {
                let progress = (transit.timer / SLIDE_TIME).min(1.0);
                if transit.dir == PipeDir::Down {
                    self.player.y = transit.from.1 + progress * SLIDE_DOWN_DIST;
                } else {
                    self.player.x = transit.from.0 + progress * SLIDE_RIGHT_DIST;
                }
                if transit.timer >= SLIDE_TIME + HOLD_TIME {
                    let target = transit.target;
                    // Consumed: `travel_to` installs the arrival transit itself.
                    self.travel_to(target);
                    return true;
                }
            }
            PipeDir::Up => {
                // Held still for a beat, then slid out. Before the delay elapses
                // Mario sits at his start position, hidden inside the pipe.
                let sliding = (transit.timer - EMERGE_DELAY).max(0.0);
                let progress = (sliding / SLIDE_TIME).min(1.0);
                self.player.y = transit.from.1 - progress * SLIDE_DOWN_DIST;
                if transit.timer >= EMERGE_DELAY + SLIDE_TIME {
                    return true; // trip over; `self.pipe` stays `None`
                }
            }
        }

        self.pipe = Some(transit);
        true
    }

    /// Load whatever the pipe led to and set up the arrival.
    ///
    /// Not private to pipes: a vine leads into a sublevel and falling out of a bonus
    /// stage leads back out of one, and both want the same `pipespawn` pairing this
    /// does — otherwise the return trip from 2-1_1 would drop you at 2-1's start
    /// instead of at the pipe on column 162.
    pub(crate) fn travel_to(&mut self, target: PipeTarget) {
        let from_sublevel = self.current.sublevel;
        match target {
            PipeTarget::None => {}
            PipeTarget::Warp(world) => {
                // A warp zone leaves the current level entirely, so it behaves like
                // finishing one: fresh clock, no arrival animation.
                self.current.warp_to_world(world);
                if self.current.exists() {
                    self.reset_level();
                    // A warp leaves the level entirely, so it gets the full "world 5-1"
                    // card rather than the sublevel blink.
                    self.begin_interlude(crate::interlude::InterludeKind::LevelScreen);
                } else {
                    self.state = GameState::Menu;
                }
                return;
            }
            PipeTarget::Sublevel(dest) => {
                let next = self.current.with_sublevel(dest);
                if !next.exists() {
                    // A pipe pointing at a file that isn't there. Rather than
                    // stranding the player mid-animation, hand control straight
                    // back where they stood.
                    tracing::warn!(
                        "pipe in {} leads to missing level {}",
                        self.current.name(),
                        next.name()
                    );
                    return;
                }
                self.current = next;
            }
        }

        // Carried across the load: the clock does not reset when the destination is
        // a sublevel of the same level (`game.lua:2111` only resets it on the
        // non-sublevel branch), and neither does the score.
        let clock = self.time_remaining;
        self.reset_level();
        self.time_remaining = clock;
        // The sublevel blink. Two lead-ins long and so it never draws anything — which is
        // exactly why a pipe reads as a blink where a flagpole reads as a card
        // (`levelscreen.lua:21-28`).
        self.begin_interlude(crate::interlude::InterludeKind::Sublevel);

        // Arrive at the `pipespawn` that pairs with the trip. Going *in*, the
        // original matches on the sublevel being entered; coming *back*, on the one
        // being left. Either way it's "the sublevel that isn't the main level".
        let pairs_with = if self.current.sublevel == 0 {
            from_sublevel
        } else {
            self.current.sublevel
        };
        if let Some((col, row)) = self.level.pipe_spawns.get(&pairs_with).copied() {
            self.place_at_pipe_exit(col, row);
        }
    }

    /// Put the player inside the exit pipe and start them rising out of it.
    fn place_at_pipe_exit(&mut self, col: i32, row: i32) {
        let x = col as f32 * TILE_SIZE;
        // Sunk by the slide distance so the rise ends with his feet on the rim.
        let surface_y = row as f32 * TILE_SIZE - self.player.height;
        let y = surface_y + SLIDE_DOWN_DIST;
        self.player.x = x;
        self.player.y = y;
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.player.on_ground = false;

        // The camera has to be where the exit is before the first frame draws,
        // otherwise the level visibly snaps once the player starts moving.
        let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
        self.camera.x = (x - self.vw / 3.0).clamp(0.0, max_camera);
        self.spawn_frontier = -1;
        self.spawned = vec![false; self.level.enemy_spawns.len()];
        self.enemies.clear();
        self.spawn_revealed_columns();

        self.pipe = Some(PipeTransit {
            dir: PipeDir::Up,
            target: PipeTarget::None,
            timer: 0.0,
            from: (x, y),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The timings are the original's, in seconds. If these drift, pipes stop
    /// feeling like pipes.
    #[test]
    fn timings_match_the_original() {
        assert_eq!(SLIDE_TIME, 0.7, "pipeanimationtime");
        assert_eq!(HOLD_TIME, 1.0, "pipeanimationdelay");
        assert_eq!(EMERGE_DELAY, 1.0, "pipeupdelay");
        // 32/16 and 16/16 blocks, at 32px per block.
        assert_eq!(SLIDE_DOWN_DIST, 64.0);
        assert_eq!(SLIDE_RIGHT_DIST, 32.0);
    }

    /// A downward trip takes slide + hold before the level swaps, so the whole
    /// transition is 1.7s of animation and not an instant cut.
    #[test]
    fn a_downward_trip_holds_before_loading() {
        assert_eq!(SLIDE_TIME + HOLD_TIME, 1.7);
    }
}
