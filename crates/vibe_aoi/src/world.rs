//! High-level [`AoiWorld`] API. Wraps a backend implementation behind a
//! single concrete type so games never have to write a generic parameter
//! or a trait object — pick a backend at construction time and forget it.

use std::collections::HashSet;

use glam::Vec2;

use crate::bruteforce::BruteForceBackend;
use crate::grid::UniformGridBackend;
use crate::observer::{AoiEvent, Observer, ObserverId};
use crate::raycast::ray_vs_shape;
use crate::shape::Shape;

/// Stable handle for an entity inside an [`AoiWorld`].
///
/// IDs are recycled after [`AoiWorld::remove`], so don't compare them
/// across remove/insert cycles. Map them to your game-side entity types
/// in whatever way is convenient (e.g. a `HashMap<EntityId, Enemy>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(transparent))]
pub struct EntityId(pub u32);

/// Performance + occupancy snapshot, primarily for VDP visualization
/// and tuning `cell_size` in production.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
pub struct AoiStats {
    pub entity_count: usize,
    pub cell_count: usize,
    pub max_entities_per_cell: usize,
    pub avg_entities_per_cell: f32,
}

/// Result of a successful [`AoiWorld::raycast`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
pub struct RaycastHit {
    pub entity: EntityId,
    /// Distance from the ray origin to the hit point along the
    /// (normalized) direction vector.
    pub distance: f32,
}

/// User-supplied predicate for filtering AOI hits.
///
/// Called once per candidate hit *after* the broadphase narrows down
/// the set, so the cost per call is the closure body itself — keep it
/// branch-light.
///
/// Arguments:
/// 1. `EntityId` — the candidate entity's stable handle.
/// 2. `&Shape`   — the candidate entity's geometric footprint.
/// 3. `&Shape`   — the *query region* for one-shot queries, or the
///    *observer region* for observer-attached filters. Use this when
///    the filter needs to know "where the camera/observer is" — for
///    example to skip entities beyond a LOD radius:
///
///    ```ignore
///    |id, shape, region| {
///        let (min, max)   = shape.aabb_bounds();
///        let entity_pos   = (min + max) * 0.5;
///        let (rmin, rmax) = region.aabb_bounds();
///        let region_pos   = (rmin + rmax) * 0.5;
///        entity_pos.distance(region_pos) < 80.0
///    }
///    ```
///
/// Closures are stored as `Box<dyn Fn>` (not `FnMut`), so they can't
/// hold mutable state. If you need per-frame counters or rotation,
/// drive that from outside the filter and capture immutable references
/// to it through `Arc<RwLock<…>>`.
///
/// `Send + Sync` is required because [`AoiWorld`] doesn't pin itself
/// to a single thread; future workers may move it across thread
/// boundaries.
pub type AoiFilter = dyn Fn(EntityId, &Shape, &Shape) -> bool + Send + Sync;

/// The spatial-query world. All entities and queries live here.
///
/// Construction picks a backend:
///
/// - [`AoiWorld::new`] — uniform grid sized for `bounds`, with
///   `cell_size` chosen automatically. The default for most games.
/// - [`AoiWorld::with_grid`] — uniform grid with an explicit `cell_size`
///   (advanced; tune when entity sizes are unusual).
/// - [`AoiWorld::with_bruteforce`] — linear scan, ideal below ~200
///   entities or as a reference oracle in tests.
pub struct AoiWorld {
    backend: Backend,
    /// Observer registry. Indexed by `ObserverId.0`. Slot reuse via
    /// `free_observers` keeps ids dense.
    observers: Vec<Option<Observer>>,
    free_observers: Vec<u32>,
}

enum Backend {
    BruteForce(BruteForceBackend),
    Grid(UniformGridBackend),
}

impl AoiWorld {
    /// Construct an [`AoiWorld`] backed by a uniform grid sized for the
    /// given world `bounds` (positive-quadrant size). `cell_size` is
    /// chosen automatically — see `docs/aoi.md` for the heuristic.
    pub fn new(bounds: Vec2) -> Self {
        let cell_size = UniformGridBackend::default_cell_size(bounds);
        Self::with_grid(bounds, cell_size)
    }

    /// Construct an [`AoiWorld`] backed by a uniform grid with an
    /// explicit `cell_size`. Useful when your entities have unusually
    /// uniform/non-uniform sizes and the default heuristic doesn't fit.
    pub fn with_grid(bounds: Vec2, cell_size: f32) -> Self {
        Self::from_backend(Backend::Grid(UniformGridBackend::new(bounds, cell_size)))
    }

    /// Construct an [`AoiWorld`] backed by linear scan. Best for small
    /// worlds (< ~200 entities) or as a baseline reference in tests.
    pub fn with_bruteforce() -> Self {
        Self::from_backend(Backend::BruteForce(BruteForceBackend::new()))
    }

    fn from_backend(backend: Backend) -> Self {
        Self {
            backend,
            observers: Vec::new(),
            free_observers: Vec::new(),
        }
    }

    // ── Entity management ──────────────────────────────────────────

    /// Register a new entity and return its handle. The same shape can
    /// later be replaced via [`AoiWorld::update`].
    pub fn insert(&mut self, shape: Shape) -> EntityId {
        match &mut self.backend {
            Backend::BruteForce(b) => b.insert(shape),
            Backend::Grid(g) => g.insert(shape),
        }
    }

    /// Replace an entity's shape. No-op if the id has been removed.
    pub fn update(&mut self, id: EntityId, shape: Shape) {
        match &mut self.backend {
            Backend::BruteForce(b) => b.update(id, shape),
            Backend::Grid(g) => g.update(id, shape),
        }
    }

    /// Remove an entity. Subsequent queries will not return it.
    pub fn remove(&mut self, id: EntityId) {
        match &mut self.backend {
            Backend::BruteForce(b) => b.remove(id),
            Backend::Grid(g) => g.remove(id),
        }
    }

    /// Look up the current shape of an entity.
    pub fn get(&self, id: EntityId) -> Option<Shape> {
        match &self.backend {
            Backend::BruteForce(b) => b.get(id),
            Backend::Grid(g) => g.get(id),
        }
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        match &self.backend {
            Backend::BruteForce(b) => b.len(),
            Backend::Grid(g) => g.len(),
        }
    }

    /// True iff `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over all live `(id, shape)` pairs in unspecified order.
    pub fn iter(&self) -> Box<dyn Iterator<Item = (EntityId, Shape)> + '_> {
        match &self.backend {
            Backend::BruteForce(b) => Box::new(b.iter()),
            Backend::Grid(g) => Box::new(g.iter()),
        }
    }

    // ── One-shot queries ───────────────────────────────────────────
    //
    // These take `&mut self` because the uniform-grid backend uses an
    // internal scratchpad to deduplicate hits across cells. Games that
    // need to query from a `&Context`-style read-only handle should
    // wrap their `AoiWorld` in a `RefCell`.

    /// Entities whose AABB bounds touch the rectangle `[min, max]`.
    /// Order is unspecified.
    pub fn query_aabb(&mut self, min: Vec2, max: Vec2) -> Vec<EntityId> {
        let half = (max - min) * 0.5;
        let center = (max + min) * 0.5;
        self.query_shape(&Shape::Aabb {
            center,
            half_extents: half,
        })
    }

    /// Entities intersecting the given circle.
    pub fn query_circle(&mut self, center: Vec2, radius: f32) -> Vec<EntityId> {
        self.query_shape(&Shape::Circle { center, radius })
    }

    /// Entities containing the given point. Useful for mouse picking.
    pub fn query_point(&mut self, p: Vec2) -> Vec<EntityId> {
        self.query_shape(&Shape::Point(p))
    }

    fn query_shape(&mut self, query: &Shape) -> Vec<EntityId> {
        match &mut self.backend {
            Backend::BruteForce(b) => b.query_shape(query),
            Backend::Grid(g) => g.query_shape(query),
        }
    }

    // ── Filtered queries ───────────────────────────────────────────
    //
    // These mirror the unfiltered `query_*` series but apply a
    // user-supplied predicate (see [`AoiFilter`]) after the broadphase.
    // Use them for type filtering ("only hit enemies"), LOD culling
    // ("skip distant objects to save bandwidth"), or any other
    // game-side classification that doesn't belong in the AOI library
    // itself.
    //
    // The predicate receives the query shape as its third argument so
    // distance-based filters can compute "distance from query origin".

    /// AABB query with a per-hit predicate. See [`AoiFilter`] for the
    /// closure signature and rationale.
    pub fn query_aabb_filtered<F>(&mut self, min: Vec2, max: Vec2, filter: F) -> Vec<EntityId>
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool,
    {
        let half = (max - min) * 0.5;
        let center = (max + min) * 0.5;
        let region = Shape::Aabb {
            center,
            half_extents: half,
        };
        self.query_shape_filtered(&region, filter)
    }

    /// Circle query with a per-hit predicate. See [`AoiFilter`].
    pub fn query_circle_filtered<F>(
        &mut self,
        center: Vec2,
        radius: f32,
        filter: F,
    ) -> Vec<EntityId>
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool,
    {
        let region = Shape::Circle { center, radius };
        self.query_shape_filtered(&region, filter)
    }

    /// Point query with a per-hit predicate. See [`AoiFilter`].
    pub fn query_point_filtered<F>(&mut self, p: Vec2, filter: F) -> Vec<EntityId>
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool,
    {
        let region = Shape::Point(p);
        self.query_shape_filtered(&region, filter)
    }

    fn query_shape_filtered<F>(&mut self, region: &Shape, filter: F) -> Vec<EntityId>
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool,
    {
        // Apply the predicate after the backend's broadphase. We need
        // the candidate's Shape to feed the filter, so we re-look-up
        // each id — cheap because backends already have O(1) lookup
        // for live ids.
        let candidates = self.query_shape(region);
        candidates
            .into_iter()
            .filter(|id| match self.get(*id) {
                Some(shape) => filter(*id, &shape, region),
                None => false,
            })
            .collect()
    }

    // ── Raycast ───────────────────────────────────────────────────

    /// Cast a ray from `origin` along `dir` (need not be normalized) for
    /// at most `max_dist` units. Returns the closest entity hit.
    ///
    /// The implementation iterates every entity (broadphase opportunity
    /// noted in `docs/aoi.md`); the workloads Vibe2D targets stay well
    /// under 10⁵ entities, so the simpler code wins for now.
    pub fn raycast(&self, origin: Vec2, dir: Vec2, max_dist: f32) -> Option<RaycastHit> {
        let dir_n = dir.normalize_or_zero();
        if dir_n == Vec2::ZERO {
            return None;
        }
        let mut best: Option<RaycastHit> = None;
        for (id, shape) in self.iter() {
            if let Some(t) = ray_vs_shape(origin, dir_n, max_dist, &shape)
                && best.map(|h| t < h.distance).unwrap_or(true)
            {
                best = Some(RaycastHit {
                    entity: id,
                    distance: t,
                });
            }
        }
        best
    }

    /// Like [`AoiWorld::raycast`] but skips any entity for which
    /// `filter(id, &shape, &ray_as_point)` returns `false`. The
    /// "region" passed to the predicate is `Shape::Point(origin)` —
    /// useful when the filter wants to know where the ray started
    /// (e.g. ignore entities behind the shooter).
    pub fn raycast_filtered<F>(
        &self,
        origin: Vec2,
        dir: Vec2,
        max_dist: f32,
        filter: F,
    ) -> Option<RaycastHit>
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool,
    {
        let dir_n = dir.normalize_or_zero();
        if dir_n == Vec2::ZERO {
            return None;
        }
        let region = Shape::Point(origin);
        let mut best: Option<RaycastHit> = None;
        for (id, shape) in self.iter() {
            if !filter(id, &shape, &region) {
                continue;
            }
            if let Some(t) = ray_vs_shape(origin, dir_n, max_dist, &shape)
                && best.map(|h| t < h.distance).unwrap_or(true)
            {
                best = Some(RaycastHit {
                    entity: id,
                    distance: t,
                });
            }
        }
        best
    }

    // ── Observers ─────────────────────────────────────────────────

    /// Register an observer that will track entities entering and
    /// leaving `region`. The initial entity set is populated immediately,
    /// so the **first call to [`AoiWorld::drain_events`] reports an
    /// `Enter` event for each entity already inside the region**.
    pub fn create_observer(&mut self, region: Shape) -> ObserverId {
        self.create_observer_internal(region, None)
    }

    /// Like [`AoiWorld::create_observer`] but additionally attaches a
    /// per-observer [`AoiFilter`]. The filter is part of the
    /// observer's persistent state and applied on every subsequent
    /// [`AoiWorld::update_observer`] before the diff. See
    /// [`AoiFilter`] for the call signature and the
    /// `examples/aoi-demo` source for type-filter and LOD examples.
    pub fn create_observer_filtered<F>(&mut self, region: Shape, filter: F) -> ObserverId
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool + Send + Sync + 'static,
    {
        self.create_observer_internal(region, Some(Box::new(filter)))
    }

    fn create_observer_internal(
        &mut self,
        region: Shape,
        filter: Option<Box<AoiFilter>>,
    ) -> ObserverId {
        let id = if let Some(idx) = self.free_observers.pop() {
            self.observers[idx as usize] = Some(Observer::new());
            ObserverId(idx)
        } else {
            let idx = self.observers.len() as u32;
            self.observers.push(Some(Observer::new()));
            ObserverId(idx)
        };
        // Install the filter *before* the initial query so the very
        // first Enter set is already filter-aware. Without this, the
        // first frame would emit Enters for things the filter would
        // reject and then immediately Leaves on the next update — a
        // confusing one-frame "blip".
        if let Some(obs) = self.observers[id.0 as usize].as_mut() {
            obs.filter = filter;
        }
        let hits = self.observer_hits(id, &region);
        if let Some(obs) = self.observers[id.0 as usize].as_mut() {
            obs.diff_into_pending(hits);
        }
        id
    }

    /// Move/resize an observer's region. Diffs against the previous
    /// hit set and queues `Enter`/`Leave` events for the symmetric
    /// difference. No-op if the id has been removed. Any
    /// previously-attached filter still applies; use
    /// [`AoiWorld::set_observer_filter`] to change it.
    pub fn update_observer(&mut self, id: ObserverId, region: Shape) {
        if !self.observer_alive(id) {
            return;
        }
        let hits = self.observer_hits(id, &region);
        if let Some(obs) = self.observers[id.0 as usize].as_mut() {
            obs.diff_into_pending(hits);
        }
    }

    /// Replace (or clear, with `None`) the filter attached to an
    /// observer. The next [`AoiWorld::update_observer`] will diff
    /// against the new effective hit set, which may emit Enter/Leave
    /// events for entities that haven't physically moved — this
    /// truthfully reflects the observer's *visibility* changing
    /// (essential for LOD: when you tighten the LOD radius, things
    /// outside it should fire Leave so the client knows to drop them).
    ///
    /// No-op if the id has been removed.
    pub fn set_observer_filter<F>(&mut self, id: ObserverId, filter: Option<F>)
    where
        F: Fn(EntityId, &Shape, &Shape) -> bool + Send + Sync + 'static,
    {
        if !self.observer_alive(id) {
            return;
        }
        if let Some(obs) = self.observers[id.0 as usize].as_mut() {
            obs.filter = filter.map(|f| Box::new(f) as Box<AoiFilter>);
        }
    }

    /// Remove an observer. Pending events for it are dropped.
    pub fn remove_observer(&mut self, id: ObserverId) {
        if !self.observer_alive(id) {
            return;
        }
        self.observers[id.0 as usize] = None;
        self.free_observers.push(id.0);
    }

    /// Take and clear all events that have accumulated for `id` since
    /// the last `drain_events` call. Returns an empty vec if the id has
    /// been removed.
    pub fn drain_events(&mut self, id: ObserverId) -> Vec<AoiEvent> {
        match self
            .observers
            .get_mut(id.0 as usize)
            .and_then(|o| o.as_mut())
        {
            Some(obs) => std::mem::take(&mut obs.pending),
            None => Vec::new(),
        }
    }

    fn observer_alive(&self, id: ObserverId) -> bool {
        matches!(self.observers.get(id.0 as usize), Some(Some(_)))
    }

    /// Compute the new hit set for an observer, applying its persistent
    /// filter (if any). Pulled out so `create_observer` and
    /// `update_observer` share the exact same code path — keeping them
    /// in sync prevents subtle divergences where, e.g., the initial
    /// population skipped the filter.
    ///
    /// Note: we have to fetch the filter & query the backend in two
    /// separate steps because the filter closure borrows the world's
    /// candidate shapes while the backend query also needs `&mut self`.
    /// The double-lookup (id → shape) per candidate is the same cost
    /// pattern as `query_shape_filtered` and stays well below the
    /// broadphase's own cost.
    fn observer_hits(&mut self, id: ObserverId, region: &Shape) -> HashSet<EntityId> {
        let candidates = self.query_shape(region);
        // Move the filter out for the duration of the candidate scan
        // so we can borrow `self` to look up shapes. We restore it
        // immediately after — the observer is never observable in this
        // half-state because we don't yield back to user code in
        // between.
        let mut filter = self.observers[id.0 as usize]
            .as_mut()
            .and_then(|o| o.filter.take());
        let hits: HashSet<EntityId> = candidates
            .into_iter()
            .filter(|cid| match (&filter, self.get(*cid)) {
                (Some(f), Some(shape)) => f(*cid, &shape, region),
                (None, _) => true,
                (Some(_), None) => false,
            })
            .collect();
        if let Some(obs) = self.observers[id.0 as usize].as_mut() {
            obs.filter = filter.take();
        }
        hits
    }

    // ── Diagnostics ───────────────────────────────────────────────

    pub fn stats(&self) -> AoiStats {
        match &self.backend {
            Backend::BruteForce(b) => b.stats(),
            Backend::Grid(g) => g.stats(),
        }
    }
}

// ── VDP integration ─────────────────────────────────────────────────
//
// All VDP wiring lives behind the `vdp` feature so release builds (and
// games that opt out of debug tooling) don't pay the `serde_json`
// compile cost. Games forward VDP requests by matching method names
// starting with `aoi.` to `world.handle_vdp(method, params)`; the
// returned `serde_json::Value` becomes the `result` field of the JSON-
// RPC response. Unknown methods return `Err`, mirroring the convention
// in `vibe2d::GameBridge::handle_vdp_request`.

#[cfg(feature = "vdp")]
impl AoiWorld {
    /// Handle a VDP request scoped to AOI methods. Method names live in
    /// the `aoi.` namespace; see `docs/aoi.md` for the full method list.
    ///
    /// Games typically wire this up like:
    ///
    /// ```ignore
    /// fn handle_vdp(&mut self, method: &str, params: &Value) -> Result<Value, String> {
    ///     if method.starts_with("aoi.") {
    ///         return self.aoi.handle_vdp(method, params);
    ///     }
    ///     Err(format!("Unknown method: {method}"))
    /// }
    /// ```
    pub fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use serde_json::json;
        match method {
            "aoi.list" => {
                let entities: Vec<_> = self
                    .iter()
                    .map(|(id, shape)| json!({ "id": id, "shape": shape }))
                    .collect();
                Ok(json!({ "entities": entities }))
            }
            "aoi.queryAabb" => {
                let min = parse_vec2(params, "min")?;
                let max = parse_vec2(params, "max")?;
                Ok(json!({ "hits": self.query_aabb(min, max) }))
            }
            "aoi.queryCircle" => {
                let center = parse_vec2(params, "center")?;
                let radius = parse_f32(params, "radius")?;
                Ok(json!({ "hits": self.query_circle(center, radius) }))
            }
            "aoi.queryPoint" => {
                let p = parse_vec2(params, "point")?;
                Ok(json!({ "hits": self.query_point(p) }))
            }
            "aoi.raycast" => {
                let origin = parse_vec2(params, "origin")?;
                let dir = parse_vec2(params, "dir")?;
                let max_dist = parse_f32(params, "maxDist")?;
                Ok(json!({ "hit": self.raycast(origin, dir, max_dist) }))
            }
            "aoi.stats" => Ok(serde_json::to_value(self.stats()).map_err(|e| e.to_string())?),
            _ => Err(format!("Unknown method: {method}")),
        }
    }
}

#[cfg(feature = "vdp")]
fn parse_vec2(params: &serde_json::Value, key: &str) -> Result<Vec2, String> {
    let v = params
        .get(key)
        .ok_or_else(|| format!("missing param `{key}`"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| format!("param `{key}` must be a 2-element array"))?;
    if arr.len() != 2 {
        return Err(format!("param `{key}` must have exactly 2 elements"));
    }
    let x = arr[0]
        .as_f64()
        .ok_or_else(|| format!("param `{key}[0]` must be a number"))? as f32;
    let y = arr[1]
        .as_f64()
        .ok_or_else(|| format!("param `{key}[1]` must be a number"))? as f32;
    Ok(Vec2::new(x, y))
}

#[cfg(feature = "vdp")]
fn parse_f32(params: &serde_json::Value, key: &str) -> Result<f32, String> {
    let v = params
        .get(key)
        .ok_or_else(|| format!("missing param `{key}`"))?;
    Ok(v.as_f64()
        .ok_or_else(|| format!("param `{key}` must be a number"))? as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_world() -> AoiWorld {
        AoiWorld::with_bruteforce()
    }

    #[test]
    fn empty_world_has_zero_len() {
        let mut w = make_world();
        assert!(w.is_empty());
        assert_eq!(w.len(), 0);
        assert!(w.query_circle(Vec2::ZERO, 100.0).is_empty());
    }

    #[test]
    fn insert_returns_distinct_ids() {
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::ZERO));
        let b = w.insert(Shape::point(Vec2::ZERO));
        assert_ne!(a, b);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn get_returns_inserted_shape() {
        let mut w = make_world();
        let s = Shape::circle(Vec2::new(3.0, 4.0), 2.0);
        let id = w.insert(s);
        assert_eq!(w.get(id), Some(s));
    }

    #[test]
    fn update_replaces_shape() {
        let mut w = make_world();
        let id = w.insert(Shape::point(Vec2::ZERO));
        w.update(id, Shape::point(Vec2::new(50.0, 50.0)));
        assert!(w.query_point(Vec2::ZERO).is_empty());
        assert_eq!(w.query_point(Vec2::new(50.0, 50.0)), vec![id]);
    }

    #[test]
    fn remove_excludes_from_queries() {
        let mut w = make_world();
        let id = w.insert(Shape::circle(Vec2::ZERO, 5.0));
        w.remove(id);
        assert_eq!(w.len(), 0);
        assert!(w.query_circle(Vec2::ZERO, 100.0).is_empty());
        assert_eq!(w.get(id), None);
    }

    #[test]
    fn update_after_remove_is_noop() {
        let mut w = make_world();
        let id = w.insert(Shape::point(Vec2::ZERO));
        w.remove(id);
        w.update(id, Shape::point(Vec2::new(1.0, 1.0)));
        assert!(w.query_point(Vec2::new(1.0, 1.0)).is_empty());
    }

    #[test]
    fn query_aabb_returns_overlapping_entities_only() {
        let mut w = make_world();
        let inside = w.insert(Shape::point(Vec2::new(5.0, 5.0)));
        let outside = w.insert(Shape::point(Vec2::new(50.0, 50.0)));

        let hits = w.query_aabb(Vec2::ZERO, Vec2::splat(10.0));
        assert!(hits.contains(&inside));
        assert!(!hits.contains(&outside));
    }

    #[test]
    fn query_circle_finds_points_inside() {
        let mut w = make_world();
        let center = w.insert(Shape::point(Vec2::ZERO));
        let near = w.insert(Shape::point(Vec2::new(3.0, 4.0))); // distance 5
        let far = w.insert(Shape::point(Vec2::new(100.0, 0.0)));

        let hits = w.query_circle(Vec2::ZERO, 6.0);
        assert!(hits.contains(&center));
        assert!(hits.contains(&near));
        assert!(!hits.contains(&far));
    }

    #[test]
    fn query_point_finds_aabb_containing_it() {
        let mut w = make_world();
        let id = w.insert(Shape::aabb(Vec2::ZERO, Vec2::splat(10.0)));
        assert_eq!(w.query_point(Vec2::new(5.0, -5.0)), vec![id]);
        assert!(w.query_point(Vec2::new(20.0, 0.0)).is_empty());
    }

    #[test]
    fn iter_yields_all_entities() {
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let b = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        let collected: Vec<_> = w.iter().map(|(id, _)| id).collect();
        assert_eq!(collected.len(), 2);
        assert!(collected.contains(&a));
        assert!(collected.contains(&b));
    }

    #[test]
    fn stats_reflect_entity_count() {
        let mut w = make_world();
        for i in 0..5 {
            w.insert(Shape::point(Vec2::new(i as f32, 0.0)));
        }
        let s = w.stats();
        assert_eq!(s.entity_count, 5);
    }

    // ── Observer integration tests (P3) ────────────────────────────

    #[test]
    fn observer_initial_population_emits_enters() {
        let mut w = make_world();
        let inside = w.insert(Shape::point(Vec2::ZERO));
        let outside = w.insert(Shape::point(Vec2::new(100.0, 100.0)));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        let events = w.drain_events(obs);
        assert!(events.contains(&AoiEvent::Enter(inside)));
        assert!(!events.contains(&AoiEvent::Enter(outside)));
    }

    #[test]
    fn observer_no_motion_no_events() {
        let mut w = make_world();
        let _id = w.insert(Shape::point(Vec2::ZERO));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        let _ = w.drain_events(obs); // consume initial Enter
        // No state change → no events.
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 10.0));
        assert!(w.drain_events(obs).is_empty());
    }

    #[test]
    fn observer_emits_leave_when_region_moves_away() {
        let mut w = make_world();
        let id = w.insert(Shape::point(Vec2::ZERO));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        let _ = w.drain_events(obs);
        // Move the observer far away — `id` should leave.
        w.update_observer(obs, Shape::circle(Vec2::new(1000.0, 1000.0), 10.0));
        let events = w.drain_events(obs);
        assert!(
            events.contains(&AoiEvent::Leave(id)),
            "expected Leave({id:?}) in {events:?}"
        );
    }

    #[test]
    fn observer_emits_enter_when_entity_walks_in() {
        // Mirror of the prior test but mutating the entity instead of
        // the observer — the diff should be symmetric.
        let mut w = make_world();
        let id = w.insert(Shape::point(Vec2::new(1000.0, 1000.0)));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        assert!(w.drain_events(obs).is_empty(), "id starts outside");
        w.update(id, Shape::point(Vec2::ZERO));
        // Trigger a diff by re-asserting the same region.
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 10.0));
        assert!(w.drain_events(obs).contains(&AoiEvent::Enter(id)));
    }

    #[test]
    fn drain_events_clears_queue() {
        let mut w = make_world();
        w.insert(Shape::point(Vec2::ZERO));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        assert!(!w.drain_events(obs).is_empty());
        // Second drain in the same "frame" must be empty.
        assert!(w.drain_events(obs).is_empty());
    }

    #[test]
    fn remove_observer_drops_pending_events() {
        let mut w = make_world();
        w.insert(Shape::point(Vec2::ZERO));
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 10.0));
        w.remove_observer(obs);
        assert!(w.drain_events(obs).is_empty());
    }

    #[test]
    fn multiple_observers_track_independently() {
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(-50.0, 0.0)));
        let b = w.insert(Shape::point(Vec2::new(50.0, 0.0)));
        let left = w.create_observer(Shape::circle(Vec2::new(-50.0, 0.0), 10.0));
        let right = w.create_observer(Shape::circle(Vec2::new(50.0, 0.0), 10.0));
        let left_events = w.drain_events(left);
        let right_events = w.drain_events(right);
        assert!(left_events.contains(&AoiEvent::Enter(a)));
        assert!(!left_events.contains(&AoiEvent::Enter(b)));
        assert!(right_events.contains(&AoiEvent::Enter(b)));
        assert!(!right_events.contains(&AoiEvent::Enter(a)));
    }

    // ── Raycast integration tests (P3) ─────────────────────────────

    #[test]
    fn raycast_returns_closest_hit() {
        let mut w = make_world();
        let near = w.insert(Shape::circle(Vec2::new(20.0, 0.0), 5.0));
        let _far = w.insert(Shape::circle(Vec2::new(80.0, 0.0), 5.0));
        let hit = w.raycast(Vec2::ZERO, Vec2::X, 100.0).unwrap();
        assert_eq!(hit.entity, near);
        assert!((hit.distance - 15.0).abs() < 0.01);
    }

    #[test]
    fn raycast_misses_when_nothing_in_path() {
        let mut w = make_world();
        w.insert(Shape::circle(Vec2::new(0.0, 100.0), 5.0));
        assert!(w.raycast(Vec2::ZERO, Vec2::X, 100.0).is_none());
    }

    #[test]
    fn raycast_with_zero_dir_is_safe() {
        let mut w = make_world();
        w.insert(Shape::point(Vec2::ZERO));
        // A zero-length direction has no defined ray; we choose to
        // return None rather than panic.
        assert!(w.raycast(Vec2::ZERO, Vec2::ZERO, 100.0).is_none());
    }

    #[test]
    fn raycast_normalizes_direction() {
        // A non-unit direction must produce the same result as a
        // normalized one — the world handles normalization internally.
        let mut w = make_world();
        let id = w.insert(Shape::circle(Vec2::new(20.0, 0.0), 5.0));
        let h1 = w.raycast(Vec2::ZERO, Vec2::X, 100.0).unwrap();
        let h2 = w.raycast(Vec2::ZERO, Vec2::new(7.0, 0.0), 100.0).unwrap();
        assert_eq!(h1.entity, id);
        assert_eq!(h2.entity, id);
        assert!((h1.distance - h2.distance).abs() < 0.001);
    }

    // ── Filter / LOD tests (P4) ────────────────────────────────────

    #[test]
    fn query_circle_filtered_drops_rejected_entities() {
        // Three points all inside the query radius; filter accepts
        // only the middle one. Confirms the filter is consulted *and*
        // that rejection actually removes the id from the result.
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let b = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        let c = w.insert(Shape::point(Vec2::new(3.0, 0.0)));
        let hits = w.query_circle_filtered(Vec2::ZERO, 100.0, |id, _, _| id == b);
        assert_eq!(hits, vec![b]);
        // Verify a / c are reachable without the filter — sanity check
        // that we didn't break the broadphase.
        let all = w.query_circle(Vec2::ZERO, 100.0);
        assert_eq!(all.len(), 3);
        assert!(all.contains(&a));
        assert!(all.contains(&c));
    }

    #[test]
    fn query_filter_sees_query_region_for_lod_distance() {
        // Distance-LOD style filter: keep only entities within 10 units
        // of the query center. With a wide query radius (100) the
        // broadphase would normally return all three, but the filter
        // tightens it.
        let mut w = make_world();
        let near = w.insert(Shape::point(Vec2::new(5.0, 0.0)));
        let _mid = w.insert(Shape::point(Vec2::new(20.0, 0.0)));
        let _far = w.insert(Shape::point(Vec2::new(80.0, 0.0)));
        let hits = w.query_circle_filtered(Vec2::ZERO, 100.0, |_, shape, region| {
            // For Point entities `aabb_bounds` returns (p, p), so the
            // midpoint is the point itself.
            let (emin, emax) = shape.aabb_bounds();
            let entity_pos = (emin + emax) * 0.5;
            let (rmin, rmax) = region.aabb_bounds();
            let region_pos = (rmin + rmax) * 0.5;
            entity_pos.distance(region_pos) < 10.0
        });
        assert_eq!(hits, vec![near]);
    }

    #[test]
    fn query_aabb_filtered_works_like_circle_variant() {
        // Smoke test that the AABB and Point variants share the same
        // filter wiring as the Circle variant — guards against
        // copy-paste regression where one variant skips the filter.
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(1.0, 1.0)));
        let _b = w.insert(Shape::point(Vec2::new(2.0, 2.0)));
        let aabb_hits = w.query_aabb_filtered(Vec2::ZERO, Vec2::splat(10.0), |id, _, _| id == a);
        let point_hits = w.query_point_filtered(Vec2::new(1.0, 1.0), |id, _, _| id == a);
        assert_eq!(aabb_hits, vec![a]);
        assert_eq!(point_hits, vec![a]);
    }

    #[test]
    fn raycast_filtered_skips_blocked_entities() {
        // Without the filter, the near sphere wins. With a filter
        // that rejects it, the ray punches through to the far one.
        let mut w = make_world();
        let near = w.insert(Shape::circle(Vec2::new(20.0, 0.0), 5.0));
        let far = w.insert(Shape::circle(Vec2::new(80.0, 0.0), 5.0));
        let plain = w.raycast(Vec2::ZERO, Vec2::X, 200.0).unwrap();
        assert_eq!(plain.entity, near);
        let filtered = w
            .raycast_filtered(Vec2::ZERO, Vec2::X, 200.0, |id, _, _| id != near)
            .unwrap();
        assert_eq!(filtered.entity, far);
    }

    #[test]
    fn observer_filter_excludes_at_creation() {
        // The filter is applied to the *initial* hit set so the first
        // drain doesn't see Enters for entities the filter rejects —
        // this avoids a "blip" of Enter+Leave on frame 0/1.
        let mut w = make_world();
        let kept = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let dropped = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        let obs = w.create_observer_filtered(Shape::circle(Vec2::ZERO, 100.0), move |id, _, _| {
            id != dropped
        });
        let events = w.drain_events(obs);
        assert!(events.contains(&AoiEvent::Enter(kept)));
        assert!(!events.contains(&AoiEvent::Enter(dropped)));
    }

    #[test]
    fn observer_filter_persists_across_updates() {
        // After the initial drain, moving the observer (with the same
        // region) must NOT spuriously emit events for filter-rejected
        // entities. This is the key invariant that distinguishes a
        // persistent observer filter from a per-frame
        // post-process: the filter is part of the diff baseline.
        let mut w = make_world();
        let kept = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let dropped = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        let obs = w.create_observer_filtered(Shape::circle(Vec2::ZERO, 100.0), move |id, _, _| {
            id != dropped
        });
        let _ = w.drain_events(obs); // consume initial Enter for `kept`
        // Re-assert the same region — no physical change.
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 100.0));
        let events = w.drain_events(obs);
        assert!(
            events.is_empty(),
            "filtered observer must not churn events on no-op update, got {events:?}"
        );
        // Move kept far outside the region → it should leave.
        w.update(kept, Shape::point(Vec2::new(1000.0, 0.0)));
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 100.0));
        let events = w.drain_events(obs);
        assert!(
            events.contains(&AoiEvent::Leave(kept)),
            "expected Leave({kept:?}) in {events:?}"
        );
    }

    #[test]
    fn observer_lod_distance_filter() {
        // Real-world LOD shape: filter accepts entities within 10
        // units of the observer center. Move the observer and verify
        // distant entities never enter even though they're inside
        // the broadphase region.
        let mut w = make_world();
        let _at_5 = w.insert(Shape::point(Vec2::new(5.0, 0.0)));
        let _at_50 = w.insert(Shape::point(Vec2::new(50.0, 0.0)));
        let obs =
            w.create_observer_filtered(Shape::circle(Vec2::ZERO, 100.0), |_, shape, region| {
                let (emin, emax) = shape.aabb_bounds();
                let (rmin, rmax) = region.aabb_bounds();
                let ep = (emin + emax) * 0.5;
                let rp = (rmin + rmax) * 0.5;
                ep.distance(rp) < 10.0
            });
        let events = w.drain_events(obs);
        // Only `at_5` is within LOD range.
        let enters: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AoiEvent::Enter(id) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(enters.len(), 1, "got events: {events:?}");
    }

    #[test]
    fn set_observer_filter_changes_visibility_set() {
        // Replacing the filter with `set_observer_filter` must produce
        // Enter events for entities the *new* filter accepts but the
        // *old* one rejected (and Leave events for the inverse). This
        // is what powers "the player toggled show-allies on" UX.
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let b = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        // Start with a filter that only accepts `a`.
        let obs =
            w.create_observer_filtered(Shape::circle(Vec2::ZERO, 100.0), move |id, _, _| id == a);
        let _ = w.drain_events(obs); // consume initial Enter(a)
        // Swap to a filter that only accepts `b`.
        w.set_observer_filter::<fn(EntityId, &Shape, &Shape) -> bool>(obs, None);
        // We just cleared the filter (None). Re-querying should now
        // emit Enter(b) (b was previously filtered out) and *not*
        // re-emit Enter(a) (it's still in the set).
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 100.0));
        let events = w.drain_events(obs);
        assert!(
            events.contains(&AoiEvent::Enter(b)),
            "clearing filter should emit Enter for previously-rejected entity, got {events:?}"
        );
        assert!(
            !events.contains(&AoiEvent::Enter(a)),
            "already-visible entity should not re-Enter, got {events:?}"
        );
    }

    #[test]
    fn set_observer_filter_to_stricter_emits_leaves() {
        // Tightening a filter (e.g. shrinking LOD radius) must emit
        // Leave for entities that fall out of the new visibility set.
        // This is what lets a network LOD layer say "drop this entity
        // from the client".
        let mut w = make_world();
        let a = w.insert(Shape::point(Vec2::new(1.0, 0.0)));
        let b = w.insert(Shape::point(Vec2::new(2.0, 0.0)));
        // Start with no filter (both visible).
        let obs = w.create_observer(Shape::circle(Vec2::ZERO, 100.0));
        let _ = w.drain_events(obs);
        // Install a filter that only accepts `a`.
        w.set_observer_filter(obs, Some(move |id: EntityId, _: &Shape, _: &Shape| id == a));
        w.update_observer(obs, Shape::circle(Vec2::ZERO, 100.0));
        let events = w.drain_events(obs);
        assert!(
            events.contains(&AoiEvent::Leave(b)),
            "tightening filter should emit Leave({b:?}), got {events:?}"
        );
        assert!(
            !events.contains(&AoiEvent::Leave(a)),
            "still-accepted entity must not Leave, got {events:?}"
        );
    }
}
