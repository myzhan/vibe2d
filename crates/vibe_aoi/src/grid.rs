//! Uniform-grid backend.
//!
//! The world is partitioned into equal-sized square cells. Each cell
//! holds a `Vec<EntityId>`; large entities that span multiple cells are
//! registered in *every* cell they touch (the per-entity reverse index
//! `entity_cells` records which cells, so updates and removes are
//! O(touched cells) instead of O(all cells)).
//!
//! Queries collect candidates from the cells the query shape touches,
//! deduplicate via a `seen` bitset, and then run the precise
//! `Shape::intersects` test as a narrow-phase check.
//!
//! See `docs/aoi.md` for the rationale on choosing uniform grid over
//! quadtree, and for the `cell_size` heuristic used by
//! [`crate::AoiWorld::new`].

use glam::Vec2;

use crate::shape::Shape;
use crate::world::{AoiStats, EntityId};

const MIN_CELL_SIZE: f32 = 16.0;
const MAX_CELL_SIZE: f32 = 256.0;

enum Slot {
    Live { shape: Shape, cells: Vec<u32> },
    Free,
}

pub(crate) struct UniformGridBackend {
    /// Kept for diagnostics and the test-only `bounds()` accessor.
    /// Not consulted on the query hot path because cells already encode
    /// the world extent.
    #[allow(dead_code)]
    bounds: Vec2,
    cell_size: f32,
    cols: u32,
    rows: u32,
    /// Indexed by entity slot. `cells` lists the cell indices this
    /// entity is currently registered in (so `remove` doesn't have to
    /// scan the whole grid).
    slots: Vec<Slot>,
    free: Vec<u32>,
    live_count: usize,
    /// `cells[idx]` holds entity ids whose AABB touches that cell.
    cells: Vec<Vec<u32>>,
    /// Reusable dedupe scratchpad for queries. Lives on the backend so
    /// repeated queries don't reallocate; sized to `slots.len()`.
    /// Touched indices are tracked in `seen_touched` so we can clear in
    /// O(touched) instead of O(slots.len()).
    seen: Vec<bool>,
    seen_touched: Vec<u32>,
}

impl UniformGridBackend {
    /// Construct a uniform-grid backend with an explicit `cell_size`.
    /// `bounds` is the world's positive-quadrant size; coordinates
    /// outside `[0, bounds]` are still legal but will be quantized to
    /// the edge cells (and queries beyond the world will short-circuit
    /// to empty results).
    pub fn new(bounds: Vec2, cell_size: f32) -> Self {
        // Defensive clamp: a cell_size of 0 or negative would divide by
        // zero in `cell_index`. The actual heuristic for choosing a sane
        // value lives in `default_cell_size`.
        let cell_size = cell_size.max(1.0);
        let cols = (bounds.x / cell_size).ceil().max(1.0) as u32;
        let rows = (bounds.y / cell_size).ceil().max(1.0) as u32;
        let total = (cols as usize) * (rows as usize);
        Self {
            bounds,
            cell_size,
            cols,
            rows,
            slots: Vec::new(),
            free: Vec::new(),
            live_count: 0,
            cells: vec![Vec::new(); total],
            seen: Vec::new(),
            seen_touched: Vec::new(),
        }
    }

    /// Heuristic used by `AoiWorld::new(bounds)`: target ~32 cells across
    /// the larger world dimension, then clamp into a sane absolute range
    /// so a tiny world doesn't get sub-pixel cells and a huge world
    /// doesn't get a single mega-cell.
    pub fn default_cell_size(bounds: Vec2) -> f32 {
        (bounds.max_element() / 32.0).clamp(MIN_CELL_SIZE, MAX_CELL_SIZE)
    }

    pub fn insert(&mut self, shape: Shape) -> EntityId {
        self.live_count += 1;
        let idx = if let Some(idx) = self.free.pop() {
            self.slots[idx as usize] = Slot::Live {
                shape,
                cells: Vec::new(),
            };
            idx
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Slot::Live {
                shape,
                cells: Vec::new(),
            });
            idx
        };
        self.register_cells(idx, shape);
        // Grow seen scratchpad to match slot count.
        if self.seen.len() < self.slots.len() {
            self.seen.resize(self.slots.len(), false);
        }
        EntityId(idx)
    }

    pub fn update(&mut self, id: EntityId, shape: Shape) {
        let i = id.0 as usize;
        let Some(slot) = self.slots.get_mut(i) else {
            return;
        };
        if !matches!(slot, Slot::Live { .. }) {
            return;
        }
        // Same-cell-set fast path is tempting but skipped for clarity;
        // remove + reinsert is O(touched cells) which is already small.
        self.unregister_cells(id.0);
        if let Some(Slot::Live { shape: s, .. }) = self.slots.get_mut(i) {
            *s = shape;
        }
        self.register_cells(id.0, shape);
    }

    pub fn remove(&mut self, id: EntityId) {
        let i = id.0 as usize;
        let Some(slot) = self.slots.get_mut(i) else {
            return;
        };
        if !matches!(slot, Slot::Live { .. }) {
            return;
        }
        self.unregister_cells(id.0);
        self.slots[i] = Slot::Free;
        self.free.push(id.0);
        self.live_count -= 1;
    }

    pub fn get(&self, id: EntityId) -> Option<Shape> {
        match self.slots.get(id.0 as usize) {
            Some(Slot::Live { shape, .. }) => Some(*shape),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.live_count
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityId, Shape)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| match slot {
                Slot::Live { shape, .. } => Some((EntityId(idx as u32), *shape)),
                Slot::Free => None,
            })
    }

    pub fn query_shape(&mut self, query: &Shape) -> Vec<EntityId> {
        let (qmin, qmax) = query.aabb_bounds();
        let (cx0, cy0, cx1, cy1) = self.cell_range(qmin, qmax);

        let mut hits = Vec::new();
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let cidx = (cy * self.cols + cx) as usize;
                for &eid in &self.cells[cidx] {
                    let ui = eid as usize;
                    if self.seen[ui] {
                        continue;
                    }
                    self.seen[ui] = true;
                    self.seen_touched.push(eid);
                    if let Slot::Live { shape, .. } = &self.slots[ui]
                        && query.intersects(shape)
                    {
                        hits.push(EntityId(eid));
                    }
                }
            }
        }
        // Reset only the bits we set — O(touched) clear.
        for eid in self.seen_touched.drain(..) {
            self.seen[eid as usize] = false;
        }
        hits
    }

    pub fn stats(&self) -> AoiStats {
        let mut max_per_cell = 0usize;
        let mut total_per_cell = 0usize;
        for cell in &self.cells {
            max_per_cell = max_per_cell.max(cell.len());
            total_per_cell += cell.len();
        }
        let cell_count = self.cells.len();
        let avg = if cell_count == 0 {
            0.0
        } else {
            total_per_cell as f32 / cell_count as f32
        };
        AoiStats {
            entity_count: self.live_count,
            cell_count,
            max_entities_per_cell: max_per_cell,
            avg_entities_per_cell: avg,
        }
    }

    // ── internals ──────────────────────────────────────────────────

    fn cell_range(&self, min: Vec2, max: Vec2) -> (u32, u32, u32, u32) {
        let cx0 = self.clamp_col(min.x);
        let cy0 = self.clamp_row(min.y);
        let cx1 = self.clamp_col(max.x);
        let cy1 = self.clamp_row(max.y);
        (cx0, cy0, cx1, cy1)
    }

    fn clamp_col(&self, x: f32) -> u32 {
        let raw = (x / self.cell_size).floor();
        if raw < 0.0 {
            0
        } else if raw >= self.cols as f32 {
            self.cols - 1
        } else {
            raw as u32
        }
    }

    fn clamp_row(&self, y: f32) -> u32 {
        let raw = (y / self.cell_size).floor();
        if raw < 0.0 {
            0
        } else if raw >= self.rows as f32 {
            self.rows - 1
        } else {
            raw as u32
        }
    }

    fn register_cells(&mut self, eid: u32, shape: Shape) {
        let (min, max) = shape.aabb_bounds();
        let (cx0, cy0, cx1, cy1) = self.cell_range(min, max);
        let mut touched = Vec::with_capacity(((cx1 - cx0 + 1) * (cy1 - cy0 + 1)) as usize);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let cidx = cy * self.cols + cx;
                self.cells[cidx as usize].push(eid);
                touched.push(cidx);
            }
        }
        if let Slot::Live { cells, .. } = &mut self.slots[eid as usize] {
            *cells = touched;
        }
    }

    fn unregister_cells(&mut self, eid: u32) {
        // Take cells out of the slot first to satisfy the borrow checker
        // before mutating self.cells. We restore an empty Vec into the
        // slot; register_cells will refill it on the next update.
        let cells_to_clear: Vec<u32> = match &mut self.slots[eid as usize] {
            Slot::Live { cells, .. } => std::mem::take(cells),
            Slot::Free => return,
        };
        for cidx in cells_to_clear {
            let bucket = &mut self.cells[cidx as usize];
            if let Some(pos) = bucket.iter().position(|&x| x == eid) {
                bucket.swap_remove(pos);
            }
        }
    }

    /// Test-only accessor for asserting cell occupancy in unit tests.
    #[cfg(test)]
    pub(crate) fn cell_at(&self, col: u32, row: u32) -> &[u32] {
        &self.cells[(row * self.cols + col) as usize]
    }

    /// Test-only accessor exposing world bounds for assertions.
    #[cfg(test)]
    pub(crate) fn bounds(&self) -> Vec2 {
        self.bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cell_size_clamps_to_range() {
        // Tiny world → clamped up to MIN_CELL_SIZE.
        assert_eq!(
            UniformGridBackend::default_cell_size(Vec2::new(50.0, 50.0)),
            MIN_CELL_SIZE
        );
        // Huge world → clamped down to MAX_CELL_SIZE.
        assert_eq!(
            UniformGridBackend::default_cell_size(Vec2::new(100_000.0, 100_000.0)),
            MAX_CELL_SIZE
        );
        // Mid-sized world (~1024 → 32 cells of size 32) → 32.0.
        assert_eq!(
            UniformGridBackend::default_cell_size(Vec2::new(1024.0, 1024.0)),
            32.0
        );
    }

    #[test]
    fn insert_registers_into_correct_cell() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        let id = b.insert(Shape::point(Vec2::new(10.0, 10.0)));
        // Point at (10, 10) with cell_size 25 → cell (0, 0).
        assert!(b.cell_at(0, 0).contains(&id.0));
        assert!(!b.cell_at(1, 0).contains(&id.0));
    }

    #[test]
    fn large_circle_spans_multiple_cells() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        // Circle at (50, 50) radius 30 → AABB (20, 20)-(80, 80) → cells
        // (0..=3, 0..=3) on a 4x4 grid.
        let id = b.insert(Shape::circle(Vec2::new(50.0, 50.0), 30.0));
        let mut count = 0;
        for cx in 0..4 {
            for cy in 0..4 {
                if b.cell_at(cx, cy).contains(&id.0) {
                    count += 1;
                }
            }
        }
        assert!(count >= 4, "expected ≥ 4 cells touched, got {count}");
    }

    #[test]
    fn update_moves_entity_between_cells() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        let id = b.insert(Shape::point(Vec2::new(10.0, 10.0)));
        b.update(id, Shape::point(Vec2::new(80.0, 80.0)));
        assert!(!b.cell_at(0, 0).contains(&id.0));
        assert!(b.cell_at(3, 3).contains(&id.0));
    }

    #[test]
    fn remove_clears_all_cells() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        let id = b.insert(Shape::circle(Vec2::new(50.0, 50.0), 30.0));
        b.remove(id);
        for cx in 0..4 {
            for cy in 0..4 {
                assert!(!b.cell_at(cx, cy).contains(&id.0));
            }
        }
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn query_circle_dedupes_across_cells() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        // Big circle that crosses several cells; querying a region that
        // overlaps multiple of those cells must not return the same id
        // multiple times.
        let id = b.insert(Shape::circle(Vec2::new(50.0, 50.0), 30.0));
        let hits = b.query_shape(&Shape::aabb(Vec2::new(50.0, 50.0), Vec2::splat(40.0)));
        let n = hits.iter().filter(|&&e| e == id).count();
        assert_eq!(n, 1, "id appeared {n} times — dedup is broken");
    }

    #[test]
    fn query_outside_world_is_handled() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        b.insert(Shape::point(Vec2::new(50.0, 50.0)));
        // Query well outside the world — must not panic and must not
        // spuriously match.
        let hits = b.query_shape(&Shape::Aabb {
            center: Vec2::new(500.0, 500.0),
            half_extents: Vec2::splat(10.0),
        });
        assert!(hits.is_empty());
        // Sanity-check bounds() is still the configured value.
        assert_eq!(b.bounds(), Vec2::new(100.0, 100.0));
    }

    #[test]
    fn query_clipped_to_world_still_finds_entities() {
        // A query that *originates* outside but *extends* into the world
        // should still hit entities inside.
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        let id = b.insert(Shape::point(Vec2::new(5.0, 5.0)));
        let hits = b.query_shape(&Shape::Aabb {
            center: Vec2::new(-10.0, -10.0),
            half_extents: Vec2::splat(30.0),
        });
        assert!(hits.contains(&id));
    }

    #[test]
    fn id_is_recycled_after_remove() {
        let mut b = UniformGridBackend::new(Vec2::new(100.0, 100.0), 25.0);
        let a = b.insert(Shape::point(Vec2::ZERO));
        b.remove(a);
        let c = b.insert(Shape::point(Vec2::ZERO));
        assert_eq!(a.0, c.0);
    }
}
