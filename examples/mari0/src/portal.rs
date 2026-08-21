//! The portal gun: projectiles, the two portal slots, and teleportation.
//!
//! A portal is one tile wide and two tiles long, mounted flush to a face, and it is
//! identified by the tile it is anchored to — see [`PortalAnchor`], which explains
//! why the anchor is normalised differently per face. The placement rules and the
//! coordinate transform live in `portal_math`, as pure functions; this file is the
//! glue that fires shots, decides what they hit, and moves the player through.
//!
//! A single portal is not a hole: both must exist before anything routes through,
//! which is why every entry test starts by requiring both slots.
//!
//! The entry rules are shared by every mover via [`portal_sweep`] — Mario, enemies,
//! items and fireballs all travel by one rule, as they do in the original, where each
//! object calls `checkportal*` from its own movement code.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::enemies::{EnemyState, EnemyType};
use crate::game::Mari0Game;
use crate::physics::*;
use crate::player::Orientation;
use crate::portal_math::{PortalAnchor, portal_position, portal_transform, tendency_for};
use crate::world::Level;

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

    /// The coordinate of the mouth's plane along its own normal, in world pixels.
    ///
    /// For an up/down face that's a `y`; for left/right an `x`. Matches the
    /// original's plane, which is the anchor row/column adjusted per facing
    /// (`portalY - 1` for up, `portalX - 1` for left, as-is otherwise).
    pub(crate) fn plane(&self) -> f32 {
        let (cx, cy) = self.centre();
        match self.orientation() {
            Orientation::Up | Orientation::Down => cy,
            Orientation::Left | Orientation::Right => cx,
        }
    }

    /// Columns the mouth spans (up/down faces).
    pub(crate) fn cols(&self) -> [i32; 2] {
        let c = self.anchor.cells();
        [c[0].0, c[1].0]
    }

    /// Rows the mouth spans (left/right faces).
    pub(crate) fn rows(&self) -> [i32; 2] {
        let c = self.anchor.cells();
        [c[0].1, c[1].1]
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

    /// Leave a portal fast enough and the screen detonates.
    ///
    /// Only up, left and right count (`mario.lua:3097-3110`) — there is no rainboom for
    /// being fired *downward*, presumably because gravity gets you there for free. Each one
    /// spends `rainboom_allowed`, which only a floor restores, so it is once per landing.
    pub(crate) fn check_rainboom(&mut self, ctx: &Context) {
        if !self.sonic_rainboom || !self.rainboom_allowed {
            return;
        }
        // The original is handed the exit portal's facing and checks the matching velocity
        // component against the threshold. Reading the direction back *out* of the exit
        // velocity is the same test with one fewer thing to thread through: only one
        // component can be over 45 blocks/s, and its sign is the direction.
        let rotation = if self.player.vy < -RAINBOOM_SPEED {
            -std::f32::consts::FRAC_PI_2
        } else if self.player.vx > RAINBOOM_SPEED {
            0.0
        } else if self.player.vx < -RAINBOOM_SPEED {
            std::f32::consts::PI
        } else {
            // Including every downward exit: there is no rainboom for being fired *down*,
            // presumably because gravity gets you there for free.
            return;
        };
        self.rainbooms.push(crate::effects::Rainboom {
            x: self.player.center_x(),
            y: self.player.center_y(),
            rotation,
            timer: 0.0,
            frame: 0,
        });
        self.earthquake = RAINBOOM_EARTHQUAKE;
        self.rainboom_allowed = false;
        ctx.audio.play("rainboom");
        self.rainboom_clear_enemies();
        // And you keep the hat (`mario.lua:3133`). It replaces the stack rather than
        // adding to it, so whatever you picked in the menu is gone — you broke the sound
        // barrier, you wear what that earns.
        self.hats = vec![crate::hats::HAT_BEST_PONY];
    }

    /// Everything on screen dies (`mario.lua:3115-3131`).
    ///
    /// Not a side effect of the shockwave — the loop has no range test, so it takes the
    /// whole level's live enemies at once, off-screen ones included. Which kinds are
    /// eligible is [`EnemyType::cleared_by_rainboom`]'s business.
    ///
    /// Points are paid at the flat fire rate, the same as a fireball kill, and float up
    /// where each body was (`addpoints`). Bowser is paid for here rather than in the sweep's
    /// `else` branch as the original has it, because the branch he is routed to instead
    /// (`bowser:firedeath`, `bowser.lua:193`) is where the original pays *his* 5000 — and
    /// this port has no `firedeath`. Same total, one call site, as on the fireball path.
    fn rainboom_clear_enemies(&mut self) {
        let mut popups = Vec::new();
        for enemy in &mut self.enemies {
            if !enemy.enemy_type.cleared_by_rainboom() || enemy.state == EnemyState::Dead {
                continue;
            }
            if enemy.enemy_type == EnemyType::Bowser {
                enemy.hp = enemy.hp.saturating_sub(RAINBOOM_BOWSER_HITS);
                if enemy.hp > 0 {
                    continue;
                }
            }
            popups.push((enemy.x, enemy.y, enemy.enemy_type.fire_points()));
            enemy.shotted();
        }
        for (x, y, value) in popups {
            self.score += value;
            self.score_popups.push(crate::effects::ScorePopup {
                x,
                y,
                value: Some(value),
                timer: 0.0,
            });
        }
    }

    /// Step the rainbooms and let the shake decay.
    ///
    /// The decay is proportional to the shake itself plus a floor
    /// (`game.lua:139`), so it falls off fast at first and then stops dead rather than
    /// trailing away asymptotically.
    pub(crate) fn update_rainbooms(&mut self, dt: f32) {
        if self.earthquake > 0.0 {
            self.earthquake = (self.earthquake - dt * self.earthquake * 2.0 - 0.001).max(0.0);
        }
        for r in &mut self.rainbooms {
            r.timer += dt;
            while r.timer > RAINBOOM_DELAY {
                r.timer -= RAINBOOM_DELAY;
                r.frame += 1;
            }
        }
        self.rainbooms.retain(|r| r.frame < RAINBOOM_FRAMES);
    }

    /// Spawn and drift the dust coming out of the open portals.
    ///
    /// The wander is a deterministic function of the particle's index and the shared
    /// animation clock rather than `math.random`, for the same reason every other
    /// randomness in this port is: a replay has to come out the same way twice. The plume
    /// still reads as a plume.
    pub(crate) fn update_portal_particles(&mut self, dt: f32) {
        self.portal_particle_timer += dt;
        while self.portal_particle_timer > PORTAL_PARTICLE_TIME {
            self.portal_particle_timer -= PORTAL_PARTICLE_TIME;
            for index in 0..2 {
                let Some(portal) = &self.portals[index] else {
                    continue;
                };
                if !portal.active {
                    continue;
                }
                let facing = portal.anchor.facing;
                let (cx, cy) = portal.centre();
                let (vx, vy) = match facing {
                    crate::player::Orientation::Left => (-PORTAL_PARTICLE_SPEED, 0.0),
                    crate::player::Orientation::Right => (PORTAL_PARTICLE_SPEED, 0.0),
                    crate::player::Orientation::Up => (0.0, -PORTAL_PARTICLE_SPEED),
                    crate::player::Orientation::Down => (0.0, PORTAL_PARTICLE_SPEED),
                };
                self.portal_particles.push(crate::effects::PortalParticle {
                    x: cx,
                    y: cy,
                    vx,
                    vy,
                    timer: 0.0,
                    portal: index,
                    facing_up: facing == crate::player::Orientation::Up,
                });
            }
        }
        for (i, p) in self.portal_particles.iter_mut().enumerate() {
            p.timer += dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            let phase = self.coin_spin * 7.0 + i as f32 * 2.3;
            p.vx += phase.sin() * PORTAL_PARTICLE_WANDER * dt * 60.0;
            p.vy += phase.cos() * PORTAL_PARTICLE_WANDER * dt * 60.0;
            // A floor portal's plume never rains back into it.
            if p.facing_up && p.vy > 0.0 {
                p.vy = 0.0;
            }
        }
        self.portal_particles
            .retain(|p| p.timer < PORTAL_PARTICLE_DURATION);
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

    /// Run the three entry tests for the player.
    ///
    /// `next_x`/`next_y` are where the player is about to move to. See
    /// [`portal_sweep`] for why the swept form matters.
    pub(crate) fn check_portal_entry(&mut self, ctx: &Context, next_x: f32, next_y: f32) -> bool {
        if self.player.teleport_cooldown > 0.0 {
            return false;
        }
        let Some((p0, p1)) = self.portal_pair() else {
            return false;
        };
        let body = PortalBody {
            x: self.player.x,
            y: self.player.y,
            w: self.player.width,
            h: self.player.height,
            vx: self.player.vx,
            vy: self.player.vy,
        };
        match portal_sweep(&self.level, (&p0, &p1), body, next_x, next_y) {
            PortalOutcome::None => false,
            PortalOutcome::Bounced { vx, vy } => {
                self.player.vx = vx;
                self.player.vy = vy;
                self.player.is_jumping = false;
                true
            }
            PortalOutcome::Through {
                exit,
                straight_through,
            } => {
                self.player.x = exit.x;
                self.player.y = exit.y;
                self.player.vx = exit.vx;
                self.player.vy = exit.vy;
                self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
                self.player.on_ground = false;
                if !straight_through {
                    self.player.is_jumping = false;
                }
                ctx.audio.play("portalenter");
                self.check_rainboom(ctx);
                true
            }
        }
    }

    /// Both portals, in slot order, or `None` unless the pair is complete.
    pub(crate) fn portal_pair(&self) -> Option<(Portal, Portal)> {
        match (&self.portals[0], &self.portals[1]) {
            (Some(a), Some(b)) if a.active && b.active => Some((a.clone(), b.clone())),
            _ => None,
        }
    }

    /// Containment fallback for the player: teleport if the centre cell is inside a
    /// mouth.
    ///
    /// Run *after* movement and deliberately **without** a clearance check — the
    /// original's `inportal` teleports unconditionally. It exists to catch bodies the
    /// two swept tests missed, so refusing here would leave them stuck in a wall.
    pub(crate) fn check_in_portal(&mut self, ctx: &Context) -> bool {
        if self.player.teleport_cooldown > 0.0 {
            return false;
        }
        let Some((p0, p1)) = self.portal_pair() else {
            return false;
        };
        let body = PortalBody {
            x: self.player.x,
            y: self.player.y,
            w: self.player.width,
            h: self.player.height,
            vx: self.player.vx,
            vy: self.player.vy,
        };
        let Some(exit) = portal_containment((&p0, &p1), body) else {
            return false;
        };
        self.player.x = exit.x;
        self.player.y = exit.y;
        self.player.vx = exit.vx;
        self.player.vy = exit.vy;
        self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
        self.player.on_ground = false;
        ctx.audio.play("portalenter");
        true
    }
}

/// Carry one non-player body through a portal, if it is entering one.
///
/// Returns the new `(x, y, vx, vy)` when something happened. `allow_containment` is
/// the original's `mask[2]`: fireballs, bullet bills and thrown hammers take the
/// swept tests but are exempt from the `inportal` fallback, so they can't be snapped
/// through a mouth they were merely crossing.
///
/// A free function rather than a method so a caller can hold `&mut self.enemies`
/// while passing `&self.level` — the two are disjoint fields, which a method on
/// `&self` would hide from the borrow checker.
pub(crate) fn portal_carry(
    level: &Level,
    pair: Option<&(Portal, Portal)>,
    body: PortalBody,
    dt: f32,
    allow_containment: bool,
) -> Option<(f32, f32, f32, f32)> {
    let (p0, p1) = pair?;
    let next_x = body.x + body.vx * dt;
    let next_y = body.y + body.vy * dt;
    match portal_sweep(level, (p0, p1), body, next_x, next_y) {
        PortalOutcome::Through { exit, .. } => Some((exit.x, exit.y, exit.vx, exit.vy)),
        PortalOutcome::Bounced { vx, vy } => Some((body.x, body.y, vx, vy)),
        PortalOutcome::None if allow_containment => {
            portal_containment((p0, p1), body).map(|e| (e.x, e.y, e.vx, e.vy))
        }
        PortalOutcome::None => None,
    }
}

/// A body that can travel through a portal.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PortalBody {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
}

/// What a sweep decided.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PortalOutcome {
    /// Nothing to do.
    None,
    /// Went through. `straight_through` marks an up↔down pair, which keeps its jump
    /// state because it is a plain fall-through rather than a change of direction.
    Through {
        exit: crate::portal_math::Exit,
        straight_through: bool,
    },
    /// The exit was obstructed, so the body bounced off the mouth instead.
    Bounced { vx: f32, vy: f32 },
}

/// The two swept entry tests, in the original's order, for any body.
///
/// `checkportalVER` then `checkportalHOR` (`physics.lua:524-714`). Each asks whether
/// the body's centre **crossed the mouth's plane during this step**, which is what
/// catches a body moving fast enough to clear the whole mouth within one frame — an
/// overlap test can only ever notice a body that happens to be inside the mouth on
/// the frame you look.
///
/// VER runs first so a corner qualifying for both resolves as a wall portal rather
/// than a floor one.
///
/// Every mover shares this, which is the point: enemies, items, fireballs and Mario
/// all travel by the same rule, as they do in the original, where each object calls
/// these from its own movement code.
pub(crate) fn portal_sweep(
    level: &Level,
    (p0, p1): (&Portal, &Portal),
    body: PortalBody,
    next_x: f32,
    next_y: f32,
) -> PortalOutcome {
    // ── VER: the vertical mouths (left/right faces) ──
    // Row test uses the body's **top edge** cell (`math.floor(self.y+1)`).
    let row = (body.y / TILE_SIZE).floor() as i32;
    let half_w = body.w / 2.0;
    for (entry, exit) in [(p0, p1), (p1, p0)] {
        if !matches!(entry.orientation(), Orientation::Left | Orientation::Right) {
            continue;
        }
        if !entry.rows().contains(&row)
            || !in_range(entry.plane(), body.x + half_w, next_x + half_w)
        {
            continue;
        }
        let heading_in = match entry.orientation() {
            Orientation::Right => body.vx <= 0.0,
            Orientation::Left => body.vx >= 0.0,
            _ => false,
        };
        if !heading_in {
            continue;
        }
        let out = transform_body(body, entry, exit);
        return if rect_is_clear(level, out.x, out.y, body.w, body.h) {
            PortalOutcome::Through {
                exit: out,
                straight_through: false,
            }
        } else {
            // A blocked wall portal bounces with **no damping and no minimum**,
            // unlike the floor case below.
            PortalOutcome::Bounced {
                vx: -body.vx,
                vy: body.vy,
            }
        };
    }

    // ── HOR: the horizontal mouths (up/down faces) ──
    // Column test uses the body's **left edge** cell (`math.floor(self.x+1)`).
    let col = (body.x / TILE_SIZE).floor() as i32;
    let half_h = body.h / 2.0;
    for (entry, exit) in [(p0, p1), (p1, p0)] {
        if !matches!(entry.orientation(), Orientation::Up | Orientation::Down) {
            continue;
        }
        if !entry.cols().contains(&col)
            || !in_range(entry.plane(), body.y + half_h, next_y + half_h)
        {
            continue;
        }
        let heading_in = match entry.orientation() {
            Orientation::Up => body.vy >= 0.0,
            Orientation::Down => body.vy <= 0.0,
            _ => false,
        };
        if !heading_in {
            continue;
        }
        let out = transform_body(body, entry, exit);
        return if rect_is_clear(level, out.x, out.y, body.w, body.h) {
            PortalOutcome::Through {
                exit: out,
                straight_through: matches!(
                    (entry.orientation(), exit.orientation()),
                    (Orientation::Up, Orientation::Down) | (Orientation::Down, Orientation::Up)
                ),
            }
        } else {
            // A blocked floor/ceiling portal loses a little speed and has a minimum
            // magnitude, so a body can't settle into a zero-speed oscillation.
            let mut vy = -body.vy * 0.95;
            const MIN_BOUNCE: f32 = 2.0 * TILE_SIZE;
            if vy.abs() < MIN_BOUNCE {
                vy = MIN_BOUNCE * if vy < 0.0 { -1.0 } else { 1.0 };
            }
            PortalOutcome::Bounced { vx: body.vx, vy }
        };
    }

    PortalOutcome::None
}

/// The containment fallback (`inportal`) for any body: is its centre cell inside a
/// mouth?
///
/// No clearance check, by design — see [`Mari0Game::check_in_portal`].
pub(crate) fn portal_containment(
    (p0, p1): (&Portal, &Portal),
    body: PortalBody,
) -> Option<crate::portal_math::Exit> {
    let cell = (
        ((body.x + body.w / 2.0) / TILE_SIZE).floor() as i32,
        ((body.y + body.h / 2.0) / TILE_SIZE).floor() as i32,
    );
    for (entry, exit) in [(p0, p1), (p1, p0)] {
        if entry.anchor.cells().contains(&cell) {
            return Some(transform_body(body, entry, exit));
        }
    }
    None
}

/// Transform a body through a portal pair.
///
/// `live` is always true. The original passes it on `inportal`'s *first* branch and
/// omits it on the second, so the minimum exit speed applied only when entering
/// through portal 1 — the branches are otherwise identical, so that is a copy-paste
/// slip rather than a rule. **Corrected**: an asymmetry between the two portals is
/// something no player could predict or use.
fn transform_body(body: PortalBody, entry: &Portal, exit: &Portal) -> crate::portal_math::Exit {
    portal_transform(
        body.x,
        body.y,
        body.w,
        body.h,
        body.vx,
        body.vy,
        0.0,
        entry.anchor,
        exit.anchor,
        GRAVITY,
        true,
    )
}
