//! AOI demo — three independently-moving circles observe a shared
//! field of static scatter points. Any point covered by *any* circle
//! lights up green; points fade back to gray when no circle covers
//! them anymore.
//!
//! Drives the entire demo with `vibe_aoi`:
//! - Scatter entities come in two flavors:
//!   - **Round dots** — `Shape::Circle`, **participate** in the lit/dim
//!     coverage logic. These are what the observer filter accepts.
//!   - **Square dots** — `Shape::Aabb`, **never light up**. They exist
//!     to demonstrate type filtering: every observer's `AoiFilter`
//!     rejects them, so they stay gray no matter which observer ring
//!     passes over them. This mirrors a real game where, e.g., decoy
//!     props are inside the AOI broadphase but irrelevant to gameplay.
//! - Each moving circle is an **observer region** (not an entity)
//!   carrying a persistent `AoiFilter`. The filter does two jobs:
//!   1. **Type filter** — `kind == Round` (squares always rejected).
//!   2. **Distance LOD** (toggled with `[L]`) — drop round dots whose
//!      center is more than `LOD_RADIUS` away from the observer center.
//!      Models the "don't replicate to clients beyond LOD distance"
//!      pattern in networked games. With `LOD_RADIUS == CIRCLE_RADIUS`
//!      this is a no-op, so the LOD lever is set to `< CIRCLE_RADIUS`
//!      and you can watch the lit set shrink when LOD is on.
//! - Wall collision is plain business logic (flip the velocity vector
//!   on the appropriate axis); we explicitly do *not* use a physics
//!   engine here, since the design doc draws the AOI/physics boundary
//!   at "AOI tells you who's nearby, physics tells you what to do
//!   about it".
//!
//! Visual treatment:
//! - Observer circles are rendered as **transparent rings** via
//!   `Screen::draw_circle_outline`. Round dots underneath stay visible.
//! - Round dots use `Screen::draw_circle`; square dots are drawn as
//!   actual axis-aligned squares using the engine's white-pixel
//!   texture so they're trivially distinguishable from rounds at a
//!   glance.
//! - The stats panel is built with `vibe_ui` (panel + labels) so it
//!   composites on top of the world via the engine's UI pipeline
//!   instead of getting baked into the world layer.
//!
//! VDP wiring lives in [`AoiDemo::handle_vdp`], which forwards anything
//! starting with `aoi.` to `vibe_aoi::AoiWorld::handle_vdp` and exposes
//! a small custom `demo.*` namespace for inspection from the test
//! suite.

use std::sync::Arc;

use glam::Vec2;
use vibe_aoi::{AoiEvent, AoiWorld, EntityId, ObserverId, Shape};
use vibe2d::prelude::*;

const WORLD_W: f32 = 512.0;
const WORLD_H: f32 = 288.0;
/// Total scatter count (rounds + squares). Stays at 200 so legacy
/// VDP integration tests that asserted `entity_count == 200` still
/// hold.
const NUM_POINTS: usize = 200;
/// Fraction of the scatter that's `Square` (the rest is `Round`). A
/// 30 % square mix is dense enough to make filtering obvious in
/// screenshots without crowding out the lit-up rounds.
const SQUARE_FRACTION: f32 = 0.30;
const CIRCLE_RADIUS: f32 = 28.0;
/// Radius of each round scatter dot, and half-extent of each square
/// dot. Both shapes use the same effective size so neither feels
/// visually "weightier" than the other — the only difference the
/// player should perceive is the shape and the color behavior.
const POINT_RADIUS: f32 = 2.5;
/// Distance LOD threshold, in virtual pixels. When LOD is enabled
/// (toggle with `[L]`), each observer's filter additionally rejects
/// any round dot whose center is more than this far from the observer
/// center. Set to ~60 % of CIRCLE_RADIUS so the visible lit "core" is
/// noticeably smaller than the broadphase ring.
const LOD_RADIUS: f32 = 18.0;

/// Static classification for each scatter entity. Stored alongside
/// the AOI world in `AoiDemo::scatter_kind`, keyed by `EntityId`,
/// because the AOI library is intentionally agnostic about
/// game-specific tags — this is the recommended pattern for any
/// metadata that drives a filter closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScatterKind {
    /// `Shape::Circle`. Participates in lit/dim coverage logic — the
    /// observer filters accept these.
    Round,
    /// `Shape::Aabb`. Always filtered out. Visible as a gray square
    /// regardless of observer overlap.
    Square,
}

/// One independently-moving observer circle.
struct MovingCircle {
    pos: Vec2,
    vel: Vec2,
    observer: ObserverId,
    /// Tint applied to the ring outline. Each circle gets a distinct
    /// hue so it's visually obvious which one is which when they
    /// overlap.
    color: Color,
}

struct AoiDemo {
    aoi: AoiWorld,
    /// Stable handles for the static scatter field so we can flip their
    /// highlight state from observer events without re-querying.
    points: Vec<EntityId>,
    point_to_index: std::collections::HashMap<EntityId, usize>,
    /// Static type tag for each scatter entity. Wrapped in `Arc` so
    /// observer filter closures can each hold a cheap clone — closures
    /// stored in `vibe_aoi` are `'static + Send + Sync`, so they can't
    /// borrow demo-side data, and rebuilding the table on every filter
    /// swap would be wasteful. The map is built once in `new()` and
    /// never mutated again.
    scatter_kind: Arc<std::collections::HashMap<EntityId, ScatterKind>>,
    /// Per-point reference count: the number of *circles* currently
    /// covering it. A point is lit iff `cover_count > 0`. Using a
    /// counter (rather than a `bool`) lets two overlapping circles both
    /// "own" the same point without one's Leave event prematurely
    /// turning it off.
    cover_count: Vec<u32>,

    circles: Vec<MovingCircle>,

    /// Whether the distance-LOD layer is currently active. Toggled by
    /// the `[L]` key (action `toggle_lod`). When this flips, every
    /// observer gets a fresh `AoiFilter` installed via
    /// `set_observer_filter` — the resulting Enter/Leave churn is
    /// exactly what makes the LOD effect visible (rounds at the
    /// observer's edge wink off when LOD turns on).
    lod_enabled: bool,

    /// Cached stats refreshed once per frame in `update`, so `update_ui`
    /// can render without re-borrowing the AOI world.
    last_stats: vibe_aoi::AoiStats,
    enter_count_total: u64,
    leave_count_total: u64,
    /// Total round-dot population, cached so the stats panel doesn't
    /// have to walk `scatter_kind` every frame. (Squares = NUM_POINTS - rounds.)
    round_count: usize,
    paused: bool,

    /// 1×1 white pixel texture used for the gutter separator and the
    /// square scatter dots. Built once in `new()` via
    /// [`Renderer::create_white_pixel_texture`] — the engine no longer
    /// pre-creates it, so the demo owns the lifecycle.
    white_tex: TextureId,
    /// 256² antialiased filled disc, used for round scatter dots via
    /// [`Screen::draw_circle`]. Built in `new()`.
    circle_filled_tex: TextureId,
    /// 256² antialiased ring, used for the moving observer rings via
    /// [`Screen::draw_circle_outline`]. 8 % stroke ratio reads well at
    /// the radii used here.
    circle_ring_tex: TextureId,
}

impl AoiDemo {
    fn current_shape(c: &MovingCircle) -> Shape {
        Shape::circle(c.pos, CIRCLE_RADIUS)
    }

    fn lit_count(&self) -> usize {
        self.cover_count.iter().filter(|&&c| c > 0).count()
    }

    /// Build the observer filter closure for the current LOD state.
    /// Pulled into a helper because both `new()` (initial install) and
    /// `update()` (whenever `[L]` is pressed) need to produce one, and
    /// the rules must match exactly or the two paths would diverge.
    ///
    /// The closure is `'static + Send + Sync`, so it can't borrow
    /// `self.scatter_kind` — instead we clone the `Arc` into the
    /// closure. Cloning an `Arc` is one atomic increment, so even with
    /// 3 observers × N filter swaps per second the overhead is
    /// invisible.
    fn build_filter(
        kind_table: Arc<std::collections::HashMap<EntityId, ScatterKind>>,
        lod_enabled: bool,
    ) -> impl Fn(EntityId, &Shape, &Shape) -> bool + Send + Sync + 'static {
        move |id, entity_shape, observer_region| {
            // 1. Type filter — squares are never observed, regardless
            //    of LOD. This is the demo's stand-in for "decoy props
            //    don't replicate to clients".
            if kind_table.get(&id).copied() != Some(ScatterKind::Round) {
                return false;
            }
            // 2. Distance LOD — only when the toggle is on.
            if !lod_enabled {
                return true;
            }
            // For round dots `aabb_bounds()` returns the inscribed-AABB
            // of the disc, so the midpoint is the disc center. Same
            // for the observer's circular region.
            let (e_min, e_max) = entity_shape.aabb_bounds();
            let entity_center = (e_min + e_max) * 0.5;
            let (r_min, r_max) = observer_region.aabb_bounds();
            let region_center = (r_min + r_max) * 0.5;
            entity_center.distance(region_center) < LOD_RADIUS
        }
    }

    /// Push a freshly-built filter into every observer. Called once at
    /// startup and again on each `[L]` press. The next
    /// `update_observer` for each circle will diff against the new
    /// effective hit set, emitting Enter/Leave events as appropriate
    /// — that's the per-observer state churn that drives the visible
    /// "shrink/expand" effect when LOD toggles.
    fn refresh_filters(&mut self) {
        for c in &self.circles {
            self.aoi.set_observer_filter(
                c.observer,
                Some(Self::build_filter(
                    self.scatter_kind.clone(),
                    self.lod_enabled,
                )),
            );
        }
    }

    /// Re-run a single circle's observer query and apply the resulting
    /// Enter/Leave events to `cover_count`. Pulled out so both `update`
    /// (each frame for every circle) and the VDP `demo.setCirclePos`
    /// helper can share the same logic.
    fn sync_circle_observer(&mut self, idx: usize) {
        let shape = Self::current_shape(&self.circles[idx]);
        self.aoi.update_observer(self.circles[idx].observer, shape);
        for ev in self.aoi.drain_events(self.circles[idx].observer) {
            match ev {
                AoiEvent::Enter(id) => {
                    if let Some(&pi) = self.point_to_index.get(&id) {
                        self.cover_count[pi] += 1;
                    }
                    self.enter_count_total += 1;
                }
                AoiEvent::Leave(id) => {
                    if let Some(&pi) = self.point_to_index.get(&id) {
                        // Saturating in case the AOI layer ever fires a
                        // spurious Leave we didn't see an Enter for —
                        // panicking from a debug demo for a numeric
                        // underflow would be obnoxious.
                        self.cover_count[pi] = self.cover_count[pi].saturating_sub(1);
                    }
                    self.leave_count_total += 1;
                }
            }
        }
    }

    /// Bounce a circle off the world bounds in place. Pure business
    /// logic — see the module docstring for why we don't run this
    /// through a physics engine.
    fn step_circle_motion(c: &mut MovingCircle, dt: f32) {
        c.pos += c.vel * dt;
        if c.pos.x - CIRCLE_RADIUS < 0.0 {
            c.pos.x = CIRCLE_RADIUS;
            c.vel.x = c.vel.x.abs();
        } else if c.pos.x + CIRCLE_RADIUS > WORLD_W {
            c.pos.x = WORLD_W - CIRCLE_RADIUS;
            c.vel.x = -c.vel.x.abs();
        }
        if c.pos.y - CIRCLE_RADIUS < 0.0 {
            c.pos.y = CIRCLE_RADIUS;
            c.vel.y = c.vel.y.abs();
        } else if c.pos.y + CIRCLE_RADIUS > WORLD_H {
            c.pos.y = WORLD_H - CIRCLE_RADIUS;
            c.vel.y = -c.vel.y.abs();
        }
    }
}

impl Game for AoiDemo {
    fn new(ctx: &mut Context, renderer: &Renderer) -> Self {
        // Build the procedural textures the demo needs and register
        // them into the asset manager so the renderer can reach them
        // by `TextureId` during sprite batching. The engine no longer
        // ships any built-in textures — every game declares what it
        // needs up front.
        let white_tex = ctx
            .assets
            .register_texture("aoi_demo_white", renderer.create_white_pixel_texture());
        let circle_filled_tex = ctx.assets.register_texture(
            "aoi_demo_circle_filled",
            renderer.create_filled_circle_texture("aoi_demo_circle_filled", 256),
        );
        let circle_ring_tex = ctx.assets.register_texture(
            "aoi_demo_circle_ring",
            renderer.create_ring_texture("aoi_demo_circle_ring", 256, 0.08),
        );

        let mut aoi = AoiWorld::new(Vec2::new(WORLD_W, WORLD_H));

        // Deterministic scatter — same seed every launch so the demo is
        // reproducible (and so the integration test can hard-code
        // expected entity counts).
        let mut rng = Lcg::new(0x5EED);
        let mut points = Vec::with_capacity(NUM_POINTS);
        let mut point_to_index = std::collections::HashMap::with_capacity(NUM_POINTS);
        let mut scatter_kind_map =
            std::collections::HashMap::<EntityId, ScatterKind>::with_capacity(NUM_POINTS);
        let mut round_count = 0usize;
        for i in 0..NUM_POINTS {
            // Inset from the edges so points stay clear of any UI panel
            // overlay drawn at the corners.
            let x = rng.next_f32(20.0, WORLD_W - 20.0);
            let y = rng.next_f32(20.0, WORLD_H - 20.0);
            // Decide kind from a uniform PRNG draw (rather than e.g.
            // `i % 3 == 0`) so squares aren't bunched in a spatial
            // cluster — that way the visual contrast between filtered
            // squares and lit-up rounds is visible everywhere on
            // screen, not just in one stripe.
            let is_square = rng.next_f32(0.0, 1.0) < SQUARE_FRACTION;
            let (shape, kind) = if is_square {
                (
                    Shape::aabb(Vec2::new(x, y), Vec2::splat(POINT_RADIUS)),
                    ScatterKind::Square,
                )
            } else {
                round_count += 1;
                (
                    Shape::circle(Vec2::new(x, y), POINT_RADIUS),
                    ScatterKind::Round,
                )
            };
            let id = aoi.insert(shape);
            points.push(id);
            point_to_index.insert(id, i);
            scatter_kind_map.insert(id, kind);
        }
        // `Arc` so each observer's filter closure can hold a cheap
        // clone — closures stored in vibe_aoi must be `'static`, so
        // they can't borrow the demo's table directly.
        let scatter_kind = Arc::new(scatter_kind_map);
        let cover_count = vec![0u32; NUM_POINTS];

        // LOD starts off so the demo's first impression is the
        // baseline behavior; pressing `[L]` then visibly shrinks
        // each observer's lit core to LOD_RADIUS.
        let lod_enabled = false;

        // Three circles, each with a distinct color and an off-axis
        // velocity so they cross paths and overlap occasionally — this
        // is what exercises the per-point cover_count refcount.
        let circle_specs = [
            (
                Vec2::new(WORLD_W * 0.25, WORLD_H * 0.30),
                Vec2::new(95.0, 60.0),
                Color::from_hex(0xFFD13F), // amber
            ),
            (
                Vec2::new(WORLD_W * 0.75, WORLD_H * 0.65),
                Vec2::new(-72.0, 88.0),
                Color::from_hex(0x4FC3F7), // sky blue
            ),
            (
                Vec2::new(WORLD_W * 0.50, WORLD_H * 0.50),
                Vec2::new(110.0, -55.0),
                Color::from_hex(0xE57373), // coral
            ),
        ];
        let mut circles = Vec::with_capacity(circle_specs.len());
        for (pos, vel, color) in circle_specs {
            let observer = aoi.create_observer_filtered(
                Shape::circle(pos, CIRCLE_RADIUS),
                Self::build_filter(scatter_kind.clone(), lod_enabled),
            );
            circles.push(MovingCircle {
                pos,
                vel,
                observer,
                color,
            });
        }

        let stats = aoi.stats();

        let mut demo = Self {
            aoi,
            points,
            point_to_index,
            scatter_kind,
            cover_count,
            circles,
            lod_enabled,
            last_stats: stats,
            enter_count_total: 0,
            leave_count_total: 0,
            round_count,
            paused: false,
            white_tex,
            circle_filled_tex,
            circle_ring_tex,
        };

        // Drain the initial Enter events from each observer so frame 0
        // is visually consistent (points already inside a circle at
        // spawn time start lit). `sync_circle_observer` also bumps the
        // global enter/leave totals.
        for i in 0..demo.circles.len() {
            demo.sync_circle_observer(i);
        }
        demo
    }

    fn update(&mut self, _ctx: &mut Context, dt: f32, input: &InputState) {
        if input.is_action_just_pressed("pause") {
            self.paused = !self.paused;
        }
        // LOD toggle is processed even while paused so testers can
        // freeze the demo and then poke the LOD lever to study the
        // diff in isolation.
        if input.is_action_just_pressed("toggle_lod") {
            self.lod_enabled = !self.lod_enabled;
            // Install fresh filters everywhere — the next per-circle
            // sync below will then diff against the new visibility set
            // and emit the Enter/Leave events that drive cover_count
            // updates. (When paused, we still want the visible state
            // to update on the toggle, so we sync immediately.)
            self.refresh_filters();
            for i in 0..self.circles.len() {
                self.sync_circle_observer(i);
            }
            self.last_stats = self.aoi.stats();
        }
        if self.paused {
            return;
        }

        // Move each circle and re-sync its observer. We split into two
        // passes (motion → AOI sync) instead of interleaving so the
        // borrow story stays simple: motion takes &mut self.circles,
        // sync takes &mut self (because it touches both aoi and
        // cover_count).
        for c in &mut self.circles {
            Self::step_circle_motion(c, dt);
        }
        for i in 0..self.circles.len() {
            self.sync_circle_observer(i);
        }

        self.last_stats = self.aoi.stats();
    }

    fn update_ui(&mut self, ctx: &mut Context, input: &InputState) {
        // ── Lazy font preparation ──
        // Make sure every digit / letter the stats panel will draw this
        // frame is in the atlas before the UI builder lays it out, so
        // the first frame after a counter rolls into a new digit width
        // doesn't fall back to the half-em advance.
        let stats_lines = self.format_stats_lines();
        let mut to_prepare = String::with_capacity(256);
        for line in &stats_lines {
            to_prepare.push_str(line);
            to_prepare.push('\n');
        }
        ctx.prepare_text("body", &to_prepare);

        // Build the stats panel via the UI system so it composites on
        // top of the world via the engine's UI pipeline instead of
        // being baked into the world layer (where it would have to
        // contend with the rings drawn in `draw`).
        let vw = ctx.virtual_width;
        let vh = ctx.virtual_height;

        // Take ui_state out so we can borrow ctx.assets independently
        // of the UiContext (same pattern as the ui-demo example).
        let mut ui_state = std::mem::take(&mut ctx.ui_state);
        let mut ui = UiContext::new(&mut ui_state, input, vw, vh);

        // Place the stats panel in the side gutter to the right of
        // the AOI world. The world occupies [0, WORLD_W); we anchor the
        // panel at (WORLD_W + 8, 8) so it sits cleanly outside the play
        // area and never occludes scatter points.
        ui.set_anchor(Anchor::TopLeft);
        ui.set_padding(0.0);
        ui.set_cursor(WORLD_W + 8.0, 8.0);
        ui.set_spacing(2.0);

        if let Some(font) = ctx.assets.font("body") {
            // Opaque background — the gutter has no world content under
            // it, so we don't need any transparency, and a solid panel
            // reads more clearly as "this is a side bar, not an
            // overlay".
            let panel_style = PanelStyle {
                bg_color: UiColor::new(0.10, 0.12, 0.16, 1.0),
                padding: 8.0,
            };
            ui.panel(panel_style, |ui| {
                ui.label_colored(font, "AOI Stats", UiColor::from_hex(0x55BBFF));
                for line in &stats_lines {
                    ui.label(font, line);
                }
            });
        }

        ui.finish();
        ctx.ui_state = ui_state;
    }

    fn draw(&self, ctx: &Context, screen: &mut Screen) {
        // The demo registered its own 1×1 white pixel texture in
        // `new()` (see `Renderer::create_white_pixel_texture`); we
        // reuse it for the gutter separator and the square scatter
        // dots.
        let white = self.white_tex;

        // ── Vertical separator between the world and the side gutter ──
        // The clear color paints the entire virtual canvas, including
        // the gutter where the stats panel lives. A 1-px line at x =
        // WORLD_W gives a subtle visual "this is where the AOI world
        // ends" cue, so it's clear the bouncing rings really are
        // bounded by the world rect and not by some arbitrary edge of
        // the panel.
        screen.draw_sprite_tinted(
            white,
            WORLD_W,
            0.0,
            1.0,
            ctx.virtual_height,
            Color::from_hex(0x2A2D33),
        );

        // ── Scatter dots (drawn first so the rings overlay them) ──
        // Two visual kinds, matching the AOI Shape used at insertion:
        //   • Round → `Shape::Circle` → `draw_circle` (antialiased disc)
        //   • Square → `Shape::Aabb`  → `draw_sprite_tinted` (crisp pixel square)
        // Squares always render dim because the observer filter
        // rejects them, so `cover_count` for a square stays 0
        // forever — by construction. We still drive the lit decision
        // off `cover_count` (rather than short-circuiting on kind)
        // so the visualization stays a pure function of AOI state:
        // any future bug that lets a square leak through the filter
        // would be immediately visible as a green square.
        let lit = Color::from_hex(0x6BFF6B); // green
        let dim_round = Color::from_hex(0x666666); // gray, slightly cool
        let dim_square = Color::from_hex(0x4A4A55); // gray, slightly cool blue
        for (i, &id) in self.points.iter().enumerate() {
            let Some(shape) = self.aoi.get(id) else {
                continue;
            };
            let color = if self.cover_count[i] > 0 {
                lit
            } else {
                match self.scatter_kind.get(&id) {
                    Some(ScatterKind::Square) => dim_square,
                    _ => dim_round,
                }
            };
            match shape {
                Shape::Circle { center, radius } => {
                    screen.draw_circle(self.circle_filled_tex, center.x, center.y, radius, color);
                }
                Shape::Aabb {
                    center,
                    half_extents,
                } => {
                    let w = half_extents.x * 2.0;
                    let h = half_extents.y * 2.0;
                    screen.draw_sprite_tinted(
                        white,
                        center.x - half_extents.x,
                        center.y - half_extents.y,
                        w,
                        h,
                        color,
                    );
                }
                Shape::Point(_) => {} // demo doesn't insert raw Points
            }
        }

        // ── Ring outlines (transparent — dots underneath stay lit) ──
        // `draw_circle_outline` blits the demo's procedural ring
        // texture, which is alpha-AA'd on both inner and outer edges.
        // When LOD is active we also draw an inner ring at LOD_RADIUS
        // so it's obvious *why* the lit core is smaller than the
        // broadphase ring — without this hint, players might wonder
        // if rounds at the edge are bugged.
        for c in &self.circles {
            screen.draw_circle_outline(
                self.circle_ring_tex,
                c.pos.x,
                c.pos.y,
                CIRCLE_RADIUS,
                c.color,
            );
            if self.lod_enabled {
                // Half-alpha tint so the LOD ring reads as
                // secondary/diagnostic rather than as another full
                // observer boundary.
                let mut lod_color = c.color;
                lod_color.a *= 0.45;
                screen.draw_circle_outline(
                    self.circle_ring_tex,
                    c.pos.x,
                    c.pos.y,
                    LOD_RADIUS,
                    lod_color,
                );
            }
        }
    }

    fn clear_color(&self) -> Color {
        Color::from_hex(0x121417)
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        let circles: Vec<_> = self
            .circles
            .iter()
            .map(|c| {
                serde_json::json!({
                    "x": c.pos.x,
                    "y": c.pos.y,
                    "vx": c.vel.x,
                    "vy": c.vel.y,
                    "radius": CIRCLE_RADIUS,
                })
            })
            .collect();
        serde_json::json!({
            "circles": circles,
            "lit_count": self.lit_count(),
            "enter_total": self.enter_count_total,
            "leave_total": self.leave_count_total,
            "paused": self.paused,
        })
    }

    #[cfg(feature = "vdp")]
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if method.starts_with("aoi.") {
            // Forward the whole `aoi.*` namespace straight to the AOI
            // world so VDP clients (and the integration test) get the
            // same view of the spatial state the game does.
            return self.aoi.handle_vdp(method, params);
        }
        match method {
            // Toggle pause from outside (for deterministic stepping in
            // tests — pair with `engine.step` to advance frame-by-frame).
            "demo.setPaused" => {
                let p = params
                    .get("paused")
                    .and_then(|v| v.as_bool())
                    .ok_or("missing bool param `paused`")?;
                self.paused = p;
                Ok(serde_json::json!({ "paused": self.paused }))
            }
            // Teleport one specific circle. Useful for asserting that
            // moving an observer onto a known point triggers an Enter
            // event.
            //
            // Params: `{ "index": 0|1|2, "x": f32, "y": f32 }`.
            //
            // We deliberately re-run `update_observer` and apply the
            // pending events **here** rather than waiting for the next
            // `update()` tick, because tests typically `pause()` first
            // and then teleport — and `update()` short-circuits while
            // paused, which would leave `cover_count` stale and make
            // the next VDP `inspect()` lie about the lit set.
            "demo.setCirclePos" => {
                let idx = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(0);
                if idx >= self.circles.len() {
                    return Err(format!(
                        "circle index {} out of range (have {})",
                        idx,
                        self.circles.len()
                    ));
                }
                let x = params
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or("missing number param `x`")? as f32;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or("missing number param `y`")? as f32;
                // Stop *all* circles and freeze the demo so subsequent
                // `step_and_wait` calls don't drift any observer onto a
                // different hit set than the test just asserted. Tests
                // that need to resume motion can re-enable with
                // `demo.setPaused {paused: false}`.
                for c in &mut self.circles {
                    c.vel = Vec2::ZERO;
                }
                self.circles[idx].pos = Vec2::new(x, y);
                self.paused = true;
                self.sync_circle_observer(idx);
                Ok(serde_json::json!({ "index": idx, "x": x, "y": y }))
            }
            _ => Err(format!("Unknown method: {method}")),
        }
    }
}

impl AoiDemo {
    /// Build the per-frame stats label strings. Pulled out so `update_ui`
    /// can `prepare_text` them and the `panel { ... }` builder can render
    /// them without recomputing.
    ///
    /// Layout note: every line is just `"key: value"` flush left. We
    /// intentionally don't pad with spaces to align the value column —
    /// the body font is proportional, so a space is narrower than a
    /// digit and any "manual table" we'd build with `format!("{:8}…")`
    /// looks crooked anyway. Keeping it left-aligned and compact reads
    /// cleaner than a fake table that almost-but-doesn't quite line up.
    fn format_stats_lines(&self) -> Vec<String> {
        let squares = self
            .last_stats
            .entity_count
            .saturating_sub(self.round_count);
        vec![
            format!("entities: {}", self.last_stats.entity_count),
            format!("round: {}", self.round_count),
            format!("square: {} (filtered)", squares),
            format!("cells: {}", self.last_stats.cell_count),
            format!("max/cell: {}", self.last_stats.max_entities_per_cell),
            format!("avg/cell: {:.2}", self.last_stats.avg_entities_per_cell),
            format!("circles: {}", self.circles.len()),
            format!("hits: {}", self.lit_count()),
            format!("enters: {}", self.enter_count_total),
            format!("leaves: {}", self.leave_count_total),
            format!("lod: {}", if self.lod_enabled { "on" } else { "off" }),
            // Keybinds — separated visually by being last; no padding
            // tricks needed because `[Space]` and `[L]` already have
            // distinct prefixes.
            format!("[Space] {}", if self.paused { "resume" } else { "pause" }),
            "[L] toggle lod".to_string(),
        ]
    }
}

// ── Tiny deterministic PRNG ──────────────────────────────────────────
//
// Same LCG used in the vibe_aoi consistency tests. Inlined here rather
// than exposed from the library because randomness is incidental to
// AOI's public contract.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.state >> 16) as u32
    }
    fn next_f32(&mut self, lo: f32, hi: f32) -> f32 {
        let t = (self.next_u32() as f32) / (u32::MAX as f32);
        lo + t * (hi - lo)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    vibe2d::run::<AoiDemo>("game.yaml");
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    wasm_bindgen_futures::spawn_local(async {
        vibe2d::run_web::<AoiDemo>("game.yaml").await;
    });
}
