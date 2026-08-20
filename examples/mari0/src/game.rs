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
use crate::lab::Lab;
use crate::level;
use crate::maze::MazeState;
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
    /// Bubbles Mario has breathed out. Only ever non-empty in a water level.
    pub(crate) bubbles: Vec<Bubble>,
    pub(crate) star_timer: f32, // player star invincibility timer

    /// The pipe trip in progress, if any. While set, the player has no control.
    pub(crate) pipe: Option<PipeTransit>,

    /// Progress through this level's maze spans, if it has any.
    pub(crate) maze: MazeState,

    /// The lab's signal network: buttons, doors, indicators and the wiring.
    pub(crate) lab: Lab,
    /// Weighted cubes. Bodies, not lab elements — see `cube.rs`.
    pub(crate) cubes: Vec<crate::cube::Cube>,
    /// Where the title screen's cursor is.
    pub(crate) menu: crate::menu::MenuCursor,
    /// Saved state: best score ever, and the furthest level reached per mappack.
    pub(crate) storage: Storage,
    pub(crate) high_score: u32,
    pub(crate) furthest: Vec<(String, u32, u32)>,
    /// Fireworks earned by the last flagpole — 200 points each (see
    /// `flagpole::FIREWORK_SCORE`, which the original's own comment gets wrong), and the
    /// count is a function of the clock's last digit. Kept so the end-of-level screen
    /// (and a test) can see how many went up.
    pub(crate) fireworks: u32,
    /// Emancipation grills, resolved to spans at load.
    pub(crate) grills: Vec<crate::emancipation::Grill>,
    /// The player's centre last frame, for the grills' swept crossing test.
    pub(crate) previous_player_centre: (f32, f32),
    /// Gel in the air. What lands becomes paint in `level.gels`.
    pub(crate) gel_blobs: Vec<crate::gel::GelBlob>,
    /// Which of the three splat sprites the next blob gets, and how far across the
    /// nozzle it starts. A cycle rather than the original's `math.random`, so a replay
    /// is reproducible.
    pub(crate) gel_frame: u32,
    /// Last frame's light-bridge slabs, so a slab that has just appeared can shove
    /// whatever is standing in it. Kept apart from `level.solid_rects`, which also holds
    /// the dispensers — those don't push.
    pub(crate) bridge_rects: Vec<[f32; 4]>,

    /// The highest checkpoint passed, as a tile cell.
    ///
    /// Survives death — that's the whole point — and is cleared when a new level
    /// starts or the game ends (`levelscreen.lua:34`, `:49`).
    pub(crate) checkpoint: Option<(i32, i32)>,
    /// How many checkpoints have been passed, so the next one to watch for is a
    /// single index rather than a scan.
    pub(crate) checkpoints_passed: usize,
    /// The axe ending in progress, if the player has taken the axe.
    pub(crate) castle: Option<crate::castle::CastleEnding>,
    /// The flagpole ending in progress, if the player has grabbed the pole. Owns him for
    /// the whole sequence, the way the axe ending does.
    pub(crate) flag: Option<crate::flagpole::FlagSequence>,
    /// Firework bursts currently on screen.
    pub(crate) fireworks_shown: Vec<crate::flagpole::Firework>,
    /// The shared coin-spin phase, in seconds.
    ///
    /// One counter for every coin in the level, because the original has one
    /// (`coinanimation`, `game.lua:149`) — coins spin in unison, they don't each keep
    /// their own phase. Separate from the clock so it keeps turning in levels with no
    /// time limit.
    pub(crate) coin_spin: f32,
    /// Springs, and the ride in progress if Mario is on one.
    pub(crate) springs: Vec<crate::spring::Spring>,
    pub(crate) spring_ride: Option<crate::spring::SpringRide>,
    /// Seesaw rigs. Built at load like the springs, because the original creates them in
    /// its parsing loop rather than revealing them with the camera.
    pub(crate) seesaws: Vec<crate::seesaw::Seesaw>,
    /// Vines growing in the world. Unlike springs these are not placed at load: a vine
    /// exists only once its block has been hit — except in a `bonusstage`, which opens
    /// with one already on its way up.
    pub(crate) vines: Vec<crate::vine::Vine>,
    /// What the vine is doing to Mario: holding him, carrying him out of the level, or
    /// climbing him into a bonus room. `None` most of the time.
    pub(crate) vine: Option<crate::vine::VineState>,
    /// Moving platforms currently in the world, and which of the level's are still
    /// waiting for the camera.
    pub(crate) platforms: Vec<crate::platform::Platform>,
    pub(crate) platforms_spawned: Vec<bool>,
    /// The shaft spawners. Copied out of the level because they mutate (each carries a
    /// release timer) and are dropped once the camera passes them.
    pub(crate) platform_spawners: Vec<crate::platform::PlatformSpawner>,
    /// Deterministic stand-in for `math.random`: cannon delays, bill altitudes.
    ///
    /// Reseeded per level so a level always plays out the same way — the VDP probes
    /// and the autopilot both depend on that.
    pub(crate) rng: Rng,
    /// Has the player passed `firestart`? One-way, unlike the other two zones.
    pub(crate) fire_started: bool,
    pub(crate) fire_timer: f32,
    pub(crate) fire_delay: f32,
    /// Is the player inside this level's `flyingfishstart`…`flyingfishend` stretch?
    pub(crate) flying_fish_zone: bool,
    pub(crate) flying_fish_timer: f32,
    pub(crate) flying_fish_delay: f32,
    /// Is the player inside this level's `bulletbillstart`…`bulletbillend` stretch?
    ///
    /// Unlike lakitu's retirement this one is **not** a latch: crossing `end` turns it
    /// back off (`mario.lua:985-991`), and 5-3 uses both ends to fence off one run of
    /// the level. 6-3 has only a start, so once on it stays on.
    pub(crate) bullet_bill_zone: bool,
    /// Time since the zone last dropped a bill, and how long it waits this round.
    pub(crate) bullet_bill_timer: f32,
    pub(crate) bullet_bill_delay: f32,
    /// Has the player walked past this level's `lakitoend` column?
    ///
    /// A latch, never cleared while the level runs (`mario.lua:993-995` only ever
    /// sets it true), which is what makes lakitu's retirement final: once past the
    /// column he goes passive and drifts off to the left for good, even if you walk
    /// back. Levels without a `lakitoend` never set it.
    pub(crate) lakito_retired: bool,
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
    pub(crate) tex_lakito: TextureId,
    pub(crate) tex_bullet_bill: TextureId,
    pub(crate) tex_hammer_bro: TextureId,
    pub(crate) tex_hammer: TextureId,
    pub(crate) tex_squid: TextureId,
    pub(crate) tex_spring: TextureId,
    pub(crate) tex_vine: TextureId,
    pub(crate) tex_seesaw: TextureId,
    pub(crate) tex_bubble: TextureId,
    pub(crate) tex_castle_flag: TextureId,
    pub(crate) tex_bowser: TextureId,
    pub(crate) tex_fire: TextureId,
    pub(crate) tex_decoys: TextureId,
    pub(crate) tex_platform: TextureId,
    pub(crate) tex_platform_bonus: TextureId,
    pub(crate) tex_spikey: TextureId,
    pub(crate) tex_cheep_red: TextureId,
    pub(crate) tex_cheep_white: TextureId,
    pub(crate) tex_coin: TextureId,
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

    // ── The lab ──────────────────────────────────────────────────────
    pub(crate) tex_portal_tiles: TextureId,
    pub(crate) tex_laser: TextureId,
    pub(crate) tex_laser_side: TextureId,
    pub(crate) tex_laser_detector: TextureId,
    pub(crate) tex_light_bridge: TextureId,
    pub(crate) tex_light_bridge_side: TextureId,
    pub(crate) tex_button_base: TextureId,
    pub(crate) tex_button_cap: TextureId,
    pub(crate) tex_push_button: TextureId,
    pub(crate) tex_door_piece: TextureId,
    pub(crate) tex_door_centre: TextureId,
    pub(crate) tex_wall_indicator: TextureId,
    pub(crate) tex_wall_timer: TextureId,
    pub(crate) tex_cube: TextureId,
    pub(crate) tex_cube_dispenser: TextureId,
    pub(crate) tex_gel_dispenser: TextureId,
    pub(crate) tex_faith_plate: TextureId,
    pub(crate) tex_grill_side: TextureId,
    pub(crate) tex_grill_particle: TextureId,
    /// Blob art, indexed by [`crate::level::Gel`] order: blue, orange, white.
    pub(crate) tex_gel: [TextureId; 3],
    /// The same three colours as paint on a tile face.
    pub(crate) tex_gel_ground: [TextureId; 3],

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
            self.record_progress();
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
        // Re-parsed rather than threaded through `Level`: the lab network is a
        // separate graph over the same placements, and `Level` is already the
        // gameplay view of the tile data.
        let lab_placements = level::load(&self.current.pack, &self.current.name())
            .and_then(|r| r.ok())
            .map(|p| p.markers.lab)
            .unwrap_or_default();
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
        self.platforms.clear();
        self.springs = level
            .springs
            .iter()
            .map(|(x, y)| crate::spring::Spring::new(*x, *y))
            .collect();
        self.spring_ride = None;
        self.flag = None;
        self.fireworks_shown.clear();
        self.seesaws = level
            .seesaws
            .iter()
            .map(|(x, y, kind)| crate::seesaw::Seesaw::new(*x, *y, *kind))
            .collect();
        self.castle = None;
        self.platforms_spawned = vec![false; level.platform_spawns.len()];
        self.platform_spawners = level.platform_spawners.clone();
        self.lakito_retired = false;
        self.bullet_bill_zone = false;
        self.bullet_bill_timer = 0.0;
        self.flying_fish_zone = false;
        self.flying_fish_timer = 0.0;
        self.fire_started = false;
        self.fire_timer = 0.0;
        self.fire_delay = 1.0;
        // Seeded from the level's shape so different levels differ but a reload of the
        // same one repeats. `time_limit` and `width` are both stable per file.
        self.rng =
            Rng::new((level.width as u32).wrapping_mul(2_654_435_761) ^ level.time_limit as u32);
        self.bullet_bill_delay = self.rng.tenths(BULLET_BILL_ZONE_MIN, BULLET_BILL_ZONE_MAX);
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
        self.bubbles.clear();
        self.star_timer = 0.0;
        self.pipe = None;
        self.maze = MazeState::for_level(level.maze_starts.len());
        self.lab = Lab::build(&lab_placements);
        self.level = level;
        // After the graph, because each cube needs the index of the `box` element it
        // stands for — that wire is how its dispenser hears about its death.
        self.spawn_level_cubes();
        self.gel_blobs.clear();
        self.bridge_rects.clear();
        // Grills need the tile grid, so they are resolved after the level is in place.
        self.build_grills();
        self.previous_player_centre = (self.player.center_x(), self.player.center_y());
        // `self.portals` was just cleared, so there are no holes either. Explicit
        // rather than relying on the freshly-loaded level starting empty.
        self.refresh_portal_holes();
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

        // A bonus room does not start with Mario standing in it — it starts with him
        // below the floor, climbing in on a vine (`game.lua:2139-2141`). Last, because
        // it moves the player and the camera has to follow him there.
        self.vines.clear();
        self.vine = None;
        if self.level.bonusstage {
            self.start_vine_intro();
            self.camera = Camera { x: 0.0 };
        }
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

    /// Drop bullet bills in from the right edge while the player is in the zone.
    ///
    /// A second, quite different source from the cannons: no cannon, no range check
    /// and **no cap** on how many are alive (`game.lua:826-831`). Bills simply appear
    /// two blocks past the right edge of the screen at a random altitude and fly left,
    /// which is why 5-3's tightrope section feels like weather rather than gunfire.
    ///
    /// The `while` is the original's, and it matters: each pass draws a *fresh* delay,
    /// so a long frame can release several at once with different gaps behind them.
    fn update_bullet_bill_zone(&mut self, dt: f32, ctx: &mut Context) {
        if let Some(start) = self.level.bullet_bill_start
            && self.player.x >= start as f32 * TILE_SIZE
        {
            self.bullet_bill_zone = true;
        }
        if let Some(end) = self.level.bullet_bill_end
            && self.player.x >= end as f32 * TILE_SIZE
        {
            self.bullet_bill_zone = false;
        }
        if !self.bullet_bill_zone {
            return;
        }
        self.bullet_bill_timer += dt;
        while self.bullet_bill_timer > self.bullet_bill_delay {
            self.bullet_bill_timer -= self.bullet_bill_delay;
            self.bullet_bill_delay = self.rng.tenths(BULLET_BILL_ZONE_MIN, BULLET_BILL_ZONE_MAX);
            let row = self
                .rng
                .range(BULLET_BILL_ZONE_ROWS.0, BULLET_BILL_ZONE_ROWS.1);
            self.enemies.push(Enemy::bullet_bill(
                self.camera.x + self.vw + 2.0 * TILE_SIZE,
                row as f32 * TILE_SIZE,
                -1.0,
            ));
            ctx.audio.play("bulletbill");
        }
    }

    /// Breathe fire, either from Bowser or from a `firestart` zone with no Bowser in it.
    ///
    /// Three things make this different from the other two zone spawners. The latch is
    /// **one-way** — there is no `fireend` entity. The gate includes *Bowser's own
    /// state*: he stops breathing while backing away, dying or falling
    /// (`game.lua:806`), which is the other half of why getting behind him disarms him.
    /// And when he is present the breath comes from **his mouth**, aimed a random couple
    /// of blocks around his starting row, rather than from the screen edge
    /// (`fire.lua:4-16`).
    fn update_fire_breath(&mut self, dt: f32, ctx: &mut Context) {
        if let Some(start) = self.level.fire_start
            && self.player.x >= start as f32 * TILE_SIZE
        {
            self.fire_started = true;
        }
        if !self.fire_started {
            return;
        }
        // Whoever is breathing has to be in a state to do it.
        let bowser = self
            .enemies
            .iter()
            .find(|e| e.enemy_type == EnemyType::Bowser)
            .map(|e| (e.x, e.y, e.spawn_y, e.backing_off, e.state));
        if let Some((.., backing_off, state)) = bowser
            && (backing_off || state != EnemyState::Walking)
        {
            return;
        }
        self.fire_timer += dt;
        while self.fire_timer > self.fire_delay {
            self.fire_timer -= self.fire_delay;
            // `math.random(4)` — whole seconds, 1..=4, not tenths like the others.
            self.fire_delay = self.rng.range(1, 4) as f32;
            let (x, y, target) = match bowser {
                Some((bx, by, spawn_y, ..)) => (
                    bx - 0.75 * TILE_SIZE,
                    by + 0.25 * TILE_SIZE,
                    spawn_y - (self.rng.range(1, 3) as f32 - 2.0 / 16.0) * TILE_SIZE,
                ),
                // No Bowser: it comes in from the right edge at a random height in the
                // lower half, which is how the fire corridors without him work.
                None => {
                    let row = self.rng.range(8, 10) as f32;
                    let y = row * TILE_SIZE;
                    (self.camera.x + self.vw, y, y)
                }
            };
            self.enemies.push(Enemy::fire(x, y, target));
            ctx.audio.play("fire");
        }
    }

    /// Send flying fish leaping out of the water while the player is in the zone.
    ///
    /// The same shape as the bullet-bill zone down to the two-way latch, but the fish
    /// come from *below*: they start under the bottom of the world at a random column
    /// **inside the visible screen** (`flyingfish.lua:5`) rather than off the edge, so
    /// they burst up through the floor in front of you.
    ///
    /// Their sideways speed is the player's own plus a nudge, which makes them
    /// impossible to outrun by design.
    fn update_flying_fish_zone(&mut self, dt: f32) {
        if let Some(start) = self.level.flying_fish_start
            && self.player.x >= start as f32 * TILE_SIZE
        {
            self.flying_fish_zone = true;
        }
        if let Some(end) = self.level.flying_fish_end
            && self.player.x >= end as f32 * TILE_SIZE
        {
            self.flying_fish_zone = false;
        }
        if !self.flying_fish_zone {
            return;
        }
        self.flying_fish_timer += dt;
        while self.flying_fish_timer > self.flying_fish_delay {
            self.flying_fish_timer -= self.flying_fish_delay;
            self.flying_fish_delay = self.rng.tenths(FLYING_FISH_MIN, FLYING_FISH_MAX);
            let col = self.rng.range(0, (self.vw / TILE_SIZE) as i32);
            let drift = self.rng.range(FLYING_FISH_DRIFT.0, FLYING_FISH_DRIFT.1) as f32;
            self.enemies.push(Enemy::flying_fish(
                self.camera.x + col as f32 * TILE_SIZE,
                self.level.height as f32 * TILE_SIZE,
                self.player.vx + drift * TILE_SIZE,
            ));
        }
    }

    /// Retire lakitu once the player is past this level's `lakitoend` column.
    ///
    /// Checked against the *player*, not the camera (`mario.lua:993`), so running
    /// ahead is what dismisses him rather than the view catching up.
    fn check_lakito_retired(&mut self) {
        if let Some(col) = self.level.lakito_end
            && self.player.x >= col as f32 * TILE_SIZE
        {
            self.lakito_retired = true;
        }
    }

    /// Purely cosmetic cycles: the portal frame animation and the aim-dot phase.
    ///
    /// Kept separate from the rest of the update so a pipe transition — which
    /// suspends input, physics and the clock — can still let the portals shimmer.
    fn update_visual_timers(&mut self, dt: f32) {
        self.coin_spin += dt;
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

        // ── The castle ending ──
        // Owns the player like a pipe does, and for longer: from the axe until Bowser
        // is in the lava nothing moves at all, and even after Mario is released the
        // input stays disabled while he walks to the toad.
        if self.update_castle(dt, ctx) {
            self.update_visual_timers(dt);
            self.update_enemies(dt, ctx);
            // The camera still follows him to the toad; without this the last walk
            // happens off the right of a frozen view.
            let target_x = self.player.center_x() - self.vw / 3.0;
            let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
            self.camera.x = target_x.max(self.camera.x).clamp(0.0, max_camera);
            return;
        }

        // ── The flagpole ending ──
        // Same contract as the axe: from the grab to the next level there is no input at
        // all. The camera keeps following him into the castle, and the clock is *not*
        // ticked here — the countdown beat spends it deliberately, 50 points a unit.
        if self.update_flagpole(ctx, dt) {
            self.update_visual_timers(dt);
            self.update_enemies(dt, ctx);
            let target_x = self.player.center_x() - self.vw / 3.0;
            let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
            self.camera.x = target_x.max(self.camera.x).clamp(0.0, max_camera);
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
        let crouch_held = input.is_action_pressed("crouch");

        // ── Vines ──
        // A vine owns the player the way a pipe does, but not equally. *Climbing* one
        // leaves the controls live, and the clock follows the controls in the original
        // (`game.lua:189-196` stops it for any player whose `controlsenabled` is false),
        // so a climb burns time and its two cut-scene halves — the bonus-stage intro
        // and the ride out of the top of the level — do not.
        //
        // The side-hop is bound to the *press*, not the hold: the first tap swings him
        // round the stem and only the second lets go, which needs edges to tell apart.
        if self.update_vine(
            ctx,
            dt,
            input.is_action_pressed("climb_up"),
            input.is_action_pressed("crouch"),
            input.is_action_just_pressed("move_left"),
            input.is_action_just_pressed("move_right"),
        ) {
            self.update_visual_timers(dt);
            if self.vine_has_control() {
                self.update_enemies(dt, ctx);
                if self.tick_clock(ctx, dt) {
                    self.die(ctx);
                }
            }
            return;
        }

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

        // ── Ducking ──
        // Only a big Mario standing still on the ground, and it is a *toggle held*: let
        // go of down and he stands straight back up (`mario.lua:949-957`). Before the
        // movement, because a crouched Mario cannot walk.
        if self.player.on_ground && !self.player.is_jumping && self.player.is_big {
            let want = crouch_held && !self.level.underwater;
            if want != self.player.ducking {
                // Standing up needs headroom. The original never checks — it cannot walk
                // while crouched, so it can only ever be crouched somewhere it stood up
                // from — but a portal or a light bridge can put a ceiling over him after
                // the fact, and growing into it would push him through the floor.
                if want
                    || crate::physics::rect_is_clear(
                        &self.level,
                        self.player.x,
                        self.player.y - (PLAYER_BIG_H - DUCK_HEIGHT),
                        self.player.width,
                        PLAYER_BIG_H,
                    )
                {
                    self.player.set_ducking(want);
                }
            }
        }

        // ── Horizontal movement (sprint = higher accel & max speed) ──
        // The limits are looked up per frame rather than read from the constants, because
        // orange gel replaces them for as long as you stand on it and water replaces them
        // outright — see `gel::speed_limits`.
        let (max_walk, max_run, walk_accel, run_accel) = self.speed_limits();
        let accel = if sprint { run_accel } else { walk_accel };
        let max_speed = if sprint { max_run } else { max_walk };
        // A crouched Mario is rooted: the original's ground branches are all guarded on
        // `ducking == false`, so he keeps whatever speed he had and loses it to friction.
        let (move_left, move_right) = if self.player.ducking {
            (false, false)
        } else {
            (move_left, move_right)
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
        // Above the limit, speed is bled off rather than clamped — see `SUPER_FRICTION`.
        if self.player.vx.abs() > max_speed {
            let slowed = (self.player.vx.abs() - SUPER_FRICTION * dt).max(max_speed);
            self.player.vx = slowed * self.player.vx.signum();
        }

        // ── Jump, or a swimming stroke ──
        // Underwater the jump has **no ground check at all** (`mario.lua:1579-1589`):
        // every press is a stroke, wherever you are, and that is what swimming *is*. It
        // also cancels a crouch, and the force is flat — `uwjumpforceadd` is 0, so your
        // speed makes no difference to it the way it does on land.
        if jump_just && self.level.underwater {
            self.player.set_ducking(false);
            self.player.vy = -UW_JUMP_FORCE;
            self.player.is_jumping = true;
            self.player.on_ground = false;
            self.combo_index = 0;
            self.combo_active = false;
            ctx.audio.play("swim");
        } else if jump_just && self.player.on_ground {
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
        // Water reverses the usual relationship between the two figures. On land the
        // rising gravity is the *smaller* one (30 against 80) so a held jump floats;
        // underwater it is the larger (12 against 9), so a stroke is checked quickly and
        // the sink that follows is slow. That asymmetry is the whole feel of swimming.
        let rising = self.player.is_jumping && self.player.vy < 0.0;
        let grav = match (self.level.underwater, rising) {
            (true, true) => UW_GRAVITY_JUMPING,
            (true, false) => UW_GRAVITY,
            (false, true) => GRAVITY_JUMPING,
            (false, false) => GRAVITY,
        };
        self.player.vy += grav * dt;
        self.player.vy = self.player.vy.min(MAX_Y_SPEED);

        // ── The surface ──
        // Rise until your *feet* clear the waterline and you are shoved back down. You
        // can swim up to it and never out of it, which is what keeps a water level a
        // closed box (`mario.lua:1499-1501`).
        if self.level.underwater && self.player.bottom() < UW_MAX_HEIGHT {
            self.player.vy = UW_PUSH_DOWN_SPEED;
        }

        // ── Portal entry (swept), before the move ──
        // The two swept tests need where the player is *about* to be: a body moving
        // fast enough crosses a portal's mouth entirely within one step, and an
        // overlap test after the fact never sees it. Teleporting here also means the
        // collision resolver below never gets the chance to stop the player on the
        // wall the portal is mounted in.
        let teleported = self.check_portal_entry(
            ctx,
            self.player.x + self.player.vx * dt,
            self.player.y + self.player.vy * dt,
        );

        // ── Springs ──
        // A spring seizes the player for two tenths of a second, so it has to come
        // before the resolver — otherwise the same frame both parks him on the surface
        // and then collides him off it. Like the pipe it is a state that owns him, but
        // a short one, and the jump button still counts: pressing it during the ride is
        // what charges the launch.
        if self.update_springs(dt, jump_pressed) {
            self.update_visual_timers(dt);
            return;
        }

        // ── Move & collide ──
        // Kept from before the resolver zeroes them: a blue-gel bounce is computed from
        // the speed the player *arrived* with, not the zero they leave with.
        let impact_vx = self.player.vx;
        let impact_vy = self.player.vy;
        if !teleported {
            self.player.vx = move_and_collide_x(
                &mut self.player.x,
                self.player.y,
                self.player.width,
                self.player.height,
                self.player.vx,
                &self.level,
                dt,
                Body::Normal,
            );

            let (new_vy, on_ground) = move_and_collide_y(
                self.player.x,
                &mut self.player.y,
                self.player.width,
                self.player.height,
                self.player.vy,
                &self.level,
                dt,
                Body::Normal,
            );
            self.player.vy = new_vy;
            self.player.on_ground = on_ground;

            // Blue gel, floor then wall. Holding down cancels either — that is how you
            // stop on the stuff.
            let crouching = input.is_action_pressed("crouch");
            if on_ground {
                self.blue_floor_bounce(impact_vy, crouching, dt);
            }
            if self.player.vx == 0.0 && impact_vx != 0.0 {
                self.blue_wall_bounce(impact_vx, crouching);
            }
        } else {
            // A teleport ends the frame's ground contact: you left the surface.
            self.player.on_ground = false;
        }

        // Last resort: anything already sitting inside a mouth goes through, with no
        // clearance check, exactly as `inportal` does.
        self.check_in_portal(ctx);

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

        if self.player.on_ground {
            self.player.is_jumping = false;
            if self.combo_active {
                self.combo_index = 0;
                self.combo_active = false;
            }
        }

        // ── Block hit from below ──
        if self.player.vy == 0.0 && !self.player.on_ground {
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

        // ── Grabbing a vine ──
        // After the move, where the original's collision pass would have reported the
        // overlap, and after the block hit — so the frame you headbutt the brick is also
        // the frame the vine exists, even though it has no height yet to catch you.
        self.check_vine_grab();

        // ── Pit death ──
        // Except in a bonus room, where the pit is the door: you ride the platform to
        // the end, drop off it and land back in the level that sent you
        // (`mario.lua:2603-2607`). Nothing else about a bonus stage gets you out.
        if self.player.y > (self.level.height as f32) * TILE_SIZE + 100.0 {
            if self.level.bonusstage {
                self.leave_bonus_stage();
            } else {
                self.die(ctx);
            }
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

        // ── Enemies ──
        self.update_enemies(dt, ctx);
        // After the enemies, not before: the gate reads Bowser's `backing_off`, and a
        // frame of staleness there lets one breath out the instant you get behind him.
        self.update_fire_breath(dt, ctx);

        // ── Gel ──
        // Before the cubes and the lab so paint laid this frame is what everything else
        // reads.
        self.update_gels(dt);

        // ── Cubes ──
        // After the enemies so a cube dropped on a goomba lands on where it actually
        // is, and before the lab so a cube resting on a plate is sensed this frame.
        self.update_cubes(dt);

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

        // ── The axe ending ──
        // Checked before the flagpole because a castle has no flagpole at all: these
        // are the two mutually exclusive ways a level ends.
        self.check_axe(ctx);

        // ── The flagpole ──
        // Grabbing it hands the level over to a scripted sequence: slide down the pole,
        // hang, run into the castle, cash in the clock, raise the castle's flag, and set
        // off the fireworks. The score is paid out *across* those beats rather than in
        // one lump, which is what the ticking is for.
        self.check_flagpole(ctx);

        // ── Camera ──
        let target_x = self.player.center_x() - self.vw / 3.0;
        self.camera.x = target_x.max(self.camera.x); // never scroll back
        let max_camera = (self.level.width as f32 * TILE_SIZE - self.vw).max(0.0);
        self.camera.x = self.camera.x.clamp(0.0, max_camera);

        // Newly revealed columns spawn their enemies before anything updates, so
        // a goomba that appears this frame still gets its first step.
        self.spawn_revealed_columns();
        self.check_checkpoint_passed();
        self.check_lakito_retired();
        self.update_bullet_bill_zone(dt, ctx);
        // No sound: a leaping fish is silent in the original, and there are a lot of them.
        self.update_flying_fish_zone(dt);
        self.check_maze_gate();
        self.update_maze();
        self.update_lab(ctx, dt, input.is_action_just_pressed("use"));
        self.update_platforms(dt);
        // After the platforms, and it *extends* their rect list rather than replacing
        // it — `update_platforms` rebuilds that wholesale every frame.
        self.update_seesaws(dt);
        self.bump_bonus_platforms();
        self.update_plates_and_grills(ctx, dt);

        // ── Bubbles ──
        self.update_bubbles(dt);

        // ── Timer and low-time music ──
        if self.tick_clock(ctx, dt) {
            self.die(ctx);
            return;
        }

        // ── Animation ──
        // Not while holding a vine. A grab is only detected *after* the move, so the
        // frame Mario catches one still reaches here — and this would overwrite the
        // climbing pose he was just put into with "falling", making the first frame of
        // every climb flicker. The original never gets this far: `mario:update` returns
        // from inside its vine branch (`mario.lua:856`).
        if self.vine.is_some() {
            // leave `anim_state` where the vine put it
        } else if self.player.ducking {
            self.player.anim_state = PlayerAnim::Duck;
        } else if !self.player.on_ground && self.level.underwater {
            // The swimming sprite is only used off the ground; walking the sea floor
            // still runs (`mario.lua:1516` gates it on jumping/falling). The phase lives
            // in `[1, 3)` so its floor is 1 or 2 and never 0.
            self.player.anim_state = PlayerAnim::Swim;
            self.player.swim_phase += UW_SWIM_ANIM_SPEED * dt;
            while self.player.swim_phase >= 3.0 {
                self.player.swim_phase -= 2.0;
            }
        } else if !self.player.on_ground {
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

    /// Breathe out, and drift what has already been breathed out upward.
    ///
    /// The wander is a cycle rather than the original's `math.random`, for the same
    /// reason the cannon delays are: a replay has to come out the same way twice. It
    /// alternates between the two intervals instead of picking one at random too.
    fn update_bubbles(&mut self, dt: f32) {
        if self.level.underwater {
            self.player.bubble_timer += dt;
            while self.player.bubble_timer > BUBBLE_TIMES[self.player.bubble_index] {
                self.player.bubble_timer -= BUBBLE_TIMES[self.player.bubble_index];
                self.player.bubble_index = (self.player.bubble_index + 1) % BUBBLE_TIMES.len();
                // Out of his mouth, which is up and forward of his box origin.
                self.bubbles.push(Bubble {
                    x: self.player.x + 8.0 / 12.0 * TILE_SIZE,
                    y: self.player.y + 2.0 / 12.0 * TILE_SIZE,
                    vy: -BUBBLE_SPEED,
                });
            }
        }
        for (i, b) in self.bubbles.iter_mut().enumerate() {
            // Deterministic stand-in for the original's random walk: each bubble wanders
            // on its own phase, so a column of them does not rise in lockstep.
            let phase = (self.coin_spin * 3.0 + i as f32 * 1.7).sin();
            b.vy = -BUBBLE_SPEED + phase * BUBBLE_MARGIN;
            b.y += b.vy * dt;
        }
        self.bubbles.retain(|b| b.y >= BUBBLE_MAX_Y);
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
        // The saved progress is read into the struct below via `load_progress`, called
        // on the way out — `Storage::load` never fails, it just comes back empty.
        let t = |n: &str| Self::tex(ctx, n);

        let vw = ctx.virtual_width;
        let vh = ctx.virtual_height;

        let current = LevelId::new(START_PACK, 1, 1);
        let level = load_level(&current.pack, &current.name());
        let player_start = level.player_start;
        let spawned = vec![false; level.enemy_spawns.len()];
        let time_limit = level.time_limit;

        let mut game = Self {
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
            maze: MazeState::default(),
            lab: Lab::default(),
            cubes: Vec::new(),
            menu: crate::menu::MenuCursor::default(),
            storage: Storage::load("mari0"),
            high_score: 0,
            furthest: Vec::new(),
            fireworks: 0,
            grills: Vec::new(),
            previous_player_centre: (0.0, 0.0),
            gel_blobs: Vec::new(),
            gel_frame: 0,
            bridge_rects: Vec::new(),
            checkpoint: None,
            checkpoints_passed: 0,
            lakito_retired: false,
            castle: None,
            coin_spin: 0.0,
            springs: Vec::new(),
            spring_ride: None,
            seesaws: Vec::new(),
            flag: None,
            fireworks_shown: Vec::new(),
            vines: Vec::new(),
            vine: None,
            bubbles: Vec::new(),
            platforms: Vec::new(),
            platforms_spawned: Vec::new(),
            platform_spawners: Vec::new(),
            rng: Rng::new(1),
            fire_started: false,
            fire_timer: 0.0,
            fire_delay: 1.0,
            flying_fish_zone: false,
            flying_fish_timer: 0.0,
            flying_fish_delay: FLYING_FISH_MIN,
            bullet_bill_zone: false,
            bullet_bill_timer: 0.0,
            bullet_bill_delay: BULLET_BILL_ZONE_MIN,
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
            tex_lakito: t("lakito"),
            tex_bullet_bill: t("bullet_bill"),
            tex_hammer_bro: t("hammer_bro"),
            tex_hammer: t("hammer"),
            tex_squid: t("squid"),
            tex_spring: t("spring"),
            tex_vine: t("vine"),
            tex_seesaw: t("seesaw"),
            tex_bubble: t("bubble"),
            tex_castle_flag: t("castle_flag"),
            tex_bowser: t("bowser"),
            tex_fire: t("fire"),
            tex_decoys: t("decoys"),
            tex_platform: t("platform"),
            tex_platform_bonus: t("platform_bonus"),
            tex_spikey: t("spikey"),
            tex_cheep_red: t("cheep_red"),
            tex_cheep_white: t("cheep_white"),
            tex_coin: t("coin"),
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
            tex_portal_tiles: t("portal_tiles"),
            tex_laser: t("laser"),
            tex_laser_side: t("laser_side"),
            tex_laser_detector: t("laser_detector"),
            tex_light_bridge: t("light_bridge"),
            tex_light_bridge_side: t("light_bridge_side"),
            tex_button_base: t("button_base"),
            tex_button_cap: t("button_cap"),
            tex_push_button: t("push_button"),
            tex_door_piece: t("door_piece"),
            tex_door_centre: t("door_centre"),
            tex_wall_indicator: t("wall_indicator"),
            tex_wall_timer: t("wall_timer"),
            tex_cube: t("cube"),
            tex_cube_dispenser: t("cube_dispenser"),
            tex_gel_dispenser: t("gel_dispenser"),
            tex_faith_plate: t("faith_plate"),
            tex_grill_side: t("grill_side"),
            tex_grill_particle: t("grill_particle"),
            tex_gel: [t("gel_blue"), t("gel_orange"), t("gel_white")],
            tex_gel_ground: [
                t("gel_blue_ground"),
                t("gel_orange_ground"),
                t("gel_white_ground"),
            ],
            vw,
            vh,
        };
        game.load_progress();
        game
    }

    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        if self.music_restart {
            self.music_restart = false;
            self.start_music(ctx);
        }

        match self.state {
            GameState::Menu => self.update_menu(input),
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
