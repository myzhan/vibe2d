//! Gameplay constants, converted from Mari0's units.
//!
//! The original works in blocks and blocks/second with `1 block = 16px`
//! (`variables.lua:1-2`). This port renders at `TILE_SIZE = 32` because the
//! 512x480 virtual resolution is built around a 16x15 tile screen, so every
//! original constant appears here multiplied by 32 — the comment on each line
//! keeps the original value visible so it can be checked against the Lua source.

// ── Physics Constants (mari0-inspired, 1 block = 32px) ─────────────
pub(crate) const TILE_SIZE: f32 = 32.0;
pub(crate) const GRAVITY: f32 = 2560.0; // 80 blocks/s^2
/// Friction applied to horizontal speed **above** the walk/run limit
/// (`superfriction = 100`, `variables.lua:20`).
///
/// The limit is not a clamp: anything that throws you faster than you could accelerate
/// — a faith plate, orange gel, a portal — keeps its speed and bleeds it off at this
/// rate. Hard-clamping instead is what made a diagonal faith plate barely move you.
pub(crate) const SUPER_FRICTION: f32 = 100.0 * TILE_SIZE;
pub(crate) const GRAVITY_JUMPING: f32 = 960.0; // reduced while holding jump
pub(crate) const JUMP_VELOCITY: f32 = -512.0; // initial upward (walking)
pub(crate) const JUMP_VELOCITY_RUN: f32 = -608.0; // higher jump when sprinting (like original SMB)
pub(crate) const MAX_WALK_SPEED: f32 = 204.8; // 6.4 blocks/s
pub(crate) const MAX_RUN_SPEED: f32 = 358.4; // 11.2 blocks/s (sprint with fire/shift)
pub(crate) const WALK_ACCEL: f32 = 256.0; // 8 blocks/s^2
pub(crate) const RUN_ACCEL: f32 = 512.0; // 16 blocks/s^2 (sprint, fast acceleration)
pub(crate) const FRICTION: f32 = 448.0; // 14 blocks/s^2
pub(crate) const MAX_Y_SPEED: f32 = 3200.0; // terminal velocity
pub(crate) const STOMP_BOUNCE: f32 = -300.0; // bounce velocity after stomp

// Portal
pub(crate) const PORTAL_GUN_DELAY: f32 = 0.2;
pub(crate) const PROJECTILE_SPEED: f32 = 800.0;
pub(crate) const PORTAL_TELEPORT_COOLDOWN: f32 = 0.15;

// Portal animation (matches original mari0: 6 frames at 0.08s per frame)
pub(crate) const PORTAL_ANIM_FRAMES: u32 = 6;
pub(crate) const PORTAL_ANIM_DELAY: f32 = 0.08;

// Enemy
pub(crate) const ENEMY_SPEED: f32 = 64.0; // 2 blocks/s

// ── Piranha plant (variables.lua:280-285) ──
// The most widely used enemy in SMB: 22 of the 32 main levels place one.
/// Seconds spent extended before retracting.
pub(crate) const PLANT_OUT_TIME: f32 = 2.0;
/// Seconds spent retracted before rising again.
pub(crate) const PLANT_IN_TIME: f32 = 1.8;
/// Frame flip interval for the snapping mouth.
pub(crate) const PLANT_ANIM_DELAY: f32 = 0.15;
/// Travel distance, 23/16 blocks.
pub(crate) const PLANT_MOVE_DIST: f32 = 23.0 / 16.0 * TILE_SIZE;
/// Rise/fall speed, 2.3 blocks/s.
pub(crate) const PLANT_MOVE_SPEED: f32 = 2.3 * TILE_SIZE;
/// A retracted plant will not emerge while the player is within ±3 blocks
/// horizontally — standing on the pipe is what makes it safe to wait.
pub(crate) const PLANT_PLAYER_NEAR: f32 = 3.0 * TILE_SIZE;

// ── Flying koopa (variables.lua:105, 140-141) ──
/// Vertical travel of a hovering koopa, 7.5 blocks.
pub(crate) const KOOPA_FLYING_DISTANCE: f32 = 7.5 * TILE_SIZE;
/// Seconds for one full up-down cycle.
pub(crate) const KOOPA_FLYING_TIME: f32 = 7.0;
/// Reduced gravity used by a flying koopa once it loses its wings.
pub(crate) const KOOPA_FLYING_GRAVITY: f32 = 30.0 * TILE_SIZE;

// ── Firebar (variables.lua:276-278) ──
/// Degrees advanced per tick.
pub(crate) const FIREBAR_ANGLE_STEP: f32 = 11.25;
/// Seconds per tick — 3.4s for a full revolution.
pub(crate) const FIREBAR_DELAY: f32 = 3.4 / (360.0 / FIREBAR_ANGLE_STEP);
/// Spacing between fireballs along the bar, in blocks.
pub(crate) const FIREBAR_SEGMENT_SPACING: f32 = 0.5 * TILE_SIZE;

// ── Up-fire, the lava geyser (variables.lua:263-265) ──
pub(crate) const UPFIRE_FORCE: f32 = 19.0 * TILE_SIZE;
pub(crate) const UPFIRE_GRAVITY: f32 = 20.0 * TILE_SIZE;

// ── Lakitu and his spinies (variables.lua:143-149) ──
/// Seconds between thrown spiny eggs.
pub(crate) const LAKITO_THROW_TIME: f32 = 4.0;
/// How long before the throw he ducks into his cloud (the wind-up frame).
pub(crate) const LAKITO_HIDE_TIME: f32 = 0.5;
/// Seconds a shot lakitu stays gone before returning at the screen edge.
pub(crate) const LAKITO_RESPAWN: f32 = 16.0;
/// How far past the player he may drift before turning around, in blocks.
pub(crate) const LAKITO_SPACE: f32 = 4.0;
/// He aims at where the player *will be*, this many seconds ahead. Chasing the
/// current position would let you outrun him by simply holding a direction.
pub(crate) const LAKITO_DISTANCE_TIME: f32 = 1.5;
/// Speed of the one-way drift he settles into past `lakitoend`, in blocks/s.
pub(crate) const LAKITO_PASSIVE_SPEED: f32 = 3.0 * TILE_SIZE;
/// He stops throwing while this many spinies are already out (`lakito.lua:70`).
pub(crate) const LAKITO_MAX_SPINIES: usize = 3;
/// Upward toss given to a spiny egg, 10 blocks/s (`goomba.lua:57`).
pub(crate) const SPIKEY_TOSS_SPEED: f32 = 10.0 * TILE_SIZE;
/// A falling egg is lighter than everything else in the game: 30 blocks/s² rather
/// than the usual 80 (`goomba.lua:56`), which is what makes the lob readable.
pub(crate) const SPIKEY_FALL_GRAVITY: f32 = 30.0 * TILE_SIZE;
/// How far an egg must fall past its release point before it stops being able to
/// hit the lakitu who threw it (`goomba.lua:132`), in blocks.
pub(crate) const SPIKEY_HITS_LAKITO_WITHIN: f32 = 2.0 * TILE_SIZE;
/// Frame flip interval shared by goombas and spinies (`goombaanimationspeed`).
pub(crate) const GOOMBA_ANIM_SPEED: f32 = 0.2;
/// Points for downing lakitu (`firepoints["lakito"]`, `variables.lua:36`).
pub(crate) const LAKITO_SCORE: u32 = 200;

// ── Enemies killed by fire, a star or a shell (variables.lua:162-164) ──
/// Constant horizontal speed of a shot enemy, 4 blocks/s.
pub(crate) const SHOT_SPEED_X: f32 = 4.0 * TILE_SIZE;
/// Initial upward kick, 8 blocks/s.
pub(crate) const SHOT_JUMP_FORCE: f32 = 8.0 * TILE_SIZE;
/// Shot enemies fall at 60 blocks/s², not the world's 80.
pub(crate) const SHOT_GRAVITY: f32 = 60.0 * TILE_SIZE;
/// How long a shot enemy is kept around — long enough to fall clear of the screen.
pub(crate) const SHOT_DEATH_TIME: f32 = 3.0;

// ── Cheep-cheep (variables.lua:120-124) ──
pub(crate) const CHEEP_RED_SPEED: f32 = 1.8 * TILE_SIZE;
pub(crate) const CHEEP_WHITE_SPEED: f32 = 1.0 * TILE_SIZE;
/// Vertical bob speed.
pub(crate) const CHEEP_Y_SPEED: f32 = 0.3 * TILE_SIZE;
/// Bob amplitude, 1 block.
pub(crate) const CHEEP_HEIGHT: f32 = 1.0 * TILE_SIZE;
pub(crate) const SHELL_SPEED: f32 = 384.0; // 12 blocks/s (mari0)
pub(crate) const ENEMY_DEATH_TIME: f32 = 0.5;

// Block interaction
pub(crate) const BLOCK_BOUNCE_TIME: f32 = 0.2;
pub(crate) const BLOCK_BOUNCE_HEIGHT: f32 = 0.4 * TILE_SIZE; // 12.8px
pub(crate) const COIN_POPUP_TIME: f32 = 0.4;
pub(crate) const COIN_POPUP_SPEED: f32 = -320.0; // initial upward velocity
pub(crate) const SCORE_POPUP_TIME: f32 = 0.8;
pub(crate) const SCORE_POPUP_HEIGHT: f32 = 2.5 * TILE_SIZE; // 80px
pub(crate) const MULTI_COIN_TIMEOUT: f32 = 4.0;
pub(crate) const BRICK_BREAK_SCORE: u32 = 50;
pub(crate) const DEBRIS_GRAVITY: f32 = 1920.0; // 60*32

// Items (mushroom, star, 1-up)
pub(crate) const ITEM_POP_TIME: f32 = 0.7; // time to emerge from block
pub(crate) const ITEM_SPEED: f32 = 115.2; // 3.6 blocks/s horizontal
pub(crate) const ITEM_SCORE: u32 = 1000;
pub(crate) const STAR_JUMP_FORCE: f32 = -416.0; // 13 blocks/s upward
pub(crate) const STAR_ANIM_DELAY: f32 = 0.04;
pub(crate) const STAR_DURATION: f32 = 12.0; // seconds of invincibility

// Fireball (fire flower power-up)
pub(crate) const FIREBALL_SPEED: f32 = 480.0; // 15 blocks/s horizontal
pub(crate) const FIREBALL_BOUNCE: f32 = -320.0; // 10 blocks/s upward bounce
pub(crate) const FIREBALL_SIZE: f32 = 16.0; // 8px * 2 scale
pub(crate) const FIREBALL_EXPLODE_TIME: f32 = 0.12;
pub(crate) const FIREBALL_ANIM_DELAY: f32 = 0.04;
pub(crate) const MAX_FIREBALLS: usize = 2;

// Scoring
pub(crate) const COMBO_SCORES: [u32; 10] = [100, 200, 400, 500, 800, 1000, 2000, 4000, 5000, 8000];
pub(crate) const COIN_SCORE: u32 = 200;

// Player sizes (in pixels) — match tile size like original Mario
pub(crate) const PLAYER_SMALL_W: f32 = 32.0;
pub(crate) const PLAYER_SMALL_H: f32 = 32.0;
pub(crate) const PLAYER_BIG_W: f32 = 32.0;
pub(crate) const PLAYER_BIG_H: f32 = 64.0;

// Sprite render sizes (original cell × 2 scale, separate from collision box)
pub(crate) const MARIO_SPRITE_SCALE: f32 = 2.0;
pub(crate) const MARIO_SMALL_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_SMALL_SPRITE_H: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_BIG_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_BIG_SPRITE_H: f32 = 36.0 * MARIO_SPRITE_SCALE; // 72

// ── SMB Tileset IDs (smbtiles.png: 374×102, 22×6 grid, 17×17 cells) ──
// Tile 1 = empty sky. All other IDs map directly to smbtiles.png cells.
//
// Only the handful the game loop names live here; the ids that exist purely so
// `game.setTile` can accept a friendly name are in `vdp.rs`, which is where
// they're used and where they compile away with the feature.
pub(crate) const SMB_EMPTY: u32 = 1;
pub(crate) const SMB_BRICK: u32 = 7;
pub(crate) const SMB_QUESTION: u32 = 8;
pub(crate) const SMB_HIDDEN_BLOCK: u32 = 115;
