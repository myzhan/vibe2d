//! The portal gun: projectiles, the two portal slots, and teleportation.
//!
//! A portal is one tile wide and two tiles long, mounted flush to a face, and it is
//! identified by the tile it is anchored to — see [`PortalAnchor`], which explains
//! why the anchor is normalised differently per face. The placement rules and the
//! coordinate transform live in `portal_math`, as pure functions; this file is the
//! glue that fires shots, decides what they hit, and moves the player through.
//!
//! A single portal is not a hole: both must exist before anything routes through,
//! which is why `check_portal_teleport` starts by requiring both slots.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::*;
use crate::player::Orientation;
use crate::portal_math::{PortalAnchor, portal_position, portal_transform, tendency_for};

#[derive(Clone)]
pub(crate) struct Portal {
    pub(crate) anchor: PortalAnchor,
    pub(crate) active: bool,
    pub(crate) open_scale: f32, // 0→1 opening animation (original: dt*15)
}

impl Portal {
    /// Centre of the mouth, in world pixels. What the renderer draws around.
    pub(crate) fn centre(&self) -> (f32, f32) {
        self.anchor.mouth_centre()
    }

    pub(crate) fn orientation(&self) -> Orientation {
        self.anchor.facing
    }

    /// The coordinate of the mouth's plane along its own normal, in world pixels.
    ///
    /// For an up/down face that's a `y`; for left/right an `x`. Matches the
    /// original's plane, which is the anchor row/column adjusted per facing
    /// (`portalY - 1` for up, `portalX - 1` for left, as-is otherwise).
    pub(crate) fn plane(&self) -> f32 {
        let (cx, cy) = self.centre();
        match self.orientation() {
            Orientation::Up | Orientation::Down => cy,
            Orientation::Left | Orientation::Right => cx,
        }
    }

    /// Columns the mouth spans (up/down faces).
    pub(crate) fn cols(&self) -> [i32; 2] {
        let c = self.anchor.cells();
        [c[0].0, c[1].0]
    }

    /// Rows the mouth spans (left/right faces).
    pub(crate) fn rows(&self) -> [i32; 2] {
        let c = self.anchor.cells();
        [c[0].1, c[1].1]
    }
}

pub(crate) struct PortalProjectile {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) portal_index: usize,
    pub(crate) active: bool,
}

impl Mari0Game {
    /// Fire portal `index` (0 = blue, 1 = orange) along the crosshair.
    ///
    /// Only one in-flight shot per slot: re-firing retires the previous one so a
    /// held trigger can't stack projectiles.
    pub(crate) fn fire_projectile(&mut self, index: usize) {
        let angle = self.crosshair_angle;
        self.projectiles
            .retain(|p| p.portal_index != index || !p.active);
        self.projectiles.push(PortalProjectile {
            x: self.player.center_x(),
            y: self.player.center_y(),
            vx: angle.cos() * PROJECTILE_SPEED,
            vy: angle.sin() * PROJECTILE_SPEED,
            portal_index: index,
            active: true,
        });
    }

    /// Recompute which cells the portal pair has opened.
    ///
    /// A lone portal is **not** a hole: `modifyportaltiles` only removes the tile
    /// objects once both portals exist. Until then the wall you shot is still a wall,
    /// which is why a single portal does nothing at all.
    pub(crate) fn refresh_portal_holes(&mut self) {
        self.level.portal_holes.clear();
        let (Some(a), Some(b)) = (&self.portals[0], &self.portals[1]) else {
            return;
        };
        if !a.active || !b.active {
            return;
        }
        for cell in a.anchor.cells().into_iter().chain(b.anchor.cells()) {
            self.level.portal_holes.insert(cell);
        }
    }

    /// The two portals as anchors, for the placement rules to exclude.
    fn portal_anchors(&self) -> [Option<PortalAnchor>; 2] {
        [
            self.portals[0]
                .as_ref()
                .filter(|p| p.active)
                .map(|p| p.anchor),
            self.portals[1]
                .as_ref()
                .filter(|p| p.active)
                .map(|p| p.anchor),
        ]
    }

    pub(crate) fn update_projectiles(&mut self, ctx: &Context, dt: f32) {
        // Collected first: placing a portal needs `&self.level` while the loop holds
        // `&mut self.projectiles`.
        let mut placed: Vec<(usize, PortalAnchor)> = Vec::new();

        for proj in &mut self.projectiles {
            if !proj.active {
                continue;
            }
            proj.x += proj.vx * dt;
            proj.y += proj.vy * dt;

            let col = (proj.x / TILE_SIZE).floor() as i32;
            let row = (proj.y / TILE_SIZE).floor() as i32;
            let tile = get_tile(&self.level, col, row);

            if is_solid(tile) {
                // Which face it struck, from the cell it came out of.
                let prev_col = ((proj.x - proj.vx * dt) / TILE_SIZE).floor() as i32;
                let prev_row = ((proj.y - proj.vy * dt) / TILE_SIZE).floor() as i32;
                let side = if prev_col < col {
                    Orientation::Left
                } else if prev_col > col {
                    Orientation::Right
                } else if prev_row < row {
                    Orientation::Up
                } else {
                    Orientation::Down
                };
                placed.push((
                    proj.portal_index,
                    PortalAnchor {
                        cell: (col, row),
                        facing: side,
                    },
                ));
                // The shot is spent either way — a face that refuses a portal still
                // absorbs it.
                proj.active = false;
            }

            // Out of bounds
            if proj.x < -100.0
                || proj.x > (self.level.width as f32 * TILE_SIZE) + 100.0
                || proj.y < -100.0
                || proj.y > (self.level.height as f32 * TILE_SIZE) + 100.0
            {
                proj.active = false;
            }
        }
        self.projectiles.retain(|p| p.active);

        for (index, hit) in placed {
            self.place_portal(ctx, index, hit);
        }
    }

    /// Try to mount portal `index` on the struck face.
    ///
    /// Fails **silently** when no valid two-tile span exists — no sound, no message.
    /// That's the original's behaviour (`getportalposition` simply returns false) and
    /// the lab levels are designed around which walls will and won't take a portal.
    fn place_portal(&mut self, ctx: &Context, index: usize, hit: PortalAnchor) {
        let (hit_x, hit_y) = (hit.cell.0 as f32 * TILE_SIZE, hit.cell.1 as f32 * TILE_SIZE);
        // The impact point within the tile decides which candidate span is tried
        // first. Approximated from the cell centre, which is where the projectile
        // was when it registered the hit.
        let tendency = tendency_for(hit_x + TILE_SIZE / 2.0, hit_y + TILE_SIZE / 2.0, hit.facing);
        let anchors = self.portal_anchors();
        let Some(cell) = portal_position(
            &self.level,
            hit.cell,
            hit.facing,
            tendency,
            &anchors,
            // A portal may be re-placed over where it already was.
            index,
        ) else {
            return;
        };

        self.portals[index] = Some(Portal {
            anchor: PortalAnchor {
                cell,
                facing: hit.facing,
            },
            active: true,
            open_scale: 0.0,
        });
        if index == 0 {
            ctx.audio.play("portal1open");
        } else {
            ctx.audio.play("portal2open");
        }
        self.refresh_portal_holes();
    }

    /// Run the three entry tests in the original's order.
    ///
    /// `next_x`/`next_y` are where the player is about to move to. Two swept tests
    /// come first — they catch a body *crossing* a mouth's plane within one step,
    /// which an overlap test misses entirely at speed — and `in_portal` mops up
    /// afterwards for anything already sitting inside a mouth.
    ///
    /// Order matters and is the original's: `checkportalVER` before
    /// `checkportalHOR`, so a corner that qualifies for both resolves as a wall
    /// portal rather than a floor one.
    pub(crate) fn check_portal_entry(&mut self, ctx: &Context, next_x: f32, next_y: f32) -> bool {
        if self.player.teleport_cooldown > 0.0 || self.portal_pair().is_none() {
            return false;
        }
        self.check_portal_ver(ctx, next_x) || self.check_portal_hor(ctx, next_y)
    }

    /// Both portals, in slot order, or `None` unless the pair is complete.
    fn portal_pair(&self) -> Option<(Portal, Portal)> {
        match (&self.portals[0], &self.portals[1]) {
            (Some(a), Some(b)) if a.active && b.active => Some((a.clone(), b.clone())),
            _ => None,
        }
    }

    /// Swept test for the vertical mouths (left/right faces).
    ///
    /// The row test uses the player's **top edge** cell, as the original does
    /// (`math.floor(self.y+1)`).
    fn check_portal_ver(&mut self, ctx: &Context, next_x: f32) -> bool {
        let Some((p0, p1)) = self.portal_pair() else {
            return false;
        };
        let row = (self.player.y / TILE_SIZE).floor() as i32;
        let half_w = self.player.width / 2.0;

        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            if !matches!(entry.orientation(), Orientation::Left | Orientation::Right) {
                continue;
            }
            if !entry.rows().contains(&row)
                || !in_range(entry.plane(), self.player.x + half_w, next_x + half_w)
            {
                continue;
            }
            // Only a body moving *into* the face can use it.
            let heading_in = match entry.orientation() {
                Orientation::Right => self.player.vx <= 0.0,
                Orientation::Left => self.player.vx >= 0.0,
                _ => false,
            };
            if !heading_in {
                continue;
            }

            let out = self.transform_player(entry, exit);
            if rect_is_clear(
                &self.level,
                out.x,
                out.y,
                self.player.width,
                self.player.height,
            ) {
                self.commit_teleport(ctx, out);
            } else {
                // Blocked exit on a wall portal: bounce, **no damping and no
                // minimum**, unlike the floor case.
                self.player.vx = -self.player.vx;
            }
            self.player.is_jumping = false;
            return true;
        }
        false
    }

    /// Swept test for the horizontal mouths (up/down faces).
    ///
    /// The column test uses the player's **left edge** cell (`math.floor(self.x+1)`).
    fn check_portal_hor(&mut self, ctx: &Context, next_y: f32) -> bool {
        let Some((p0, p1)) = self.portal_pair() else {
            return false;
        };
        let col = (self.player.x / TILE_SIZE).floor() as i32;
        let half_h = self.player.height / 2.0;

        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            if !matches!(entry.orientation(), Orientation::Up | Orientation::Down) {
                continue;
            }
            if !entry.cols().contains(&col)
                || !in_range(entry.plane(), self.player.y + half_h, next_y + half_h)
            {
                continue;
            }
            let heading_in = match entry.orientation() {
                Orientation::Up => self.player.vy >= 0.0,
                Orientation::Down => self.player.vy <= 0.0,
                _ => false,
            };
            if !heading_in {
                continue;
            }

            let out = self.transform_player(entry, exit);
            if rect_is_clear(
                &self.level,
                out.x,
                out.y,
                self.player.width,
                self.player.height,
            ) {
                self.commit_teleport(ctx, out);
            } else {
                // Blocked exit on a floor/ceiling portal: bounce with a little loss
                // and a minimum magnitude, so a body can't get stuck oscillating at
                // nearly zero speed.
                self.player.vy = -self.player.vy * 0.95;
                const MIN_BOUNCE: f32 = 2.0 * TILE_SIZE;
                if self.player.vy.abs() < MIN_BOUNCE {
                    self.player.vy = MIN_BOUNCE * self.player.vy.signum().max(-1.0);
                }
            }
            // An up↔down pair is a straight fall-through and keeps its jump state;
            // everything else lands the player into a fall.
            let straight_through = matches!(
                (entry.orientation(), exit.orientation()),
                (Orientation::Up, Orientation::Down) | (Orientation::Down, Orientation::Up)
            );
            if !straight_through {
                self.player.is_jumping = false;
            }
            return true;
        }
        false
    }

    /// Containment fallback: teleport anything whose centre cell is inside a mouth.
    ///
    /// Run *after* movement, and deliberately **without** a clearance check — the
    /// original's `inportal` teleports unconditionally. It exists to catch bodies the
    /// two swept tests missed, so refusing here would leave them stuck in a wall.
    pub(crate) fn check_in_portal(&mut self, ctx: &Context) -> bool {
        if self.player.teleport_cooldown > 0.0 {
            return false;
        }
        let Some((p0, p1)) = self.portal_pair() else {
            return false;
        };
        let cell = (
            (self.player.center_x() / TILE_SIZE).floor() as i32,
            (self.player.center_y() / TILE_SIZE).floor() as i32,
        );
        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            if !entry.anchor.cells().contains(&cell) {
                continue;
            }
            let out = self.transform_player(entry, exit);
            self.commit_teleport(ctx, out);
            return true;
        }
        false
    }

    /// Transform the player through a portal pair.
    ///
    /// `live` is always true here. The original passes it on `inportal`'s *first*
    /// branch and omits it on the second, so the minimum exit speed applied only
    /// when entering through portal 1 — the two branches are otherwise identical, so
    /// that is a copy-paste slip rather than a rule. **Corrected**: a player could
    /// not predict or use an asymmetry between the two portals.
    fn transform_player(&self, entry: &Portal, exit: &Portal) -> crate::portal_math::Exit {
        portal_transform(
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
            self.player.vx,
            self.player.vy,
            0.0,
            entry.anchor,
            exit.anchor,
            GRAVITY,
            true,
        )
    }

    fn commit_teleport(&mut self, ctx: &Context, out: crate::portal_math::Exit) {
        self.player.x = out.x;
        self.player.y = out.y;
        self.player.vx = out.vx;
        self.player.vy = out.vy;
        self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
        self.player.on_ground = false;
        ctx.audio.play("portalenter");
    }
}
