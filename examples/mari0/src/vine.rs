//! Vines: the beanstalk out of a brick, and the one climbing state Mario has.
//!
//! Five levels have one — 2-1, 3-1, 4-2_1, 5-2 and 6-2 — and each is a brick with
//! entity 14 in it. Hitting the brick sprouts a vine that grows up out of it, and
//! the vine is the only thing in the game you can hold onto: gravity is switched
//! off, up and down move you along it, and left/right first hop you round to the
//! other side and only then let go.
//!
//! Three states share this module, matching the original's `self.vine` flag and its
//! two `animation` values:
//!
//! - [`VineState::Grip`] — hanging on, with the controls (and the clock) still live.
//! - [`VineState::Leaving`] — past the top of the screen, rising out of the level
//!   until the destination sublevel loads. A cut-scene.
//! - [`VineState::Intro`] — the far end of that trip. A `bonusstage` level opens with
//!   Mario climbing into view on a vine that grows for him first (`vinestart`), which
//!   is why the bonus rooms all have a one-cell hole in their floor at column 4.
//!
//! The one piece of the original that is *drawing* rather than state is the scissor.
//! A vine sprouts from inside its brick, so `vine:draw` clips everything to above
//! `coy - 1.5` blocks — without it the curled tip is visible sitting on top of a
//! brick it has not come out of yet. See `render.rs`.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::physics::{aabb_overlap, blocks_movement};
use crate::pipe::PipeTarget;
use crate::player::PlayerAnim;
use crate::world::Level;

/// Which side of the vine Mario is hanging on.
///
/// Not cosmetic: it decides which way a left/right press sends him, because the first
/// press only swings him round the stem and the second is what drops him
/// (`mario.lua:1602-1622`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum VineSide {
    Left,
    Right,
}

impl VineSide {
    /// Mario faces the stem he is holding, so the side he is on is the way he looks.
    fn faces_right(self) -> bool {
        self == VineSide::Left
    }
}

/// One vine. Grows from `y` upward to `limit` and never shrinks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Vine {
    /// Left edge of the box you can hold.
    pub(crate) x: f32,
    /// Top of that box. Falls from the block's row to `limit` as the vine grows.
    pub(crate) y: f32,
    /// How far up `y` travels. A vine from a brick runs off the top of the level; the
    /// bonus-stage intro's stops short, because Mario has to end up level with it.
    pub(crate) limit: f32,
    /// Height of the box. Zero at first — the bottom is fixed at [`Vine::foot`], which
    /// starts out *above* `y`.
    pub(crate) height: f32,
    /// Row of the block it came out of. Only the draw needs it, for the scissor.
    pub(crate) co_row: i32,
    /// Sublevel the top of this vine leads to, from the block's entity argument.
    pub(crate) dest: u32,
}

impl Vine {
    /// `x = cox - 0.5 - width/2` and `y = coy - 1` (`vine.lua:22-23`) put the stem on
    /// the cell's centre line with its top flush with the cell's own top.
    fn new(col: i32, row: i32, limit: f32, dest: u32) -> Self {
        let mut v = Vine {
            x: col as f32 * TILE_SIZE + (TILE_SIZE - VINE_W) / 2.0,
            y: row as f32 * TILE_SIZE,
            limit,
            height: 0.0,
            co_row: row,
            dest,
        };
        v.height = (v.foot() - v.y).max(0.0);
        v
    }

    /// A vine sprouting from a brick at this cell.
    pub(crate) fn from_block(col: i32, row: i32, dest: u32) -> Self {
        // `limit = -1`: a block vine grows a block past the top of the level and keeps
        // going nowhere, so there is always stem above wherever you have climbed to.
        Vine::new(col, row, -TILE_SIZE, dest)
    }

    /// The vine a `bonusstage` level grows for its opening animation.
    ///
    /// `vine:new(5, 16, "start")` (`mario.lua:222`) — column 5 and row *16*, one row
    /// below a 15-row level, so this one has no brick under it at all. Its limit is
    /// 9+1/16 rather than -1 because Mario has to stop level with its tip instead of
    /// riding it off the top of the screen.
    pub(crate) fn intro() -> Self {
        Vine::new(
            VINE_INTRO_COL,
            crate::level::LEVEL_HEIGHT as i32,
            VINE_INTRO_LIMIT,
            0,
        )
    }

    /// Bottom of the grabbable box, which never moves.
    ///
    /// `height = coy - y - 1.7` (`vine.lua:38`) is the same statement read the other
    /// way round: the bottom sits 1.7 blocks above the *bottom* of the block, i.e. 0.7
    /// of a block clear of its top face. The stem drawn below that line is scenery.
    fn foot(&self) -> f32 {
        (self.co_row + 1) as f32 * TILE_SIZE - VINE_FOOT
    }

    /// Is it done growing?
    pub(crate) fn grown(&self) -> bool {
        self.y <= self.limit
    }

    pub(crate) fn rect(&self) -> [f32; 4] {
        [self.x, self.y, VINE_W, self.height]
    }

    fn update(&mut self, dt: f32) {
        if self.y <= self.limit {
            return;
        }
        self.y = (self.y - VINE_SPEED * dt).max(self.limit);
        self.height = (self.foot() - self.y).max(0.0);
    }

    /// The scissor the whole vine is drawn through, as a screen-space height.
    ///
    /// `(coy - 1.5) * 16` (`vine.lua:48`). Half a block above the brick's top face,
    /// so the tip is hidden until it has actually cleared the brick.
    pub(crate) fn clip_bottom(&self) -> f32 {
        (self.co_row as f32 - 0.5) * TILE_SIZE
    }

    /// How many stem pieces are drawn below the tip.
    ///
    /// `ceil(height - 14/16 + .7)` (`vine.lua:51`), in blocks.
    pub(crate) fn stem_count(&self) -> i32 {
        let blocks = self.height / TILE_SIZE;
        (blocks - 14.0 / 16.0 + 0.7).ceil().max(0.0) as i32
    }
}

/// What the vine is doing to Mario right now.
#[derive(Debug, Clone, Copy)]
pub(crate) enum VineState {
    /// Hanging on one, under his own control.
    Grip {
        /// Index into `game.vines`.
        vine: usize,
        side: VineSide,
        /// Drives the two-frame climbing animation. Reset whenever he stops moving,
        /// which is what makes a stationary Mario always show frame 2.
        move_timer: f32,
    },
    /// Above the top of the level and still rising; the sublevel loads at the end.
    Leaving { dest: u32, move_timer: f32 },
    /// A `bonusstage` level's opening. Mario waits at the bottom while the vine
    /// grows, climbs it, swings round and is handed the controls.
    Intro {
        timer: f32,
        climbing: bool,
        dropping_off: bool,
        move_timer: f32,
    },
}

/// Which of the two climbing frames a move timer is showing.
///
/// `ceil(fmod(t, delay*2)/delay)`, floored at 1 (`mario.lua:815-816`) — so it is 1 for
/// the first `delay` and 2 for the second, and a Mario who has just stopped shows 2.
fn climb_frame(timer: f32, delay: f32) -> u32 {
    let phase = timer % (delay * 2.0);
    ((phase / delay).ceil() as u32).clamp(1, 2)
}

/// The row of the first cell blocking `rect`, scanned from the leading edge.
///
/// The original's `checkrect(..., {"tile", "portalwall"})` returns whichever hit its
/// object list happened to yield first; scanning from the direction of travel picks
/// the one that actually stopped him, which is what the clamp then needs.
fn blocking_row(level: &Level, rect: [f32; 4], going_up: bool) -> Option<i32> {
    let [x, y, w, h] = rect;
    let left = (x / TILE_SIZE).floor() as i32;
    let right = ((x + w - 0.01) / TILE_SIZE).floor() as i32;
    let top = (y / TILE_SIZE).floor() as i32;
    let bottom = ((y + h - 0.01) / TILE_SIZE).floor() as i32;
    let rows: Vec<i32> = if going_up {
        (top..=bottom).collect()
    } else {
        (top..=bottom).rev().collect()
    };
    for row in rows {
        for col in left..=right {
            if blocks_movement(level, col, row) {
                return Some(row);
            }
        }
    }
    None
}

impl crate::game::Mari0Game {
    /// Grow every vine, and drive whatever the vine is doing to Mario.
    ///
    /// Returns true while a vine owns the player, so the caller skips the normal
    /// movement pass — the same contract as `update_pipe` and `update_springs`.
    pub(crate) fn update_vine(
        &mut self,
        ctx: &Context,
        dt: f32,
        up: bool,
        down: bool,
        left_pressed: bool,
        right_pressed: bool,
    ) -> bool {
        for v in &mut self.vines {
            v.update(dt);
        }

        let Some(state) = self.vine else {
            return false;
        };
        match state {
            VineState::Grip {
                vine,
                mut side,
                mut move_timer,
            } => {
                // A left/right press is a swing round the stem the first time and a
                // drop the second, so it is checked before the climb: letting go this
                // frame means none of the rest applies.
                if right_pressed {
                    if side == VineSide::Left {
                        self.player.x += 8.0 / 16.0 * TILE_SIZE;
                        side = VineSide::Right;
                    } else {
                        self.drop_vine(VineSide::Right);
                        return true;
                    }
                } else if left_pressed {
                    if side == VineSide::Right {
                        self.player.x -= 8.0 / 16.0 * TILE_SIZE;
                        side = VineSide::Left;
                    } else {
                        self.drop_vine(VineSide::Left);
                        return true;
                    }
                }
                self.player.facing_right = side.faces_right();
                self.player.anim_state = PlayerAnim::Climb;

                if up {
                    move_timer += dt;
                    self.player.climb_frame = climb_frame(move_timer, VINE_FRAME_DELAY);
                    self.player.y -= VINE_MOVE_SPEED * dt;
                    if let Some(row) = blocking_row(&self.level, self.player_rect(), true) {
                        self.player.y = (row + 1) as f32 * TILE_SIZE;
                        self.player.climb_frame = 2;
                    }
                } else if down {
                    // The original also runs a portal check on the way down
                    // (`checkportalHOR`), so you can slide off a vine through a portal
                    // in the floor. Not ported: a portal under a vine leaves a hole
                    // rather than a wall, so he slides past it and drops off the bottom
                    // as normal instead of coming out sideways.
                    move_timer += dt;
                    self.player.climb_frame = climb_frame(move_timer, VINE_FRAME_DELAY_DOWN);
                    self.player.y += VINE_MOVE_DOWN_SPEED * dt;
                    if let Some(row) = blocking_row(&self.level, self.player_rect(), false) {
                        self.player.y = row as f32 * TILE_SIZE - self.player.height;
                        self.player.climb_frame = 2;
                    }
                } else {
                    self.player.climb_frame = 2;
                    move_timer = 0.0;
                }

                // High enough to leave the level altogether. Measured off his *head*,
                // and against a fixed row rather than the vine's tip — a block vine
                // always reaches further than this.
                if self.player.y + self.player.height <= VINE_ANIM_START {
                    let dest = self.vines.get(vine).map_or(0, |v| v.dest);
                    self.player.anim_state = PlayerAnim::Climb;
                    self.player.climb_frame = 2;
                    self.vine = Some(VineState::Leaving {
                        dest,
                        move_timer: 0.0,
                    });
                    return true;
                }

                // Slid off the bottom, or the vine he was on is gone.
                let still_on = self
                    .vines
                    .get(vine)
                    .is_some_and(|v| aabb_overlap(self.player_rect(), v.rect()));
                if !still_on {
                    self.drop_vine(side);
                    return true;
                }

                self.vine = Some(VineState::Grip {
                    vine,
                    side,
                    move_timer,
                });
                true
            }

            VineState::Leaving {
                dest,
                mut move_timer,
            } => {
                move_timer += dt;
                self.player.climb_frame = climb_frame(move_timer, VINE_FRAME_DELAY);
                self.player.anim_state = PlayerAnim::Climb;
                self.player.y -= VINE_MOVE_SPEED * dt;
                // Four blocks clear of the top, at which point he is long out of sight
                // and the swap cannot be seen (`mario.lua:637`).
                if self.player.y < -VINE_ANIM_START {
                    self.vine = None;
                    self.travel_to(PipeTarget::Sublevel(dest));
                } else {
                    self.vine = Some(VineState::Leaving { dest, move_timer });
                }
                true
            }

            VineState::Intro {
                mut timer,
                mut climbing,
                mut dropping_off,
                mut move_timer,
            } => {
                self.player.anim_state = PlayerAnim::Climb;
                timer += dt;

                // He sets off once the vine has grown `vineanimationgrowheight` — which
                // at `VINE_SPEED` takes marginally *longer* than the vine needs to reach
                // its limit, so what you actually see is a finished vine being climbed.
                if !dropping_off && timer - dt <= VINE_ANIM_MARIO_START && timer > VINE_ANIM_MARIO_START
                {
                    climbing = true;
                    ctx.audio.play("vine");
                }

                if climbing {
                    move_timer += dt;
                    self.player.climb_frame = climb_frame(move_timer, VINE_FRAME_DELAY);
                    self.player.y -= VINE_MOVE_SPEED * dt;
                    let stop = VINE_INTRO_START_Y - VINE_ANIM_GROW_HEIGHT + VINE_ANIM_STOP;
                    if self.player.y <= stop {
                        climbing = false;
                        dropping_off = true;
                        timer = 0.0;
                        self.player.y = stop;
                        self.player.climb_frame = 2;
                        // Round to the far side of the stem, which is also the way he
                        // will step off.
                        self.player.x += 9.0 / 16.0 * TILE_SIZE;
                        self.player.facing_right = VineSide::Right.faces_right();
                    }
                }

                if dropping_off
                    && timer - dt <= VINE_ANIM_DROP_DELAY
                    && timer > VINE_ANIM_DROP_DELAY
                {
                    self.player.x += 7.0 / 16.0 * TILE_SIZE;
                    self.player.anim_state = PlayerAnim::Fall;
                    self.vine = None;
                    return true;
                }

                self.vine = Some(VineState::Intro {
                    timer,
                    climbing,
                    dropping_off,
                    move_timer,
                });
                true
            }
        }
    }

    /// Is the player on a vine *with the controls*, rather than in one of the two
    /// cut-scenes?
    ///
    /// The clock follows the controls in the original (`game.lua:189-196` stops it for
    /// any player whose `controlsenabled` is false), so a climb burns time and the
    /// bonus-stage intro does not.
    pub(crate) fn vine_has_control(&self) -> bool {
        matches!(self.vine, Some(VineState::Grip { .. }))
    }

    /// Look for a vine the player has walked or fallen into, and grab it.
    ///
    /// Runs after the move, where the original's collision pass would have reported the
    /// overlap. Only the *first* touch grabs: while gripping, the original masks vine
    /// collisions off entirely (`mask[18] = true`) and re-checks with an explicit rect
    /// test instead.
    pub(crate) fn check_vine_grab(&mut self) {
        if self.vine.is_some() {
            return;
        }
        let rect = self.player_rect();
        let Some(i) = self
            .vines
            .iter()
            .position(|v| aabb_overlap(rect, v.rect()))
        else {
            return;
        };
        // Grabbing out of a portal mouth is refused (`mario.lua:2299`) — the portal
        // would be moving him at the same time as the vine pinned him.
        if self.player_inside_portal() {
            return;
        }
        self.grab_vine(i);
    }

    fn grab_vine(&mut self, i: usize) {
        let v = self.vines[i];
        // Which side he arrived on decides where he is parked, and both cases leave him
        // overlapping the stem's centre line by 2/16 (`mario.lua:2311-2321`).
        let centre = v.x + VINE_W / 2.0;
        let side = if v.x > self.player.x {
            VineSide::Left
        } else {
            VineSide::Right
        };
        self.player.x = match side {
            VineSide::Left => centre - self.player.width + 2.0 / 16.0 * TILE_SIZE,
            VineSide::Right => centre - 2.0 / 16.0 * TILE_SIZE,
        };
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.player.is_jumping = false;
        self.player.on_ground = false;
        self.player.facing_right = side.faces_right();
        self.player.anim_state = PlayerAnim::Climb;
        self.player.climb_frame = 2;
        self.vine = Some(VineState::Grip {
            vine: i,
            side,
            move_timer: 0.0,
        });
    }

    /// Let go, stepping 7/16 of a block clear so he doesn't fall straight back on.
    fn drop_vine(&mut self, towards: VineSide) {
        self.player.x += match towards {
            VineSide::Right => 7.0 / 16.0 * TILE_SIZE,
            VineSide::Left => -7.0 / 16.0 * TILE_SIZE,
        };
        self.player.anim_state = PlayerAnim::Fall;
        self.vine = None;
    }

    /// Start a `bonusstage` level's opening climb.
    ///
    /// Called from the level load, which is also where the original decides it
    /// (`game.lua:2139-2141` turns the start into a `vinestart` animation).
    pub(crate) fn start_vine_intro(&mut self) {
        self.vines = vec![Vine::intro()];
        // `x = 4-3/16, y = 15` (`mario.lua:207-215`) — a row below a 15-row level, so
        // he starts under the floor and climbs up through the hole in it.
        self.player.x = (VINE_INTRO_COL as f32) * TILE_SIZE - 3.0 / 16.0 * TILE_SIZE;
        self.player.y = VINE_INTRO_START_Y;
        self.player.vx = 0.0;
        self.player.vy = 0.0;
        self.player.on_ground = false;
        self.player.facing_right = VineSide::Left.faces_right();
        self.player.anim_state = PlayerAnim::Climb;
        self.player.climb_frame = 2;
        self.vine = Some(VineState::Intro {
            timer: 0.0,
            climbing: false,
            dropping_off: false,
            move_timer: 0.0,
        });
    }

    /// Fall out of a bonus stage and land back in the level that sent you.
    ///
    /// A pit is not fatal in a `bonusstage`; it is the way out (`mario.lua:2603-2607`).
    /// Routed through the pipe machinery so the arrival lands on the `pipespawn` that
    /// pairs with the sublevel being left — for 2-1 that is the pipe at column 162,
    /// which is where the original puts you too.
    pub(crate) fn leave_bonus_stage(&mut self) {
        self.vine = None;
        self.travel_to(PipeTarget::Sublevel(0));
    }

    pub(crate) fn player_rect(&self) -> [f32; 4] {
        [
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
        ]
    }

    /// Is the player standing in a portal mouth? Needs *both* portals, since a lone
    /// one is not a hole (`game.lua:3365`).
    fn player_inside_portal(&self) -> bool {
        let Some((a, b)) = self.portal_pair() else {
            return false;
        };
        let rect = self.player_rect();
        [a, b].iter().any(|p| {
            p.anchor.cells().iter().any(|&(col, row)| {
                let (x, y, w, h) = crate::physics::tile_rect(col, row);
                aabb_overlap(rect, [x, y, w, h])
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The timings and speeds are the original's. If these drift, so does the whole
    /// animation, and it is 4.6 seconds long — long enough to notice.
    #[test]
    fn the_numbers_are_the_originals() {
        assert_eq!(VINE_SPEED / TILE_SIZE, 2.13, "vinespeed");
        assert_eq!(VINE_MOVE_SPEED / TILE_SIZE, 3.21, "vinemovespeed");
        assert_eq!(VINE_MOVE_DOWN_SPEED, 2.0 * VINE_MOVE_SPEED, "twice as fast");
        assert_eq!(VINE_FRAME_DELAY, 0.15);
        assert_eq!(VINE_FRAME_DELAY_DOWN, 0.075);
        assert_eq!(VINE_ANIM_STOP / TILE_SIZE, 1.75);
        assert_eq!(VINE_ANIM_DROP_DELAY, 0.5);
    }

    /// A brick vine's box starts empty and its bottom is above the brick's top face,
    /// so the stem right by the brick is decoration you cannot hold.
    #[test]
    fn a_fresh_vine_has_nothing_to_hold() {
        let v = Vine::from_block(83, 5, 1);
        assert_eq!(v.height, 0.0, "no box until it has grown past its own foot");
        assert!(
            v.foot() < 5.0 * TILE_SIZE,
            "the foot is above the brick's top face"
        );
        // Centred on the brick, narrower than it.
        assert!(v.x > 83.0 * TILE_SIZE);
        assert!(v.x + VINE_W < 84.0 * TILE_SIZE);
    }

    /// It grows up at `vinespeed` and stops dead on its limit.
    #[test]
    fn it_grows_to_its_limit_and_no_further() {
        let mut v = Vine::from_block(83, 5, 1);
        let start = v.y;
        v.update(0.5);
        assert!(
            (start - v.y - VINE_SPEED * 0.5).abs() < 0.01,
            "grew at vinespeed"
        );
        assert!(v.height > 0.0, "and now has something to hold");
        for _ in 0..600 {
            v.update(1.0 / 60.0);
        }
        assert!(v.grown());
        assert_eq!(v.y, -TILE_SIZE, "a block past the top of the level");
        // The box reaches from its fixed foot all the way up.
        assert!((v.height - (v.foot() + TILE_SIZE)).abs() < 0.01);
    }

    /// The intro vine stops short, because Mario has to end up level with its tip
    /// rather than climbing off the top of the screen.
    #[test]
    fn the_intro_vine_stops_short() {
        let mut v = Vine::intro();
        for _ in 0..600 {
            v.update(1.0 / 60.0);
        }
        assert!(v.grown());
        assert!(
            v.y > 0.0,
            "it stays on screen, unlike a block vine: {}",
            v.y
        );
        // Where Mario is left standing must be inside the grown vine's box.
        let mario_stop = VINE_INTRO_START_Y - VINE_ANIM_GROW_HEIGHT + VINE_ANIM_STOP;
        assert!(
            mario_stop > v.y && mario_stop < v.y + v.height,
            "Mario ends up on the vine, not above it: stop {mario_stop}, vine {}..{}",
            v.y,
            v.y + v.height
        );
    }

    /// The vine has grown its 6 blocks *before* Mario sets off, which is why the intro
    /// reads as "climb a vine" and not "chase a vine".
    #[test]
    fn the_vine_finishes_growing_before_he_starts() {
        let grow_time = (VINE_INTRO_START_Y - Vine::intro().limit) / VINE_SPEED;
        assert!(
            grow_time < VINE_ANIM_MARIO_START,
            "vine takes {grow_time}s, he waits {VINE_ANIM_MARIO_START}s"
        );
    }

    /// Two frames, alternating on the delay, and a stopped Mario shows the second.
    #[test]
    fn the_climb_alternates_between_two_frames() {
        assert_eq!(climb_frame(0.0, VINE_FRAME_DELAY), 1);
        assert_eq!(climb_frame(VINE_FRAME_DELAY * 0.5, VINE_FRAME_DELAY), 1);
        assert_eq!(climb_frame(VINE_FRAME_DELAY * 1.5, VINE_FRAME_DELAY), 2);
        // Wraps rather than running off the end of the sheet.
        assert_eq!(climb_frame(VINE_FRAME_DELAY * 2.5, VINE_FRAME_DELAY), 1);
        assert_eq!(climb_frame(VINE_FRAME_DELAY * 3.5, VINE_FRAME_DELAY), 2);
    }

    /// Whichever side he grabs from, he faces the stem.
    #[test]
    fn he_faces_the_vine_he_is_holding() {
        assert!(VineSide::Left.faces_right());
        assert!(!VineSide::Right.faces_right());
    }

    /// A grown vine is drawn as a tip plus enough stem to cover its box.
    #[test]
    fn a_grown_vine_draws_a_stem_for_every_block_of_it() {
        let mut v = Vine::from_block(83, 5, 1);
        assert_eq!(v.stem_count(), 0, "nothing to stack under the tip yet");
        for _ in 0..600 {
            v.update(1.0 / 60.0);
        }
        let blocks = v.height / TILE_SIZE;
        let stems = v.stem_count() as f32;
        assert!(
            stems >= blocks - 1.0 && stems <= blocks + 1.0,
            "one stem per block of vine: {blocks} blocks, {stems} stems"
        );
    }
}
