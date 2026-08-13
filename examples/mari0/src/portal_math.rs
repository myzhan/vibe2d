//! Portal placement and the coordinate transform, as pure functions.
//!
//! Both are ports of code with no general case: `getportalposition`
//! (`game.lua:3076`) tries two candidate spans against four conditions each, and
//! `portalcoords` (`physics.lua:715`) hand-writes all **sixteen** entry/exit
//! orientation pairs. There is no matrix hiding in there, and the reason is the
//! anchor: it is normalised differently per face, so every pair needs its own ±1.
//!
//! ## Units
//!
//! The original works in blocks with 1-based tile indices where tile row `r`
//! occupies `y ∈ [r-1, r)`. This port uses 32px blocks with 0-based indices where
//! row `r` occupies `[r, r+1)` blocks. Those two conventions put the *same physical
//! point* at the *same block coordinate* — only the tile index differs, by exactly
//! one. So the arithmetic below is the Lua verbatim, with anchors converted by `+1`
//! on the way in, and positions merely scaled by `TILE_SIZE`.
//!
//! Checked against `directrange`, which must come out positive on the outside of
//! the portal for all four facings; it does, for all four, under this mapping.

use std::f32::consts::PI;

use crate::constants::*;
use crate::player::Orientation;
use crate::world::Level;

/// Where a portal is mounted: the tile it is anchored to, and the face it sits on.
///
/// The anchor is **normalised by face**, exactly as `getportalposition` returns it:
/// an up-facing portal reports the *lower* column of the two it spans, a
/// down-facing one the *higher*, a right-facing one the *lower* row and a
/// left-facing one the *higher*. That asymmetry is not tidiness — it is what the
/// ±1 constants in the transform are compensating for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PortalAnchor {
    pub(crate) cell: (i32, i32),
    pub(crate) facing: Orientation,
}

impl PortalAnchor {
    /// The two cells the portal covers.
    pub(crate) fn cells(self) -> [(i32, i32); 2] {
        let (c, r) = self.cell;
        match self.facing {
            Orientation::Up => [(c, r), (c + 1, r)],
            Orientation::Down => [(c - 1, r), (c, r)],
            Orientation::Right => [(c, r), (c, r + 1)],
            Orientation::Left => [(c, r - 1), (c, r)],
        }
    }

    /// Centre of the portal's mouth, in world pixels.
    ///
    /// Used for drawing and for the overlap test; the transform works from the
    /// anchor directly.
    pub(crate) fn mouth_centre(self) -> (f32, f32) {
        let (c, r) = (self.cell.0 as f32, self.cell.1 as f32);
        match self.facing {
            // Top face of the anchor row, spanning two columns to the right.
            Orientation::Up => ((c + 1.0) * TILE_SIZE, r * TILE_SIZE),
            // Bottom face, spanning two columns to the left.
            Orientation::Down => (c * TILE_SIZE, (r + 1.0) * TILE_SIZE),
            // Right face, spanning two rows down.
            Orientation::Right => ((c + 1.0) * TILE_SIZE, (r + 1.0) * TILE_SIZE),
            // Left face, spanning two rows up.
            Orientation::Left => (c * TILE_SIZE, r * TILE_SIZE),
        }
    }

    /// Recover the anchor from a mouth centre — the inverse of `mouth_centre`.
    ///
    /// Exists so `game.setPortal` can keep taking pixel coordinates: that is the
    /// wire format `tests/vdp_full_test.py` already asserts on, and changing it
    /// would be a breaking change to the test suite for no gain. Gameplay always
    /// goes the other way, from a struck tile, so this compiles away with the
    /// feature.
    #[cfg(any(feature = "vdp", test))]
    pub(crate) fn from_mouth_centre(x: f32, y: f32, facing: Orientation) -> Self {
        let (cx, cy) = (x / TILE_SIZE, y / TILE_SIZE);
        let cell = match facing {
            Orientation::Up => (cx.round() as i32 - 1, cy.round() as i32),
            Orientation::Down => (cx.round() as i32, cy.round() as i32 - 1),
            Orientation::Right => (cx.round() as i32 - 1, cy.round() as i32 - 1),
            Orientation::Left => (cx.round() as i32, cy.round() as i32),
        };
        Self { cell, facing }
    }
}

/// Result of moving a body through a portal pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Exit {
    /// Top-left corner, world pixels.
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) rotation: f32,
}

/// Move a body from `entry` to `exit`, transforming position, velocity and rotation.
///
/// `live` enables the minimum exit speed, which only applies to the three
/// combinations that exit **upward** (`up→up`, `left→up`, `right→up`). It is
/// `sqrt(2 * gravity * height)` — exactly the speed needed to rise the body's own
/// height — so a body can never be left half-embedded in the floor it emerged from.
///
/// Magnitudes are otherwise **conserved exactly**: no combination scales speed.
/// That is what makes the classic infinite fall between two facing portals build up
/// speed, so it needs no special-casing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn portal_transform(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    vx: f32,
    vy: f32,
    rotation: f32,
    entry: PortalAnchor,
    exit: PortalAnchor,
    gravity: f32,
    live: bool,
) -> Exit {
    // Into the original's units: blocks, and 1-based tile indices for anchors.
    let (w, h) = (width / TILE_SIZE, height / TILE_SIZE);
    // `portalcoords` starts by moving to the body's centre and ends by moving back.
    let cx = x / TILE_SIZE + w / 2.0;
    let cy = y / TILE_SIZE + h / 2.0;
    let (mut sx, mut sy) = (vx / TILE_SIZE, vy / TILE_SIZE);
    let mut rot = rotation;
    let grav = gravity / TILE_SIZE;

    let (en_x, en_y) = (entry.cell.0 as f32 + 1.0, entry.cell.1 as f32 + 1.0);
    let (ex_x, ex_y) = (exit.cell.0 as f32 + 1.0, exit.cell.1 as f32 + 1.0);

    // Signed depth along the entry portal's normal; positive outside the wall.
    let direct = match entry.facing {
        Orientation::Up => en_y - cy - 1.0,
        Orientation::Right => cx - en_x,
        Orientation::Down => cy - en_y,
        Orientation::Left => en_x - cx - 1.0,
    };

    // Normalised position across the mouth, 0..1. Consumed only by the eight
    // perpendicular combinations; the parallel ones keep the original offset
    // instead. Degenerate when the body is exactly two blocks across, which is why
    // the original special-cases that to 0 rather than dividing by zero.
    let relative = match entry.facing {
        Orientation::Up => {
            if w == 2.0 {
                0.0
            } else {
                ((cx - w / 2.0) - en_x + 1.0) / (2.0 - w)
            }
        }
        Orientation::Right => {
            if h == 2.0 {
                0.0
            } else {
                ((cy - h / 2.0) - en_y + 1.0) / (2.0 - h)
            }
        }
        Orientation::Down => {
            if w == 2.0 {
                0.0
            } else {
                ((cx - w / 2.0) - en_x + 2.0) / (2.0 - w)
            }
        }
        Orientation::Left => {
            if h == 2.0 {
                0.0
            } else {
                ((cy - h / 2.0) - en_y + 2.0) / (2.0 - h)
            }
        }
    };

    // Rising by your own height, in blocks/s.
    let min_up = |h: f32| (2.0 * grav * h).sqrt();

    use Orientation::*;
    let (mut nx, mut ny);
    match (entry.facing, exit.facing) {
        // ── Parallel and antiparallel ───────────────────────────────
        (Up, Up) => {
            nx = cx + (ex_x - en_x);
            ny = ex_y + direct - 1.0;
            sy = -sy;
            rot -= PI;
            if live && sy > -min_up(h) {
                sy = -min_up(h);
            }
        }
        (Down, Down) => {
            nx = cx + (ex_x - en_x);
            ny = ex_y - direct;
            sy = -sy;
            rot -= PI;
        }
        // Speed untouched: this is the pair that lets a fall accelerate forever.
        (Up, Down) => {
            nx = cx + (ex_x - en_x) - 1.0;
            ny = ex_y - direct;
            // Cheap low-frame-rate guard, reproduced: keeps the body from being
            // placed where the next step would put it back inside the entry portal.
            if en_y > ex_y {
                let step = sy * (1.0 / 60.0);
                while ny + 0.5 + step > en_y {
                    ny -= 0.01;
                }
                while ny + 0.5 < ex_y {
                    ny += 0.01;
                }
            }
            // And a clamp so it can't be shoved into the wall sideways.
            nx = nx.clamp(ex_x - 2.0 + w / 2.0, ex_x - w / 2.0);
        }
        (Down, Up) => {
            nx = cx + (ex_x - en_x) + 1.0;
            ny = ex_y + direct - 1.0;
        }
        (Left, Right) => {
            nx = ex_x - direct;
            ny = cy + (ex_y - en_y) + 1.0;
        }
        (Right, Left) => {
            nx = ex_x + direct - 1.0;
            ny = cy + (ex_y - en_y) - 1.0;
        }
        (Right, Right) => {
            nx = ex_x - direct;
            ny = cy + (ex_y - en_y);
            sx = -sx;
        }
        (Left, Left) => {
            nx = ex_x + direct - 1.0;
            ny = cy + (ex_y - en_y);
            sx = -sx;
        }
        // ── Perpendicular ───────────────────────────────────────────
        // The two swap forms are **not** interchangeable: `(sy, -sx)` pairs with
        // `rot -= π/2` and `(-sy, sx)` with `rot += π/2`. Mixing them mirrors the
        // launch direction, which reads as the portal firing you the wrong way.
        (Up, Right) => {
            ny = ex_y - relative * (2.0 - h) - h / 2.0 + 1.0;
            nx = ex_x - direct;
            (sx, sy) = (sy, -sx);
            rot -= PI / 2.0;
        }
        (Up, Left) => {
            ny = ex_y + relative * (2.0 - h) + h / 2.0 - 2.0;
            nx = ex_x + direct - 1.0;
            (sx, sy) = (-sy, sx);
            rot += PI / 2.0;
        }
        (Down, Left) => {
            ny = ex_y - relative * (2.0 - h) - h / 2.0;
            nx = ex_x + direct - 1.0;
            (sx, sy) = (sy, -sx);
            rot -= PI / 2.0;
        }
        (Down, Right) => {
            ny = ex_y + relative * (2.0 - h) + h / 2.0 - 1.0;
            nx = ex_x - direct;
            (sx, sy) = (-sy, sx);
            rot += PI / 2.0;
        }
        (Left, Up) => {
            nx = ex_x + relative * (2.0 - w) + w / 2.0 - 1.0;
            ny = ex_y + direct - 1.0;
            (sx, sy) = (sy, -sx);
            rot -= PI / 2.0;
            if live && sy > -min_up(h) {
                sy = -min_up(h);
            }
        }
        (Right, Up) => {
            nx = ex_x - relative * (2.0 - w) - w / 2.0 + 1.0;
            ny = ex_y + direct - 1.0;
            (sx, sy) = (-sy, sx);
            rot += PI / 2.0;
            if live && sy > -min_up(h) {
                sy = -min_up(h);
            }
        }
        (Left, Down) => {
            nx = ex_x - relative * (2.0 - w) - w / 2.0;
            ny = ex_y - direct;
            (sx, sy) = (-sy, sx);
            rot += PI / 2.0;
        }
        (Right, Down) => {
            nx = ex_x + relative * (2.0 - w) + w / 2.0 - 2.0;
            ny = ex_y - direct;
            (sx, sy) = (sy, -sx);
            rot -= PI / 2.0;
        }
    }

    // Back to a top-left corner, and back to pixels.
    Exit {
        x: (nx - w / 2.0) * TILE_SIZE,
        y: (ny - h / 2.0) * TILE_SIZE,
        vx: sx * TILE_SIZE,
        vy: sy * TILE_SIZE,
        rotation: rot,
    }
}

/// Where a portal lands when a shot strikes `hit` on face `side`.
///
/// A portal is one tile wide and **two tiles long**, mounted flush. Two candidate
/// spans are tried — the one on each side of the impact — in an order decided by
/// `tendency`, which is `+1` when the shot landed past the middle of the tile and
/// `-1` otherwise (`game.lua:3665-3676`). Each candidate must satisfy **four**
/// conditions: both backing tiles solid and portal-accepting, and both cells in
/// front of them empty.
///
/// If neither candidate qualifies the shot **fails silently** — no sound, no
/// message, no portal. That is the original's behaviour and it is load-bearing for
/// how the lab levels are designed.
pub(crate) fn portal_position(
    level: &Level,
    hit: (i32, i32),
    side: Orientation,
    tendency: i32,
    existing: &[Option<PortalAnchor>; 2],
    ignore: usize,
) -> Option<(i32, i32)> {
    let occupied = |cell: (i32, i32)| {
        existing
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != ignore)
            .filter_map(|(_, a)| *a)
            .any(|a| a.cells().contains(&cell))
    };
    // A backing tile must be solid, accept portals, and not already be host to the
    // other portal.
    let backing = |cell: (i32, i32)| {
        let tile = level_tile(level, cell);
        crate::level::tiles::is_solid(tile)
            && crate::level::tiles::props(tile).portalable()
            && !occupied(cell)
    };
    let clear = |cell: (i32, i32)| !crate::level::tiles::is_solid(level_tile(level, cell));

    match side {
        Orientation::Up | Orientation::Down => {
            let front_dy = if side == Orientation::Up { -1 } else { 1 };
            let pairs = if tendency == -1 {
                [(hit.0 - 1, hit.0), (hit.0, hit.0 + 1)]
            } else {
                [(hit.0, hit.0 + 1), (hit.0 - 1, hit.0)]
            };
            for (c0, c1) in pairs {
                if backing((c0, hit.1))
                    && backing((c1, hit.1))
                    && clear((c0, hit.1 + front_dy))
                    && clear((c1, hit.1 + front_dy))
                {
                    // Up reports the lower column, down the higher.
                    let col = if side == Orientation::Up { c0 } else { c1 };
                    return Some((col, hit.1));
                }
            }
        }
        Orientation::Left | Orientation::Right => {
            let front_dx = if side == Orientation::Right { 1 } else { -1 };
            let pairs = if tendency == -1 {
                [(hit.1 - 1, hit.1), (hit.1, hit.1 + 1)]
            } else {
                [(hit.1, hit.1 + 1), (hit.1 - 1, hit.1)]
            };
            for (r0, r1) in pairs {
                if backing((hit.0, r0))
                    && backing((hit.0, r1))
                    && clear((hit.0 + front_dx, r0))
                    && clear((hit.0 + front_dx, r1))
                {
                    // Right reports the lower row, left the higher.
                    let row = if side == Orientation::Right { r0 } else { r1 };
                    return Some((hit.0, row));
                }
            }
        }
    }
    None
}

/// Tile id at a cell, treating everything outside the level as empty.
fn level_tile(level: &Level, (col, row): (i32, i32)) -> u16 {
    if col < 0 || row < 0 || col as usize >= level.width || row as usize >= level.height {
        return crate::level::tiles::TILE_EMPTY;
    }
    level.tiles[row as usize][col as usize] as u16
}

/// Which half of the struck tile the shot landed in.
///
/// `+1` past the midpoint, `-1` before it. Decides which of the two candidate spans
/// is tried first, so it is what makes a portal appear on the side you aimed at.
pub(crate) fn tendency_for(hit_x: f32, hit_y: f32, side: Orientation) -> i32 {
    let fract = match side {
        Orientation::Up | Orientation::Down => (hit_x / TILE_SIZE).fract(),
        Orientation::Left | Orientation::Right => (hit_y / TILE_SIZE).fract(),
    };
    if fract > 0.5 { 1 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = GRAVITY;

    fn anchor(col: i32, row: i32, facing: Orientation) -> PortalAnchor {
        PortalAnchor {
            cell: (col, row),
            facing,
        }
    }

    /// Small Mario: one block square.
    fn transform(
        pos: (f32, f32),
        vel: (f32, f32),
        entry: PortalAnchor,
        exit: PortalAnchor,
    ) -> Exit {
        portal_transform(
            pos.0, pos.1, TILE_SIZE, TILE_SIZE, vel.0, vel.1, 0.0, entry, exit, G, false,
        )
    }

    #[test]
    fn a_portal_covers_two_cells_normalised_by_face() {
        assert_eq!(anchor(5, 9, Orientation::Up).cells(), [(5, 9), (6, 9)]);
        assert_eq!(anchor(6, 9, Orientation::Down).cells(), [(5, 9), (6, 9)]);
        assert_eq!(anchor(5, 9, Orientation::Right).cells(), [(5, 9), (5, 10)]);
        assert_eq!(anchor(5, 10, Orientation::Left).cells(), [(5, 9), (5, 10)]);
    }

    /// The pairs that leave velocity alone. This is what makes an endless fall
    /// between two facing portals keep accelerating, with no code for it.
    #[test]
    fn opposed_faces_conserve_velocity_exactly() {
        for (entry, exit) in [
            (Orientation::Up, Orientation::Down),
            (Orientation::Down, Orientation::Up),
            (Orientation::Left, Orientation::Right),
            (Orientation::Right, Orientation::Left),
        ] {
            let out = transform(
                (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
                (137.0, -412.0),
                anchor(10, 6, entry),
                anchor(20, 12, exit),
            );
            assert_eq!(
                (out.vx, out.vy),
                (137.0, -412.0),
                "{entry:?}→{exit:?} must not touch velocity"
            );
        }
    }

    /// Same-facing pairs mirror the axis they share and nothing else.
    #[test]
    fn same_facing_pairs_mirror_one_axis() {
        let up = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (137.0, -412.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Up),
        );
        assert_eq!((up.vx, up.vy), (137.0, 412.0), "up→up flips vy");

        let right = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (137.0, -412.0),
            anchor(10, 6, Orientation::Right),
            anchor(20, 12, Orientation::Right),
        );
        assert_eq!(
            (right.vx, right.vy),
            (-137.0, -412.0),
            "right→right flips vx"
        );
    }

    /// Speed magnitude survives every one of the sixteen combinations. No branch
    /// scales it, and the whole feel of the gun depends on that.
    #[test]
    fn every_combination_conserves_speed() {
        let faces = [
            Orientation::Up,
            Orientation::Down,
            Orientation::Left,
            Orientation::Right,
        ];
        let (vx, vy) = (137.0_f32, -412.0_f32);
        let speed = (vx * vx + vy * vy).sqrt();
        for entry in faces {
            for exit in faces {
                let out = transform(
                    (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
                    (vx, vy),
                    anchor(10, 6, entry),
                    anchor(20, 12, exit),
                );
                let got = (out.vx * out.vx + out.vy * out.vy).sqrt();
                assert!(
                    (got - speed).abs() < 0.01,
                    "{entry:?}→{exit:?} changed speed: {speed} → {got}"
                );
            }
        }
    }

    /// The two perpendicular swap forms must stay distinct. If they were merged,
    /// half the combinations would launch mirrored — the bug this pins.
    #[test]
    fn the_two_perpendicular_swaps_are_not_interchangeable() {
        // up→right uses (sy, -sx); up→left uses (-sy, sx). With the same input they
        // must disagree on both components.
        let a = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (100.0, -200.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Right),
        );
        let b = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (100.0, -200.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Left),
        );
        assert_eq!((a.vx, a.vy), (-200.0, -100.0));
        assert_eq!((b.vx, b.vy), (200.0, 100.0));
    }

    /// Only the three upward exits get a minimum speed, and it is exactly enough to
    /// rise the body's own height.
    #[test]
    fn upward_exits_get_a_minimum_speed() {
        let height = TILE_SIZE;
        // In blocks: sqrt(2 * (G/TILE) * 1). Back to px/s.
        let expected = (2.0 * (G / TILE_SIZE) * 1.0).sqrt() * TILE_SIZE;

        for (entry, exit) in [
            (Orientation::Up, Orientation::Up),
            (Orientation::Left, Orientation::Up),
            (Orientation::Right, Orientation::Up),
        ] {
            let out = portal_transform(
                10.0 * TILE_SIZE,
                4.0 * TILE_SIZE,
                TILE_SIZE,
                height,
                0.0,
                0.0,
                0.0,
                anchor(10, 6, entry),
                anchor(20, 12, exit),
                G,
                true,
            );
            assert!(
                (out.vy + expected).abs() < 0.01,
                "{entry:?}→{exit:?} should be boosted to {expected}, got {}",
                out.vy
            );
        }
    }

    /// The minimum is a floor applied *after* the flip, not an assignment.
    ///
    /// Two cases, and the ordering is what distinguishes them. `up→up` mirrors `vy`
    /// first, so entering upward leaves downward and the floor then forces it back
    /// up to exactly the minimum; entering downward leaves upward already faster
    /// than the minimum and is left alone.
    #[test]
    fn the_minimum_speed_is_a_floor_applied_after_the_flip() {
        let fast = 5000.0;
        let min_px = (2.0 * (G / TILE_SIZE) * 1.0).sqrt() * TILE_SIZE;

        let rising = portal_transform(
            10.0 * TILE_SIZE,
            4.0 * TILE_SIZE,
            TILE_SIZE,
            TILE_SIZE,
            0.0,
            -fast,
            0.0,
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Up),
            G,
            true,
        );
        assert!(
            (rising.vy + min_px).abs() < 0.01,
            "entering upward: flipped to downward, then floored back up to {min_px}, got {}",
            rising.vy
        );

        let falling = portal_transform(
            10.0 * TILE_SIZE,
            4.0 * TILE_SIZE,
            TILE_SIZE,
            TILE_SIZE,
            0.0,
            fast,
            0.0,
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Up),
            G,
            true,
        );
        assert_eq!(
            falling.vy, -fast,
            "entering downward: already faster than the floor, untouched"
        );
    }

    /// Without `live` there is no floor at all — that is how projectiles and other
    /// non-player bodies go through.
    #[test]
    fn the_minimum_speed_only_applies_when_live() {
        let out = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (0.0, 0.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Up),
        );
        assert_eq!(out.vy, 0.0, "a dead transform leaves a still body still");
    }

    /// Rotation accumulates by the documented quarter and half turns.
    #[test]
    fn rotation_follows_the_turn() {
        let half = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (0.0, 0.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Up),
        );
        assert!((half.rotation + PI).abs() < 1e-5, "up→up turns half around");

        let quarter = transform(
            (10.0 * TILE_SIZE, 4.0 * TILE_SIZE),
            (0.0, 0.0),
            anchor(10, 6, Orientation::Up),
            anchor(20, 12, Orientation::Right),
        );
        assert!((quarter.rotation + PI / 2.0).abs() < 1e-5);
    }

    /// The mouth centre and the anchor must round-trip, in both directions, for
    /// every face — `game.setPortal` relies on it.
    #[test]
    fn anchor_and_mouth_centre_round_trip() {
        for facing in [
            Orientation::Up,
            Orientation::Down,
            Orientation::Left,
            Orientation::Right,
        ] {
            for cell in [(5, 9), (0, 0), (42, 13)] {
                let a = PortalAnchor { cell, facing };
                let (x, y) = a.mouth_centre();
                assert_eq!(
                    PortalAnchor::from_mouth_centre(x, y, facing),
                    a,
                    "{facing:?} at {cell:?} did not round-trip via ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn tendency_splits_the_tile_at_its_midpoint() {
        // 0.25 into the tile → before the midpoint.
        assert_eq!(
            tendency_for(10.25 * TILE_SIZE, 0.0, Orientation::Up),
            -1,
            "left half"
        );
        assert_eq!(
            tendency_for(10.75 * TILE_SIZE, 0.0, Orientation::Up),
            1,
            "right half"
        );
        assert_eq!(tendency_for(0.0, 4.75 * TILE_SIZE, Orientation::Left), 1);
    }
}
