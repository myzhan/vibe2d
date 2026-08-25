//! Linear-scan backend.
//!
//! Stores entities in a `Vec<Slot>` indexed by `EntityId.0`. Removed
//! slots are reused via a free list so ids stay dense, but **id
//! recycling means a stale id can collide with a fresh entity** — see
//! the doc on [`crate::EntityId`].
//!
//! Every query is O(n). This is the right tool for tiny worlds (< ~200
//! entities) and as a reference oracle for testing the more complex
//! `UniformGrid` backend.

use crate::shape::Shape;
use crate::world::{AoiStats, EntityId};

enum Slot {
    Live(Shape),
    Free,
}

pub(crate) struct BruteForceBackend {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live_count: usize,
}

impl BruteForceBackend {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            live_count: 0,
        }
    }

    pub fn insert(&mut self, shape: Shape) -> EntityId {
        self.live_count += 1;
        if let Some(idx) = self.free.pop() {
            self.slots[idx as usize] = Slot::Live(shape);
            EntityId(idx)
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Slot::Live(shape));
            EntityId(idx)
        }
    }

    pub fn update(&mut self, id: EntityId, shape: Shape) {
        if let Some(slot) = self.slots.get_mut(id.0 as usize)
            && matches!(slot, Slot::Live(_))
        {
            *slot = Slot::Live(shape);
        }
    }

    pub fn remove(&mut self, id: EntityId) {
        if let Some(slot) = self.slots.get_mut(id.0 as usize)
            && matches!(slot, Slot::Live(_))
        {
            *slot = Slot::Free;
            self.free.push(id.0);
            self.live_count -= 1;
        }
    }

    pub fn get(&self, id: EntityId) -> Option<Shape> {
        match self.slots.get(id.0 as usize) {
            Some(Slot::Live(s)) => Some(*s),
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
                Slot::Live(s) => Some((EntityId(idx as u32), *s)),
                Slot::Free => None,
            })
    }

    pub fn query_shape(&self, query: &Shape) -> Vec<EntityId> {
        self.iter()
            .filter_map(|(id, s)| if query.intersects(&s) { Some(id) } else { None })
            .collect()
    }

    pub fn stats(&self) -> AoiStats {
        // BruteForce has no spatial cells; report the entity bucket as a
        // single "cell" so VDP visualization still has a meaningful row.
        let count = self.live_count;
        AoiStats {
            entity_count: count,
            cell_count: if count == 0 { 0 } else { 1 },
            max_entities_per_cell: count,
            avg_entities_per_cell: count as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    #[test]
    fn id_is_recycled_after_remove() {
        let mut b = BruteForceBackend::new();
        let a = b.insert(Shape::point(Vec2::ZERO));
        b.remove(a);
        let c = b.insert(Shape::point(Vec2::ZERO));
        assert_eq!(a.0, c.0, "free list should reuse the slot index");
    }

    #[test]
    fn live_count_tracks_inserts_and_removes() {
        let mut b = BruteForceBackend::new();
        let ids: Vec<_> = (0..10)
            .map(|i| b.insert(Shape::point(Vec2::new(i as f32, 0.0))))
            .collect();
        assert_eq!(b.len(), 10);
        for id in ids.iter().take(3) {
            b.remove(*id);
        }
        assert_eq!(b.len(), 7);
    }

    #[test]
    fn double_remove_is_safe() {
        let mut b = BruteForceBackend::new();
        let id = b.insert(Shape::point(Vec2::ZERO));
        b.remove(id);
        b.remove(id); // second remove must not underflow live_count
        assert_eq!(b.len(), 0);
    }
}
