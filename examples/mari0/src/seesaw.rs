//! Seesaws — two platforms on one rope, and the only thing in the game your weight
//! moves.
//!
//! Nine of them exist, spread over 3-3, 4-3 and 6-3, and between them they use each of
//! `seesawtype`'s nine entries exactly once. A rig is a beam with a pulley at each end
//! and a platform hanging under each pulley; the two platforms are joined, so whatever
//! one does the other does in reverse.
//!
//! What makes it feel like rope rather than a lever is that the speed **accumulates**.
//! Standing on one side adds [`SEESAW_SPEED`] to that side's velocity every second and
//! subtracts it from the other's, and nothing caps it — the platform you are on gets
//! faster the longer you stand there, until the rope runs out. [`SEESAW_FRICTION`]
//! bleeds the speed off again, but only while the weight does not support the direction
//! of travel, and it is set to exactly `SEESAW_SPEED` so stepping off cancels your pull
//! rather than braking harder than you pulled.
//!
//! Run out of rope with weight on the far side and the rig gives: both platforms stop
//! dead and then drop at [`SEESAW_GRAVITY`], which is seven times the pull. That is the
//! trap — the platform you rode down takes you with it.
//!
//! Structurally this is a cousin of `platform.rs`: each platform contributes a
//! [`SolidRect`] so the existing non-tile collision lets you stand on it, and the carry
//! rules live here because that machinery cannot move a rider.

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::in_range;
use crate::world::SolidRect;

/// The nine rigs, as `(range, dist1, dist2, size)` in blocks (`seesaw.lua:4-13`).
///
/// `range` is how far apart the pulleys are, `dist1`/`dist2` how far each platform
/// starts below the beam, and `size` the platform width. The three 1.5-wide ones are
/// 6-3's, which is what makes that level the hard one.
const SEESAW_TYPES: [(f32, f32, f32, f32); 9] = [
    (7.0, 4.0, 6.0, 3.0),
    (4.0, 2.0, 6.0, 3.0),
    (7.0, 3.0, 6.0, 3.0),
    (8.0, 3.0, 7.0, 3.0),
    (5.0, 3.0, 7.0, 3.0),
    (6.0, 3.0, 7.0, 3.0),
    (4.0, 3.0, 7.0, 1.5),
    (3.0, 3.0, 7.0, 1.5),
    (3.0, 4.0, 7.0, 1.5),
];

/// Which end of the rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum SeesawSide {
    Left,
    Right,
}

/// One hanging platform. Position is the top-left of its collision box, in world pixels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeesawPlatform {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) vy: f32,
    /// How many riders were counted **last** frame. That staleness is the original's:
    /// the rig reads the counts its platforms reported on the previous pass
    /// (`game.lua:358` runs before `:442`), so your weight takes effect a frame late.
    pub(crate) riders: u32,
    /// Off the bottom of the world. Stops being solid and stops being drawn, but the
    /// rig keeps it — the original leaves the seesaw pointing at the deleted platform.
    pub(crate) gone: bool,
}

impl SeesawPlatform {
    pub(crate) fn rect(&self) -> [f32; 4] {
        [self.x, self.y, self.w, PLATFORM_HEIGHT]
    }
}

/// One rig: the beam, and the two platforms slung under it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seesaw {
    /// The cell the level placed it on. The beam is drawn from here.
    pub(crate) col: i32,
    pub(crate) row: i32,
    /// Which of the nine types, as the level stated it.
    pub(crate) kind: u16,
    /// Pulley separation, in blocks.
    pub(crate) range: f32,
    /// How far below the beam each platform started, in blocks. Their sum, less
    /// [`SEESAW_ROPE_SLACK`], is the rope length the pair share for good.
    pub(crate) dist1: f32,
    pub(crate) dist2: f32,
    pub(crate) left: SeesawPlatform,
    pub(crate) right: SeesawPlatform,
    /// Which platform's rope gave. Once set the rig only falls, and it is never cleared.
    pub(crate) falloff: Option<SeesawSide>,
}

impl Seesaw {
    /// Build a rig from its cell and type argument.
    ///
    /// The type is 1-based and out-of-range values fall back to the first rig, which is
    /// what `t == nil` does in the original.
    pub(crate) fn new(col: i32, row: i32, kind: u16) -> Self {
        let (range, dist1, dist2, size) = SEESAW_TYPES
            .get((kind.max(1) - 1) as usize)
            .copied()
            .unwrap_or(SEESAW_TYPES[0]);
        let mut s = Seesaw {
            col,
            row,
            kind,
            range,
            dist1,
            dist2,
            left: SeesawPlatform {
                x: 0.0,
                y: 0.0,
                w: size * TILE_SIZE,
                vy: 0.0,
                riders: 0,
                gone: false,
            },
            right: SeesawPlatform {
                x: 0.0,
                y: 0.0,
                w: size * TILE_SIZE,
                vy: 0.0,
                riders: 0,
                gone: false,
            },
            falloff: None,
        };
        // `x - size/2 - 0.5` (`seesawplatform.lua:9`) centres the platform on its
        // pulley. Unlike a moving platform there is no whole-number special case, so
        // even the 3-wide ones are centred.
        let centre = |pulley_x: f32| pulley_x - size / 2.0 * TILE_SIZE - TILE_SIZE / 2.0;
        s.left.x = centre(s.anchor_x());
        s.right.x = centre(s.anchor_x() + range * TILE_SIZE);
        s.left.y = s.anchor_y() + dist1 * TILE_SIZE + SEESAW_PLATFORM_DROP;
        s.right.y = s.anchor_y() + dist2 * TILE_SIZE + SEESAW_PLATFORM_DROP;
        s
    }

    /// The left pulley, which is what the platform positions and the beam hang off.
    pub(crate) fn anchor_x(&self) -> f32 {
        (self.col + 1) as f32 * TILE_SIZE
    }

    /// The height the ropes are measured from. Both platforms stop here.
    pub(crate) fn anchor_y(&self) -> f32 {
        (self.row + 1) as f32 * TILE_SIZE
    }

    /// The rope the pair share: with one platform hauled up to the beam, this is how far
    /// the other one hangs.
    pub(crate) fn rope(&self) -> f32 {
        (self.dist1 + self.dist2) * TILE_SIZE - SEESAW_ROPE_SLACK
    }

    pub(crate) fn platform(&self, side: SeesawSide) -> &SeesawPlatform {
        match side {
            SeesawSide::Left => &self.left,
            SeesawSide::Right => &self.right,
        }
    }

    fn platform_mut(&mut self, side: SeesawSide) -> &mut SeesawPlatform {
        match side {
            SeesawSide::Left => &mut self.left,
            SeesawSide::Right => &mut self.right,
        }
    }

    /// How far a platform has dropped below the beam. This is also the length of rope
    /// drawn on that side.
    pub(crate) fn drop_of(&self, side: SeesawSide) -> f32 {
        self.platform(side).y - self.anchor_y()
    }

    /// One step of the rig itself: the pull, the friction, and the ends of the rope.
    ///
    /// Deliberately separate from the platforms' own step, and runs *first*, because
    /// that is the order the original's two update loops happen to be in — which is
    /// also why the rider counts it reads are a frame old.
    fn step(&mut self, dt: f32) {
        if self.falloff.is_some() {
            self.left.vy += SEESAW_GRAVITY * dt;
            self.right.vy += SEESAW_GRAVITY * dt;
            return;
        }

        // Positive means the left is heavier, and the left is what goes *down*.
        let imbalance = self.left.riders as f32 - self.right.riders as f32;
        self.left.vy += imbalance * SEESAW_SPEED * dt;
        self.right.vy -= imbalance * SEESAW_SPEED * dt;

        // Friction only opposes motion the current weight does not justify, so a loaded
        // seesaw keeps accelerating and an unloaded one coasts to a stop. The clamp
        // stops friction from *reversing* the travel when the sides are level.
        if self.left.vy > 0.0 && imbalance <= 0.0 {
            self.left.vy -= SEESAW_FRICTION * dt;
            if imbalance == 0.0 && self.left.vy < 0.0 {
                self.left.vy = 0.0;
            }
        } else if self.left.vy < 0.0 && imbalance >= 0.0 {
            self.left.vy += SEESAW_FRICTION * dt;
        }
        // The right's conditions are the left's mirrored, because it travels the other
        // way: `speedy > 0 and speed >= 0` against the left's `speed <= 0`.
        if self.right.vy > 0.0 && imbalance >= 0.0 {
            self.right.vy -= SEESAW_FRICTION * dt;
            // **Corrected.** The original zeroes the *left* platform here
            // (`seesaw.lua:63`) in the middle of the right platform's block — a
            // copy-paste slip. Left as-is it leaves the right platform with a residual
            // speed that never settles, so the pair slowly drift out of step and the
            // rope's total length stops holding.
            if imbalance == 0.0 && self.right.vy < 0.0 {
                self.right.vy = 0.0;
            }
        } else if self.right.vy < 0.0 && imbalance <= 0.0 {
            self.right.vy += SEESAW_FRICTION * dt;
        }

        // Out of rope. If the far side is loaded the rig gives; otherwise both platforms
        // are pinned to the ends of the rope.
        //
        // "Pinned" is softer than it sounds, and deliberately so: this runs *before* the
        // platforms move, and it does not zero their speed. So a platform still carrying
        // momentum is dragged back past the limit, corrected again next frame, and ends
        // up parked within one frame's travel of the beam rather than exactly on it —
        // settling onto it only once friction has killed the speed. Zeroing the speed
        // here instead would make a coasting seesaw stop dead, which is not how the
        // original feels.
        if self.drop_of(SeesawSide::Left) <= 0.0 {
            if self.right.riders > 0 {
                self.begin_falloff(SeesawSide::Right);
            } else {
                self.left.y = self.anchor_y();
                self.right.y = self.anchor_y() + self.rope();
            }
        }
        if self.drop_of(SeesawSide::Right) <= 0.0 {
            if self.left.riders > 0 {
                self.begin_falloff(SeesawSide::Left);
            } else {
                self.right.y = self.anchor_y();
                self.left.y = self.anchor_y() + self.rope();
            }
        }
    }

    fn begin_falloff(&mut self, side: SeesawSide) {
        self.falloff = Some(side);
        self.left.vy = 0.0;
        self.right.vy = 0.0;
    }
}

impl Mari0Game {
    /// Advance every rig, then every platform, in the original's order.
    ///
    /// Called after `update_platforms`, and *extends* the platform rect list rather than
    /// replacing it — that function rebuilds the list wholesale each frame.
    pub(crate) fn update_seesaws(&mut self, dt: f32) {
        for s in &mut self.seesaws {
            s.step(dt);
        }

        let bottom = (self.level.height + 1) as f32 * TILE_SIZE;
        for i in 0..self.seesaws.len() {
            for side in [SeesawSide::Left, SeesawSide::Right] {
                if self.seesaws[i].platform(side).gone {
                    continue;
                }
                let p = *self.seesaws[i].platform(side);
                let riders = self.carry_seesaw_rider(&p, dt);
                let platform = self.seesaws[i].platform_mut(side);
                platform.riders = riders;
                platform.y += p.vy * dt;
                if platform.y > bottom {
                    platform.gone = true;
                }
            }
        }

        self.level
            .platform_rects
            .extend(self.seesaws.iter().flat_map(|s| {
                [&s.left, &s.right].map(|p| SolidRect {
                    rect: p.rect(),
                    cubes_pass: false,
                })
            }));
    }

    /// Count and carry whoever is standing on this platform. Returns the rider count.
    ///
    /// The original sweeps every enemy as well as the player, and an enemy's weight
    /// counts exactly as much as yours (`seesawplatform.lua:36-52`). Only the player is
    /// swept here: **no shipped level can put an enemy on a seesaw** — all nine hang
    /// over pits with nothing placed on or above them — so the enemy half of that loop
    /// is unreachable in the game as shipped.
    ///
    /// Two details are load-bearing. The rider is *snapped* to the surface plus one step
    /// of the platform's travel rather than nudged, so a platform accelerating downward
    /// never outruns him; and someone leaving is not a rider, or the seesaw would pin
    /// him to itself mid-leap.
    ///
    /// "Leaving" is tested as `jumping` **or** rising. The original only checks the
    /// `jumping` flag (`seesawplatform.lua:46`), which the port clears the moment the
    /// jump button comes up — so a tapped jump would be caught and snapped straight back
    /// down. `platform.rs` reads the same situation off `vy` for the same reason.
    fn carry_seesaw_rider(&mut self, p: &SeesawPlatform, dt: f32) -> u32 {
        let (px, py) = (self.player.x, self.player.y);
        let (pw, ph) = (self.player.width, self.player.height);
        if self.player.is_jumping || self.player.vy < 0.0 {
            return 0;
        }
        // `inrange(w.x, self.x - w.width, self.x + self.width)`: the rider's left edge
        // inside the platform's span widened by his own width — an overlap test written
        // in terms of one corner.
        if !in_range(px, p.x - pw, p.x + p.w) {
            return 0;
        }
        let surface = p.y - ph;
        if !in_range(
            py,
            surface - SEESAW_RIDE_TOLERANCE,
            surface + SEESAW_RIDE_TOLERANCE,
        ) {
            return 0;
        }
        self.player.y = surface + p.vy * dt;
        // Not in the original, which leaves ground contact to the collision pass. Set
        // here for the same reason `platform.rs` sets it: the platform rects are a frame
        // stale, so without it a rider on a descending platform reads as airborne and
        // cannot jump off.
        self.player.on_ground = true;
        self.player.vy = 0.0;
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nine rigs, verbatim. These are the whole feature's shape: get one wrong and
    /// one level's platforms hang at the wrong height for good.
    #[test]
    fn the_nine_types_are_the_originals() {
        assert_eq!(SEESAW_TYPES[0], (7.0, 4.0, 6.0, 3.0));
        assert_eq!(SEESAW_TYPES[8], (3.0, 4.0, 7.0, 1.5));
        // Types 7-9 are the narrow ones, and they are 6-3's.
        for (i, rig) in SEESAW_TYPES.iter().enumerate() {
            let expected = if i >= 6 { 1.5 } else { 3.0 };
            assert_eq!(rig.3, expected, "type {} width", i + 1);
        }
    }

    /// An out-of-range type falls back to the first rig rather than panicking.
    #[test]
    fn an_unknown_type_falls_back_to_the_first() {
        let fallback = Seesaw::new(10, 2, 99);
        let first = Seesaw::new(10, 2, 1);
        assert_eq!(fallback.range, first.range);
        assert_eq!(fallback.dist1, first.dist1);
        // 0 is what an absent argument becomes; it must not underflow the index.
        assert_eq!(Seesaw::new(10, 2, 0).range, first.range);
    }

    /// The platforms hang under their pulleys, at the two stated depths.
    #[test]
    fn the_platforms_hang_under_their_pulleys() {
        let s = Seesaw::new(82, 2, 1); // 3-3's first: range 7, 4 and 6 down, 3 wide
        assert_eq!(s.left.w, 3.0 * TILE_SIZE);
        // Centred on the pulleys, which are `range` apart.
        let left_centre = s.left.x + s.left.w / 2.0;
        let right_centre = s.right.x + s.right.w / 2.0;
        assert!((right_centre - left_centre - 7.0 * TILE_SIZE).abs() < 0.01);
        // And the right one starts lower, because dist2 > dist1.
        assert!(s.right.y > s.left.y);
        assert!(
            (s.drop_of(SeesawSide::Left) - (4.0 * TILE_SIZE + SEESAW_PLATFORM_DROP)).abs() < 0.01
        );
    }

    /// The pair's total drop is fixed — that is what makes it a rope and not two lifts.
    /// It has to hold at the start *and* at the end of the rope.
    #[test]
    fn the_two_drops_always_sum_to_the_rope() {
        for kind in 1..=9u16 {
            let s = Seesaw::new(50, 2, kind);
            let start = s.drop_of(SeesawSide::Left) + s.drop_of(SeesawSide::Right);
            let mut hauled = s;
            hauled.left.y = hauled.anchor_y();
            hauled.right.y = hauled.anchor_y() + hauled.rope();
            let end = hauled.drop_of(SeesawSide::Left) + hauled.drop_of(SeesawSide::Right);
            assert!(
                (start - end).abs() < 0.01,
                "type {kind}: {start} at rest vs {end} hauled up"
            );
        }
    }

    /// Weight on the left drives the left down and the right up, in step.
    #[test]
    fn a_rider_on_one_side_drives_it_down_and_the_other_up() {
        let mut s = Seesaw::new(50, 2, 3);
        s.left.riders = 1;
        s.step(1.0 / 60.0);
        assert!(s.left.vy > 0.0, "the loaded side goes down");
        assert!((s.left.vy + s.right.vy).abs() < 0.001, "equal and opposite");
        assert!((s.left.vy - SEESAW_SPEED / 60.0).abs() < 0.001);
        // And it keeps accelerating — there is no terminal speed, only the rope.
        let first = s.left.vy;
        s.step(1.0 / 60.0);
        assert!(
            s.left.vy > first * 1.9,
            "still building: {first} → {}",
            s.left.vy
        );
    }

    /// Step off and friction cancels exactly what you were adding, so it coasts to a
    /// halt instead of springing back.
    #[test]
    fn stepping_off_lets_it_coast_to_a_stop() {
        let mut s = Seesaw::new(50, 2, 3);
        s.left.riders = 1;
        for _ in 0..30 {
            s.step(1.0 / 60.0);
        }
        let moving = s.left.vy;
        assert!(moving > 0.0);
        s.left.riders = 0;
        for _ in 0..600 {
            s.step(1.0 / 60.0);
        }
        assert_eq!(s.left.vy, 0.0, "settles exactly, not near-zero");
        assert_eq!(
            s.right.vy, 0.0,
            "and so does the far side — the original's slip left this one drifting"
        );
    }

    /// Coast into the end of the rope with nobody aboard and it simply stops — and the
    /// pinned positions still satisfy the rope invariant.
    ///
    /// This is the "jump off in time" case. The rider's weight is what decides between
    /// stopping and collapsing, and by the time the rope runs out he is gone.
    #[test]
    fn coasting_into_the_end_of_the_rope_just_stops() {
        let mut s = Seesaw::new(50, 2, 2);
        // Left almost hauled up, still travelling, nobody on either platform.
        s.left.y = s.anchor_y() + 4.0;
        s.right.y = s.anchor_y() + s.rope() - 4.0;
        s.left.vy = -2.0 * TILE_SIZE;
        s.right.vy = 2.0 * TILE_SIZE;
        for _ in 0..30 {
            s.step(1.0 / 60.0);
            s.left.y += s.left.vy / 60.0;
            s.right.y += s.right.vy / 60.0;
        }
        assert!(s.falloff.is_none(), "nobody aboard, so nothing gives");
        assert!(
            (s.drop_of(SeesawSide::Left)).abs() < 0.01,
            "the left is pinned to the beam: {}",
            s.drop_of(SeesawSide::Left)
        );
        assert!((s.drop_of(SeesawSide::Right) - s.rope()).abs() < 0.01);
    }

    /// Ride one side all the way down and the rig gives — then falls far faster than it
    /// ever pulled. This is the trap: the platform you are standing on is the one that
    /// drops away.
    #[test]
    fn riding_to_the_end_of_the_rope_collapses_it() {
        let mut s = Seesaw::new(50, 2, 2);
        s.right.riders = 1; // ride the right down, which hauls the left up
        for _ in 0..600 {
            s.step(1.0 / 60.0);
            s.left.y += s.left.vy / 60.0;
            s.right.y += s.right.vy / 60.0;
            if s.falloff.is_some() {
                break;
            }
        }
        assert_eq!(
            s.falloff,
            Some(SeesawSide::Right),
            "the left topped out with weight still on the right, so the right is what goes"
        );
        // Both stop dead, then fall together.
        let before = s.left.vy;
        s.step(1.0 / 60.0);
        assert!(s.left.vy > before);
        assert!(
            (s.left.vy - s.right.vy).abs() < 0.001,
            "both fall, not just one"
        );
        // The collapse is far faster than anything the rig does under weight.
        const { assert!(SEESAW_GRAVITY > 7.0 * SEESAW_SPEED) };
    }
}
