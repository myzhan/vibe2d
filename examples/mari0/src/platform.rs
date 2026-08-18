//! Moving platforms — one class in the original, six behaviours.
//!
//! `platform.lua` is a single type whose `dir` field selects between an oscillating
//! lift, a sliding one, a one-way elevator, a platform that falls under your weight,
//! and the bonus-stage conveyor. They share a body (half a block tall, `size` blocks
//! wide, no gravity, `static = true`) and almost nothing else, so the shared parts
//! live in [`Platform`] and the differences in the [`PlatformKind`] match arms.
//!
//! **They are solids, not enemies.** Each one contributes a [`SolidRect`] every frame,
//! which is what makes the existing non-tile collision let you stand on it. What that
//! machinery cannot do is *carry* you, so the carry rules are here — and they are the
//! fiddly part, because the original's tests for "is this thing riding me" differ
//! between the horizontal and vertical cases in ways that are load-bearing.

use crate::constants::*;
use crate::game::Mari0Game;
use crate::world::SolidRect;

/// Which of the six behaviours a platform has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum PlatformKind {
    /// `dir = "right"`, entity 19. Slides back and forth on a cosine.
    ///
    /// Despite the name it travels *left* of where it started: the position is
    /// `startx - f(t) * distance` (`platform.lua:51`). "Right" names the axis, not
    /// the direction.
    Horizontal,
    /// `dir = "up"`, entity 18. Rises and falls on a cosine over 8.625 blocks.
    Vertical,
    /// `dir = "justup"`. A one-way lift climbing at a constant 3.5 blocks/s, made by
    /// a spawner and destroyed when it leaves the top of the world.
    JustUp,
    /// `dir = "justdown"`. The same going down.
    JustDown,
    /// `dir = "fall"`, entity 32. Still until something stands on it.
    Fall,
    /// `dir = "justright"`, entity 92. The bonus-stage platform: still until Mario
    /// hits it **from below**, then it slides right for good.
    Bonus,
}

impl PlatformKind {
    /// Does this one need a rider check at all?
    fn carries(self) -> bool {
        // Every kind that moves does, which is all of them once `Fall` and `Bonus`
        // have been started.
        true
    }
}

/// One platform. Position is the top-left of its collision box, in world pixels.
#[derive(Debug, Clone)]
pub(crate) struct Platform {
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// Width in pixels. The level's argument is in blocks and can be fractional —
    /// 1.5 is a real width in the shipped data.
    pub(crate) w: f32,
    pub(crate) kind: PlatformKind,
    /// Where it was created, which both oscillating kinds measure their travel from.
    pub(crate) start_x: f32,
    pub(crate) start_y: f32,
    /// Phase for the oscillating kinds; unused by the rest.
    pub(crate) timer: f32,
    /// This frame's velocity. Derived rather than integrated for the oscillating
    /// kinds — see [`Mari0Game::update_platforms`].
    pub(crate) vx: f32,
    pub(crate) vy: f32,
}

impl Platform {
    /// Build one from its spawn cell.
    ///
    /// The two offsets are the original's (`platform.lua:6-11`) and the x one is
    /// conditional on the width being a whole number of blocks, which is the sort of
    /// thing that only shows up on the one platform in the game that is 1.5 wide.
    pub(crate) fn new(cell_x: i32, cell_y: i32, kind: PlatformKind, size_blocks: f32) -> Self {
        let x = if size_blocks.fract() == 0.0 {
            cell_x as f32 * TILE_SIZE
        } else {
            (cell_x as f32 - size_blocks / 2.0 + 0.5) * TILE_SIZE
        };
        let y = cell_y as f32 * TILE_SIZE + PLATFORM_CELL_DROP;
        let vy = match kind {
            PlatformKind::JustUp => -PLATFORM_JUST_SPEED,
            PlatformKind::JustDown => PLATFORM_JUST_SPEED,
            _ => 0.0,
        };
        Platform {
            x,
            y,
            w: size_blocks * TILE_SIZE,
            kind,
            start_x: x,
            start_y: y,
            timer: 0.0,
            vx: 0.0,
            vy,
        }
    }

    /// The collision box, for the solid-rect list.
    pub(crate) fn rect(&self) -> [f32; 4] {
        [self.x, self.y, self.w, PLATFORM_HEIGHT]
    }
}

/// A platform spawner: the pair of them at the top and bottom of an elevator shaft.
///
/// Unlike the platforms themselves these are built at **load**, not revealed by the
/// camera (`game.lua:2386-2389` creates them in the parsing loop), and each deletes
/// itself once the camera has gone past (`platformspawner.lua:23`).
#[derive(Debug, Clone)]
pub(crate) struct PlatformSpawner {
    pub(crate) cell: (i32, i32),
    pub(crate) up: bool,
    pub(crate) size_blocks: f32,
    pub(crate) timer: f32,
}

/// The cosine easing both oscillating kinds run on (`platform.lua:39-41`).
///
/// `(-cos(2πt) + 1) / 2` over `t ∈ 0..1`: starts and ends at rest, so the platform
/// eases into each turnaround instead of snapping. Same curve the flying koopa uses.
fn ease(t: f32) -> f32 {
    (-(t * std::f32::consts::TAU).cos() + 1.0) / 2.0
}

impl Mari0Game {
    /// Move every platform, carry whatever is riding it, and republish the solids.
    ///
    /// Publishes into `level.platform_rects`, which is a separate list from the lab's
    /// `solid_rects` on purpose — see the field's own note.
    pub(crate) fn update_platforms(&mut self, dt: f32) {
        self.spawn_from_platform_spawners(dt);

        let mut riders: Vec<(f32, f32)> = Vec::new();
        for p in &mut self.platforms {
            match p.kind {
                PlatformKind::Horizontal => {
                    p.timer = (p.timer + dt) % PLATFORM_HOR_TIME;
                    // The velocity is *derived from the position*, not integrated:
                    // the original computes where it should be this frame and then
                    // back-solves `speedx` so the carry code has something to apply
                    // (`platform.lua:51-52`). Integrating instead would drift.
                    let target = p.start_x - ease(p.timer / PLATFORM_HOR_TIME) * PLATFORM_HOR_DIST;
                    p.vx = (target - p.x) / dt;
                    p.vy = 0.0;
                }
                PlatformKind::Vertical => {
                    p.timer = (p.timer + dt) % PLATFORM_VER_TIME;
                    // `starty - 15/16` — and `start_y` already carries that offset from
                    // construction, so the original applies it twice
                    // (`platform.lua:11`, `:59`). Reproduced: it makes the platform
                    // hop up half a block on its first frame, which is visible, and
                    // shifts the whole travel range up by that much for good.
                    let target = p.start_y - PLATFORM_CELL_DROP
                        + ease(p.timer / PLATFORM_VER_TIME) * PLATFORM_VER_DIST;
                    p.vy = (target - p.y) / dt;
                    p.vx = 0.0;
                }
                PlatformKind::JustUp => p.vy = -PLATFORM_JUST_SPEED,
                PlatformKind::JustDown => p.vy = PLATFORM_JUST_SPEED,
                PlatformKind::Fall | PlatformKind::Bonus => {}
            }
            if p.kind.carries() {
                riders.push((p.vx, p.vy));
            }
        }

        // Carry, then move. The player is checked against each platform's *old*
        // position, which is what the original does — it moves the platform and its
        // riders in the same step from the same reference point.
        for i in 0..self.platforms.len() {
            let p = self.platforms[i].clone();
            let carried = self.carry_rider(&p, dt);
            if p.kind == PlatformKind::Fall {
                // A falling platform's speed is *set* from the rider count every
                // frame rather than accumulated (`platform.lua:132`), so it drops at a
                // flat 4 blocks/s while you stand on it and stops the instant you step
                // off. It never accelerates, and it never comes back.
                self.platforms[i].vy = if carried { PLATFORM_FALL_SPEED } else { 0.0 };
            }
            let vx = self.platforms[i].vx;
            let vy = self.platforms[i].vy;
            self.platforms[i].x += vx * dt;
            self.platforms[i].y += vy * dt;
        }

        // The two one-way lifts are removed at the ends of their shafts, and the
        // faller once it is off the bottom of the world.
        let bottom = self.level.height as f32 * TILE_SIZE + TILE_SIZE;
        self.platforms.retain(|p| match p.kind {
            PlatformKind::JustUp => p.y > -TILE_SIZE,
            PlatformKind::JustDown | PlatformKind::Fall => p.y < bottom,
            _ => true,
        });

        self.level.platform_rects = self
            .platforms
            .iter()
            .map(|p| SolidRect {
                rect: p.rect(),
                cubes_pass: false,
            })
            .collect();
    }

    /// Move the player along with a platform he is standing on. Returns whether he was.
    ///
    /// The two axes test for "riding" differently, and both differences are the
    /// original's:
    ///
    /// - **Horizontally** the test is exact — `w.y == self.y - w.height`
    ///   (`platform.lua:77`) — because a platform that only moves sideways leaves you
    ///   resting precisely on its surface, frame after frame. It also refuses to carry
    ///   you into a wall (`:78`).
    /// - **Vertically** the test has a ±0.1 block tolerance and skips you while
    ///   `jumping` (`:100-101`), then *snaps* you to the surface rather than nudging
    ///   you. Without the tolerance a descending platform would leave you behind every
    ///   frame; without the jumping check it would pin you to itself mid-jump.
    fn carry_rider(&mut self, p: &Platform, dt: f32) -> bool {
        let (px, py) = (self.player.x, self.player.y);
        let (pw, ph) = (self.player.width, self.player.height);
        // Horizontal overlap is `inrange(w.x, self.x - w.width, self.x + self.width)`,
        // i.e. the player's *left edge* within the platform's span widened by his own
        // width — an overlap test written in terms of one corner.
        if !(px > p.x - pw && px < p.x + p.w) {
            return false;
        }
        let surface = p.y - ph;

        if p.vx != 0.0 {
            if (py - surface).abs() < 0.01 {
                let dx = p.vx * dt;
                // Only if the destination is clear; a platform will not shove you
                // through a wall.
                let mut probe = px + dx;
                let blocked = crate::physics::move_and_collide_x(
                    &mut probe,
                    py,
                    pw,
                    ph,
                    dx / dt,
                    &self.level,
                    dt,
                    crate::physics::Body::Normal,
                ) == 0.0;
                if !blocked {
                    self.player.x += dx;
                }
                return true;
            }
            return false;
        }

        if p.vy != 0.0 || p.kind == PlatformKind::Fall {
            if self.player.vy < 0.0 {
                // Rising: he's jumping off, so let him go.
                return false;
            }
            if (py - surface).abs() < PLATFORM_RIDE_TOLERANCE {
                self.player.y = surface + p.vy * dt;
                self.player.on_ground = true;
                return true;
            }
        }
        false
    }

    /// Release a platform from each spawner that is due one.
    ///
    /// A spawner is dropped once the camera passes it, so an elevator shaft stops
    /// producing as soon as you have left it behind.
    fn spawn_from_platform_spawners(&mut self, dt: f32) {
        let cam_x = self.camera.x;
        let mut made: Vec<Platform> = Vec::new();
        for s in &mut self.platform_spawners {
            s.timer += dt;
            while s.timer > PLATFORM_SPAWN_DELAY {
                s.timer -= PLATFORM_SPAWN_DELAY;
                // The two spawners release from *different* offsets: the upward one a
                // block below its cell, the downward one half a block above
                // (`platformspawner.lua:16-19`), so a shaft's two streams don't line up.
                let (kind, cell_y) = if s.up {
                    (PlatformKind::JustUp, s.cell.1 + 1)
                } else {
                    (PlatformKind::JustDown, s.cell.1)
                };
                let mut p = Platform::new(s.cell.0, cell_y, kind, s.size_blocks);
                if !s.up {
                    p.y -= TILE_SIZE / 2.0;
                    p.start_y = p.y;
                }
                made.push(p);
            }
        }
        self.platforms.append(&mut made);
        self.platform_spawners
            .retain(|s| (s.cell.0 as f32) * TILE_SIZE >= cam_x - TILE_SIZE / 2.0);
    }

    /// Start the bonus-stage platform when Mario headbutts it.
    ///
    /// It is the only platform with a trigger, and the trigger is from *underneath*
    /// (`platform.lua:168-172`) — you have to hit it with your head, not walk onto it.
    pub(crate) fn bump_bonus_platforms(&mut self) {
        let head = [self.player.x, self.player.y - 2.0, self.player.width, 2.0];
        for p in &mut self.platforms {
            if p.kind == PlatformKind::Bonus
                && p.vx == 0.0
                && crate::physics::aabb_overlap(head, p.rect())
            {
                p.vx = PLATFORM_BONUS_SPEED;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The width offset is conditional on the size being whole blocks.
    #[test]
    fn a_fractional_platform_is_centred_and_a_whole_one_is_not() {
        let whole = Platform::new(10, 5, PlatformKind::Horizontal, 2.0);
        assert_eq!(whole.x, 10.0 * TILE_SIZE);
        // 1.5 wide: `x - size/2 - 0.5` in the original's 1-based coordinates, which is
        // a quarter of a block left of the cell here.
        let half = Platform::new(10, 5, PlatformKind::Horizontal, 1.5);
        assert_eq!(half.x, (10.0 - 0.25) * TILE_SIZE);
        assert_eq!(half.w, 1.5 * TILE_SIZE);
    }

    /// The easing starts and ends at rest, which is what stops the turnaround snapping.
    #[test]
    fn the_travel_curve_eases_at_both_ends() {
        assert!(ease(0.0).abs() < 1e-6);
        assert!((ease(0.5) - 1.0).abs() < 1e-6);
        assert!(ease(1.0).abs() < 1e-6);
        // Symmetric about the midpoint.
        assert!((ease(0.25) - ease(0.75)).abs() < 1e-6);
        // Slowest at the ends, fastest a quarter of the way through. Not at the
        // midpoint — that's the far end of the travel, where it's turning around and
        // momentarily still.
        assert!(ease(0.1) - ease(0.0) < ease(0.3) - ease(0.2));
        assert!(ease(0.55) - ease(0.45) < ease(0.3) - ease(0.2));
    }

    /// The one-way lifts leave the constructor already moving.
    #[test]
    fn the_shaft_lifts_start_at_speed() {
        let up = Platform::new(4, 10, PlatformKind::JustUp, 3.0);
        assert_eq!(up.vy, -PLATFORM_JUST_SPEED);
        let down = Platform::new(4, 2, PlatformKind::JustDown, 3.0);
        assert_eq!(down.vy, PLATFORM_JUST_SPEED);
        // Everything else starts still, including the faller and the bonus platform —
        // both of those wait to be triggered.
        for kind in [
            PlatformKind::Horizontal,
            PlatformKind::Vertical,
            PlatformKind::Fall,
            PlatformKind::Bonus,
        ] {
            assert_eq!(Platform::new(4, 4, kind, 2.0).vy, 0.0, "{kind:?}");
            assert_eq!(Platform::new(4, 4, kind, 2.0).vx, 0.0, "{kind:?}");
        }
    }

    /// A platform is half a block tall and sits just below its cell's top edge.
    #[test]
    fn a_platform_is_half_a_block_thick() {
        let p = Platform::new(7, 9, PlatformKind::Fall, 3.0);
        let [_, y, w, h] = p.rect();
        assert_eq!(h, PLATFORM_HEIGHT);
        const { assert!(PLATFORM_HEIGHT < TILE_SIZE) };
        assert_eq!(w, 3.0 * TILE_SIZE);
        assert!(
            y > 9.0 * TILE_SIZE && y < 10.0 * TILE_SIZE,
            "it hangs inside its own cell, not on the boundary: {y}"
        );
    }
}
