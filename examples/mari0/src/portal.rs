//! The portal gun: projectiles, the two portal slots, and teleportation.
//!
//! A portal is one tile wide and two tiles long, mounted flush to a face. A
//! single portal is not a hole — both must exist before anything routes through,
//! which is why `check_portal_teleport` starts by requiring both slots.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::*;
use crate::player::Orientation;

#[derive(Clone)]
pub(crate) struct Portal {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) orientation: Orientation,
    pub(crate) active: bool,
    pub(crate) open_scale: f32, // 0→1 opening animation (original: dt*15)
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

    pub(crate) fn update_projectiles(&mut self, ctx: &Context, dt: f32) {
        for proj in &mut self.projectiles {
            if !proj.active {
                continue;
            }
            proj.x += proj.vx * dt;
            proj.y += proj.vy * dt;

            // Check tile collision
            let col = (proj.x / TILE_SIZE).floor() as i32;
            let row = (proj.y / TILE_SIZE).floor() as i32;
            let tile = get_tile(&self.level, col, row);

            if is_solid(tile) && is_portal_surface(tile) {
                // Determine which face was hit by checking where the projectile came from
                let prev_x = proj.x - proj.vx * dt;
                let prev_y = proj.y - proj.vy * dt;
                let prev_col = (prev_x / TILE_SIZE).floor() as i32;
                let prev_row = (prev_y / TILE_SIZE).floor() as i32;

                let orient = if prev_col < col {
                    Orientation::Left
                } else if prev_col > col {
                    Orientation::Right
                } else if prev_row < row {
                    Orientation::Up
                } else {
                    Orientation::Down
                };

                let (portal_x, portal_y) = match orient {
                    Orientation::Left => (
                        col as f32 * TILE_SIZE,
                        row as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    ),
                    Orientation::Right => (
                        (col + 1) as f32 * TILE_SIZE,
                        row as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    ),
                    Orientation::Up => (
                        col as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        row as f32 * TILE_SIZE,
                    ),
                    Orientation::Down => (
                        col as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        (row + 1) as f32 * TILE_SIZE,
                    ),
                };

                self.portals[proj.portal_index] = Some(Portal {
                    x: portal_x,
                    y: portal_y,
                    orientation: orient,
                    active: true,
                    open_scale: 0.0,
                });
                if proj.portal_index == 0 {
                    ctx.audio.play("portal1open");
                } else {
                    ctx.audio.play("portal2open");
                }
                proj.active = false;
            } else if is_solid(tile) {
                // Hit non-portal surface, destroy
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
    }

    pub(crate) fn check_portal_teleport(&mut self, ctx: &Context) {
        if self.player.teleport_cooldown > 0.0 {
            return;
        }
        let (p0, p1) = match (&self.portals[0], &self.portals[1]) {
            (Some(a), Some(b)) if a.active && b.active => (a.clone(), b.clone()),
            _ => return,
        };

        // Check overlap with either portal
        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            let portal_rect = match entry.orientation {
                Orientation::Left | Orientation::Right => {
                    (entry.x - 4.0, entry.y - 32.0, 8.0, 64.0)
                }
                Orientation::Up | Orientation::Down => (entry.x - 32.0, entry.y - 4.0, 64.0, 8.0),
            };

            if aabb_overlap(
                [
                    self.player.x,
                    self.player.y,
                    self.player.width,
                    self.player.height,
                ],
                [portal_rect.0, portal_rect.1, portal_rect.2, portal_rect.3],
            ) {
                // Check player is moving into the portal
                let entering = match entry.orientation {
                    Orientation::Left => self.player.vx > 0.0,
                    Orientation::Right => self.player.vx < 0.0,
                    Orientation::Up => self.player.vy > 0.0,
                    Orientation::Down => self.player.vy < 0.0,
                };
                if !entering {
                    continue;
                }

                // Teleport
                let (new_vx, new_vy) = transform_velocity(
                    self.player.vx,
                    self.player.vy,
                    entry.orientation,
                    exit.orientation,
                );

                // Position at exit portal
                let offset = 8.0;
                let (new_x, new_y) = match exit.orientation {
                    Orientation::Up => (
                        exit.x - self.player.width / 2.0,
                        exit.y - self.player.height - offset,
                    ),
                    Orientation::Down => (exit.x - self.player.width / 2.0, exit.y + offset),
                    Orientation::Left => (
                        exit.x - self.player.width - offset,
                        exit.y - self.player.height / 2.0,
                    ),
                    Orientation::Right => (exit.x + offset, exit.y - self.player.height / 2.0),
                };

                self.player.x = new_x;
                self.player.y = new_y;
                self.player.vx = new_vx;
                self.player.vy = new_vy;
                self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
                self.player.on_ground = false;
                ctx.audio.play("portalenter");
                return;
            }
        }
    }
}
