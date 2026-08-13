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

    /// The mouth as an `[x, y, w, h]` rect, two tiles long and a few pixels deep.
    ///
    /// Used for the overlap test that decides whether a body is entering. Kept thin
    /// on the normal axis so brushing past a portal doesn't count as entering it.
    pub(crate) fn mouth_rect(&self) -> [f32; 4] {
        let (cx, cy) = self.centre();
        const DEPTH: f32 = 8.0;
        const LENGTH: f32 = 2.0 * TILE_SIZE;
        match self.orientation() {
            Orientation::Left | Orientation::Right => {
                [cx - DEPTH / 2.0, cy - LENGTH / 2.0, DEPTH, LENGTH]
            }
            Orientation::Up | Orientation::Down => {
                [cx - LENGTH / 2.0, cy - DEPTH / 2.0, LENGTH, DEPTH]
            }
        }
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

    pub(crate) fn check_portal_teleport(&mut self, ctx: &Context) {
        if self.player.teleport_cooldown > 0.0 {
            return;
        }
        let (p0, p1) = match (&self.portals[0], &self.portals[1]) {
            (Some(a), Some(b)) if a.active && b.active => (a.clone(), b.clone()),
            _ => return,
        };

        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            if !aabb_overlap(
                [
                    self.player.x,
                    self.player.y,
                    self.player.width,
                    self.player.height,
                ],
                entry.mouth_rect(),
            ) {
                continue;
            }

            // Moving *into* the face, not merely touching it.
            let entering = match entry.orientation() {
                Orientation::Left => self.player.vx > 0.0,
                Orientation::Right => self.player.vx < 0.0,
                Orientation::Up => self.player.vy > 0.0,
                Orientation::Down => self.player.vy < 0.0,
            };
            if !entering {
                continue;
            }

            // `live`: the player is the case the minimum exit speed exists for, so
            // that emerging from a floor portal always clears the floor.
            let out = portal_transform(
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
            );

            self.player.x = out.x;
            self.player.y = out.y;
            self.player.vx = out.vx;
            self.player.vy = out.vy;
            self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
            self.player.on_ground = false;
            ctx.audio.play("portalenter");
            return;
        }
    }
}
