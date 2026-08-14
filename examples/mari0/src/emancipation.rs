//! Faith plates and emancipation grills — the last two lab fixtures.
//!
//! They have nothing in common mechanically; they are together because both are
//! *lines* the level draws across a corridor and both act on whatever crosses them.
//!
//! ## Faith plate (`faithplate.lua`)
//!
//! A launcher, and the launch is an **absolute assignment, not an impulse**: `up` sets
//! `speedy = -40`, `right` sets `speedy = -30, speedx = +30`, `left` mirrors it. However
//! fast you arrive, you leave at the same speed — which is what makes them predictable
//! enough to build a level out of.
//!
//! ## Emancipation grill (`emancipationgrill.lua`)
//!
//! A curtain of light that spans its corridor: it grows from its own cell along its axis
//! until it hits solid tile at both ends, and the cells it covers are its `involvedtiles`.
//! Crossing it **fizzles your portals** and destroys anything else that can be
//! emancipated (`emancipatecheck`: Mario, goombas, cubes).
//!
//! The crossing test is **swept and one-dimensional**: the body's position along the
//! grill's axis has to be inside the span, and the grill's line has to fall between the
//! body's current and next position across it (`physics.lua:137-150`). A body that
//! teleports over it is not caught — you have to actually pass through.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::{GameState, Mari0Game};
use crate::lab::LabKind;
use crate::physics::*;
use crate::player::Orientation;

/// How long a plate's kick animation lasts (`faithplatetime = 0.3`).
pub(crate) const FAITH_PLATE_TIME: f32 = 0.3;

/// The launch speeds, in blocks/s (`faithplate.lua:30-38`).
const FAITH_UP: f32 = 40.0 * TILE_SIZE;
const FAITH_DIAGONAL_UP: f32 = 30.0 * TILE_SIZE;
const FAITH_DIAGONAL_SIDE: f32 = 30.0 * TILE_SIZE;

/// A grill's line sits 2/16 of a block into its own cell — `u.y - 14/16` measured from
/// the far side (`physics.lua:141`).
const GRILL_LINE_INSET: f32 = 2.0 / 16.0 * TILE_SIZE;

/// One emancipation grill, resolved to the span it covers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grill {
    /// The cell the entity was placed in.
    pub(crate) cell: (i32, i32),
    /// Horizontal grills lie along a row; vertical ones stand in a column.
    pub(crate) horizontal: bool,
    /// First and last cell of the span, along the grill's axis.
    pub(crate) start: i32,
    pub(crate) end: i32,
}

impl Grill {
    /// Grow a grill from its cell in both directions until solid tile stops it.
    ///
    /// The original grows through anything non-solid, so a grill in an open room spans
    /// the whole room. Note it is placed in a *hole* — a grill whose own cell is solid
    /// deletes itself on load (`emancipationgrill.lua:9-11`).
    pub(crate) fn resolve(
        level: &crate::world::Level,
        cell: (i32, i32),
        horizontal: bool,
    ) -> Option<Self> {
        let solid = |c: i32, r: i32| is_solid(get_tile(level, c, r));
        if solid(cell.0, cell.1) {
            return None;
        }
        let (mut start, mut end) = if horizontal {
            (cell.0, cell.0)
        } else {
            (cell.1, cell.1)
        };
        let limit = if horizontal {
            level.width as i32
        } else {
            level.height as i32
        };
        while start > 0 {
            let probe = if horizontal {
                (start - 1, cell.1)
            } else {
                (cell.0, start - 1)
            };
            if solid(probe.0, probe.1) {
                break;
            }
            start -= 1;
        }
        while end + 1 < limit {
            let probe = if horizontal {
                (end + 1, cell.1)
            } else {
                (cell.0, end + 1)
            };
            if solid(probe.0, probe.1) {
                break;
            }
            end += 1;
        }
        Some(Grill {
            cell,
            horizontal,
            start,
            end,
        })
    }

    /// The line's coordinate across the corridor, in world pixels.
    pub(crate) fn line(&self) -> f32 {
        if self.horizontal {
            self.cell.1 as f32 * TILE_SIZE + GRILL_LINE_INSET
        } else {
            self.cell.0 as f32 * TILE_SIZE + GRILL_LINE_INSET
        }
    }

    /// The span's extent along the grill's own axis, in world pixels.
    pub(crate) fn span(&self) -> (f32, f32) {
        (
            self.start as f32 * TILE_SIZE,
            (self.end + 1) as f32 * TILE_SIZE,
        )
    }

    /// Did a body whose centre was at `from` and is now at `to` cross this curtain?
    ///
    /// `along` is the body's centre on the grill's own axis. Both tests have to pass: in
    /// the span, and across the line during this step.
    pub(crate) fn crossed(&self, along: f32, from: f32, to: f32) -> bool {
        let (lo, hi) = self.span();
        along >= lo && along <= hi && in_range(self.line(), from, to)
    }
}

impl Mari0Game {
    /// Resolve every grill in the level. Called once, after the tiles are loaded.
    pub(crate) fn build_grills(&mut self) {
        self.grills.clear();
        for element in &self.lab.elements {
            let horizontal = match element.kind {
                LabKind::GrillHor => true,
                LabKind::GrillVer => false,
                _ => continue,
            };
            if let Some(grill) = Grill::resolve(&self.level, element.cell, horizontal) {
                self.grills.push(grill);
            }
        }
    }

    /// Launch anything sitting on a faith plate, and run the grills.
    pub(crate) fn update_plates_and_grills(&mut self, ctx: &mut Context, dt: f32) {
        self.faith_plates(dt);
        self.emancipate(ctx, dt);
    }

    /// Faith plates: an absolute launch for the player, cubes and enemies alike.
    ///
    /// The trigger is a one-block strip on top of the plate, inset half a block from each
    /// end (`checkrect(self.x+.5, self.y-0.125, 1, 0.125)`).
    fn faith_plates(&mut self, dt: f32) {
        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::FaithPlate {
                continue;
            }
            // The animation timer runs 0 → 1 over `faithplatetime`.
            let element = &mut self.lab.elements[index];
            if element.timer < 1.0 {
                element.timer = (element.timer + dt / FAITH_PLATE_TIME).min(1.0);
            }
            let (cell, dir) = (element.cell, element.axis.unwrap_or(Orientation::Up));
            let sense = plate_sense_rect(cell);
            let (vx, vy) = match dir {
                Orientation::Up => (None, -FAITH_UP),
                Orientation::Right => (Some(FAITH_DIAGONAL_SIDE), -FAITH_DIAGONAL_UP),
                _ => (Some(-FAITH_DIAGONAL_SIDE), -FAITH_DIAGONAL_UP),
            };

            let mut fired = false;
            if self.state == GameState::Playing
                && aabb_overlap(
                    [
                        self.player.x,
                        self.player.y,
                        self.player.width,
                        self.player.height,
                    ],
                    sense,
                )
            {
                self.player.vy = vy;
                if let Some(vx) = vx {
                    self.player.vx = vx;
                }
                self.player.on_ground = false;
                self.player.is_jumping = false;
                fired = true;
            }
            for cube in &mut self.cubes {
                if cube.held || !aabb_overlap(cube.rect(), sense) {
                    continue;
                }
                cube.vy = vy;
                if let Some(vx) = vx {
                    cube.vx = vx;
                }
                cube.falling = true;
                fired = true;
            }
            for enemy in &mut self.enemies {
                let height = crate::enemies::enemy_height(enemy.enemy_type, enemy.state);
                if !aabb_overlap([enemy.x, enemy.y, PLAYER_SMALL_W, height], sense) {
                    continue;
                }
                enemy.vy = vy;
                if let Some(vx) = vx {
                    enemy.vx = vx;
                }
                enemy.on_ground = false;
                fired = true;
            }
            if fired {
                self.lab.elements[index].timer = 0.0;
            }
        }
    }

    /// Emancipation grills: fizzle the player's portals, destroy everything else.
    ///
    /// The swept test uses each body's *centre*, tracked from where it was at the start
    /// of the frame. Mario keeps his own previous position for this; cubes and enemies use
    /// their velocity, which is the same thing one frame apart.
    fn emancipate(&mut self, ctx: &mut Context, dt: f32) {
        if self.grills.is_empty() {
            return;
        }
        let mut fizzle = false;
        for grill in &self.grills {
            let (px, py) = (self.player.center_x(), self.player.center_y());
            let (prev_x, prev_y) = self.previous_player_centre;
            let crossed = if grill.horizontal {
                grill.crossed(px, prev_y, py)
            } else {
                grill.crossed(py, prev_x, px)
            };
            if crossed && self.state == GameState::Playing {
                fizzle = true;
            }
        }
        if fizzle {
            // `mario:emancipate` removes both portals and any shot still in flight —
            // the grill is the level designer's reset button.
            self.portals = [None, None];
            self.projectiles.clear();
            self.refresh_portal_holes();
            ctx.audio.play("portalenter");
        }

        // Cubes and enemies: destroyed outright.
        let grills = std::mem::take(&mut self.grills);
        self.cubes.retain(|cube| {
            let (cx, cy) = (cube.x + 12.0, cube.y + 12.0);
            let (px, py) = (cx - cube.vx * dt, cy - cube.vy * dt);
            !grills.iter().any(|g| {
                if g.horizontal {
                    g.crossed(cx, py, cy)
                } else {
                    g.crossed(cy, px, cx)
                }
            })
        });
        for enemy in &mut self.enemies {
            if enemy.state == crate::enemies::EnemyState::Dead {
                continue;
            }
            let h = crate::enemies::enemy_height(enemy.enemy_type, enemy.state);
            let (cx, cy) = (enemy.x + PLAYER_SMALL_W / 2.0, enemy.y + h / 2.0);
            let (px, py) = (cx - enemy.vx * dt, cy - enemy.vy * dt);
            let hit = grills.iter().any(|g| {
                if g.horizontal {
                    g.crossed(cx, py, cy)
                } else {
                    g.crossed(cy, px, cx)
                }
            });
            if hit {
                enemy.state = crate::enemies::EnemyState::Dead;
                enemy.death_timer = ENEMY_DEATH_TIME;
                enemy.flipped_death = true;
            }
        }
        self.grills = grills;

        self.previous_player_centre = (self.player.center_x(), self.player.center_y());
    }
}

/// The strip a faith plate launches from, in world pixels.
///
/// The plate itself is two blocks wide and paper-thin (`self.x = cox-1, width = 2,
/// height = 0.125`); the trigger is the middle block of it, sitting just above.
pub(crate) fn plate_sense_rect(cell: (i32, i32)) -> [f32; 4] {
    let (c, r) = (cell.0 as f32, cell.1 as f32);
    [
        (c + 0.5) * TILE_SIZE,
        (r - 0.125) * TILE_SIZE,
        TILE_SIZE,
        0.125 * TILE_SIZE,
    ]
}

/// The plate's own footprint, for drawing.
pub(crate) fn plate_rect(cell: (i32, i32)) -> [f32; 4] {
    [
        cell.0 as f32 * TILE_SIZE,
        cell.1 as f32 * TILE_SIZE,
        2.0 * TILE_SIZE,
        TILE_SIZE,
    ]
}
