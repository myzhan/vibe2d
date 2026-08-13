//! Tile collision, velocity transforms through portals, and the portal-gun
//! aim trace.
//!
//! Solidity is a lookup into the generated tile table rather than a whitelist —
//! see `level::tiles`. Movement is resolved one axis at a time, matching the
//! original's `physics.lua` split.

use crate::constants::*;
use crate::level;
use crate::player::Orientation;
use crate::world::Level;

/// One frame at the nominal 60 Hz.
///
/// Used where an impulse is conceptually "one frame of acceleration" and the
/// surrounding code has no `dt` in scope.
pub(crate) fn dt_hint() -> f32 {
    1.0 / 60.0
}

// ── Collision helpers ───────────────────────────────────────────────

/// Solidity comes from the generated tile table, not a whitelist.
///
/// The old hardcoded list named 10 tile ids. SMB actually uses **62** colliding
/// tiles, which is the direct reason only 1-1 was playable: every other level is
/// built from tiles the whitelist didn't know about.
pub(crate) fn is_solid(tile_id: u32) -> bool {
    level::tiles::is_solid(tile_id as u16)
}

/// Does this cell block movement *right now*?
///
/// Differs from [`is_solid`] only where a portal has opened a hole. The original
/// deletes the tile's collision object while leaving `map[x][y]` untouched
/// (`modifyportaltiles`), which is why the wall keeps drawing and `getTile` keeps
/// reporting solid — the aim line still stops on a wall you have already opened.
/// Only movement sees the hole.
pub(crate) fn blocks_movement(level: &Level, col: i32, row: i32) -> bool {
    is_solid(get_tile(level, col, row)) && !level.portal_holes.contains(&(col, row))
}

pub(crate) fn get_tile(level: &Level, col: i32, row: i32) -> u32 {
    if row < 0 || col < 0 || row >= level.height as i32 || col >= level.width as i32 {
        return SMB_EMPTY;
    }
    level.tiles[row as usize][col as usize]
}

pub(crate) fn tile_rect(col: i32, row: i32) -> (f32, f32, f32, f32) {
    (
        col as f32 * TILE_SIZE,
        row as f32 * TILE_SIZE,
        TILE_SIZE,
        TILE_SIZE,
    )
}

/// Do two axis-aligned boxes overlap? Each is `[x, y, width, height]`.
pub(crate) fn aabb_overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    let ([ax, ay, aw, ah], [bx, by, bw, bh]) = (a, b);
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
}

pub(crate) fn move_and_collide_x(
    player_x: &mut f32,
    player_y: f32,
    pw: f32,
    ph: f32,
    vx: f32,
    level: &Level,
    dt: f32,
) -> f32 {
    let dx = vx * dt;
    *player_x += dx;

    let left_col = (*player_x / TILE_SIZE).floor() as i32;
    let right_col = ((*player_x + pw - 0.01) / TILE_SIZE).floor() as i32;
    let top_row = (player_y / TILE_SIZE).floor() as i32;
    let bottom_row = ((player_y + ph - 0.01) / TILE_SIZE).floor() as i32;

    for row in top_row..=bottom_row {
        for col in left_col..=right_col {
            if blocks_movement(level, col, row) {
                let (tx, _ty, tw, _th) = tile_rect(col, row);
                if aabb_overlap([*player_x, player_y, pw, ph], [tx, _ty, tw, _th]) {
                    if dx > 0.0 {
                        *player_x = tx - pw;
                    } else if dx < 0.0 {
                        *player_x = tx + tw;
                    }
                    return 0.0;
                }
            }
        }
    }
    vx
}

pub(crate) fn move_and_collide_y(
    player_x: f32,
    player_y: &mut f32,
    pw: f32,
    ph: f32,
    vy: f32,
    level: &Level,
    dt: f32,
) -> (f32, bool) {
    let dy = vy * dt;
    *player_y += dy;

    let left_col = (player_x / TILE_SIZE).floor() as i32;
    let right_col = ((player_x + pw - 0.01) / TILE_SIZE).floor() as i32;
    let top_row = (*player_y / TILE_SIZE).floor() as i32;
    let bottom_row = ((*player_y + ph - 0.01) / TILE_SIZE).floor() as i32;

    let mut on_ground = false;

    for row in top_row..=bottom_row {
        for col in left_col..=right_col {
            if blocks_movement(level, col, row) {
                let (tx, ty, tw, th) = tile_rect(col, row);
                if aabb_overlap([player_x, *player_y, pw, ph], [tx, ty, tw, th]) {
                    if dy > 0.0 {
                        *player_y = ty - ph;
                        on_ground = true;
                    } else if dy < 0.0 {
                        *player_y = ty + th;
                    }
                    return (0.0, on_ground);
                }
            }
        }
    }
    (vy, on_ground)
}

// ── Portal aim-line ray-cast (matches mari0 traceline) ───────────────
/// Where the aim line met a wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AimHit {
    /// Which face of the tile was struck.
    pub(crate) side: Orientation,
    /// The struck tile.
    pub(crate) cell: (i32, i32),
}

/// Trace a line from (sx, sy) in world-pixel coords along `angle` (radians).
/// Always returns (end_x, end_y) even without a wall hit (like original mari0),
/// so dots can always be drawn along the ray.
/// Returns (end_x, end_y, Option<AimHit>).
pub(crate) fn trace_aim_line(
    level: &Level,
    sx: f32,
    sy: f32,
    angle: f32,
    cam_x: f32,
    view_w: f32,
) -> (f32, f32, Option<AimHit>) {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let step = TILE_SIZE * 0.5;
    let max_dist = 40.0 * TILE_SIZE;

    let mut dist = step;
    while dist < max_dist {
        let px = sx + cos_a * dist;
        let py = sy + sin_a * dist;
        let col = (px / TILE_SIZE).floor() as i32;
        let row = (py / TILE_SIZE).floor() as i32;

        // Out of map → return endpoint, no hit (like original)
        if col < 0 || row < 0 || col >= level.width as i32 || row >= level.height as i32 {
            return (px, py, None);
        }

        // Out of visible area (original: x > xscroll+width or x < xscroll)
        if px < cam_x - TILE_SIZE || px > cam_x + view_w + TILE_SIZE {
            return (px, py, None);
        }

        let tile = get_tile(level, col, row);
        if is_solid(tile) {
            let prev_px = sx + cos_a * (dist - step);
            let prev_py = sy + sin_a * (dist - step);
            let prev_col = (prev_px / TILE_SIZE).floor() as i32;
            let prev_row = (prev_py / TILE_SIZE).floor() as i32;

            let orient = if prev_col < col {
                Orientation::Left
            } else if prev_col > col {
                Orientation::Right
            } else if prev_row < row {
                Orientation::Up
            } else {
                Orientation::Down
            };

            let (hx, hy) = match orient {
                Orientation::Left => (col as f32 * TILE_SIZE, py),
                Orientation::Right => ((col + 1) as f32 * TILE_SIZE, py),
                Orientation::Up => (px, row as f32 * TILE_SIZE),
                Orientation::Down => (px, (row + 1) as f32 * TILE_SIZE),
            };

            // Only the geometry is reported. Whether a portal can actually be
            // *placed* needs the two-tile span test, which the caller runs — the
            // original does the same, calling `getportalposition` from its UI code
            // (`game.lua:1608`) so the crosshair can't promise a shot that will
            // silently fail.
            return (
                hx,
                hy,
                Some(AimHit {
                    side: orient,
                    cell: (col, row),
                }),
            );
        }
        dist += step;
    }
    // Max distance, no hit
    (sx + cos_a * max_dist, sy + sin_a * max_dist, None)
}
