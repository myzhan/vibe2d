//! The game struct and the `Game` trait impl.
//!
//! The per-system update and draw passes live in their own modules and are hung
//! off `Mari0Game` as inherent methods; this file holds the state, the shared
//! helpers, and the frame dispatch.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::effects::*;
use crate::enemies::*;
use crate::items::*;
use crate::level;
use crate::music::MusicPhase;
use crate::physics::*;
use crate::pipe::PipeTransit;
use crate::player::*;
use crate::portal::*;
use crate::world::*;

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum GameState {
    Menu,
    Playing,
    Dead,
    LevelComplete,
}

// ── Main game ───────────────────────────────────────────────────────

pub(crate) struct Mari0Game {
    pub(crate) state: GameState,
    /// Which level is loaded. Replaces the compile-time `include_str!` of 1-1.
    pub(crate) current: LevelId,
    pub(crate) player: Player,
    pub(crate) portals: [Option<Portal>; 2],
    pub(crate) projectiles: Vec<PortalProjectile>,
    pub(crate) crosshair_angle: f32,
    pub(crate) aim_dot_timer: f32,
    pub(crate) portal_anim_timer: f32, // global portal animation timer
    pub(crate) portal_anim_frame: u32, // current frame 0..5 (maps to original frames 1..6)
    pub(crate) enemies: Vec<Enemy>,
    /// Which of `level.enemy_spawns` have already been instantiated.
    ///
    /// Never cleared during a life, which is what stops a killed enemy from
    /// returning when the camera passes its column again — the original's
    /// `enemiesspawned` list has the same lifetime.
    pub(crate) spawned: Vec<bool>,
    /// Rightmost tile column whose spawns have been processed.
    pub(crate) spawn_frontier: i32,
    pub(crate) level: Level,
    pub(crate) camera: Camera,
    pub(crate) score: u32,
    pub(crate) coins: u32,
    pub(crate) lives: u32,
    pub(crate) combo_index: usize,
    pub(crate) combo_active: bool,
    pub(crate) time_remaining: f32,

    // Block/coin/score animations
    pub(crate) block_bounces: Vec<BlockBounce>,
    pub(crate) coin_popups: Vec<CoinPopup>,
    pub(crate) score_popups: Vec<ScorePopup>,
    pub(crate) brick_debris: Vec<BrickDebris>,
    pub(crate) items: Vec<Item>,
    pub(crate) fireballs: Vec<Fireball>,
    pub(crate) star_timer: f32, // player star invincibility timer

    /// The pipe trip in progress, if any. While set, the player has no control.
    pub(crate) pipe: Option<PipeTransit>,

    /// The highest checkpoint passed, as a tile cell.
    ///
    /// Survives death — that's the whole point — and is cleared when a new level
    /// starts or the game ends (`levelscreen.lua:34`, `:49`).
    pub(crate) checkpoint: Option<(i32, i32)>,
    /// How many checkpoints have been passed, so the next one to watch for is a
    /// single index rather than a scan.
    pub(crate) checkpoints_passed: usize,
    /// Sublevel to reload after a death; 0 is the main level.
    ///
    /// Only ever non-zero after taking a pipe out of an *intermission* stub
    /// (`mario.lua:2891-2893`), which is what keeps a death in 1-2_1 from dumping
    /// the player back into the 24-tile-wide 1-2.
    pub(crate) respawn_sublevel: u32,

    /// Where the level music is in its low-time sequence.
    pub(crate) music_phase: MusicPhase,
    /// Clock reading when the low-time warning fired, so the switch to the fast
    /// variant can be timed off the game clock rather than a separate timer.
    pub(crate) warning_started_at: Option<f32>,
    /// Set when the level changes; `update` starts the track on the next frame.
    ///
    /// Deferred rather than started inline because `reset_level` is also reached
    /// from VDP methods, which have no `Context` to play audio through.
    pub(crate) music_restart: bool,

    // Sprite sheet textures
    pub(crate) tex_tiles: TextureId,
    pub(crate) tex_mario_layers: [TextureId; 4], // layers 0-3 (small)
    pub(crate) tex_mario_big_layers: [TextureId; 4], // layers 0-3 (big)
    pub(crate) tex_goomba: TextureId,
    pub(crate) tex_koopa: TextureId,
    pub(crate) tex_koopa_red: TextureId,
    pub(crate) tex_beetle: TextureId,
    pub(crate) tex_plant: TextureId,
    pub(crate) tex_cheep_red: TextureId,
    pub(crate) tex_cheep_white: TextureId,
    pub(crate) tex_coin_anim: TextureId,
    pub(crate) tex_entities: TextureId,
    pub(crate) tex_star: TextureId,
    pub(crate) tex_flower: TextureId,
    pub(crate) tex_fireball: TextureId,
    pub(crate) tex_portal: TextureId,
    pub(crate) tex_portal_v: TextureId, // pre-rotated portal for vertical (left/right) orientation
    pub(crate) tex_portal_crosshair: TextureId,
    pub(crate) tex_portal_projectile: TextureId,
    pub(crate) tex_portal_dot: TextureId,
    pub(crate) tex_flag: TextureId,

    pub(crate) vw: f32,
    /// Virtual screen height. Needed alongside `vw` so the pipe scissor can span
    /// the full screen on the axis it isn't clipping.
    pub(crate) vh: f32,
}

impl Mari0Game {
    pub(crate) fn tex(ctx: &Context, name: &str) -> TextureId {
        ctx.assets
            .texture_id(name)
            .unwrap_or_else(|| panic!("Missing texture: {}", name))
    }

    /// Move to the next level, or back to the menu when the mappack runs out.
    ///
    /// Mari0 defines the end of a mappack as "the next level file doesn't exist"
    /// rather than a world count, so progression is a lookup, not a bound
    /// (`levelscreen.lua:32-41`).
    pub(crate) fn advance_level(&mut self) {
        let mut next = self.current.clone();
        next.advance();
        if next.exists() {
            self.current = next;
            self.start_fresh();
            self.score_carry_over();
            self.state = GameState::Playing;
        } else {
            // Mappack finished.
            self.state = GameState::Menu;
        }
    }

    /// Score, coins and lives persist across levels; the timer does not.
    pub(crate) fn score_carry_over(&mut self) {
        self.time_remaining = load_level(&self.current.pack, &self.current.name()).time_limit;
    }

    /// Sprite sheet for a koopa-like enemy. All three share the same layout.
    pub(crate) fn koopa_texture(&self, enemy_type: EnemyType) -> TextureId {
        match enemy_type {
            EnemyType::KoopaRed => self.tex_koopa_red,
            EnemyType::Beetle => self.tex_beetle,
            _ => self.tex_koopa,
        }
    }

    /// Tile a struck block becomes, for this level's environment.
    ///
    /// Not a constant: spriteset 1 uses 113, 2 uses 114 and 3/4 use 117, so a
    /// castle block that turned into the overworld's used-block art was visibly
    /// wrong (`mario.lua:2383-2432`).
    pub(crate) fn used_block_tile(&self) -> u32 {
        level::tiles::used_block_tile(self.level.spriteset, false) as u32
    }

    /// Reload the current level from its own start position.
    pub(crate) fn reset_level(&mut self) {
        self.load_current(false);
    }

    /// Start the current level as if arriving for the first time.
    ///
    /// Clears the run-scoped progress that a *death* is supposed to preserve —
    /// the checkpoint and the sublevel to respawn into. Without this, jumping
    /// between levels leaves stale progress behind: a `setLevel("1-1")` after a trip
    /// through 1-2's pipe still respawned deaths into `1-1_1`, because
    /// `respawn_sublevel` was never reset. The original clears both on its
    /// next-level branch (`levelscreen.lua:33-34`).
    pub(crate) fn start_fresh(&mut self) {
        self.checkpoint = None;
        self.checkpoints_passed = 0;
        self.respawn_sublevel = 0;
        self.reset_level();
    }

    /// Reload after a death: back into `respawn_sublevel`, at the checkpoint.
    ///
    /// The original splits these two the same way. `checkcheckpoint` is cleared at
    /// the top of every level transition and set true *only* on the death branch
    /// (`levelscreen.lua:11`, `:43`), so a checkpoint you passed never affects where
    /// a fresh level starts you.
    pub(crate) fn respawn_after_death(&mut self) {
        self.current.sublevel = self.respawn_sublevel;
        self.load_current(true);
    }

    fn load_current(&mut self, use_checkpoint: bool) {
        let level = load_level(&self.current.pack, &self.current.name());
        let start = match self.checkpoint.filter(|_| use_checkpoint) {
            // `starty = checkpointpoints[checkpointx] or 13` (`game.lua:2147`) — the
            // checkpoint names the row to stand on, not the row to occupy.
            Some((col, row)) => (
                col as f32 * TILE_SIZE,
                row as f32 * TILE_SIZE - PLAYER_SMALL_H,
            ),
            None => level.player_start,
        };
        self.player = Player::new(start.0, start.1);
        // Restore how many checkpoints are behind us, so passing the *next* one
        // still registers after a respawn (`game.lua:2161-2163`).
        self.checkpoints_passed = match self.checkpoint.filter(|_| use_checkpoint) {
            Some((col, _)) => level
                .checkpoints
                .iter()
                .position(|(c, _)| *c == col)
                .map_or(0, |i| i + 1),
            None => 0,
        };
        self.enemies.clear();
        self.spawned = vec![false; level.enemy_spawns.len()];
        // -1, not 0: the catch-up loop pre-increments, so column 0 still gets
        // processed on the first sweep.
        self.spawn_frontier = -1;
        self.portals = [None, None];
        self.projectiles.clear();
        self.crosshair_angle = 0.0;
        self.aim_dot_timer = 0.0;
        self.portal_anim_timer = 0.0;
        self.portal_anim_frame = 0;
        self.time_remaining = level.time_limit;
        self.combo_index = 0;
        self.combo_active = false;
        self.block_bounces.clear();
        self.coin_popups.clear();
        self.score_popups.clear();
        self.brick_debris.clear();
        self.items.clear();
        self.fireballs.clear();
        self.star_timer = 0.0;
        self.pipe = None;
        self.level = level;
        // Respawning at a checkpoint has to bring the camera along, or the first
        // frame draws the level's opening while the player stands 99 columns away.
        let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
        self.camera = Camera {
            x: (self.player.x - self.vw / 3.0).clamp(0.0, max_camera),
        };
        // Fill the opening screen now so the player doesn't watch the first
        // goombas pop into being after the level has already started.
        self.spawn_revealed_columns();
        self.music_phase = MusicPhase::Normal;
        self.warning_started_at = None;
        self.music_restart = true;
    }

    /// Note the highest checkpoint the player has walked past.
    ///
    /// The original tracks an index and only ever looks at the *next* checkpoint
    /// (`mario.lua:998-1005`), which is why walking backwards can't un-pass one.
    /// The comparison is `x > column`, so the trigger is the checkpoint's left edge.
    fn check_checkpoint_passed(&mut self) {
        while let Some((col, row)) = self.level.checkpoints.get(self.checkpoints_passed).copied() {
            if self.player.x <= col as f32 * TILE_SIZE {
                break;
            }
            self.checkpoints_passed += 1;
            self.checkpoint = Some((col, row));
        }
    }

    /// Purely cosmetic cycles: the portal frame animation and the aim-dot phase.
    ///
    /// Kept separate from the rest of the update so a pipe transition — which
    /// suspends input, physics and the clock — can still let the portals shimmer.
    fn update_visual_timers(&mut self, dt: f32) {
        self.aim_dot_timer += dt;
        const AIM_DOTS_CYCLE: f32 = 0.8;
        if self.aim_dot_timer >= AIM_DOTS_CYCLE {
            self.aim_dot_timer -= AIM_DOTS_CYCLE;
        }

        self.portal_anim_timer += dt;
        while self.portal_anim_timer >= PORTAL_ANIM_DELAY {
            self.portal_anim_timer -= PORTAL_ANIM_DELAY;
            self.portal_anim_frame = (self.portal_anim_frame + 1) % PORTAL_ANIM_FRAMES;
        }
    }

    pub(crate) fn update_playing(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        // ── Pipe transition ──
        // A trip through a pipe owns the player: input, physics and the clock are
        // all suspended, and the level may swap out from under us partway through.
        // Nothing below may run in that case, so this returns rather than branching.
        if self.update_pipe(dt) {
            self.update_visual_timers(dt);
            return;
        }

        // ── Input ──
        let move_left = input.is_action_pressed("move_left");
        let move_right = input.is_action_pressed("move_right");
        let jump_pressed = input.is_action_pressed("jump");
        let jump_just = input.is_action_just_pressed("jump");
        let fire_blue = input.is_action_just_pressed("portal_blue");
        let fire_orange = input.is_action_just_pressed("portal_orange");
        let fire_ball = input.is_action_just_pressed("fire");
        let sprint = input.is_action_pressed("fire"); // hold shift/F to sprint

        // Aiming: right stick when it's deflected, mouse otherwise.
        //
        // The stick wins only while actually pushed, so a plugged-in controller
        // resting at centre never fights the mouse — and either device can be
        // picked up mid-play without a mode switch.
        let aim_x = input.gamepad_axis(GamepadAxis::RightStickX);
        // Stick Y is up-positive; screen space is y-down, hence the negation.
        let aim_y = -input.gamepad_axis(GamepadAxis::RightStickY);
        if aim_x != 0.0 || aim_y != 0.0 {
            self.crosshair_angle = aim_y.atan2(aim_x);
        } else {
            // Mouse aiming (virtual coords → world coords)
            let (mx, my) = input.mouse_position();
            let world_mx = mx + self.camera.x;
            let world_my = my;
            self.crosshair_angle =
                (world_my - self.player.center_y()).atan2(world_mx - self.player.center_x());
        }

        // ── Horizontal movement (sprint = higher accel & max speed) ──
        let accel = if sprint { RUN_ACCEL } else { WALK_ACCEL };
        let max_speed = if sprint {
            MAX_RUN_SPEED
        } else {
            MAX_WALK_SPEED
        };
        if move_right {
            self.player.vx += accel * dt;
            self.player.facing_right = true;
        } else if move_left {
            self.player.vx -= accel * dt;
            self.player.facing_right = false;
        } else {
            // Apply friction
            if self.player.on_ground {
                if self.player.vx > 0.0 {
                    self.player.vx = (self.player.vx - FRICTION * dt).max(0.0);
                } else if self.player.vx < 0.0 {
                    self.player.vx = (self.player.vx + FRICTION * dt).min(0.0);
                }
            }
        }
        self.player.vx = self.player.vx.clamp(-max_speed, max_speed);

        // ── Jump (higher when sprinting, like original SMB) ──
        if jump_just && self.player.on_ground {
            self.player.vy = if sprint {
                JUMP_VELOCITY_RUN
            } else {
                JUMP_VELOCITY
            };
            self.player.is_jumping = true;
            self.player.on_ground = false;
            self.combo_index = 0;
            self.combo_active = false;
            if self.player.is_big {
                ctx.audio.play("jumpbig");
            } else {
                ctx.audio.play("jump");
            }
        }
        if !jump_pressed {
            self.player.is_jumping = false;
        }

        // ── Gravity ──
        let grav = if self.player.is_jumping && self.player.vy < 0.0 {
            GRAVITY_JUMPING
        } else {
            GRAVITY
        };
        self.player.vy += grav * dt;
        self.player.vy = self.player.vy.min(MAX_Y_SPEED);

        // ── Move & collide ──
        self.player.vx = move_and_collide_x(
            &mut self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
            self.player.vx,
            &self.level,
            dt,
        );

        let (new_vy, on_ground) = move_and_collide_y(
            self.player.x,
            &mut self.player.y,
            self.player.width,
            self.player.height,
            self.player.vy,
            &self.level,
            dt,
        );
        self.player.vy = new_vy;
        self.player.on_ground = on_ground;

        // Checked after the move, so `on_ground` and the resolved position are the
        // ones the probe reads. A pipe found here takes over from the next frame.
        //
        // `blocked_right` rather than "holding right": the collision resolver zeroes
        // `vx` when it stops the player, and a sideways pipe mouth is solid, so this
        // is the "ran into it" signal the original's check sits behind.
        let blocked_right = move_right && self.player.vx == 0.0;
        self.check_pipe_entry(ctx, input.is_action_pressed("crouch"), blocked_right);
        if self.in_pipe() {
            return;
        }

        if on_ground {
            self.player.is_jumping = false;
            if self.combo_active {
                self.combo_index = 0;
                self.combo_active = false;
            }
        }

        // ── Block hit from below ──
        if self.player.vy == 0.0 && !on_ground {
            let head_row = ((self.player.y - 1.0) / TILE_SIZE).floor() as i32;
            let left_col = ((self.player.x + 4.0) / TILE_SIZE).floor() as i32;
            let right_col = ((self.player.x + self.player.width - 4.0) / TILE_SIZE).floor() as i32;
            for col in left_col..=right_col {
                if head_row >= 0
                    && head_row < self.level.height as i32
                    && col >= 0
                    && col < self.level.width as i32
                {
                    let r = head_row as usize;
                    let c = col as usize;
                    let tile = self.level.tiles[r][c];
                    self.hit_block(ctx, r, c, tile);
                }
            }
        }

        // ── Pit death ──
        if self.player.y > (self.level.height as f32) * TILE_SIZE + 100.0 {
            self.die(ctx);
            return;
        }

        // ── Portal gun cooldown ──
        self.player.portal_cooldown = (self.player.portal_cooldown - dt).max(0.0);
        self.player.teleport_cooldown = (self.player.teleport_cooldown - dt).max(0.0);
        self.player.invincible_timer = (self.player.invincible_timer - dt).max(0.0);
        self.star_timer = (self.star_timer - dt).max(0.0);

        // ── Fire portals ──
        if fire_blue && self.player.portal_cooldown <= 0.0 {
            self.fire_projectile(0);
            self.player.portal_cooldown = PORTAL_GUN_DELAY;
            ctx.audio.play("shot");
        }
        if fire_orange && self.player.portal_cooldown <= 0.0 {
            self.fire_projectile(1);
            self.player.portal_cooldown = PORTAL_GUN_DELAY;
            ctx.audio.play("shot");
        }

        // ── Fireballs ──
        if fire_ball && self.player.is_fire && self.fireballs.len() < MAX_FIREBALLS {
            let dir = if self.crosshair_angle.cos() >= 0.0 {
                1.0
            } else {
                -1.0
            };
            self.fireballs.push(Fireball {
                x: self.player.center_x(),
                y: self.player.center_y(),
                vx: FIREBALL_SPEED * dir,
                vy: 0.0,
                anim_timer: 0.0,
                exploding: false,
                explode_timer: 0.0,
            });
            ctx.audio.play("fireball");
        }

        // ── Update projectiles ──
        self.update_projectiles(ctx, dt);

        // ── Portal teleport ──
        self.check_portal_teleport(ctx);

        // ── Enemies ──
        self.update_enemies(dt, ctx);

        // ── Items (mushroom, star, 1-up, flower) ──
        self.update_items(ctx, dt);

        // ── Fireballs ──
        self.update_fireballs(ctx, dt);

        // ── Coins ──
        for coin in &mut self.level.coins {
            if !coin.collected
                && aabb_overlap(
                    [
                        self.player.x,
                        self.player.y,
                        self.player.width,
                        self.player.height,
                    ],
                    [coin.x, coin.y, 16.0, 16.0],
                )
            {
                coin.collected = true;
                self.score += COIN_SCORE;
                self.coins += 1;
                ctx.audio.play("coin");
            }
        }

        // ── Flag/level complete ──
        if self.level.flag_x > 0.0 && self.player.x + self.player.width > self.level.flag_x {
            self.state = GameState::LevelComplete;
            let time_bonus = (self.time_remaining as u32) * 50;
            self.score += time_bonus;
            ctx.audio.play("levelend");
        }

        // ── Camera ──
        let target_x = self.player.center_x() - self.vw / 3.0;
        self.camera.x = target_x.max(self.camera.x); // never scroll back
        let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
        self.camera.x = self.camera.x.clamp(0.0, max_camera);

        // Newly revealed columns spawn their enemies before anything updates, so
        // a goomba that appears this frame still gets its first step.
        self.spawn_revealed_columns();
        self.check_checkpoint_passed();

        // ── Timer and low-time music ──
        if self.tick_clock(ctx, dt) {
            self.die(ctx);
            return;
        }

        // ── Animation ──
        if !self.player.on_ground {
            self.player.anim_state = if self.player.vy < 0.0 {
                PlayerAnim::Jump
            } else {
                PlayerAnim::Fall
            };
        } else if self.player.vx.abs() > 10.0 {
            self.player.anim_state = PlayerAnim::Run;
            self.player.run_frame += self.player.vx.abs() * dt * 0.05;
        } else {
            self.player.anim_state = PlayerAnim::Idle;
        }

        self.update_visual_timers(dt);

        // ── Portal opening animation ──
        for portal_opt in &mut self.portals {
            if let Some(portal) = portal_opt
                && portal.open_scale < 1.0
            {
                portal.open_scale = (portal.open_scale + dt * 15.0).min(1.0);
            }
        }

        // ── Block bounce animations ──
        for bounce in &mut self.block_bounces {
            bounce.timer += dt;
        }
        self.block_bounces.retain(|b| b.timer < BLOCK_BOUNCE_TIME);

        // ── Coin popup animations ──
        for popup in &mut self.coin_popups {
            popup.timer += dt;
            popup.y += popup.vy * dt;
            popup.vy += GRAVITY * dt * 0.5; // slower gravity for coin arc
        }
        self.coin_popups.retain(|c| c.timer < COIN_POPUP_TIME);

        // ── Score popup animations ──
        for popup in &mut self.score_popups {
            popup.timer += dt;
            popup.y -= (SCORE_POPUP_HEIGHT / SCORE_POPUP_TIME) * dt;
        }
        self.score_popups.retain(|s| s.timer < SCORE_POPUP_TIME);

        // ── Brick debris animations ──
        for debris in &mut self.brick_debris {
            debris.timer += dt;
            debris.x += debris.vx * dt;
            debris.vy += DEBRIS_GRAVITY * dt;
            debris.y += debris.vy * dt;
        }
        self.brick_debris.retain(|d| d.timer < 2.0);

        // ── Multi-coin block timers ──
        let expired: Vec<(usize, usize)> = self
            .level
            .multi_coin_timers
            .iter()
            .filter_map(|(k, v)| if *v <= 0.0 { Some(*k) } else { None })
            .collect();
        for key in &expired {
            self.level.multi_coin_timers.remove(key);
            // Convert to used block
            if self.level.tiles[key.0][key.1] == SMB_BRICK {
                self.level.tiles[key.0][key.1] = self.used_block_tile();
            }
            self.level.block_contents.remove(key);
        }
        for timer in self.level.multi_coin_timers.values_mut() {
            *timer -= dt;
        }
    }

    pub(crate) fn die(&mut self, ctx: &mut Context) {
        if self.lives > 1 {
            self.lives -= 1;
            self.state = GameState::Dead;
            ctx.audio.play("death");
        } else {
            self.lives = 0;
            self.state = GameState::Dead;
            ctx.audio.play("gameover");
        }
    }
}

impl Game for Mari0Game {
    fn new(ctx: &mut Context, _renderer: &Renderer) -> Self {
        let t = |n: &str| Self::tex(ctx, n);

        let vw = ctx.virtual_width;
        let vh = ctx.virtual_height;

        let current = LevelId::new(START_PACK, 1, 1);
        let level = load_level(&current.pack, &current.name());
        let player_start = level.player_start;
        let spawned = vec![false; level.enemy_spawns.len()];
        let time_limit = level.time_limit;

        Self {
            state: GameState::Menu,
            current,
            player: Player::new(player_start.0, player_start.1),
            portals: [None, None],
            projectiles: Vec::new(),
            crosshair_angle: 0.0,
            aim_dot_timer: 0.0,
            portal_anim_timer: 0.0,
            portal_anim_frame: 0,
            enemies: Vec::new(),
            spawned,
            spawn_frontier: -1,
            camera: Camera { x: 0.0 },
            score: 0,
            coins: 0,
            lives: 3,
            combo_index: 0,
            combo_active: false,
            time_remaining: time_limit,
            block_bounces: Vec::new(),
            coin_popups: Vec::new(),
            score_popups: Vec::new(),
            brick_debris: Vec::new(),
            items: Vec::new(),
            fireballs: Vec::new(),
            star_timer: 0.0,
            level,
            pipe: None,
            checkpoint: None,
            checkpoints_passed: 0,
            respawn_sublevel: 0,
            music_phase: MusicPhase::Normal,
            warning_started_at: None,
            // Starts on the first frame of play, not at construction: the menu is
            // silent, and `Context` isn't usable for audio until then anyway.
            music_restart: false,

            tex_tiles: t("tiles"),
            tex_mario_layers: [t("mario0"), t("mario1"), t("mario2"), t("mario3")],
            tex_mario_big_layers: [
                t("mario_big0"),
                t("mario_big1"),
                t("mario_big2"),
                t("mario_big3"),
            ],
            tex_goomba: t("goomba"),
            tex_koopa: t("koopa"),
            tex_koopa_red: t("koopa_red"),
            tex_beetle: t("beetle"),
            tex_plant: t("plant"),
            tex_cheep_red: t("cheep_red"),
            tex_cheep_white: t("cheep_white"),
            tex_coin_anim: t("coin_anim"),
            tex_entities: t("entities"),
            tex_star: t("star"),
            tex_flower: t("flower"),
            tex_fireball: t("fireball"),
            tex_portal: t("portal"),
            tex_portal_v: t("portal_v"),
            tex_portal_crosshair: t("portal_crosshair"),
            tex_portal_projectile: t("portal_projectile"),
            tex_portal_dot: t("portal_dot"),
            tex_flag: t("flag"),
            vw,
            vh,
        }
    }

    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        if self.music_restart {
            self.music_restart = false;
            self.start_music(ctx);
        }

        match self.state {
            GameState::Menu => {
                if input.is_action_just_pressed("jump") {
                    self.state = GameState::Playing;
                    self.reset_level();
                    self.score = 0;
                    self.coins = 0;
                    self.lives = 3;
                }
            }
            GameState::Playing => {
                self.update_playing(ctx, dt, input);
            }
            GameState::Dead => {
                if input.is_action_just_pressed("jump") {
                    if self.lives > 0 {
                        self.state = GameState::Playing;
                        self.respawn_after_death();
                    } else {
                        // Game over clears the checkpoint (`levelscreen.lua:49`).
                        self.checkpoint = None;
                        self.respawn_sublevel = 0;
                        self.state = GameState::Menu;
                    }
                }
            }
            GameState::LevelComplete => {
                if input.is_action_just_pressed("jump") {
                    self.advance_level();
                }
            }
        }
    }

    fn clear_color(&self) -> Color {
        // The level's `background` field picks one of three NES backdrops
        // (`main.lua:194-197`). Hardcoding sky blue made every underground and
        // castle level render on a daytime sky.
        match self.level.background {
            2 => Color::from_hex(0x000000), // underground / castle
            3 => Color::from_hex(0x2038EC), // underwater
            _ => Color::from_hex(0x5C94FC), // overworld sky
        }
    }

    fn draw(&self, ctx: &Context, screen: &mut Screen) {
        self.draw_world(ctx, screen);
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        self.inspect_snapshot()
    }

    #[cfg(feature = "vdp")]
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.handle_vdp_method(method, params)
    }
}
