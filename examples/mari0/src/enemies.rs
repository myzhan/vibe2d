//! The enemy roster: what each kind is, how tall it is, and how it moves.
//!
//! Scripted kinds (firebars, geysers, cheep-cheeps) ignore the walker logic
//! entirely — see `EnemyType::is_scripted`.

use std::collections::HashMap;

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::*;
use crate::portal::{PortalBody, portal_carry};
use crate::world::EnemySpawnPoint;

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum EnemyType {
    Goomba,
    Koopa,
    /// Red koopa: identical to a green one except it refuses to walk off ledges.
    KoopaRed,
    /// Buzzy beetle: a koopa that shrugs off fireballs.
    Beetle,
    /// Piranha plant. Rises and retracts on a timer, never moves horizontally,
    /// and cannot be stomped — only fire or a star kills it.
    Plant,
    /// Winged koopa. Hovers on a cosine path; stomping it removes the wings and
    /// leaves an ordinary koopa behind.
    KoopaFlying,
    /// One fireball of a rotating firebar. Indestructible; contact hurts.
    ///
    /// Each segment is its own entity carrying the bar's pivot and its index, so
    /// the whole bar is just N of these sharing a pivot.
    Firebar,
    /// Lava geyser: leaps out of the bottom of the world and falls back.
    UpFire,
    /// Cheep-cheep. Red swims fast and level; white drifts slower and bobs.
    CheepRed,
    CheepWhite,
    /// Lakitu: rides a cloud above the level, matches the player's pace and lobs
    /// spiny eggs at him. Never touched by gravity or terrain.
    Lakito,
    /// A spiny, walking. Mechanically a goomba that cannot be stomped — the
    /// original literally builds it as one (`goomba.lua:48`, `t = "spikey"`), which
    /// is why it shares the walker path and the goomba's speed and animation rate.
    Spikey,
    /// A spiny still in its egg, arcing through the air after lakitu throws it.
    ///
    /// Its own kind rather than a flag because three things differ: it falls at
    /// 30 blocks/s² instead of 80, it drifts with no horizontal speed, and for the
    /// first two blocks of its descent it can strike the lakitu who threw it.
    /// Landing turns it into a [`EnemyType::Spikey`].
    SpikeyFall,
}

impl EnemyType {
    /// Does this behave as a koopa (shell mechanics, 24px-tall sprite)?
    pub(crate) fn is_koopa_like(self) -> bool {
        matches!(
            self,
            EnemyType::Koopa | EnemyType::KoopaRed | EnemyType::Beetle
        )
    }

    /// Red koopas turn around at a ledge instead of walking off.
    pub(crate) fn avoids_ledges(self) -> bool {
        self == EnemyType::KoopaRed
    }

    /// Buzzy beetles are immune to fireballs (that's their whole point).
    pub(crate) fn fireball_immune(self) -> bool {
        self == EnemyType::Beetle
    }

    /// Can the player kill this by landing on it?
    ///
    /// Plants, firebars and geysers hurt from every direction — jumping on a
    /// firebar is how you die, not how you win. So does a spiny, and that is the
    /// whole point of one: the original's test is a single inequality on the
    /// goomba's subtype, `a == "goomba" and b.t ~= "goomba"` → kill
    /// (`mario.lua:1778`), so anything built as a goomba that *isn't* a goomba
    /// hurts from above as well.
    pub(crate) fn stompable(self) -> bool {
        !matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::Spikey
                | EnemyType::SpikeyFall
        )
    }

    /// Enemies that ignore gravity and terrain and follow their own path.
    ///
    /// Lakitu is in here on a small liberty: he does carry tile collision in the
    /// original (`lakito.lua:18`, mask index 2 is the tile category), but all three
    /// levels that place one — 4-1, 6-1 and 8-2 — are empty of solid tiles for the
    /// four rows he flies in, so a wall is something he can never reach. Letting him
    /// ignore terrain costs nothing observable and keeps him out of the walker path,
    /// which would otherwise reverse him at every wall he doesn't touch.
    pub(crate) fn is_scripted(self) -> bool {
        matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::CheepRed
                | EnemyType::CheepWhite
                | EnemyType::KoopaFlying
                | EnemyType::Lakito
        )
    }

    /// Downward acceleration while walking or falling.
    ///
    /// Only the thrown spiny egg differs from the world's gravity, and it differs a
    /// lot — see [`SPIKEY_FALL_GRAVITY`].
    pub(crate) fn gravity(self) -> f32 {
        match self {
            EnemyType::SpikeyFall => SPIKEY_FALL_GRAVITY,
            _ => GRAVITY,
        }
    }

    /// Can this kind travel through a portal?
    ///
    /// Two separate reasons a kind can't, both from the original:
    ///
    /// - **`static = true`** — plants, firebars and lava geysers are fixtures. They
    ///   have a position but never move, so the mover code that would carry them
    ///   through a portal never runs (`plant.lua:15`, `castlefire.lua:84`,
    ///   `upfire.lua:16`).
    /// - **`portalable = false`** — cheep-cheeps opt out explicitly even though they
    ///   do move (`cheepcheep.lua:33`), and so does lakitu (`lakito.lua:24`).
    pub(crate) fn portalable(self) -> bool {
        !matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::CheepRed
                | EnemyType::CheepWhite
                | EnemyType::Lakito
        )
    }

    /// Indestructible hazards: fire and stars don't remove them either.
    pub(crate) fn indestructible(self) -> bool {
        matches!(self, EnemyType::Firebar | EnemyType::UpFire)
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum EnemyState {
    Walking,
    Dead,
    Shell,
    ShellMoving,
}

#[derive(Clone)]
pub(crate) struct Enemy {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) enemy_type: EnemyType,
    pub(crate) state: EnemyState,
    pub(crate) facing_right: bool,
    pub(crate) on_ground: bool,
    pub(crate) anim_timer: f32,
    pub(crate) death_timer: f32,
    pub(crate) flipped_death: bool, // true = star/fireball kill (flip + fly off)
    /// Y position the enemy spawned at. Plants oscillate around it.
    pub(crate) spawn_y: f32,
    /// Plant emerge/retract cycle position, in seconds. Also the firebar's
    /// accumulator and the flying koopa's hover phase.
    pub(crate) cycle_timer: f32,
    /// X position the enemy spawned at. Firebars rotate around it.
    pub(crate) spawn_x: f32,
    /// Current firebar angle in degrees.
    pub(crate) angle_deg: f32,
    /// Index of this fireball along its firebar (0 = at the pivot).
    pub(crate) segment: u32,
}

/// Collision height of an enemy in its current state.
///
/// Koopa-likes stand 24px tall (48 at 2x) while walking but shrink to a shell;
/// everything else is one small-Mario tall.
pub(crate) fn enemy_height(enemy_type: EnemyType, state: EnemyState) -> f32 {
    if enemy_type.is_koopa_like() && state == EnemyState::Walking {
        48.0
    } else {
        PLAYER_SMALL_H
    }
}

impl Enemy {
    /// Instantiate one spawn point.
    fn from_spawn(sp: &EnemySpawnPoint) -> Self {
        let h = enemy_height(sp.enemy_type, EnemyState::Walking);
        Enemy {
            x: sp.x,
            // Spawn coordinates name the cell the enemy stands *on*, so a taller
            // enemy has to be lifted by its own height to rest on that surface.
            y: sp.y - h,
            vx: if sp.facing_right {
                ENEMY_SPEED
            } else {
                -ENEMY_SPEED
            },
            vy: 0.0,
            enemy_type: sp.enemy_type,
            state: EnemyState::Walking,
            facing_right: sp.facing_right,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: sp.y - h,
            cycle_timer: 0.0,
            spawn_x: sp.x,
            // Each firebar segment starts at the same angle; its distance
            // from the pivot is what differs.
            angle_deg: 0.0,
            segment: sp.segment,
        }
    }

    /// Killed by fire, a star or a kicked shell: flips over and sails off screen
    /// (`goomba.lua:177-189`).
    ///
    /// Worth a method rather than four copies because lakitu turns it into something
    /// else entirely: for him "dead" is a 16-second absence, after which he sails
    /// back in from the right edge as if nothing happened.
    pub(crate) fn shotted(&mut self) {
        self.state = EnemyState::Dead;
        self.flipped_death = true;
        self.vy = -SHOT_JUMP_FORCE;
        self.vx = if self.facing_right {
            SHOT_SPEED_X
        } else {
            -SHOT_SPEED_X
        };
        self.death_timer = if self.enemy_type == EnemyType::Lakito {
            LAKITO_RESPAWN
        } else {
            SHOT_DEATH_TIME
        };
    }

    /// A spiny egg, mid-throw. `spawn_y` is the release height, which is what the
    /// two-block window for hitting lakitu is measured from.
    fn spiny_egg(x: f32, y: f32) -> Self {
        Enemy {
            x,
            y,
            vx: 0.0,
            vy: -SPIKEY_TOSS_SPEED,
            enemy_type: EnemyType::SpikeyFall,
            state: EnemyState::Walking,
            facing_right: false,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
        }
    }
}

/// Which spawns one tile column reveals, cluster rule included.
///
/// Split out from the game struct so the rule is testable without a window: it
/// takes only the cell index and the "already spawned" flags, marks what it
/// claims, and returns the indices to instantiate.
///
/// The original recurses into `x-2, x-1, x+1, x+2` at the *same* row whenever a
/// cell actually yields an enemy, with the already-spawned list as the base case —
/// its own comment reads "spawn enemies in 5x1 line so they spawn as a unit and
/// not alone" (`game.lua:3795-3798`). So a horizontal run of goombas arrives
/// together instead of trickling in one column at a time, and the chain can reach
/// well past the five cells the comment suggests, because each newly spawned cell
/// spreads in turn. Written as a work stack rather than recursion for that reason.
///
/// A cell that yields nothing does **not** spread: in the original the recursive
/// calls sit inside the `if enemy then` branch.
pub(crate) fn column_spawn_indices(
    by_cell: &HashMap<(i32, i32), Vec<usize>>,
    spawned: &mut [bool],
    col: i32,
) -> Vec<usize> {
    let mut claimed = Vec::new();
    let mut pending: Vec<(i32, i32)> = by_cell.keys().filter(|(c, _)| *c == col).copied().collect();
    // Sorted so the order enemies enter the world is deterministic. Lua's `pairs`
    // gives an arbitrary hash order here; anything stable is closer to the intent
    // than "whatever the allocator did".
    pending.sort_unstable();

    while let Some(cell) = pending.pop() {
        let Some(indices) = by_cell.get(&cell) else {
            continue;
        };
        let fresh: Vec<usize> = indices.iter().copied().filter(|i| !spawned[*i]).collect();
        if fresh.is_empty() {
            continue;
        }
        for i in fresh {
            spawned[i] = true;
            claimed.push(i);
        }
        for d in [-2, -1, 1, 2] {
            pending.push((cell.0 + d, cell.1));
        }
    }
    claimed
}

impl Mari0Game {
    /// Instantiate everything the camera has revealed since the last call.
    ///
    /// Mari0 does not create enemies at load; it walks the columns the camera has
    /// uncovered and spawns what it finds (`game.lua:681-686`, `spawnenemy` at
    /// `:3687`). This matters for more than memory: 8-1 is **400 tiles wide**, and
    /// an enemy that existed from frame one would have walked off its ledge long
    /// before the player arrived. Spawning on reveal is the original's feel, not
    /// an optimisation.
    ///
    /// The frontier sits one screen-width plus one column ahead of the camera, so
    /// enemies come into being just off the right edge.
    pub(crate) fn spawn_revealed_columns(&mut self) {
        let screen_cols = (self.vw / TILE_SIZE).ceil() as i32;
        let target = (self.camera.x / TILE_SIZE).floor() as i32 + screen_cols + 1;
        while self.spawn_frontier < target {
            self.spawn_frontier += 1;
            for i in column_spawn_indices(
                &self.level.spawns_by_cell,
                &mut self.spawned,
                self.spawn_frontier,
            ) {
                self.enemies
                    .push(Enemy::from_spawn(&self.level.enemy_spawns[i]));
            }
        }
    }

    pub(crate) fn update_enemies(&mut self, dt: f32, ctx: &mut Context) {
        let cam_x = self.camera.x;
        // Cloned up front: the loop below holds `&mut self.enemies`.
        let portals = self.portal_pair();
        let retired = self.lakito_retired;
        // Lakitu holds his fire while three spinies are already out. Counted once,
        // before anything moves, so two lakitus in one level (no shipped level has
        // any) would both see the same tally rather than racing each other.
        let spinies_out = self
            .enemies
            .iter()
            .filter(|e| {
                matches!(e.enemy_type, EnemyType::Spikey | EnemyType::SpikeyFall)
                    && e.state != EnemyState::Dead
            })
            .count();
        // Eggs can't be pushed onto `self.enemies` from inside the loop that borrows
        // it, so they queue here and join at the end of the frame.
        let mut thrown: Vec<Enemy> = Vec::new();
        // Where lakitu aims: the player's position `LAKITO_DISTANCE_TIME` seconds
        // from now at his current speed (`lakito.lua:80`). Chasing where the player
        // *is* would let you shake him off by just holding a direction.
        let lead_x = self.player.x + self.player.vx * LAKITO_DISTANCE_TIME;

        for enemy in &mut self.enemies {
            let ew = PLAYER_SMALL_W;
            let eh = enemy_height(enemy.enemy_type, enemy.state);

            // Scripted enemies follow their own path and ignore gravity and
            // terrain entirely, so they bypass the walking/collision path below.
            if enemy.enemy_type.is_scripted() && enemy.state == EnemyState::Walking {
                enemy.anim_timer += dt;
                enemy.cycle_timer += dt;
                let start_y = enemy.spawn_y;
                match enemy.enemy_type {
                    EnemyType::Plant => {
                        if enemy.cycle_timer < PLANT_OUT_TIME {
                            // Emerging.
                            enemy.y =
                                (enemy.y - PLANT_MOVE_SPEED * dt).max(start_y - PLANT_MOVE_DIST);
                        } else if enemy.cycle_timer < PLANT_OUT_TIME + PLANT_IN_TIME {
                            // Retracting.
                            enemy.y = (enemy.y + PLANT_MOVE_SPEED * dt).min(start_y);
                        } else {
                            // Fully retracted: hold while the player is near the
                            // pipe, which is what makes waiting on top safe.
                            let player_cx = self.player.center_x();
                            let plant_cx = enemy.x + ew / 2.0;
                            if (player_cx - plant_cx).abs() > PLANT_PLAYER_NEAR {
                                enemy.cycle_timer = 0.0;
                            }
                        }
                    }
                    EnemyType::KoopaFlying => {
                        // Cosine hover: `(-cos(t*2pi)+1)/2` over the cycle
                        // (`koopa.lua:72`), which starts and ends at rest rather
                        // than snapping at the turnaround.
                        let t = (enemy.cycle_timer / KOOPA_FLYING_TIME).fract();
                        let eased = (-(t * std::f32::consts::TAU).cos() + 1.0) / 2.0;
                        enemy.y = start_y + eased * KOOPA_FLYING_DISTANCE;
                    }
                    EnemyType::Firebar => {
                        // The bar advances in fixed 11.25-degree ticks rather than
                        // continuously — 32 discrete positions per revolution, and
                        // reproducing the stepping matters for dodge timing.
                        while enemy.cycle_timer >= FIREBAR_DELAY {
                            enemy.cycle_timer -= FIREBAR_DELAY;
                            enemy.angle_deg = (enemy.angle_deg + FIREBAR_ANGLE_STEP) % 360.0;
                        }
                        let radius = enemy.segment as f32 * FIREBAR_SEGMENT_SPACING;
                        let rad = enemy.angle_deg.to_radians();
                        enemy.x = enemy.spawn_x + rad.cos() * radius;
                        enemy.y = start_y + rad.sin() * radius;
                    }
                    EnemyType::UpFire => {
                        // Leaps from below the world, arcs up, falls back, and
                        // relaunches after a random delay.
                        enemy.vy += UPFIRE_GRAVITY * dt;
                        enemy.y += enemy.vy * dt;
                        let floor = (self.level.height as f32) * TILE_SIZE;
                        if enemy.y > floor && enemy.vy > 0.0 {
                            enemy.y = floor;
                            enemy.vy = -UPFIRE_FORCE;
                        }
                    }
                    EnemyType::Lakito => {
                        if retired {
                            // Past `lakitoend` he stops caring: no more eggs, no more
                            // tracking, just a steady drift left until the cull takes
                            // him (`lakito.lua:59-60`, `:106-108`).
                            enemy.x -= LAKITO_PASSIVE_SPEED * dt;
                            enemy.facing_right = false;
                            continue;
                        }

                        if spinies_out < LAKITO_MAX_SPINIES && enemy.cycle_timer > LAKITO_THROW_TIME
                        {
                            // Released from just above him, tossed straight up. The
                            // egg carries no sideways speed at all — the arc you dodge
                            // comes from lakitu's own motion at the moment of release.
                            thrown.push(Enemy::spiny_egg(enemy.x, enemy.y - PLAYER_SMALL_H));
                            enemy.cycle_timer = 0.0;
                        }

                        // Turning is hysteretic: he only reverses once he is a full
                        // `LAKITO_SPACE` blocks past the lead point, so he oscillates
                        // slowly around the player instead of jittering on top of him.
                        let space = LAKITO_SPACE * TILE_SIZE;
                        if !enemy.facing_right && enemy.x < lead_x - space {
                            enemy.facing_right = true;
                        } else if enemy.facing_right && enemy.x > lead_x + space {
                            enemy.facing_right = false;
                        }

                        // The two directions are not mirror images, and that
                        // asymmetry is the whole character: heading right he closes
                        // at a speed proportional to the gap, so he always catches
                        // up; heading left he only ever manages 2 blocks/s, so you
                        // can outrun him going forward but never leave him behind.
                        enemy.vx = if enemy.facing_right {
                            let blocks = (enemy.x - lead_x).abs() / TILE_SIZE;
                            ((blocks - 3.0) * 2.0).round().max(2.0) * TILE_SIZE
                        } else {
                            -2.0 * TILE_SIZE
                        };
                        enemy.x += enemy.vx * dt;
                    }
                    EnemyType::CheepRed | EnemyType::CheepWhite => {
                        let speed = if enemy.enemy_type == EnemyType::CheepRed {
                            CHEEP_RED_SPEED
                        } else {
                            CHEEP_WHITE_SPEED
                        };
                        enemy.x += if enemy.facing_right { speed } else { -speed } * dt;
                        // White cheeps bob; red ones swim level.
                        if enemy.enemy_type == EnemyType::CheepWhite {
                            let bob = (enemy.cycle_timer * CHEEP_Y_SPEED).sin();
                            enemy.y = start_y + bob * CHEEP_HEIGHT;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            match enemy.state {
                EnemyState::Walking | EnemyState::ShellMoving => {
                    enemy.anim_timer += dt;

                    // Portals carry enemies too. `static = true` kinds are excluded
                    // by `portalable()`, and a shell counts as a mover, so a kicked
                    // shell can be routed through a portal like anything else.
                    if enemy.enemy_type.portalable()
                        && let Some((nx, ny, nvx, nvy)) = portal_carry(
                            &self.level,
                            portals.as_ref(),
                            PortalBody {
                                x: enemy.x,
                                y: enemy.y,
                                w: ew,
                                h: eh,
                                vx: enemy.vx,
                                vy: enemy.vy,
                            },
                            dt,
                            true,
                        )
                    {
                        enemy.x = nx;
                        enemy.y = ny;
                        enemy.vx = nvx;
                        enemy.vy = nvy;
                        enemy.facing_right = nvx > 0.0;
                        continue;
                    }

                    // Gravity. Per-kind because a thrown spiny egg is the one thing
                    // in the game that falls slower than everything else.
                    enemy.vy += enemy.enemy_type.gravity() * dt;
                    if enemy.vy > MAX_Y_SPEED {
                        enemy.vy = MAX_Y_SPEED;
                    }

                    // Horizontal movement + wall collision
                    let old_x = enemy.x;
                    enemy.x += enemy.vx * dt;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + eh - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            if blocks_movement(&self.level, col, row) {
                                let (tx, _ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap([enemy.x, enemy.y, ew, eh], [tx, _ty, tw, th]) {
                                    if enemy.vx > 0.0 {
                                        enemy.x = tx - ew;
                                    } else if enemy.vx < 0.0 {
                                        enemy.x = tx + tw;
                                    }
                                    enemy.vx = -enemy.vx;
                                    if enemy.state == EnemyState::Walking {
                                        enemy.facing_right = !enemy.facing_right;
                                    }
                                }
                            }
                        }
                    }

                    // Red koopas refuse to walk off a ledge: if the tile ahead
                    // and below is empty while they're grounded, turn around.
                    // Checked before the vertical step so the turn happens on the
                    // last solid tile rather than mid-fall.
                    if enemy.enemy_type.avoids_ledges()
                        && enemy.on_ground
                        && enemy.state == EnemyState::Walking
                    {
                        let ahead_x = if enemy.vx > 0.0 {
                            enemy.x + ew + 1.0
                        } else {
                            enemy.x - 1.0
                        };
                        let ahead_col = (ahead_x / TILE_SIZE).floor() as i32;
                        let below_row = ((enemy.y + eh + 2.0) / TILE_SIZE).floor() as i32;
                        if !is_solid(get_tile(&self.level, ahead_col, below_row)) {
                            enemy.vx = -enemy.vx;
                            enemy.facing_right = !enemy.facing_right;
                        }
                    }

                    // Vertical movement + ground/ceiling collision
                    enemy.y += enemy.vy * dt;
                    enemy.on_ground = false;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + eh - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            if blocks_movement(&self.level, col, row) {
                                let (tx, ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap([enemy.x, enemy.y, ew, eh], [tx, ty, tw, th]) {
                                    if enemy.vy > 0.0 {
                                        enemy.y = ty - eh;
                                        enemy.on_ground = true;
                                    } else if enemy.vy < 0.0 {
                                        enemy.y = ty + th;
                                    }
                                    enemy.vy = 0.0;
                                }
                            }
                        }
                    }

                    // An egg that has touched down hatches (`goomba.lua:250-272`): it
                    // becomes an ordinary walking spiny and sets off *towards* the
                    // player, which is why a spiny always greets you head-on rather
                    // than wandering off.
                    if enemy.enemy_type == EnemyType::SpikeyFall && enemy.on_ground {
                        enemy.enemy_type = EnemyType::Spikey;
                        enemy.facing_right = enemy.x < self.player.x;
                        enemy.vx = if enemy.facing_right {
                            ENEMY_SPEED
                        } else {
                            -ENEMY_SPEED
                        };
                    }

                    // Ledge detection (only for walking enemies on ground, not shells)
                    if enemy.state == EnemyState::Walking && enemy.on_ground {
                        let foot_col = if enemy.vx > 0.0 {
                            ((enemy.x + ew) / TILE_SIZE).floor() as i32
                        } else {
                            (enemy.x / TILE_SIZE).floor() as i32
                        };
                        let ground_row = ((enemy.y + eh) / TILE_SIZE).floor() as i32;
                        if !is_solid(get_tile(&self.level, foot_col, ground_row)) {
                            enemy.vx = -enemy.vx;
                            enemy.facing_right = !enemy.facing_right;
                            // Undo horizontal movement to prevent walking off
                            enemy.x = old_x;
                        }
                    }
                }
                EnemyState::Dead => {
                    enemy.death_timer -= dt;
                    if enemy.flipped_death {
                        // `shotgravity`, not the world's — a shot enemy hangs a beat
                        // longer at the top of its arc (`variables.lua:164`).
                        enemy.vy += SHOT_GRAVITY * dt;
                        enemy.y += enemy.vy * dt;
                        enemy.x += enemy.vx * dt;
                    }
                }
                EnemyState::Shell => {
                    // Gravity for stationary shell too
                    enemy.vy += GRAVITY * dt;
                    if enemy.vy > MAX_Y_SPEED {
                        enemy.vy = MAX_Y_SPEED;
                    }
                    enemy.y += enemy.vy * dt;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + PLAYER_SMALL_H - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            if blocks_movement(&self.level, col, row) {
                                let (tx, ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap(
                                    [enemy.x, enemy.y, ew, PLAYER_SMALL_H],
                                    [tx, ty, tw, th],
                                ) {
                                    if enemy.vy > 0.0 {
                                        enemy.y = ty - PLAYER_SMALL_H;
                                        enemy.on_ground = true;
                                    }
                                    enemy.vy = 0.0;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Player-enemy interaction
        let mut player_bounce = false;
        for enemy in &mut self.enemies {
            if enemy.state == EnemyState::Dead {
                continue;
            }

            let eh = match enemy.enemy_type {
                EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                _ => PLAYER_SMALL_H,
            };

            if !aabb_overlap(
                [
                    self.player.x,
                    self.player.y,
                    self.player.width,
                    self.player.height,
                ],
                [enemy.x, enemy.y, PLAYER_SMALL_W, eh],
            ) {
                continue;
            }

            // Check if stomping (player feet above enemy top half)
            let player_feet = self.player.bottom();
            let enemy_mid = enemy.y + eh / 2.0;

            if self.player.vy > 0.0 && player_feet < enemy_mid + 8.0 && enemy.enemy_type.stompable()
            {
                // Stomp!
                match enemy.state {
                    EnemyState::Walking if enemy.enemy_type == EnemyType::KoopaFlying => {
                        // Stomping a winged koopa knocks the wings off and leaves
                        // a walking koopa, rather than killing it outright.
                        enemy.enemy_type = EnemyType::Koopa;
                        enemy.vy = -KOOPA_FLYING_GRAVITY * dt_hint();
                        enemy.vx = if enemy.facing_right {
                            ENEMY_SPEED
                        } else {
                            -ENEMY_SPEED
                        };
                    }
                    EnemyState::Walking if enemy.enemy_type == EnemyType::Lakito => {
                        // Stomping lakitu doesn't finish him, it evicts him: he drops
                        // out of his cloud upside-down and is back at the right edge
                        // of the screen 16 seconds later (`lakito.lua:45-56`,
                        // `:130-133`). Straight down, because `stomp` zeroes the
                        // upward kick `shotted` had just given him.
                        enemy.state = EnemyState::Dead;
                        enemy.flipped_death = true;
                        enemy.death_timer = LAKITO_RESPAWN;
                        enemy.vx = 0.0;
                        enemy.vy = 0.0;
                    }
                    EnemyState::Walking => {
                        if enemy.enemy_type.is_koopa_like() {
                            // Koopas, red koopas and beetles all retreat into a
                            // shell rather than dying outright.
                            enemy.state = EnemyState::Shell;
                            enemy.vx = 0.0;
                        } else {
                            enemy.state = EnemyState::Dead;
                            enemy.death_timer = ENEMY_DEATH_TIME;
                        }
                    }
                    EnemyState::Shell => {
                        // Kick shell
                        enemy.state = EnemyState::ShellMoving;
                        enemy.vx = if self.player.center_x() < enemy.x + PLAYER_SMALL_W / 2.0 {
                            SHELL_SPEED
                        } else {
                            -SHELL_SPEED
                        };
                    }
                    _ => {}
                }

                let combo_score = COMBO_SCORES[self.combo_index.min(COMBO_SCORES.len() - 1)];
                self.score += combo_score;
                self.combo_index += 1;
                self.combo_active = true;
                player_bounce = true;
                ctx.audio.play("stomp");
            } else if self.star_timer > 0.0 && !enemy.enemy_type.indestructible() {
                // Star invincibility: kill enemy on contact (flip + fly off).
                // A star does not clear a firebar or a lava geyser — those are
                // level geometry with a hitbox, not enemies.
                enemy.shotted();
                let combo_score = COMBO_SCORES[self.combo_index.min(COMBO_SCORES.len() - 1)];
                self.score += combo_score;
                self.combo_index += 1;
                self.combo_active = true;
                ctx.audio.play("stomp");
            } else if self.player.invincible_timer <= 0.0 && enemy.state != EnemyState::Shell {
                // Hit by enemy from side
                if self.player.is_fire {
                    self.player.is_fire = false;
                    self.player.invincible_timer = 2.0;
                    ctx.audio.play("shrink");
                } else if self.player.is_big {
                    self.player.set_size(false);
                    self.player.invincible_timer = 2.0;
                    ctx.audio.play("shrink");
                } else {
                    self.die(ctx);
                    return;
                }
            }
        }

        if player_bounce {
            self.player.vy = STOMP_BOUNCE;
            self.player.on_ground = false;
        }

        self.enemies.append(&mut thrown);
        self.egg_may_hit_its_thrower();
        self.respawn_shot_lakitos();

        // Remove dead enemies after timer, or enemies that fell off the map
        self.enemies.retain(|e| {
            if e.state == EnemyState::Dead && e.death_timer <= 0.0 {
                return false;
            }
            if e.y > (self.level.height as f32) * TILE_SIZE + 100.0 {
                // A shot lakitu who has not yet been retired is *waiting*, not gone:
                // he has to survive falling out of the world to make it back for his
                // respawn. Once retired the timer runs down and this catches him.
                if e.enemy_type == EnemyType::Lakito && e.state == EnemyState::Dead && !retired {
                    return true;
                }
                return false;
            }
            // Scrolled well off the left edge. It does not come back: the
            // spawn record is never cleared, exactly as `enemiesspawned` isn't.
            if e.x < cam_x - 200.0 {
                return false;
            }
            true
        });
    }

    /// Lakitu can be knocked out of the sky by his own egg — for about a third of
    /// a second.
    ///
    /// Not a bug, though it reads like one. The egg leaves lakitu's hands able to
    /// collide with him (`goomba.lua:54`, mask index 21 is lakitu's category) and
    /// only loses that ability once it has fallen [`SPIKEY_HITS_LAKITO_WITHIN`]
    /// blocks past where it was released (`goomba.lua:132`). Since it is thrown
    /// *upward* and carries no sideways speed, it comes back down through his
    /// altitude roughly two thirds of a second later — by which time he has almost
    /// always moved out from under it, because his slowest speed is 2 blocks/s.
    /// Almost always: catch him mid-turnaround, where his speed passes through zero,
    /// and his own egg lands on his head and scores you 200.
    fn egg_may_hit_its_thrower(&mut self) {
        let eggs: Vec<[f32; 4]> = self
            .enemies
            .iter()
            .filter(|e| {
                e.enemy_type == EnemyType::SpikeyFall
                    && e.state == EnemyState::Walking
                    && e.y <= e.spawn_y + SPIKEY_HITS_LAKITO_WITHIN
            })
            .map(|e| [e.x, e.y, PLAYER_SMALL_W, PLAYER_SMALL_H])
            .collect();
        if eggs.is_empty() {
            return;
        }
        let mut struck = Vec::new();
        for (i, enemy) in self.enemies.iter_mut().enumerate() {
            if enemy.enemy_type != EnemyType::Lakito || enemy.state != EnemyState::Walking {
                continue;
            }
            let box_ = [enemy.x, enemy.y, PLAYER_SMALL_W, PLAYER_SMALL_H];
            if eggs.iter().any(|egg| aabb_overlap(box_, *egg)) {
                enemy.shotted();
                struck.push(i);
            }
        }
        for i in struck {
            let (x, y) = (self.enemies[i].x, self.enemies[i].y);
            self.score += LAKITO_SCORE;
            self.score_popups.push(crate::effects::ScorePopup {
                x,
                y,
                value: LAKITO_SCORE,
                timer: 0.0,
            });
        }
    }

    /// Bring a shot lakitu back at the right edge of the screen.
    ///
    /// He re-enters at the altitude he first appeared at, not where he fell from
    /// (`lakito.lua:48`), so a level's lakitu always flies the same lane. Retired
    /// lakitus are left to expire — being past `lakitoend` is permanent.
    fn respawn_shot_lakitos(&mut self) {
        if self.lakito_retired {
            return;
        }
        let (cam_x, vw) = (self.camera.x, self.vw);
        for enemy in &mut self.enemies {
            if enemy.enemy_type != EnemyType::Lakito
                || enemy.state != EnemyState::Dead
                || enemy.death_timer > 0.0
            {
                continue;
            }
            enemy.state = EnemyState::Walking;
            enemy.flipped_death = false;
            enemy.x = cam_x + vw;
            enemy.y = enemy.spawn_y;
            enemy.vx = 0.0;
            enemy.vy = 0.0;
            enemy.facing_right = false;
            enemy.cycle_timer = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level;
    use crate::world::load_level;

    /// Build a cell → indices map from a list of `(col, row)` placements.
    fn by_cell(cells: &[(i32, i32)]) -> HashMap<(i32, i32), Vec<usize>> {
        let mut map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, c) in cells.iter().enumerate() {
            map.entry(*c).or_default().push(i);
        }
        map
    }

    #[test]
    fn a_lone_enemy_spawns_with_its_own_column() {
        let cells = [(10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert!(column_spawn_indices(&map, &mut spawned, 9).is_empty());
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
    }

    /// The cluster rule: reaching one of a group drags in the neighbours within
    /// two columns, so a row of goombas arrives as a unit.
    #[test]
    fn reaching_one_of_a_group_pulls_in_neighbours_within_two_columns() {
        // 10, 11, 12 on the same row; column 10 is revealed first.
        let cells = [(10, 5), (11, 5), (12, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1, 2], "all three should arrive together");
        assert!(spawned.iter().all(|s| *s));
    }

    /// The chain keeps going: each newly spawned cell spreads in turn, so a long
    /// unbroken run comes in all at once even though each hop is only two columns.
    #[test]
    fn the_cluster_chains_along_a_long_run() {
        let cells: Vec<(i32, i32)> = (10..30).map(|c| (c, 5)).collect();
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let got = column_spawn_indices(&map, &mut spawned, 10);
        assert_eq!(got.len(), 20, "the whole run should arrive at once");
    }

    /// A gap wider than two columns stops the chain — that's what makes the rule
    /// "this group" rather than "the whole level".
    #[test]
    fn a_gap_of_three_columns_breaks_the_chain() {
        // 10, 11 … then nothing until 15.
        let cells = [(10, 5), (11, 5), (15, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "15 is four columns past 11, out of reach");
        assert!(!spawned[2]);
    }

    /// Different rows are independent: the recursion only walks sideways.
    #[test]
    fn the_cluster_does_not_spread_vertically() {
        let cells = [(10, 5), (11, 9)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
        assert!(!spawned[1], "a different row is a different group");
    }

    /// Nothing spawns twice. This is what stops a killed enemy from returning
    /// when the camera revisits its column.
    #[test]
    fn a_column_never_spawns_the_same_enemy_twice() {
        let cells = [(10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
        assert!(
            column_spawn_indices(&map, &mut spawned, 10).is_empty(),
            "second sweep of the same column yields nothing"
        );
    }

    /// A firebar puts one spawn per segment on the same pivot cell, so a cell can
    /// legitimately hold several.
    #[test]
    fn one_cell_can_hold_several_spawns() {
        let cells = [(10, 5), (10, 5), (10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1, 2]);
    }

    /// The portal exemption table, which has two independent reasons in it.
    ///
    /// Worth pinning because the two reasons look the same from the outside but
    /// aren't: plants/firebars/geysers are `static = true` fixtures, while
    /// cheep-cheeps move perfectly well and opt out with `portalable = false`. Anyone
    /// "simplifying" this to `!is_scripted()` would quietly make flying koopas
    /// non-portable, since they're scripted but do travel.
    #[test]
    fn the_portal_exemption_table_has_two_distinct_reasons() {
        for kind in [
            EnemyType::Goomba,
            EnemyType::Koopa,
            EnemyType::KoopaRed,
            EnemyType::Beetle,
            EnemyType::KoopaFlying,
        ] {
            assert!(kind.portalable(), "{kind:?} should travel through portals");
        }
        for kind in [
            // `static = true`: fixtures that never move.
            EnemyType::Plant,
            EnemyType::Firebar,
            EnemyType::UpFire,
            // `portalable = false`: moves, but opts out.
            EnemyType::CheepRed,
            EnemyType::CheepWhite,
            EnemyType::Lakito,
        ] {
            assert!(!kind.portalable(), "{kind:?} should not travel");
        }
        assert!(
            EnemyType::KoopaFlying.is_scripted() && EnemyType::KoopaFlying.portalable(),
            "scripted and portalable are independent; a flying koopa is both"
        );
    }

    /// A spiny hurts from above, which is the one thing that makes it not a goomba.
    ///
    /// The original expresses this as `b.t ~= "goomba"` rather than a per-type flag
    /// (`mario.lua:1778`), so it is easy to port a spiny as "a goomba with a different
    /// sprite" and quietly hand the player a free stomp.
    #[test]
    fn a_spiny_cannot_be_stomped_but_a_goomba_can() {
        assert!(EnemyType::Goomba.stompable());
        assert!(!EnemyType::Spikey.stompable());
        assert!(!EnemyType::SpikeyFall.stompable());
        // Lakitu, by contrast, is on the stomp list (`mario.lua:1761`).
        assert!(EnemyType::Lakito.stompable());
    }

    /// The egg falls at 30 blocks/s², nothing else does.
    #[test]
    fn only_a_thrown_spiny_egg_falls_slower_than_the_world() {
        assert_eq!(EnemyType::SpikeyFall.gravity(), SPIKEY_FALL_GRAVITY);
        const { assert!(SPIKEY_FALL_GRAVITY < GRAVITY) };
        for kind in [
            EnemyType::Goomba,
            EnemyType::Spikey,
            EnemyType::Koopa,
            EnemyType::Lakito,
        ] {
            assert_eq!(kind.gravity(), GRAVITY, "{kind:?} should fall normally");
        }
    }

    /// Lakitu opts out of portals explicitly, like a cheep-cheep — he is not a
    /// fixture, he simply refuses.
    #[test]
    fn lakito_refuses_portals_without_being_a_fixture() {
        assert!(!EnemyType::Lakito.portalable());
        assert!(EnemyType::Lakito.is_scripted());
        // The spinies he throws have no such exemption: they are goombas.
        assert!(EnemyType::Spikey.portalable());
        assert!(EnemyType::SpikeyFall.portalable());
    }

    /// "Dead" means something different for lakitu: a 16-second absence, not removal.
    #[test]
    fn a_downed_lakito_is_scheduled_to_return() {
        let mut lakito = Enemy::spiny_egg(0.0, 0.0);
        lakito.enemy_type = EnemyType::Lakito;
        lakito.shotted();
        assert_eq!(lakito.state, EnemyState::Dead);
        assert_eq!(lakito.death_timer, LAKITO_RESPAWN);

        let mut goomba = Enemy::spiny_egg(0.0, 0.0);
        goomba.enemy_type = EnemyType::Goomba;
        goomba.shotted();
        assert_eq!(goomba.death_timer, SHOT_DEATH_TIME);
        const { assert!(SHOT_DEATH_TIME < LAKITO_RESPAWN) };
    }

    /// The egg is tossed upward, which is why it can come back down onto its thrower.
    #[test]
    fn a_spiny_egg_leaves_lakitos_hands_going_up() {
        let egg = Enemy::spiny_egg(100.0, 200.0);
        assert_eq!(egg.enemy_type, EnemyType::SpikeyFall);
        assert!(egg.vy < 0.0, "thrown up, not dropped");
        assert_eq!(egg.vx, 0.0, "no sideways speed of its own");
        assert_eq!(
            egg.spawn_y, 200.0,
            "the release height is what the lakitu-hit window is measured from"
        );
    }

    /// Sweeping every column of a real level must claim every spawn exactly once.
    ///
    /// The invariant that matters for play: lazy spawning must not *lose* enemies.
    #[test]
    fn sweeping_all_columns_claims_every_spawn_exactly_once() {
        for (pack, name, _) in level::LEVELS {
            let level = load_level(pack, name);
            let mut spawned = vec![false; level.enemy_spawns.len()];
            let mut total = 0;
            // Well past both ends, since the cluster rule can reach outside the
            // level's own column range.
            for col in -4..(level.width as i32 + 4) {
                total += column_spawn_indices(&level.spawns_by_cell, &mut spawned, col).len();
            }
            assert_eq!(
                total,
                level.enemy_spawns.len(),
                "{pack}/{name}: swept {total} of {} spawns",
                level.enemy_spawns.len()
            );
            assert!(
                spawned.iter().all(|s| *s),
                "{pack}/{name}: some spawns were never claimed"
            );
        }
    }

    /// No level places a spiny, and every level with a lakitu says where he stops.
    ///
    /// Both halves of this are why lakitu and the spiny had to be built together. The
    /// entity ids exist (98 and 99) and the editor offers them, but nothing ships one:
    /// a walking spiny is only ever reached by an egg landing, so a port that adds
    /// `spikey` as a spawn point and stops there has added an enemy the player can
    /// never meet. The `lakitoend` half is the other side of the same coin — without
    /// it lakitu would follow the player into the flagpole.
    #[test]
    fn spinies_are_never_placed_and_every_lakito_has_somewhere_to_stop() {
        let mut with_lakito = Vec::new();
        for (pack, name, _) in level::LEVELS {
            let parsed = level::load(pack, name)
                .expect("shipped level")
                .expect("parses");
            for spawn in &parsed.markers.enemies {
                assert_ne!(
                    spawn.kind,
                    level::EntityKind::Spikey,
                    "{pack}/{name} places a spiny; the roster assumed none did"
                );
                assert_ne!(spawn.kind, level::EntityKind::SpikeyHalf, "{pack}/{name}");
                if spawn.kind == level::EntityKind::Lakito {
                    with_lakito.push((name, parsed.markers.lakito_end));
                }
            }
        }
        assert_eq!(
            with_lakito.len(),
            3,
            "expected 4-1, 6-1 and 8-2 to be the only lakitu levels, got {with_lakito:?}"
        );
        for (name, end) in with_lakito {
            assert!(end.is_some(), "{name} has a lakitu but no lakitoend");
        }
    }

    /// 8-1 is the width stress case the lazy spawner exists for.
    #[test]
    fn the_widest_level_holds_its_enemies_back_until_revealed() {
        let level = load_level("smb", "8-1");
        assert!(
            level.width >= 400,
            "8-1 should be ~400 tiles wide, got {}",
            level.width
        );
        let mut spawned = vec![false; level.enemy_spawns.len()];
        // One screen plus a column, exactly what `spawn_revealed_columns` opens with.
        let mut opening = 0;
        for col in 0..=17 {
            opening += column_spawn_indices(&level.spawns_by_cell, &mut spawned, col).len();
        }
        assert!(
            opening < level.enemy_spawns.len(),
            "the opening screen claimed all {} spawns; nothing was left to reveal",
            level.enemy_spawns.len()
        );
    }
}
