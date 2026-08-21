//! Springs — the one launcher you can charge.
//!
//! A spring is a solid you land on, and landing on it takes control away: for
//! [`SPRING_TIME`] seconds Mario is pinned to it, riding the compression down, and
//! whether he holds jump during those two tenths of a second decides whether he leaves
//! at [`SPRING_FORCE`] or [`SPRING_HIGH_FORCE`] — nearly double, and by far the highest
//! Mario ever gets under his own power.
//!
//! Structurally it is a smaller cousin of the pipe transit: a state on the game that
//! suspends normal control while it runs.

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::aabb_overlap;
use crate::world::SolidRect;

/// One spring, placed by the level and never moving.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spring {
    /// Top-left of the body when fully extended.
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// Seconds into the compression. At rest this sits at [`SPRING_TIME`], i.e. done.
    pub(crate) timer: f32,
}

impl Spring {
    pub(crate) fn new(cell_x: i32, cell_y: i32) -> Self {
        Spring {
            x: cell_x as f32 * TILE_SIZE,
            // `y = cell - 31/16` (`spring.lua:14`): it stands *up* out of its cell, so
            // the cell names its base and the body reaches nearly two blocks above.
            y: (cell_y + 1) as f32 * TILE_SIZE - SPRING_H,
            timer: SPRING_TIME,
        }
    }

    /// Which of the three compression frames is showing.
    ///
    /// The sequence runs 2, 3 and then back down (`frame = 6 - frame` past 3,
    /// `spring.lua:33-36`), so it squashes and rebounds rather than snapping open.
    pub(crate) fn frame(&self) -> usize {
        if self.timer >= SPRING_TIME {
            return 0;
        }
        let step = (self.timer / (SPRING_TIME / 3.0)).ceil() as usize + 1;
        let step = if step > 3 { 6 - step } else { step };
        step.saturating_sub(1).min(2)
    }

    /// How far the surface has sunk, in pixels.
    pub(crate) fn sink(&self) -> f32 {
        SPRING_Y_TABLE[self.frame()] * TILE_SIZE
    }

    /// The collision box in its current state — the top sinks as it compresses.
    pub(crate) fn rect(&self) -> [f32; 4] {
        [
            self.x,
            self.y + self.sink(),
            SPRING_W,
            SPRING_H - self.sink(),
        ]
    }
}

/// Mario riding a spring. While this is set he has no control but the jump button.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpringRide {
    /// Which spring, as an index into `game.springs`.
    pub(crate) spring: usize,
    pub(crate) timer: f32,
    /// The x he arrived with, held for the whole ride (`mario.lua:861`).
    pub(crate) x: f32,
    /// Has jump been pressed at any point during the ride? That is the charge.
    pub(crate) charged: bool,
}

impl Mari0Game {
    /// Advance the compression, and the ride if one is in progress.
    ///
    /// Returns true while a ride owns the player, so the caller can skip the normal
    /// movement pass the way it does for a pipe.
    pub(crate) fn update_springs(&mut self, dt: f32, jump_held: bool) -> bool {
        for s in &mut self.springs {
            if s.timer < SPRING_TIME {
                s.timer = (s.timer + dt).min(SPRING_TIME);
            }
        }
        self.level.spring_rects = self
            .springs
            .iter()
            .map(|s| SolidRect {
                rect: s.rect(),
                cubes_pass: false,
            })
            .collect();

        let Some(mut ride) = self.spring_ride else {
            // Not riding: did he just land on one?
            return self.check_spring_landing();
        };

        // The jump button is sampled across the *whole* ride, not on the frame he
        // leaves — two tenths of a second is short enough that requiring the press on
        // one exact frame would be unfair.
        ride.charged |= jump_held;
        ride.timer += dt;
        // He is parked on the spring's surface, and his height comes straight from the
        // animation table — so he visibly rides it down rather than hovering.
        let s = self.springs[ride.spring];
        self.player.x = ride.x;
        self.player.y = s.y - self.player.height + s.sink();
        self.player.vy = 0.0;
        self.player.on_ground = true;

        if ride.timer > SPRING_TIME {
            self.player.y = s.y - self.player.height;
            self.player.vy = if ride.charged {
                -SPRING_HIGH_FORCE
            } else {
                -SPRING_FORCE
            };
            self.player.on_ground = false;
            self.spring_ride = None;
            return false;
        }
        self.spring_ride = Some(ride);
        true
    }

    /// Start a ride if the player has come down onto a spring's top.
    fn check_spring_landing(&mut self) -> bool {
        if self.player.vy < 0.0 {
            return false;
        }
        let feet = [
            self.player.x,
            self.player.y + self.player.height - 4.0,
            self.player.width,
            8.0,
        ];
        for (i, s) in self.springs.iter().enumerate() {
            let top = [s.x, s.y + s.sink(), SPRING_W, 8.0];
            if aabb_overlap(feet, top) {
                self.springs[i].timer = 0.0;
                self.spring_ride = Some(SpringRide {
                    spring: i,
                    timer: 0.0,
                    x: self.player.x,
                    charged: false,
                });
                self.player.vy = 0.0;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spring stands out of its cell, not inside it.
    #[test]
    fn a_spring_reaches_two_blocks_above_its_cell() {
        let s = Spring::new(10, 12);
        assert_eq!(s.x, 10.0 * TILE_SIZE);
        // Base on the cell's bottom edge, body reaching up.
        let [_, y, w, h] = s.rect();
        assert_eq!(w, SPRING_W);
        assert!(
            (y + h - 13.0 * TILE_SIZE).abs() < 0.01,
            "base sits on the cell"
        );
        assert!(h > TILE_SIZE, "it is taller than one block: {h}");
    }

    /// The compression squashes and rebounds rather than snapping open.
    #[test]
    fn the_frames_go_down_and_back_up() {
        let mut s = Spring::new(0, 0);
        s.timer = SPRING_TIME;
        assert_eq!(s.frame(), 0, "at rest it is fully extended");
        assert_eq!(s.sink(), 0.0);
        // Sample across the compression: the sink must rise and then fall again.
        let mut sinks = Vec::new();
        for i in 0..=10 {
            s.timer = SPRING_TIME * i as f32 / 10.0;
            sinks.push(s.sink());
        }
        let peak = sinks.iter().cloned().fold(0.0f32, f32::max);
        assert!(peak > 0.0, "it compresses at all: {sinks:?}");
        assert_eq!(
            *sinks.last().unwrap(),
            0.0,
            "and comes back to full extension"
        );
        let peak_at = sinks.iter().position(|v| *v == peak).unwrap();
        assert!(
            peak_at > 0 && peak_at < sinks.len() - 1,
            "the squash is in the middle, not at an end: {sinks:?}"
        );
    }

    /// Charging is worth nearly double, which is the whole point of the spring.
    #[test]
    fn holding_jump_is_worth_nearly_double() {
        const { assert!(SPRING_HIGH_FORCE > 1.5 * SPRING_FORCE) };
        // And both are far beyond anything Mario can do himself, even at the top speed
        // that earns him the largest jump he has.
        const { assert!(SPRING_FORCE > JUMP_FORCE + JUMP_FORCE_ADD) };
    }
}
