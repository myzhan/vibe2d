//! The enemy roster: what each kind is, how tall it is, and how it moves.
//!
//! Scripted kinds (firebars, geysers, cheep-cheeps) ignore the walker logic
//! entirely — see `EnemyType::is_scripted`.

use std::collections::HashMap;

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::*;
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
    /// firebar is how you die, not how you win.
    pub(crate) fn stompable(self) -> bool {
        !matches!(
            self,
            EnemyType::Plant | EnemyType::Firebar | EnemyType::UpFire
        )
    }

    /// Enemies that ignore gravity and terrain and follow their own path.
    pub(crate) fn is_scripted(self) -> bool {
        matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::CheepRed
                | EnemyType::CheepWhite
                | EnemyType::KoopaFlying
        )
    }

    /// Indestructible hazards: fire and stars don't remove them either.
    pub(crate) fn indestructible(self) -> bool {
        matches!(self, EnemyType::Firebar | EnemyType::UpFire)
    }
}

#[derive(PartialEq, Clone, Copy)]
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

                    // Gravity
                    enemy.vy += GRAVITY * dt;
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
                            if is_solid(get_tile(&self.level, col, row)) {
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
                            if is_solid(get_tile(&self.level, col, row)) {
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
                        enemy.vy += GRAVITY * dt;
                        enemy.y += enemy.vy * dt;
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
                            if is_solid(get_tile(&self.level, col, row)) {
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
                enemy.state = EnemyState::Dead;
                enemy.death_timer = 3.0; // longer timer — flies off screen
                enemy.flipped_death = true;
                enemy.vy = -300.0; // launch upward
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

        // Remove dead enemies after timer, or enemies that fell off the map
        self.enemies.retain(|e| {
            if e.state == EnemyState::Dead && e.death_timer <= 0.0 {
                return false;
            }
            if e.y > (self.level.height as f32) * TILE_SIZE + 100.0 {
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
