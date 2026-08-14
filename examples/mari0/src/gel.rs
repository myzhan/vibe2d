//! The three gels: blue bounces, orange accelerates, white takes a portal.
//!
//! A gel is not an entity you interact with — it is **paint on a tile face**. Blobs fly
//! out of a dispenser, splat on the first thing they touch, and vanish; what survives is
//! an entry in the level's gel layer, which the movement code reads back. So there are
//! two halves here: the blobs (`gel.lua`, `geldispenser.lua`) and the effects the paint
//! has on Mario (scattered through `mario.lua`).
//!
//! Four things about it are easy to get backwards:
//!
//! - **The face painted is the opposite of the side that hit.** A blob whose *left* side
//!   struck a tile paints that tile's **right** face (`gel:leftcollide`), because the
//!   face it splattered against is the one looking back at it.
//! - **Blobs only paint exposed faces.** If the tile beyond the one you hit is also
//!   solid, nothing is painted — you struck a seam, not a surface.
//! - **Gel spreads along the ground.** Landing on a top face that is *already* the same
//!   colour walks up to `speedx * 0.2` cells along in the direction of travel looking
//!   for the first bare exposed top. That is why a dispenser pointed at a floor paints a
//!   strip rather than a single cell.
//! - **Orange only works with both feet on the grid.** The speed boost is a *local*
//!   shadow of the global limits, applied only while `fmod(y + height, 1) == 0` and not
//!   airborne (`mario.lua:1064-1080`), so it vanishes the instant you leave the ground
//!   and your existing speed is then bled off by friction rather than clamped.

use crate::constants::*;
use crate::game::Mari0Game;
use crate::lab::LabKind;
use crate::level::{Gel, GelFace};
use crate::physics::*;
use crate::player::Orientation;

/// A blob lives two seconds if it hits nothing (`gellifetime = 2`).
const GEL_LIFETIME: f32 = 2.0;

/// Blobs have their own gravity, much stronger than Mario's (`gel.lua:17`).
const GEL_GRAVITY: f32 = 50.0 * TILE_SIZE;

/// And their own terminal speed (`gelmaxspeed = 30`).
const GEL_MAX_SPEED: f32 = 30.0 * TILE_SIZE;

/// One blob every 0.05s out of a dispenser (`geldispensespeed`).
const GEL_DISPENSE_INTERVAL: f32 = 0.05;

/// A blob is the same size as a cube: 12/16 of a block.
const GEL_SIZE: f32 = 12.0 / 16.0 * TILE_SIZE;

/// How far along the ground gel spreads, per unit of horizontal speed
/// (`speedx * 0.2` cells, `gel.lua:120`).
const SPREAD_PER_SPEED: f32 = 0.2;

// ── The orange gel's speed limits (`gelmaxrunspeed` and friends) ──────
const GEL_MAX_RUN_SPEED: f32 = 50.0 * TILE_SIZE;
const GEL_MAX_WALK_SPEED: f32 = 25.0 * TILE_SIZE;
const GEL_RUN_ACCEL: f32 = 25.0 * TILE_SIZE;
const GEL_WALK_ACCEL: f32 = 12.5 * TILE_SIZE;

// ── The blue gel's bounces ───────────────────────────────────────────
/// Minimum impact speed for a floor bounce: `gdt * yacceleration * 10`, i.e. ten frames'
/// worth of gravity (`mario.lua:1768`). Below it you just land.
const BLUE_FLOOR_MIN_SPEED: f32 = 10.0 * GRAVITY / 60.0;
/// A wall bounce needs `|speedx| > 2` blocks/s and returns at least 15, multiplying what
/// you arrived with by 1.5 — walls fire you back *harder* than you came in.
const BLUE_WALL_MIN_SPEED: f32 = 2.0 * TILE_SIZE;
const BLUE_WALL_MIN_RETURN: f32 = 15.0 * TILE_SIZE;
const BLUE_WALL_MULTIPLIER: f32 = 1.5;
/// And it lifts you: `speedy = min(speedy, -20)`.
const BLUE_WALL_LIFT: f32 = 20.0 * TILE_SIZE;

/// One airborne blob.
pub(crate) struct GelBlob {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) gel: Gel,
    /// Seconds lived. At [`GEL_LIFETIME`] it evaporates without painting anything.
    pub(crate) age: f32,
    /// Which of the three splat sprites to draw.
    pub(crate) frame: u32,
}

impl GelBlob {
    fn rect(&self) -> [f32; 4] {
        [self.x, self.y, GEL_SIZE, GEL_SIZE]
    }
}

impl Mari0Game {
    /// Run the gel dispensers and every blob in the air.
    pub(crate) fn update_gels(&mut self, dt: f32) {
        self.dispense_gel(dt);

        let mut splats: Vec<((i32, i32), GelFace, Gel, f32)> = Vec::new();
        for blob in &mut self.gel_blobs {
            blob.age += dt;
            blob.vy = (blob.vy + GEL_GRAVITY * dt).min(GEL_MAX_SPEED);

            // A blob's own movement, one axis at a time, but it does not *resolve*
            // anything: the first tile it touches ends it. Solid rects are ignored
            // outright — a gel's mask lets it through every dispenser
            // (`gel.lua:16`, category 7), and nothing else it could hit matters.
            let (nx, ny) = (blob.x + blob.vx * dt, blob.y + blob.vy * dt);
            if let Some((cell, face)) = struck_face(&self.level, blob, nx, ny) {
                splats.push((cell, face, blob.gel, blob.vx));
                blob.age = GEL_LIFETIME;
                continue;
            }
            blob.x = nx;
            blob.y = ny;
        }
        self.gel_blobs.retain(|b| b.age < GEL_LIFETIME);

        for (cell, face, gel, vx) in splats {
            self.splat(cell, face, gel, vx);
        }
    }

    /// Emit blobs from every gel dispenser in the level.
    fn dispense_gel(&mut self, dt: f32) {
        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::GelDispenser {
                continue;
            }
            let Some((gel, dir)) = self.lab.elements[index].entity.gel_dispenser() else {
                continue;
            };
            let element = &mut self.lab.elements[index];
            element.timer += dt;
            let (cell, mut frame) = (element.cell, self.gel_frame);
            while self.lab.elements[index].timer > GEL_DISPENSE_INTERVAL {
                self.lab.elements[index].timer -= GEL_DISPENSE_INTERVAL;
                // The original jitters the blob across the nozzle with
                // `math.random()-0.5`; this walks a fixed three-step cycle instead, so a
                // replay stays a replay. The spray still reads as a spray.
                let jitter = (frame as f32 - 1.0) * 0.33 * TILE_SIZE;
                frame = (frame + 1) % 3;
                let (x, y, vx, vy) = match dir {
                    Orientation::Down => (
                        (cell.0 as f32 + 1.5 - 12.0 / 16.0) * TILE_SIZE + jitter,
                        (cell.1 as f32 + 12.0 / 16.0) * TILE_SIZE,
                        0.0,
                        10.0 * TILE_SIZE,
                    ),
                    Orientation::Right => (
                        (cell.0 as f32 + 14.0 / 16.0) * TILE_SIZE,
                        (cell.1 as f32 + 1.5 - 12.0 / 16.0) * TILE_SIZE + jitter,
                        20.0 * TILE_SIZE,
                        -4.0 * TILE_SIZE,
                    ),
                    _ => (
                        (cell.0 as f32 + 30.0 / 16.0) * TILE_SIZE,
                        (cell.1 as f32 + 1.5 - 12.0 / 16.0) * TILE_SIZE + jitter,
                        -20.0 * TILE_SIZE,
                        -4.0 * TILE_SIZE,
                    ),
                };
                self.gel_blobs.push(GelBlob {
                    x,
                    y,
                    vx,
                    vy,
                    gel,
                    age: 0.0,
                    frame,
                });
            }
            self.gel_frame = frame;
        }
    }

    /// Paint one face, with the two rules that make gel behave like a fluid.
    ///
    /// The face must be **exposed** — if the cell on the other side of it is solid, the
    /// blob hit a seam between two blocks and nothing is painted (`gel.lua:59-61`).
    ///
    /// And landing on a top face that is already this colour makes the gel *run*: it
    /// walks up to `speedx * 0.2` cells downwind for the first exposed top that isn't
    /// painted yet. Without it, a dispenser aimed at the floor would paint one cell
    /// forever instead of laying a strip.
    fn splat(&mut self, cell: (i32, i32), face: GelFace, gel: Gel, vx: f32) {
        let solid =
            |level: &crate::world::Level, c: (i32, i32)| is_solid(get_tile(level, c.0, c.1));
        if !solid(&self.level, cell) {
            return;
        }
        // The cell in front of the face: the one the blob came from.
        let (dc, dr) = match face {
            GelFace::Top => (0, -1),
            GelFace::Bottom => (0, 1),
            GelFace::Left => (-1, 0),
            GelFace::Right => (1, 0),
        };
        let front = (cell.0 + dc, cell.1 + dr);
        if solid(&self.level, front) {
            return;
        }

        if face == GelFace::Top && self.level.gels(cell).face(face) == Some(gel) {
            let reach = (vx / TILE_SIZE * SPREAD_PER_SPEED) as i32;
            let step = if reach >= 0 { 1 } else { -1 };
            let mut col = cell.0;
            for _ in 0..reach.abs() {
                col += step;
                let here = (col, cell.1);
                // The run stops at the first cell that isn't an exposed top.
                if !solid(&self.level, here) || solid(&self.level, (col, cell.1 - 1)) {
                    break;
                }
                if self.level.gels(here).face(face) != Some(gel) {
                    self.level.paint_gel(here, face, gel);
                    break;
                }
            }
            return;
        }
        self.level.paint_gel(cell, face, gel);
    }

    /// The orange gel's speed limits, if the player is standing on some.
    ///
    /// Returns `(max_walk, max_run, walk_accel, run_accel)`. Orange is a *local*
    /// override for the frame, which is why leaving it doesn't clamp your speed — you
    /// keep it and lose it to friction (`mario.lua:1058-1080`).
    pub(crate) fn ground_speed_limits(&self) -> (f32, f32, f32, f32) {
        let normal = (MAX_WALK_SPEED, MAX_RUN_SPEED, WALK_ACCEL, RUN_ACCEL);
        if !self.player.on_ground {
            return normal;
        }
        // Both feet exactly on a grid line. Standing on a tile always satisfies this —
        // the resolver puts you there — but a frame mid-fall does not.
        let bottom = self.player.y + self.player.height;
        if (bottom / TILE_SIZE).fract().abs() > 0.001 {
            return normal;
        }
        let cell = (
            (self.player.center_x() / TILE_SIZE).floor() as i32,
            (bottom / TILE_SIZE).floor() as i32,
        );
        if self.level.gels(cell).top == Some(Gel::Orange) {
            (
                GEL_MAX_WALK_SPEED,
                GEL_MAX_RUN_SPEED,
                GEL_WALK_ACCEL,
                GEL_RUN_ACCEL,
            )
        } else {
            normal
        }
    }

    /// Blue gel on the floor: bounce instead of landing.
    ///
    /// `impact` is the downward speed the player *arrived* with, since the resolver has
    /// already zeroed it. Holding down cancels the bounce — that is how you stop on blue
    /// gel (`mario.lua:1768`).
    pub(crate) fn blue_floor_bounce(&mut self, impact: f32, crouching: bool, dt: f32) -> bool {
        if crouching || impact <= BLUE_FLOOR_MIN_SPEED {
            return false;
        }
        let cell = (
            (self.player.center_x() / TILE_SIZE).floor() as i32,
            ((self.player.y + self.player.height) / TILE_SIZE).floor() as i32,
        );
        if self.level.gels(cell).top != Some(Gel::Blue) {
            return false;
        }
        // The gravity already applied this frame is added back, so a bounce doesn't lose
        // a frame's worth of height each time — that is what keeps it from decaying.
        self.player.vy = -impact + GRAVITY * dt;
        self.player.on_ground = false;
        self.player.is_jumping = false;
        true
    }

    /// Blue gel on a wall: fire the player back the way they came, faster.
    ///
    /// Only while airborne, and only above a threshold speed, so walking into a blue
    /// wall does nothing (`mario.lua:1920`).
    pub(crate) fn blue_wall_bounce(&mut self, impact: f32, crouching: bool) -> bool {
        if crouching || self.player.on_ground || impact.abs() <= BLUE_WALL_MIN_SPEED {
            return false;
        }
        // Moving right, the wall's *left* face is the one you struck.
        let (probe, face) = if impact > 0.0 {
            (self.player.x + self.player.width, GelFace::Left)
        } else {
            (self.player.x - 1.0, GelFace::Right)
        };
        let cell = (
            (probe / TILE_SIZE).floor() as i32,
            (self.player.center_y() / TILE_SIZE).floor() as i32,
        );
        if self.level.gels(cell).face(face) != Some(Gel::Blue) {
            return false;
        }
        let back = (-impact * BLUE_WALL_MULTIPLIER)
            .abs()
            .max(BLUE_WALL_MIN_RETURN);
        self.player.vx = if impact > 0.0 { -back } else { back };
        self.player.vy = self.player.vy.min(-BLUE_WALL_LIFT);
        true
    }
}

/// Which face of which cell a blob is about to hit, if any.
///
/// Vertical first, matching the resolver's order elsewhere. The face reported is the one
/// the blob's own side ran into, which is the *opposite* of the direction it was moving.
fn struck_face(
    level: &crate::world::Level,
    blob: &GelBlob,
    nx: f32,
    ny: f32,
) -> Option<((i32, i32), GelFace)> {
    let cells = |rect: [f32; 4]| {
        let [x, y, w, h] = rect;
        let left = (x / TILE_SIZE).floor() as i32;
        let right = ((x + w - 0.01) / TILE_SIZE).floor() as i32;
        let top = (y / TILE_SIZE).floor() as i32;
        let bottom = ((y + h - 0.01) / TILE_SIZE).floor() as i32;
        (left, right, top, bottom)
    };

    // Vertical.
    let (left, right, top, bottom) = cells([blob.x, ny, GEL_SIZE, GEL_SIZE]);
    for col in left..=right {
        for row in top..=bottom {
            if !is_solid(get_tile(level, col, row)) {
                continue;
            }
            // An invisible block is not a surface: the blob passes through and keeps
            // going (`gel:globalcollide`).
            if crate::level::tiles::props(get_tile(level, col, row) as u16).invisible() {
                continue;
            }
            return Some((
                (col, row),
                if blob.vy > 0.0 {
                    GelFace::Top
                } else {
                    GelFace::Bottom
                },
            ));
        }
    }
    // Horizontal.
    let (left, right, top, bottom) = cells([nx, blob.y, GEL_SIZE, GEL_SIZE]);
    for col in left..=right {
        for row in top..=bottom {
            if !is_solid(get_tile(level, col, row)) {
                continue;
            }
            if crate::level::tiles::props(get_tile(level, col, row) as u16).invisible() {
                continue;
            }
            return Some((
                (col, row),
                if blob.vx > 0.0 {
                    GelFace::Left
                } else {
                    GelFace::Right
                },
            ));
        }
    }
    None
}

/// Every blob's rect, for the renderer.
pub(crate) fn blob_rect(blob: &GelBlob) -> [f32; 4] {
    blob.rect()
}
