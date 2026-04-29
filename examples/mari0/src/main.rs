// Mari0 — Portal meets Mario
// A tribute to the original Mari0 by Maurice (Stabyourself.net)
// Original game: https://stabyourself.net/mari0/
// Built with vibe2d engine.

use std::collections::HashMap;
use vibe2d::prelude::*;

// ── Physics Constants (mari0-inspired, 1 block = 32px) ─────────────
const TILE_SIZE: f32 = 32.0;
const GRAVITY: f32 = 2560.0; // 80 blocks/s^2
const GRAVITY_JUMPING: f32 = 960.0; // reduced while holding jump
const JUMP_VELOCITY: f32 = -512.0; // initial upward (walking)
const JUMP_VELOCITY_RUN: f32 = -608.0; // higher jump when sprinting (like original SMB)
const MAX_WALK_SPEED: f32 = 204.8; // 6.4 blocks/s
const MAX_RUN_SPEED: f32 = 358.4; // 11.2 blocks/s (sprint with fire/shift)
const WALK_ACCEL: f32 = 256.0; // 8 blocks/s^2
const RUN_ACCEL: f32 = 512.0; // 16 blocks/s^2 (sprint, fast acceleration)
const FRICTION: f32 = 448.0; // 14 blocks/s^2
const MAX_Y_SPEED: f32 = 3200.0; // terminal velocity
const STOMP_BOUNCE: f32 = -300.0; // bounce velocity after stomp

// Portal
const PORTAL_GUN_DELAY: f32 = 0.2;
const PROJECTILE_SPEED: f32 = 800.0;
const PORTAL_TELEPORT_COOLDOWN: f32 = 0.15;

// Portal animation (matches original mari0: 6 frames at 0.08s per frame)
const PORTAL_ANIM_FRAMES: u32 = 6;
const PORTAL_ANIM_DELAY: f32 = 0.08;

// Enemy
const ENEMY_SPEED: f32 = 64.0; // 2 blocks/s
const SHELL_SPEED: f32 = 384.0; // 12 blocks/s (mari0)
const ENEMY_DEATH_TIME: f32 = 0.5;

// Block interaction
const BLOCK_BOUNCE_TIME: f32 = 0.2;
const BLOCK_BOUNCE_HEIGHT: f32 = 0.4 * TILE_SIZE; // 12.8px
const COIN_POPUP_TIME: f32 = 0.4;
const COIN_POPUP_SPEED: f32 = -320.0; // initial upward velocity
const SCORE_POPUP_TIME: f32 = 0.8;
const SCORE_POPUP_HEIGHT: f32 = 2.5 * TILE_SIZE; // 80px
const MULTI_COIN_TIMEOUT: f32 = 4.0;
const BRICK_BREAK_SCORE: u32 = 50;
const DEBRIS_GRAVITY: f32 = 1920.0; // 60*32

// Items (mushroom, star, 1-up)
const ITEM_POP_TIME: f32 = 0.7; // time to emerge from block
const ITEM_SPEED: f32 = 115.2; // 3.6 blocks/s horizontal
const ITEM_SCORE: u32 = 1000;
const STAR_JUMP_FORCE: f32 = -416.0; // 13 blocks/s upward
const STAR_ANIM_DELAY: f32 = 0.04;
const STAR_DURATION: f32 = 12.0; // seconds of invincibility

// Fireball (fire flower power-up)
const FIREBALL_SPEED: f32 = 480.0; // 15 blocks/s horizontal
const FIREBALL_BOUNCE: f32 = -320.0; // 10 blocks/s upward bounce
const FIREBALL_SIZE: f32 = 16.0; // 8px * 2 scale
const FIREBALL_EXPLODE_TIME: f32 = 0.12;
const FIREBALL_ANIM_DELAY: f32 = 0.04;
const MAX_FIREBALLS: usize = 2;

// Scoring
const COMBO_SCORES: [u32; 10] = [100, 200, 400, 500, 800, 1000, 2000, 4000, 5000, 8000];
const COIN_SCORE: u32 = 200;

// Player sizes (in pixels) — match tile size like original Mario
const PLAYER_SMALL_W: f32 = 32.0;
const PLAYER_SMALL_H: f32 = 32.0;
const PLAYER_BIG_W: f32 = 32.0;
const PLAYER_BIG_H: f32 = 64.0;

// Sprite render sizes (original cell × 2 scale, separate from collision box)
const MARIO_SPRITE_SCALE: f32 = 2.0;
const MARIO_SMALL_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
const MARIO_SMALL_SPRITE_H: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
const MARIO_BIG_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
const MARIO_BIG_SPRITE_H: f32 = 36.0 * MARIO_SPRITE_SCALE; // 72

// ── SMB Tileset IDs (smbtiles.png: 374×102, 22×6 grid, 17×17 cells) ──
// Tile 1 = empty sky. All other IDs map directly to smbtiles.png cells.
const SMB_EMPTY: u32 = 1;
const SMB_GROUND: u32 = 2;
const SMB_QUESTION_USED: u32 = 113;
const SMB_BRICK: u32 = 7;
const SMB_QUESTION: u32 = 8;
const SMB_PIPE_TL: u32 = 16;
const SMB_PIPE_TR: u32 = 17;
const SMB_PIPE_BL: u32 = 38;
const SMB_PIPE_BR: u32 = 39;
const SMB_STAIRCASE: u32 = 78;
const SMB_HIDDEN_BLOCK: u32 = 115;

// ── sRGB → Linear conversion (for tint colors with sRGB textures) ───
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// ── UV Helper Functions ──────────────────────────────────────────────

/// Get UV rect for a tile in smbtiles.png (374×102, 22-col grid, 17×17 cells)
fn smb_tile_uv(tile_id: u32) -> [f32; 4] {
    let col = ((tile_id - 1) % 22) as f32;
    let row = ((tile_id - 1) / 22) as f32;
    [
        col * 17.0 / 374.0,
        row * 17.0 / 102.0,
        16.0 / 374.0,
        16.0 / 102.0,
    ]
}

/// Get UV rect for a mario animation frame (512×128, 20×20 cells)
fn mario_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 20) as f32 / 512.0,
        (row * 20) as f32 / 128.0,
        20.0 / 512.0,
        20.0 / 128.0,
    ]
}

/// Get UV rect for a big mario animation frame (512×256, 20×36 cells)
fn mario_big_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 20) as f32 / 512.0,
        (row * 36) as f32 / 256.0,
        20.0 / 512.0,
        36.0 / 256.0,
    ]
}

/// Get UV rect for goomba frame (32×64, 16×16 cells)
fn goomba_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 16) as f32 / 32.0,
        (row * 16) as f32 / 64.0,
        16.0 / 32.0,
        16.0 / 64.0,
    ]
}

/// Get UV rect for koopa frame (128×128, 16×24 cells)
fn koopa_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 16) as f32 / 128.0,
        (row * 24) as f32 / 128.0,
        16.0 / 128.0,
        24.0 / 128.0,
    ]
}

/// Get UV rect for coin animation frame (16×32, 2 vertical frames)
fn coin_frame_uv(frame: u32) -> [f32; 4] {
    [0.0, (frame * 16) as f32 / 32.0, 1.0, 16.0 / 32.0]
}

/// Get UV rect for entity in entities.png (170×170, 17px cells, 16px sprites)
fn entity_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 17) as f32 / 170.0,
        (row * 17) as f32 / 170.0,
        16.0 / 170.0,
        16.0 / 170.0,
    ]
}

/// Get UV rect for star frame in star.png (64×16, 4 frames)
fn star_frame_uv(frame: u32) -> [f32; 4] {
    [(frame * 16) as f32 / 64.0, 0.0, 16.0 / 64.0, 1.0]
}

/// Get UV rect for flower frame in flower.png (64×16, 4 frames)
fn flower_frame_uv(frame: u32) -> [f32; 4] {
    [(frame * 16) as f32 / 64.0, 0.0, 16.0 / 64.0, 1.0]
}

/// Get UV rect for fireball frame in fireball.png (80×16, frames 0-3 are 8×8)
fn fireball_uv(frame: u32) -> [f32; 4] {
    [(frame * 8) as f32 / 80.0, 0.0, 8.0 / 80.0, 8.0 / 16.0]
}

/// Get UV rect for fireball explosion in fireball.png (80×16, frames at offset 32, 16×16)
fn fireball_explode_uv(frame: u32) -> [f32; 4] {
    [(32 + frame * 16) as f32 / 80.0, 0.0, 16.0 / 80.0, 1.0]
}

// ── Enums & Structs ────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum GameState {
    Menu,
    Playing,
    Dead,
    LevelComplete,
}

#[derive(PartialEq, Clone, Copy)]
enum PlayerAnim {
    Idle,
    Run,
    Jump,
    Fall,
}

#[derive(PartialEq, Clone, Copy)]
enum Orientation {
    Up,
    Down,
    Left,
    Right,
}

#[derive(PartialEq, Clone, Copy)]
enum EnemyType {
    Goomba,
    Koopa,
}

#[derive(PartialEq, Clone, Copy)]
enum EnemyState {
    Walking,
    Dead,
    Shell,
    ShellMoving,
}

struct Player {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    width: f32,
    height: f32,
    on_ground: bool,
    facing_right: bool,
    is_big: bool,
    is_fire: bool,
    is_jumping: bool,
    anim_state: PlayerAnim,
    run_frame: f32,
    invincible_timer: f32,
    portal_cooldown: f32,
    teleport_cooldown: f32,
}

impl Player {
    fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            width: PLAYER_SMALL_W,
            height: PLAYER_SMALL_H,
            on_ground: false,
            facing_right: true,
            is_big: false,
            is_fire: false,
            is_jumping: false,
            anim_state: PlayerAnim::Idle,
            run_frame: 0.0,
            invincible_timer: 0.0,
            portal_cooldown: 0.0,
            teleport_cooldown: 0.0,
        }
    }

    fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }
    fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }
    fn bottom(&self) -> f32 {
        self.y + self.height
    }

    fn set_size(&mut self, big: bool) {
        let was_big = self.is_big;
        self.is_big = big;
        if big {
            self.width = PLAYER_BIG_W;
            self.height = PLAYER_BIG_H;
        } else {
            self.width = PLAYER_SMALL_W;
            self.height = PLAYER_SMALL_H;
        }
        if was_big && !big {
            self.y += PLAYER_BIG_H - PLAYER_SMALL_H;
        } else if !was_big && big {
            self.y -= PLAYER_BIG_H - PLAYER_SMALL_H;
        }
    }
}

#[derive(Clone)]
struct Portal {
    x: f32,
    y: f32,
    orientation: Orientation,
    active: bool,
    open_scale: f32, // 0→1 opening animation (original: dt*15)
}

struct PortalProjectile {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    portal_index: usize,
    active: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockContent {
    Coin,
    MultiCoin(u32),
    Mushroom,
    Star,
    OneUp,
    // Note: there is intentionally no `FireFlower` variant. In SMB the
    // FireFlower is *produced* dynamically when a big Mario hits a
    // `Mushroom` block (see the `BlockContent::Mushroom` arm in the block
    // hit logic) — level data only ever stores `Mushroom`. If a future
    // level needs to force a FireFlower regardless of Mario's state, add
    // the variant back together with a tilemap producer.
}

#[derive(Clone, Copy, PartialEq)]
enum ItemType {
    Mushroom,
    Star,
    OneUp,
    FireFlower,
}

struct Fireball {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    anim_timer: f32,
    exploding: bool,
    explode_timer: f32,
}

struct Item {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    item_type: ItemType,
    emerging: bool, // still popping out of block
    emerge_y: f32,  // target y after emerging
    emerge_timer: f32,
    anim_timer: f32,
}

#[derive(Clone)]
struct Enemy {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    enemy_type: EnemyType,
    state: EnemyState,
    facing_right: bool,
    on_ground: bool,
    activated: bool,
    anim_timer: f32,
    death_timer: f32,
    flipped_death: bool, // true = star/fireball kill (flip + fly off)
}

#[derive(Clone)]
struct CoinInstance {
    x: f32,
    y: f32,
    collected: bool,
}

struct BlockBounce {
    col: i32,
    row: i32,
    timer: f32,
}

struct CoinPopup {
    x: f32,
    y: f32,
    vy: f32,
    timer: f32,
}

struct ScorePopup {
    x: f32,
    y: f32,
    value: u32,
    timer: f32,
}

struct BrickDebris {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    timer: f32,
}

struct Level {
    tiles: Vec<Vec<u32>>,
    width: usize,
    height: usize,
    coins: Vec<CoinInstance>,
    enemy_spawns: Vec<(EnemyType, f32, f32, bool)>, // (type, x, y, facing_right)
    block_contents: HashMap<(usize, usize), BlockContent>,
    multi_coin_timers: HashMap<(usize, usize), f32>,
    player_start: (f32, f32),
    flag_x: f32,
    time_limit: f32,
}

struct Camera {
    x: f32,
}

// ── Level data (embedded from mari0 1-1.txt) ────────────────────────

const LEVEL_DATA: &str = include_str!("../assets/mari0/1-1.txt");

// mari0 1-1 grid: 224 columns × 15 rows
const LEVEL_COLS: usize = 224;

// ── Level loading ───────────────────────────────────────────────────

fn load_level(content: &str) -> Level {
    // Split data from metadata (separated by semicolons)
    let data_part = content.split(';').next().unwrap_or("");

    // Parse metadata
    let mut time_limit = 400.0;
    for meta in content.split(';').skip(1) {
        if let Some(val) = meta.trim().strip_prefix("timelimit=") {
            if let Ok(t) = val.parse::<f32>() {
                time_limit = t;
            }
        }
    }

    // Parse comma-separated tile values
    let values: Vec<&str> = data_part.split(',').collect();
    let cols = LEVEL_COLS;
    let rows = values.len() / cols;

    let mut tiles = Vec::with_capacity(rows);
    let mut enemy_spawns = Vec::new();
    let mut block_contents = HashMap::new();
    let mut flag_x = 0.0;

    for row in 0..rows {
        let mut tile_row = vec![SMB_EMPTY; cols];
        for col in 0..cols {
            let idx = row * cols + col;
            if idx >= values.len() {
                break;
            }
            let val = values[idx].trim();

            // Parse entity markers: "tile_id-entity_type[-subtype]"
            let parts: Vec<&str> = val.split('-').collect();
            let tile_id: u32 = parts[0].parse().unwrap_or(SMB_EMPTY);
            tile_row[col] = tile_id;

            if parts.len() >= 2 {
                let entity_type: u32 = parts[1].parse().unwrap_or(0);
                let px = col as f32 * TILE_SIZE;
                let py = row as f32 * TILE_SIZE;
                let subtype: u32 = if parts.len() >= 3 {
                    parts[2].parse().unwrap_or(0)
                } else {
                    0
                };

                match entity_type {
                    // Block contents
                    2 => {
                        block_contents.insert((row, col), BlockContent::Mushroom);
                    }
                    3 => {
                        block_contents.insert((row, col), BlockContent::OneUp);
                    }
                    4 => {
                        block_contents.insert((row, col), BlockContent::Star);
                    }
                    5 => {
                        let count = if subtype > 0 { subtype } else { 5 };
                        block_contents.insert((row, col), BlockContent::MultiCoin(count));
                    }
                    // Enemies
                    6 => enemy_spawns.push((EnemyType::Goomba, px, py, false)),
                    7 => enemy_spawns.push((EnemyType::Koopa, px, py, false)),
                    9 => enemy_spawns.push((EnemyType::Goomba, px, py, true)), // right-aligned goomba
                    // Level markers
                    11 => {
                        flag_x = px;
                    }
                    _ => {}
                }
            }

            // Question blocks without explicit content default to Coin
            if tile_id == SMB_QUESTION && !block_contents.contains_key(&(row, col)) {
                block_contents.insert((row, col), BlockContent::Coin);
            }
        }
        tiles.push(tile_row);
    }

    // Player starts at col 3, standing on ground (row 13 is ground, player at row 12)
    let player_start = (3.0 * TILE_SIZE, 13.0 * TILE_SIZE - PLAYER_SMALL_H);

    Level {
        tiles,
        width: cols,
        height: rows,
        coins: Vec::new(),
        enemy_spawns,
        block_contents,
        multi_coin_timers: HashMap::new(),
        player_start,
        flag_x,
        time_limit,
    }
}

// ── Collision helpers ───────────────────────────────────────────────

fn is_solid(tile_id: u32) -> bool {
    matches!(
        tile_id,
        SMB_GROUND
            | SMB_QUESTION_USED
            | SMB_BRICK
            | SMB_QUESTION
            | SMB_PIPE_TL
            | SMB_PIPE_TR
            | SMB_PIPE_BL
            | SMB_PIPE_BR
            | SMB_STAIRCASE
            | SMB_HIDDEN_BLOCK
    )
}

fn is_portal_surface(tile_id: u32) -> bool {
    matches!(
        tile_id,
        SMB_GROUND | SMB_QUESTION_USED | SMB_BRICK | SMB_PIPE_BL | SMB_PIPE_BR | SMB_STAIRCASE
    )
}

fn get_tile(level: &Level, col: i32, row: i32) -> u32 {
    if row < 0 || col < 0 || row >= level.height as i32 || col >= level.width as i32 {
        return SMB_EMPTY;
    }
    level.tiles[row as usize][col as usize]
}

fn tile_rect(col: i32, row: i32) -> (f32, f32, f32, f32) {
    (
        col as f32 * TILE_SIZE,
        row as f32 * TILE_SIZE,
        TILE_SIZE,
        TILE_SIZE,
    )
}

fn aabb_overlap(ax: f32, ay: f32, aw: f32, ah: f32, bx: f32, by: f32, bw: f32, bh: f32) -> bool {
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
}

fn move_and_collide_x(
    player_x: &mut f32,
    player_y: f32,
    pw: f32,
    ph: f32,
    vx: f32,
    level: &Level,
    dt: f32,
) -> f32 {
    let dx = vx * dt;
    *player_x += dx;

    let left_col = (*player_x / TILE_SIZE).floor() as i32;
    let right_col = ((*player_x + pw - 0.01) / TILE_SIZE).floor() as i32;
    let top_row = (player_y / TILE_SIZE).floor() as i32;
    let bottom_row = ((player_y + ph - 0.01) / TILE_SIZE).floor() as i32;

    for row in top_row..=bottom_row {
        for col in left_col..=right_col {
            if is_solid(get_tile(level, col, row)) {
                let (tx, _ty, tw, _th) = tile_rect(col, row);
                if aabb_overlap(*player_x, player_y, pw, ph, tx, _ty, tw, _th) {
                    if dx > 0.0 {
                        *player_x = tx - pw;
                    } else if dx < 0.0 {
                        *player_x = tx + tw;
                    }
                    return 0.0;
                }
            }
        }
    }
    vx
}

fn move_and_collide_y(
    player_x: f32,
    player_y: &mut f32,
    pw: f32,
    ph: f32,
    vy: f32,
    level: &Level,
    dt: f32,
) -> (f32, bool) {
    let dy = vy * dt;
    *player_y += dy;

    let left_col = (player_x / TILE_SIZE).floor() as i32;
    let right_col = ((player_x + pw - 0.01) / TILE_SIZE).floor() as i32;
    let top_row = (*player_y / TILE_SIZE).floor() as i32;
    let bottom_row = ((*player_y + ph - 0.01) / TILE_SIZE).floor() as i32;

    let mut on_ground = false;

    for row in top_row..=bottom_row {
        for col in left_col..=right_col {
            if is_solid(get_tile(level, col, row)) {
                let (tx, ty, tw, th) = tile_rect(col, row);
                if aabb_overlap(player_x, *player_y, pw, ph, tx, ty, tw, th) {
                    if dy > 0.0 {
                        *player_y = ty - ph;
                        on_ground = true;
                    } else if dy < 0.0 {
                        *player_y = ty + th;
                    }
                    return (0.0, on_ground);
                }
            }
        }
    }
    (vy, on_ground)
}

// ── Portal velocity transform ───────────────────────────────────────

fn transform_velocity(
    vx: f32,
    vy: f32,
    entry_orient: Orientation,
    exit_orient: Orientation,
) -> (f32, f32) {
    // Speed entering along normal of entry portal
    let speed = match entry_orient {
        Orientation::Up => -vy,
        Orientation::Down => vy,
        Orientation::Left => -vx,
        Orientation::Right => vx,
    };
    let speed = speed.abs().max(200.0); // minimum exit speed

    // Exit along normal of exit portal
    match exit_orient {
        Orientation::Up => (0.0, -speed),
        Orientation::Down => (0.0, speed),
        Orientation::Left => (-speed, 0.0),
        Orientation::Right => (speed, 0.0),
    }
}

// ── Portal aim-line ray-cast (matches mari0 traceline) ───────────────
/// Trace a line from (sx, sy) in world-pixel coords along `angle` (radians).
/// Always returns (end_x, end_y) even without a wall hit (like original mari0),
/// so dots can always be drawn along the ray.
/// Returns (end_x, end_y, Option<(orientation, can_portal)>).
fn trace_aim_line(
    level: &Level,
    sx: f32,
    sy: f32,
    angle: f32,
    cam_x: f32,
    view_w: f32,
) -> (f32, f32, Option<(Orientation, bool)>) {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let step = TILE_SIZE * 0.5;
    let max_dist = 40.0 * TILE_SIZE;

    let mut dist = step;
    while dist < max_dist {
        let px = sx + cos_a * dist;
        let py = sy + sin_a * dist;
        let col = (px / TILE_SIZE).floor() as i32;
        let row = (py / TILE_SIZE).floor() as i32;

        // Out of map → return endpoint, no hit (like original)
        if col < 0 || row < 0 || col >= level.width as i32 || row >= level.height as i32 {
            return (px, py, None);
        }

        // Out of visible area (original: x > xscroll+width or x < xscroll)
        if px < cam_x - TILE_SIZE || px > cam_x + view_w + TILE_SIZE {
            return (px, py, None);
        }

        let tile = get_tile(level, col, row);
        if is_solid(tile) {
            let prev_px = sx + cos_a * (dist - step);
            let prev_py = sy + sin_a * (dist - step);
            let prev_col = (prev_px / TILE_SIZE).floor() as i32;
            let prev_row = (prev_py / TILE_SIZE).floor() as i32;

            let orient = if prev_col < col {
                Orientation::Left
            } else if prev_col > col {
                Orientation::Right
            } else if prev_row < row {
                Orientation::Up
            } else {
                Orientation::Down
            };

            let (hx, hy) = match orient {
                Orientation::Left => (col as f32 * TILE_SIZE, py),
                Orientation::Right => ((col + 1) as f32 * TILE_SIZE, py),
                Orientation::Up => (px, row as f32 * TILE_SIZE),
                Orientation::Down => (px, (row + 1) as f32 * TILE_SIZE),
            };

            let can_portal = is_portal_surface(tile);
            return (hx, hy, Some((orient, can_portal)));
        }
        dist += step;
    }
    // Max distance, no hit
    (sx + cos_a * max_dist, sy + sin_a * max_dist, None)
}

// ── Main game ───────────────────────────────────────────────────────

struct Mari0Game {
    state: GameState,
    player: Player,
    portals: [Option<Portal>; 2],
    projectiles: Vec<PortalProjectile>,
    crosshair_angle: f32,
    aim_dot_timer: f32,
    portal_anim_timer: f32, // global portal animation timer
    portal_anim_frame: u32, // current frame 0..5 (maps to original frames 1..6)
    enemies: Vec<Enemy>,
    level: Level,
    camera: Camera,
    score: u32,
    coins: u32,
    lives: u32,
    combo_index: usize,
    combo_active: bool,
    time_remaining: f32,

    // Block/coin/score animations
    block_bounces: Vec<BlockBounce>,
    coin_popups: Vec<CoinPopup>,
    score_popups: Vec<ScorePopup>,
    brick_debris: Vec<BrickDebris>,
    items: Vec<Item>,
    fireballs: Vec<Fireball>,
    star_timer: f32, // player star invincibility timer

    // Sprite sheet textures
    tex_tiles: TextureId,
    tex_mario_layers: [TextureId; 4],     // layers 0-3 (small)
    tex_mario_big_layers: [TextureId; 4], // layers 0-3 (big)
    tex_goomba: TextureId,
    tex_koopa: TextureId,
    tex_coin_anim: TextureId,
    tex_entities: TextureId,
    tex_star: TextureId,
    tex_flower: TextureId,
    tex_fireball: TextureId,
    tex_portal: TextureId,
    tex_portal_v: TextureId, // pre-rotated portal for vertical (left/right) orientation
    tex_portal_crosshair: TextureId,
    tex_portal_projectile: TextureId,
    tex_portal_dot: TextureId,
    tex_flag: TextureId,

    vw: f32,
}

fn spawn_enemies_from_level(level: &Level) -> Vec<Enemy> {
    level
        .enemy_spawns
        .iter()
        .map(|(et, x, y, face_right)| {
            let h = match et {
                EnemyType::Koopa => 48.0, // koopa is taller: 24px * 2 scale
                EnemyType::Goomba => PLAYER_SMALL_H,
            };
            Enemy {
                x: *x,
                y: *y - h,
                vx: if *face_right {
                    ENEMY_SPEED
                } else {
                    -ENEMY_SPEED
                },
                vy: 0.0,
                enemy_type: *et,
                state: EnemyState::Walking,
                facing_right: *face_right,
                on_ground: false,
                activated: false,
                anim_timer: 0.0,
                death_timer: 0.0,
                flipped_death: false,
            }
        })
        .collect()
}

impl Mari0Game {
    fn tex(ctx: &Context, name: &str) -> TextureId {
        ctx.assets
            .texture_id(name)
            .unwrap_or_else(|| panic!("Missing texture: {}", name))
    }

    fn reset_level(&mut self) {
        let level = load_level(LEVEL_DATA);
        self.player = Player::new(level.player_start.0, level.player_start.1);
        self.enemies = spawn_enemies_from_level(&level);
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
        self.camera = Camera { x: 0.0 };
        self.level = level;
    }

    fn update_playing(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        // ── Input ──
        let move_left = input.is_action_pressed("move_left");
        let move_right = input.is_action_pressed("move_right");
        let jump_pressed = input.is_action_pressed("jump");
        let jump_just = input.is_action_just_pressed("jump");
        let fire_blue = input.is_action_just_pressed("portal_blue");
        let fire_orange = input.is_action_just_pressed("portal_orange");
        let fire_ball = input.is_action_just_pressed("fire");
        let sprint = input.is_action_pressed("fire"); // hold shift/F to sprint

        // Mouse aiming (virtual coords → world coords)
        let (mx, my) = input.mouse_position();
        let world_mx = mx + self.camera.x;
        let world_my = my;
        self.crosshair_angle =
            (world_my - self.player.center_y()).atan2(world_mx - self.player.center_x());

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
                    self.player.x,
                    self.player.y,
                    self.player.width,
                    self.player.height,
                    coin.x,
                    coin.y,
                    16.0,
                    16.0,
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

        // ── Timer ──
        self.time_remaining -= dt;
        if self.time_remaining <= 0.0 {
            self.time_remaining = 0.0;
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

        // ── Portal aim dots animation timer ──
        self.aim_dot_timer += dt;
        const AIM_DOTS_CYCLE: f32 = 0.8;
        if self.aim_dot_timer >= AIM_DOTS_CYCLE {
            self.aim_dot_timer -= AIM_DOTS_CYCLE;
        }

        // ── Portal animation (global frame cycle, matches original) ──
        self.portal_anim_timer += dt;
        while self.portal_anim_timer >= PORTAL_ANIM_DELAY {
            self.portal_anim_timer -= PORTAL_ANIM_DELAY;
            self.portal_anim_frame = (self.portal_anim_frame + 1) % PORTAL_ANIM_FRAMES;
        }

        // ── Portal opening animation ──
        for portal_opt in &mut self.portals {
            if let Some(portal) = portal_opt {
                if portal.open_scale < 1.0 {
                    portal.open_scale = (portal.open_scale + dt * 15.0).min(1.0);
                }
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
                self.level.tiles[key.0][key.1] = SMB_QUESTION_USED;
            }
            self.level.block_contents.remove(key);
        }
        for timer in self.level.multi_coin_timers.values_mut() {
            *timer -= dt;
        }
    }

    fn fire_projectile(&mut self, index: usize) {
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

    fn update_projectiles(&mut self, ctx: &Context, dt: f32) {
        for proj in &mut self.projectiles {
            if !proj.active {
                continue;
            }
            proj.x += proj.vx * dt;
            proj.y += proj.vy * dt;

            // Check tile collision
            let col = (proj.x / TILE_SIZE).floor() as i32;
            let row = (proj.y / TILE_SIZE).floor() as i32;
            let tile = get_tile(&self.level, col, row);

            if is_solid(tile) && is_portal_surface(tile) {
                // Determine which face was hit by checking where the projectile came from
                let prev_x = proj.x - proj.vx * dt;
                let prev_y = proj.y - proj.vy * dt;
                let prev_col = (prev_x / TILE_SIZE).floor() as i32;
                let prev_row = (prev_y / TILE_SIZE).floor() as i32;

                let orient = if prev_col < col {
                    Orientation::Left
                } else if prev_col > col {
                    Orientation::Right
                } else if prev_row < row {
                    Orientation::Up
                } else {
                    Orientation::Down
                };

                let (portal_x, portal_y) = match orient {
                    Orientation::Left => (
                        col as f32 * TILE_SIZE,
                        row as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    ),
                    Orientation::Right => (
                        (col + 1) as f32 * TILE_SIZE,
                        row as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    ),
                    Orientation::Up => (
                        col as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        row as f32 * TILE_SIZE,
                    ),
                    Orientation::Down => (
                        col as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        (row + 1) as f32 * TILE_SIZE,
                    ),
                };

                self.portals[proj.portal_index] = Some(Portal {
                    x: portal_x,
                    y: portal_y,
                    orientation: orient,
                    active: true,
                    open_scale: 0.0,
                });
                if proj.portal_index == 0 {
                    ctx.audio.play("portal1open");
                } else {
                    ctx.audio.play("portal2open");
                }
                proj.active = false;
            } else if is_solid(tile) {
                // Hit non-portal surface, destroy
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
    }

    fn check_portal_teleport(&mut self, ctx: &Context) {
        if self.player.teleport_cooldown > 0.0 {
            return;
        }
        let (p0, p1) = match (&self.portals[0], &self.portals[1]) {
            (Some(a), Some(b)) if a.active && b.active => (a.clone(), b.clone()),
            _ => return,
        };

        // Check overlap with either portal
        for (entry, exit) in [(&p0, &p1), (&p1, &p0)] {
            let portal_rect = match entry.orientation {
                Orientation::Left | Orientation::Right => {
                    (entry.x - 4.0, entry.y - 32.0, 8.0, 64.0)
                }
                Orientation::Up | Orientation::Down => (entry.x - 32.0, entry.y - 4.0, 64.0, 8.0),
            };

            if aabb_overlap(
                self.player.x,
                self.player.y,
                self.player.width,
                self.player.height,
                portal_rect.0,
                portal_rect.1,
                portal_rect.2,
                portal_rect.3,
            ) {
                // Check player is moving into the portal
                let entering = match entry.orientation {
                    Orientation::Left => self.player.vx > 0.0,
                    Orientation::Right => self.player.vx < 0.0,
                    Orientation::Up => self.player.vy > 0.0,
                    Orientation::Down => self.player.vy < 0.0,
                };
                if !entering {
                    continue;
                }

                // Teleport
                let (new_vx, new_vy) = transform_velocity(
                    self.player.vx,
                    self.player.vy,
                    entry.orientation,
                    exit.orientation,
                );

                // Position at exit portal
                let offset = 8.0;
                let (new_x, new_y) = match exit.orientation {
                    Orientation::Up => (
                        exit.x - self.player.width / 2.0,
                        exit.y - self.player.height - offset,
                    ),
                    Orientation::Down => (exit.x - self.player.width / 2.0, exit.y + offset),
                    Orientation::Left => (
                        exit.x - self.player.width - offset,
                        exit.y - self.player.height / 2.0,
                    ),
                    Orientation::Right => (exit.x + offset, exit.y - self.player.height / 2.0),
                };

                self.player.x = new_x;
                self.player.y = new_y;
                self.player.vx = new_vx;
                self.player.vy = new_vy;
                self.player.teleport_cooldown = PORTAL_TELEPORT_COOLDOWN;
                self.player.on_ground = false;
                ctx.audio.play("portalenter");
                return;
            }
        }
    }

    fn update_enemies(&mut self, dt: f32, ctx: &mut Context) {
        let cam_x = self.camera.x;
        let view_w = self.vw;

        for enemy in &mut self.enemies {
            // Activate enemies when they come within view + margin
            if !enemy.activated {
                if enemy.x < cam_x + view_w + 48.0 {
                    enemy.activated = true;
                }
                continue;
            }

            let ew = PLAYER_SMALL_W;
            let eh = match enemy.enemy_type {
                EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                _ => PLAYER_SMALL_H,
            };

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
                                if aabb_overlap(enemy.x, enemy.y, ew, eh, tx, _ty, tw, th) {
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
                                if aabb_overlap(enemy.x, enemy.y, ew, eh, tx, ty, tw, th) {
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
                                    enemy.x,
                                    enemy.y,
                                    ew,
                                    PLAYER_SMALL_H,
                                    tx,
                                    ty,
                                    tw,
                                    th,
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
            if enemy.state == EnemyState::Dead || !enemy.activated {
                continue;
            }

            let eh = match enemy.enemy_type {
                EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                _ => PLAYER_SMALL_H,
            };

            if !aabb_overlap(
                self.player.x,
                self.player.y,
                self.player.width,
                self.player.height,
                enemy.x,
                enemy.y,
                PLAYER_SMALL_W,
                eh,
            ) {
                continue;
            }

            // Check if stomping (player feet above enemy top half)
            let player_feet = self.player.bottom();
            let enemy_mid = enemy.y + eh / 2.0;

            if self.player.vy > 0.0 && player_feet < enemy_mid + 8.0 {
                // Stomp!
                match enemy.state {
                    EnemyState::Walking => match enemy.enemy_type {
                        EnemyType::Goomba => {
                            enemy.state = EnemyState::Dead;
                            enemy.death_timer = ENEMY_DEATH_TIME;
                        }
                        EnemyType::Koopa => {
                            enemy.state = EnemyState::Shell;
                            enemy.vx = 0.0;
                        }
                    },
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
            } else if self.star_timer > 0.0 {
                // Star invincibility: kill enemy on contact (flip + fly off)
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
            // Remove enemies far behind camera
            if e.activated && e.x < cam_x - 200.0 {
                return false;
            }
            true
        });
    }

    fn die(&mut self, ctx: &mut Context) {
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

    fn hit_block(&mut self, ctx: &Context, row: usize, col: usize, tile: u32) {
        let key = (row, col);
        let bx = col as f32 * TILE_SIZE;
        let by = row as f32 * TILE_SIZE;

        match tile {
            SMB_QUESTION => {
                let content = self
                    .level
                    .block_contents
                    .get(&key)
                    .copied()
                    .unwrap_or(BlockContent::Coin);
                match content {
                    BlockContent::Coin => {
                        // Turn question block into used block
                        self.level.tiles[row][col] = SMB_QUESTION_USED;
                        self.level.block_contents.remove(&key);
                        self.score += COIN_SCORE;
                        self.coins += 1;
                        if self.coins >= 100 {
                            self.coins -= 100;
                            self.lives += 1;
                        }
                        self.coin_popups.push(CoinPopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            vy: COIN_POPUP_SPEED,
                            timer: 0.0,
                        });
                        self.score_popups.push(ScorePopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            value: COIN_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("coin");
                    }
                    BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                        self.level.tiles[row][col] = SMB_QUESTION_USED;
                        self.level.block_contents.remove(&key);
                        let item_type = match content {
                            BlockContent::Mushroom => {
                                if self.player.is_big {
                                    ItemType::FireFlower
                                } else {
                                    ItemType::Mushroom
                                }
                            }
                            BlockContent::Star => ItemType::Star,
                            _ => ItemType::OneUp,
                        };
                        self.items.push(Item {
                            x: bx,
                            y: by,
                            vx: 0.0,
                            vy: 0.0,
                            item_type,
                            emerging: true,
                            emerge_y: by - TILE_SIZE,
                            emerge_timer: 0.0,
                            anim_timer: 0.0,
                        });
                        ctx.audio.play("mushroomappear");
                    }
                    BlockContent::MultiCoin(remaining) => {
                        // Start timer on first hit
                        if !self.level.multi_coin_timers.contains_key(&key) {
                            self.level.multi_coin_timers.insert(key, MULTI_COIN_TIMEOUT);
                        }
                        if remaining > 1 {
                            self.level
                                .block_contents
                                .insert(key, BlockContent::MultiCoin(remaining - 1));
                        } else {
                            self.level.tiles[row][col] = SMB_QUESTION_USED;
                            self.level.block_contents.remove(&key);
                            self.level.multi_coin_timers.remove(&key);
                        }
                        self.score += COIN_SCORE;
                        self.coins += 1;
                        if self.coins >= 100 {
                            self.coins -= 100;
                            self.lives += 1;
                        }
                        self.coin_popups.push(CoinPopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            vy: COIN_POPUP_SPEED,
                            timer: 0.0,
                        });
                        self.score_popups.push(ScorePopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            value: COIN_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("coin");
                    }
                }
                self.block_bounces.push(BlockBounce {
                    col: col as i32,
                    row: row as i32,
                    timer: 0.0,
                });
                ctx.audio.play("blockhit");
            }
            SMB_BRICK => {
                if let Some(content) = self.level.block_contents.get(&key).copied() {
                    // Brick with content
                    match content {
                        BlockContent::MultiCoin(remaining) => {
                            if !self.level.multi_coin_timers.contains_key(&key) {
                                self.level.multi_coin_timers.insert(key, MULTI_COIN_TIMEOUT);
                            }
                            if remaining > 1 {
                                self.level
                                    .block_contents
                                    .insert(key, BlockContent::MultiCoin(remaining - 1));
                            } else {
                                self.level.tiles[row][col] = SMB_QUESTION_USED;
                                self.level.block_contents.remove(&key);
                                self.level.multi_coin_timers.remove(&key);
                            }
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            if self.coins >= 100 {
                                self.coins -= 100;
                                self.lives += 1;
                            }
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                        BlockContent::Coin => {
                            self.level.tiles[row][col] = SMB_QUESTION_USED;
                            self.level.block_contents.remove(&key);
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            if self.coins >= 100 {
                                self.coins -= 100;
                                self.lives += 1;
                            }
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                        BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                            self.level.tiles[row][col] = SMB_QUESTION_USED;
                            self.level.block_contents.remove(&key);
                            let item_type = match content {
                                BlockContent::Mushroom => {
                                    if self.player.is_big {
                                        ItemType::FireFlower
                                    } else {
                                        ItemType::Mushroom
                                    }
                                }
                                BlockContent::Star => ItemType::Star,
                                _ => ItemType::OneUp,
                            };
                            self.items.push(Item {
                                x: bx,
                                y: by,
                                vx: 0.0,
                                vy: 0.0,
                                item_type,
                                emerging: true,
                                emerge_y: by - TILE_SIZE,
                                emerge_timer: 0.0,
                                anim_timer: 0.0,
                            });
                            ctx.audio.play("mushroomappear");
                        }
                    }
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                } else if self.player.is_big {
                    // Big Mario breaks empty brick
                    self.level.tiles[row][col] = SMB_EMPTY;
                    self.score += BRICK_BREAK_SCORE;
                    self.score_popups.push(ScorePopup {
                        x: bx,
                        y: by - TILE_SIZE,
                        value: BRICK_BREAK_SCORE,
                        timer: 0.0,
                    });
                    // 4 debris particles
                    let cx = bx + TILE_SIZE * 0.5;
                    let cy = by + TILE_SIZE * 0.5;
                    for &(dvx, dvy) in &[
                        (-112.0f32, -736.0f32),
                        (112.0, -736.0),
                        (-112.0, -448.0),
                        (112.0, -448.0),
                    ] {
                        self.brick_debris.push(BrickDebris {
                            x: cx,
                            y: cy,
                            vx: dvx,
                            vy: dvy,
                            timer: 0.0,
                        });
                    }
                    ctx.audio.play("blockbreak");
                } else {
                    // Small Mario just bounces the brick
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                }
            }
            SMB_HIDDEN_BLOCK => {
                if let Some(content) = self.level.block_contents.get(&key).copied() {
                    self.level.tiles[row][col] = SMB_QUESTION_USED;
                    self.level.block_contents.remove(&key);
                    match content {
                        BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                            let item_type = match content {
                                BlockContent::Mushroom => {
                                    if self.player.is_big {
                                        ItemType::FireFlower
                                    } else {
                                        ItemType::Mushroom
                                    }
                                }
                                BlockContent::Star => ItemType::Star,
                                _ => ItemType::OneUp,
                            };
                            self.items.push(Item {
                                x: bx,
                                y: by,
                                vx: 0.0,
                                vy: 0.0,
                                item_type,
                                emerging: true,
                                emerge_y: by - TILE_SIZE,
                                emerge_timer: 0.0,
                                anim_timer: 0.0,
                            });
                            ctx.audio.play("mushroomappear");
                        }
                        _ => {
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                    }
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                }
            }
            _ => {}
        }
    }

    fn update_items(&mut self, ctx: &Context, dt: f32) {
        // Update item physics
        let level = &self.level;
        for item in &mut self.items {
            if item.emerging {
                item.emerge_timer += dt;
                let progress = (item.emerge_timer / ITEM_POP_TIME).min(1.0);
                item.y = item.emerge_y + TILE_SIZE * (1.0 - progress);
                if progress >= 1.0 {
                    item.emerging = false;
                    item.y = item.emerge_y;
                    // Flower stays in place; mushroom/star/1-up move horizontally
                    if item.item_type != ItemType::FireFlower {
                        item.vx = ITEM_SPEED;
                    }
                    if item.item_type == ItemType::Star {
                        item.vy = STAR_JUMP_FORCE;
                    }
                }
                continue;
            }

            item.anim_timer += dt;

            // Gravity
            item.vy += GRAVITY * dt;
            if item.vy > MAX_Y_SPEED {
                item.vy = MAX_Y_SPEED;
            }

            let iw = TILE_SIZE;
            let ih = TILE_SIZE;

            // Horizontal movement + wall collision
            item.x += item.vx * dt;
            let left_col = (item.x / TILE_SIZE).floor() as i32;
            let right_col = ((item.x + iw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (item.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((item.y + ih - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap(item.x, item.y, iw, ih, tx, ty, tw, th) {
                            if item.vx > 0.0 {
                                item.x = tx - iw;
                            } else if item.vx < 0.0 {
                                item.x = tx + tw;
                            }
                            item.vx = -item.vx;
                        }
                    }
                }
            }

            // Vertical movement + ground/ceiling collision
            item.y += item.vy * dt;
            let left_col = (item.x / TILE_SIZE).floor() as i32;
            let right_col = ((item.x + iw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (item.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((item.y + ih - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap(item.x, item.y, iw, ih, tx, ty, tw, th) {
                            if item.vy > 0.0 {
                                item.y = ty - ih;
                                if item.item_type == ItemType::Star {
                                    item.vy = STAR_JUMP_FORCE; // star bounces
                                } else {
                                    item.vy = 0.0;
                                }
                            } else if item.vy < 0.0 {
                                item.y = ty + th;
                                item.vy = 0.0;
                            }
                        }
                    }
                }
            }
        }

        // Player-item collision
        let px = self.player.x;
        let py = self.player.y;
        let pw = self.player.width;
        let ph = self.player.height;

        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].emerging {
                i += 1;
                continue;
            }
            if aabb_overlap(
                px,
                py,
                pw,
                ph,
                self.items[i].x,
                self.items[i].y,
                TILE_SIZE,
                TILE_SIZE,
            ) {
                let item = self.items.remove(i);
                match item.item_type {
                    ItemType::Mushroom => {
                        if !self.player.is_big {
                            self.player.set_size(true);
                        }
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                    ItemType::Star => {
                        self.star_timer = STAR_DURATION;
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                    ItemType::OneUp => {
                        self.lives += 1;
                        ctx.audio.play("oneup");
                    }
                    ItemType::FireFlower => {
                        if !self.player.is_big {
                            self.player.set_size(true);
                        }
                        self.player.is_fire = true;
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                }
            } else {
                i += 1;
            }
        }

        // Remove items that fell off the map
        let map_bottom = self.level.height as f32 * TILE_SIZE + 100.0;
        self.items.retain(|item| item.y < map_bottom);
    }

    fn update_fireballs(&mut self, _ctx: &Context, dt: f32) {
        // Physics update
        let level = &self.level;
        for fb in &mut self.fireballs {
            if fb.exploding {
                fb.explode_timer += dt;
                fb.anim_timer += dt;
                continue;
            }
            fb.anim_timer += dt;

            // Gravity
            fb.vy += GRAVITY * dt;
            if fb.vy > MAX_Y_SPEED {
                fb.vy = MAX_Y_SPEED;
            }

            let fw = FIREBALL_SIZE;
            let fh = FIREBALL_SIZE;

            // Horizontal movement + wall collision → explode
            fb.x += fb.vx * dt;
            let left_col = (fb.x / TILE_SIZE).floor() as i32;
            let right_col = ((fb.x + fw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (fb.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((fb.y + fh - 0.01) / TILE_SIZE).floor() as i32;
            'h_check: for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap(fb.x, fb.y, fw, fh, tx, ty, tw, th) {
                            fb.exploding = true;
                            fb.explode_timer = 0.0;
                            break 'h_check;
                        }
                    }
                }
            }
            if fb.exploding {
                continue;
            }

            // Vertical movement + ground bounce / ceiling
            fb.y += fb.vy * dt;
            let left_col = (fb.x / TILE_SIZE).floor() as i32;
            let right_col = ((fb.x + fw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (fb.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((fb.y + fh - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap(fb.x, fb.y, fw, fh, tx, ty, tw, th) {
                            if fb.vy > 0.0 {
                                fb.y = ty - fh;
                                fb.vy = FIREBALL_BOUNCE; // bounce off floor
                            } else if fb.vy < 0.0 {
                                fb.y = ty + th;
                                fb.vy = 0.0;
                            }
                        }
                    }
                }
            }
        }

        // Fireball-enemy collision (separate pass to avoid borrow issues)
        let mut fb_explode = Vec::new();
        let mut enemy_kills = Vec::new();
        for (fi, fb) in self.fireballs.iter().enumerate() {
            if fb.exploding {
                continue;
            }
            for (ei, enemy) in self.enemies.iter().enumerate() {
                if enemy.state == EnemyState::Dead || !enemy.activated {
                    continue;
                }
                let eh = match enemy.enemy_type {
                    EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                    _ => PLAYER_SMALL_H,
                };
                if aabb_overlap(
                    fb.x,
                    fb.y,
                    FIREBALL_SIZE,
                    FIREBALL_SIZE,
                    enemy.x,
                    enemy.y,
                    PLAYER_SMALL_W,
                    eh,
                ) {
                    fb_explode.push(fi);
                    enemy_kills.push(ei);
                    break;
                }
            }
        }
        for &fi in &fb_explode {
            self.fireballs[fi].exploding = true;
            self.fireballs[fi].explode_timer = 0.0;
        }
        for &ei in &enemy_kills {
            self.enemies[ei].state = EnemyState::Dead;
            self.enemies[ei].death_timer = 3.0; // longer timer — flies off screen
            self.enemies[ei].flipped_death = true;
            self.enemies[ei].vy = -300.0; // launch upward
            self.score += 100;
        }

        // Remove expired fireballs
        let map_bottom = self.level.height as f32 * TILE_SIZE + 100.0;
        self.fireballs.retain(|fb| {
            if fb.exploding && fb.explode_timer >= FIREBALL_EXPLODE_TIME {
                return false;
            }
            if fb.y > map_bottom {
                return false;
            }
            true
        });
    }

    fn draw_smb_tile(&self, screen: &mut Screen, tile_id: u32, x: f32, y: f32) {
        screen.draw_sprite_region(
            self.tex_tiles,
            smb_tile_uv(tile_id),
            [x, y, TILE_SIZE, TILE_SIZE],
        );
    }
}

impl Game for Mari0Game {
    fn new(ctx: &mut Context, _renderer: &Renderer) -> Self {
        let t = |n: &str| Self::tex(ctx, n);

        let vw = ctx.virtual_width;

        let level = load_level(LEVEL_DATA);
        let player_start = level.player_start;
        let enemies = spawn_enemies_from_level(&level);
        let time_limit = level.time_limit;

        Self {
            state: GameState::Menu,
            player: Player::new(player_start.0, player_start.1),
            portals: [None, None],
            projectiles: Vec::new(),
            crosshair_angle: 0.0,
            aim_dot_timer: 0.0,
            portal_anim_timer: 0.0,
            portal_anim_frame: 0,
            enemies,
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
        }
    }

    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
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
                        self.reset_level();
                    } else {
                        self.state = GameState::Menu;
                    }
                }
            }
            GameState::LevelComplete => {
                if input.is_action_just_pressed("jump") {
                    self.state = GameState::Menu;
                }
            }
        }
    }

    fn draw(&self, ctx: &Context, screen: &mut Screen) {
        let cam_x = self.camera.x;

        // Portal tint colors (convert sRGB → linear for GPU)
        let portal_blue = Color {
            r: srgb_to_linear(0.3),
            g: srgb_to_linear(0.6),
            b: 1.0,
            a: 1.0,
        };
        let portal_orange = Color {
            r: 1.0,
            g: srgb_to_linear(0.5),
            b: srgb_to_linear(0.0),
            a: 1.0,
        };
        let portal_colors = [portal_blue, portal_orange];

        // ── Emerging items (drawn BEHIND tiles so they appear from under blocks) ──
        for item in &self.items {
            if !item.emerging {
                continue;
            }
            let ix = item.x - cam_x;
            let iy = item.y;
            let dst = [ix, iy, TILE_SIZE, TILE_SIZE];
            match item.item_type {
                ItemType::Mushroom => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(1, 0), dst);
                }
                ItemType::OneUp => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(2, 0), dst);
                }
                ItemType::Star => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_star, star_frame_uv(frame), dst);
                }
                ItemType::FireFlower => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_flower, flower_frame_uv(frame), dst);
                }
            }
        }

        // ── Tiles (all non-empty cells from original mari0 data) ──
        let start_col = (cam_x / TILE_SIZE).floor() as i32;
        let end_col = ((cam_x + self.vw) / TILE_SIZE).ceil() as i32 + 1;
        for row in 0..self.level.height as i32 {
            for col in start_col..end_col.min(self.level.width as i32) {
                if col < 0 {
                    continue;
                }
                let tile_id = get_tile(&self.level, col, row);
                if tile_id != SMB_EMPTY && tile_id != SMB_HIDDEN_BLOCK {
                    let x = col as f32 * TILE_SIZE - cam_x;
                    let mut y = row as f32 * TILE_SIZE;
                    // Block bounce offset
                    for bounce in &self.block_bounces {
                        if bounce.col == col && bounce.row == row {
                            let t = bounce.timer / BLOCK_BOUNCE_TIME;
                            // sin curve: up then back down
                            y -= (t * std::f32::consts::PI).sin() * BLOCK_BOUNCE_HEIGHT;
                        }
                    }
                    self.draw_smb_tile(screen, tile_id, x, y);
                }
            }
        }

        // ── Flag sprite (drawn beside the flagpole pole) ──
        if self.level.flag_x > 0.0 {
            let fx = self.level.flag_x - cam_x;
            if fx > -TILE_SIZE && fx < self.vw + TILE_SIZE {
                screen.draw_sprite(
                    self.tex_flag,
                    fx - TILE_SIZE,
                    3.0 * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
            }
        }

        // ── Coins ──
        let coin_frame = ((self.time_remaining * 4.0) as u32) % 2;
        let coin_src = coin_frame_uv(coin_frame);
        for coin in &self.level.coins {
            if !coin.collected {
                let x = coin.x - cam_x;
                screen.draw_sprite_region(self.tex_coin_anim, coin_src, [x, coin.y, 16.0, 16.0]);
            }
        }

        // ── Portals (animated, matches original mari0 portal.png layout) ──
        for (i, portal_opt) in self.portals.iter().enumerate() {
            if let Some(portal) = portal_opt {
                if !portal.active {
                    continue;
                }
                let px = portal.x - cam_x;
                let py = portal.y;
                let color = portal_colors[i];
                let scale = portal.open_scale;
                if scale <= 0.0 {
                    continue;
                }

                // Animation frame (0..5 maps to original 1-indexed frames 1..6)
                let frame_y = (self.portal_anim_frame + 1) as f32; // y offset in strip units

                match portal.orientation {
                    Orientation::Left | Orientation::Right => {
                        // Vertical portal: use portal_v.png (32×64, pre-rotated)
                        // UV: x = (frame+1)*4/32, y = portal_idx*0.5, w = 4/32, h = 0.5
                        let src = [frame_y * (4.0 / 32.0), i as f32 * 0.5, 4.0 / 32.0, 0.5];
                        let h = 64.0 * scale;
                        let dst = [px - 4.0, py - h / 2.0, 8.0, h];
                        screen.draw_sprite_region_tinted(self.tex_portal_v, src, dst, color);
                    }
                    Orientation::Up | Orientation::Down => {
                        // Horizontal portal: use portal.png (64×32)
                        // UV: x = portal_idx*0.5, y = (frame+1)*4/32, w = 0.5, h = 4/32
                        let src = [i as f32 * 0.5, frame_y * (4.0 / 32.0), 0.5, 4.0 / 32.0];
                        let w = 64.0 * scale;
                        let dst = [px - w / 2.0, py - 4.0, w, 8.0];
                        screen.draw_sprite_region_tinted(self.tex_portal, src, dst, color);
                    }
                }
            }
        }

        // ── Enemies ──
        for enemy in &self.enemies {
            let ex = enemy.x - cam_x;
            let ey = enemy.y;
            let eh = match enemy.enemy_type {
                EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                _ => PLAYER_SMALL_H,
            };
            let dst = [ex, ey, PLAYER_SMALL_W, eh];
            match enemy.state {
                EnemyState::Dead => {
                    if enemy.flipped_death {
                        // Star/fireball kill: draw walking sprite upside-down
                        match enemy.enemy_type {
                            EnemyType::Goomba => {
                                let src = goomba_uv(0, 0);
                                screen.draw_sprite_region_flipped(
                                    self.tex_goomba,
                                    src,
                                    dst,
                                    false,
                                    true,
                                );
                            }
                            EnemyType::Koopa => {
                                let src = koopa_uv(0, 0);
                                screen.draw_sprite_region_flipped(
                                    self.tex_koopa,
                                    src,
                                    dst,
                                    false,
                                    true,
                                );
                            }
                        }
                    } else {
                        // Stomp kill: squashed sprite
                        match enemy.enemy_type {
                            EnemyType::Goomba => {
                                screen.draw_sprite_region(self.tex_goomba, goomba_uv(1, 0), dst);
                            }
                            EnemyType::Koopa => {
                                screen.draw_sprite_region(self.tex_koopa, koopa_uv(4, 0), dst);
                            }
                        }
                    }
                }
                EnemyState::Shell | EnemyState::ShellMoving => {
                    screen.draw_sprite_region(self.tex_koopa, koopa_uv(4, 0), dst);
                }
                EnemyState::Walking => {
                    match enemy.enemy_type {
                        EnemyType::Goomba => {
                            // Only one walking frame (col 0); animation = flip horizontally
                            let src = goomba_uv(0, 0);
                            let flip = ((enemy.anim_timer * 5.0) as u32) % 2 == 1;
                            if flip {
                                screen.draw_sprite_region_flipped(
                                    self.tex_goomba,
                                    src,
                                    dst,
                                    true,
                                    false,
                                );
                            } else {
                                screen.draw_sprite_region(self.tex_goomba, src, dst);
                            }
                        }
                        EnemyType::Koopa => {
                            let frame = ((enemy.anim_timer * 4.0) as u32) % 2;
                            let src = koopa_uv(frame, 0);
                            if enemy.facing_right {
                                screen.draw_sprite_region_flipped(
                                    self.tex_koopa,
                                    src,
                                    dst,
                                    true,
                                    false,
                                );
                            } else {
                                screen.draw_sprite_region(self.tex_koopa, src, dst);
                            }
                        }
                    }
                }
            }
        }

        // ── Items (mushroom, star, 1-up) — only non-emerging (emerging drawn behind tiles) ──
        for item in &self.items {
            if item.emerging {
                continue;
            }
            let ix = item.x - cam_x;
            let iy = item.y;
            let dst = [ix, iy, TILE_SIZE, TILE_SIZE];
            match item.item_type {
                ItemType::Mushroom => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(1, 0), dst);
                }
                ItemType::OneUp => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(2, 0), dst);
                }
                ItemType::Star => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_star, star_frame_uv(frame), dst);
                }
                ItemType::FireFlower => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_flower, flower_frame_uv(frame), dst);
                }
            }
        }

        // ── Fireballs ──
        for fb in &self.fireballs {
            let fx = fb.x - cam_x;
            let fy = fb.y;
            if fb.exploding {
                let frame = ((fb.explode_timer / FIREBALL_ANIM_DELAY) as u32).min(2);
                let dst = [
                    fx - FIREBALL_SIZE * 0.5,
                    fy - FIREBALL_SIZE * 0.5,
                    TILE_SIZE,
                    TILE_SIZE,
                ];
                screen.draw_sprite_region(self.tex_fireball, fireball_explode_uv(frame), dst);
            } else {
                let frame = ((fb.anim_timer / FIREBALL_ANIM_DELAY) as u32) % 4;
                let dst = [fx, fy, FIREBALL_SIZE, FIREBALL_SIZE];
                screen.draw_sprite_region(self.tex_fireball, fireball_uv(frame), dst);
            }
        }

        // ── Coin popups ──
        let coin_frame = ((self.time_remaining * 8.0) as u32) % 2;
        let coin_src = coin_frame_uv(coin_frame);
        for popup in &self.coin_popups {
            let cx = popup.x - cam_x + 8.0; // center 16px coin in 32px tile
            let cy = popup.y + 8.0;
            let alpha = 1.0 - (popup.timer / COIN_POPUP_TIME).min(1.0);
            let color = Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: alpha,
            };
            screen.draw_sprite_region_tinted(
                self.tex_coin_anim,
                coin_src,
                [cx, cy, 16.0, 16.0],
                color,
            );
        }

        // ── Brick debris ──
        for debris in &self.brick_debris {
            let dx = debris.x - cam_x;
            let dy = debris.y;
            // Draw a small piece of brick tile (quarter of the tile)
            let quarter_uv = smb_tile_uv(SMB_BRICK);
            let half_w = TILE_SIZE * 0.5;
            screen.draw_sprite_region(
                self.tex_tiles,
                quarter_uv,
                [dx - half_w * 0.5, dy - half_w * 0.5, half_w, half_w],
            );
        }

        // ── Player ──
        if self.state == GameState::Playing
            || self.state == GameState::Dead
            || self.state == GameState::LevelComplete
        {
            let visible = self.player.invincible_timer <= 0.0
                || ((self.player.invincible_timer * 10.0) as u32 % 2 == 0);
            if visible {
                // Gun-angle sprite row (mari0 getAngleFrame):
                // Row 0 = gun up, 1 = diagonal up, 2 = horizontal, 3 = down
                // Compute angle from vertical ("up"): acos(-sin(crosshair_angle))
                // Use -sin(angle) as the "up-component" and compare to cos(π/8) thresholds
                let up_comp = -self.crosshair_angle.sin();
                let angle_row: u32 = if up_comp > 0.924 {
                    // < π/8 from vertical
                    0
                } else if up_comp > 0.383 {
                    // < 3π/8
                    1
                } else if up_comp > -0.383 {
                    // < 5π/8
                    2
                } else {
                    3
                };

                // Player faces mouse direction (mari0: pointingangle > 0 → face left)
                let face_right = self.crosshair_angle.cos() >= 0.0;

                let src = if self.player.is_big {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_big_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_big_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_big_uv(5, angle_row),
                    }
                } else {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_uv(5, angle_row),
                    }
                };
                let px = self.player.x - cam_x;
                let py = self.player.y;
                let (sw, sh) = if self.player.is_big {
                    (MARIO_BIG_SPRITE_W, MARIO_BIG_SPRITE_H)
                } else {
                    (MARIO_SMALL_SPRITE_W, MARIO_SMALL_SPRITE_H)
                };
                let bottom_pad = 2.0 * MARIO_SPRITE_SCALE; // 4px
                let sx = px + (self.player.width - sw) / 2.0;
                let sy = py + self.player.height - sh + bottom_pad;
                let dst = [sx, sy, sw, sh];

                // Mari0 4-layer palette rendering (Player 1 = Red Mario)
                // Draw order: layer1 (primary), layer2 (secondary), layer3 (tertiary), layer0 (outline)
                // Colors are sRGB values from original mari0; convert to linear for GPU tint multiplication
                let mario_colors = if self.player.is_fire {
                    [
                        Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }, // layer1: white shirt (fire)
                        Color {
                            r: srgb_to_linear(224.0 / 255.0),
                            g: srgb_to_linear(32.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer2: red overalls
                        Color {
                            r: srgb_to_linear(252.0 / 255.0),
                            g: srgb_to_linear(152.0 / 255.0),
                            b: srgb_to_linear(56.0 / 255.0),
                            a: 1.0,
                        }, // layer3: skin
                    ]
                } else {
                    [
                        Color {
                            r: srgb_to_linear(224.0 / 255.0),
                            g: srgb_to_linear(32.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer1: red shirt
                        Color {
                            r: srgb_to_linear(136.0 / 255.0),
                            g: srgb_to_linear(112.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer2: brown
                        Color {
                            r: srgb_to_linear(252.0 / 255.0),
                            g: srgb_to_linear(152.0 / 255.0),
                            b: srgb_to_linear(56.0 / 255.0),
                            a: 1.0,
                        }, // layer3: skin
                    ]
                };
                let layers = if self.player.is_big {
                    &self.tex_mario_big_layers
                } else {
                    &self.tex_mario_layers
                };
                // Layers 1-3 with palette colors
                for (i, color) in mario_colors.iter().enumerate() {
                    let tex = layers[i + 1];
                    if face_right {
                        screen.draw_sprite_region_tinted(tex, src, dst, *color);
                    } else {
                        screen
                            .draw_sprite_region_flipped_tinted(tex, src, dst, true, false, *color);
                    }
                }
                // Layer 0 (outline) drawn last, white tint (as-is)
                if face_right {
                    screen.draw_sprite_region(layers[0], src, dst);
                } else {
                    screen.draw_sprite_region_flipped(layers[0], src, dst, true, false);
                }
            }
        }

        // ── Projectiles (with particle trail, matches original mari0) ──
        for proj in &self.projectiles {
            let color = portal_colors[proj.portal_index];

            // Trail: 5 fading copies behind the projectile
            let half_color = Color {
                r: color.r * 0.5,
                g: color.g * 0.5,
                b: color.b * 0.5,
                a: color.a,
            };
            for ti in (1..=5).rev() {
                let t = ti as f32 * 0.008; // 8ms apart
                let tx = proj.x - proj.vx * t - cam_x;
                let ty = proj.y - proj.vy * t;
                let alpha = 0.6 - ti as f32 * 0.12;
                if alpha <= 0.0 {
                    continue;
                }
                let tc = Color {
                    r: half_color.r,
                    g: half_color.g,
                    b: half_color.b,
                    a: alpha,
                };
                screen.draw_sprite_tinted(
                    self.tex_portal_projectile,
                    tx - 5.0,
                    ty - 5.0,
                    10.0,
                    10.0,
                    tc,
                );
            }

            // Main projectile orb (8×8 source at 2x scale = 16×16)
            screen.draw_sprite_tinted(
                self.tex_portal_projectile,
                proj.x - cam_x - 8.0,
                proj.y - 8.0,
                16.0,
                16.0,
                color,
            );
        }

        // ── Portal aiming line + crosshair (matches mari0 game.lua:1600-1662) ──
        // mari0 constants: portaldotsdistance=1.2, portaldotstime=0.8,
        //   portaldotsinner=10, portaldotsouter=70, scale=2
        if self.state == GameState::Playing {
            let source_x = self.player.center_x();
            let source_y = self.player.center_y();
            let angle = self.crosshair_angle;
            const SCALE: f32 = 2.0; // our render scale (TILE_SIZE/16)

            let (end_x, end_y, hit_info) =
                trace_aim_line(&self.level, source_x, source_y, angle, cam_x, self.vw);

            // Portal possible? (original: cox ~= false and getportalposition ~= false)
            let portal_possible = matches!(hit_info, Some((_, true)));

            // Dot color: green if portal can be placed, red otherwise (original: setColor)
            let dot_rgb = if portal_possible {
                (0.0_f32, 1.0_f32, 0.0_f32)
            } else {
                (1.0, 0.0, 0.0)
            };

            // Distance in pixels from source to endpoint
            let dx_px = end_x - source_x;
            let dy_px = end_y - source_y;
            let dist_px = (dx_px * dx_px + dy_px * dy_px).sqrt();

            // Distance in tile units (original works in tile coords)
            let dist_tiles = dist_px / (16.0 * SCALE);

            // Draw animated dots from source to endpoint (always, like original)
            let dot_count = (dist_tiles / 1.2) as i32 + 1; // portaldotsdistance = 1.2
            let phase = self.aim_dot_timer / 0.8; // portaldotstime = 0.8

            for i in 0..dot_count {
                let t = ((i as f32) + phase) / (dist_tiles / 1.2).max(1.0);
                if t >= 1.0 {
                    continue;
                }

                // Dot position in screen coords
                let dot_screen_x = (source_x - cam_x) + dx_px * t;
                let dot_screen_y = (source_y) + dy_px * t;

                // xplus/yplus = offset from source in screen pixels
                let xplus = dx_px * t;
                let yplus = dy_px * t;

                // Alpha fade near source (original: radius in base pixels)
                let radius = (xplus * xplus + yplus * yplus).sqrt() / SCALE;
                let mut alpha = 1.0_f32;
                if radius < 70.0 {
                    // portaldotsouter
                    // Original: alpha = (radius-inner)*(outer-inner), clamped
                    alpha = ((radius - 10.0) / (70.0 - 10.0)).clamp(0.0, 1.0);
                }

                let dot_color = Color {
                    r: dot_rgb.0,
                    g: dot_rgb.1,
                    b: dot_rgb.2,
                    a: alpha,
                };

                // Dot size = scale×scale = 2×2 pixels, offset -0.25*scale (original)
                let off = 0.25 * SCALE; // 0.5
                screen.draw_sprite_tinted(
                    self.tex_portal_dot,
                    (dot_screen_x - off).floor(),
                    (dot_screen_y - off).floor(),
                    SCALE,
                    SCALE,
                    dot_color,
                );
            }

            // Crosshair only drawn when a wall is hit (original: if cox ~= false)
            if let Some((orient, _)) = hit_info {
                let ch_color = Color {
                    r: dot_rgb.0,
                    g: dot_rgb.1,
                    b: dot_rgb.2,
                    a: 1.0,
                };
                let ch_screen_x = end_x - cam_x;
                let ch_screen_y = end_y;

                // Original: portalcrosshairimg 8×8, drawn with origin (4,8), at scale×scale
                // origin (4,8) = center-bottom of the 8px image
                // Rendered size = 8*scale × 8*scale = 16×16
                let ch_w = 8.0 * SCALE; // 16
                let ch_h = 8.0 * SCALE; // 16

                // Position crosshair so its edge touches the wall surface
                // Original rotates based on side; we approximate with position offset
                let (cx, cy) = match orient {
                    Orientation::Up => {
                        // Wall above: crosshair hangs down from hit point
                        (ch_screen_x - ch_w * 0.5, ch_screen_y)
                    }
                    Orientation::Down => {
                        // Wall below: crosshair extends up from hit point
                        (ch_screen_x - ch_w * 0.5, ch_screen_y - ch_h)
                    }
                    Orientation::Left => {
                        // Wall to the left: crosshair extends right
                        (ch_screen_x, ch_screen_y - ch_h * 0.5)
                    }
                    Orientation::Right => {
                        // Wall to the right: crosshair extends left
                        (ch_screen_x - ch_w, ch_screen_y - ch_h * 0.5)
                    }
                };

                screen.draw_sprite_tinted(
                    self.tex_portal_crosshair,
                    cx.floor(),
                    cy.floor(),
                    ch_w,
                    ch_h,
                    ch_color,
                );
            }
        }

        // ── Score popups (floating text) ──
        // Draw before HUD so they appear in game world
        {
            let hud_font = ctx.assets.font("hud");
            if let Some(font) = hud_font {
                for popup in &self.score_popups {
                    let sx = popup.x - cam_x;
                    let sy = popup.y;
                    let alpha = 1.0 - (popup.timer / SCORE_POPUP_TIME).min(1.0);
                    if alpha > 0.0 {
                        // Draw score text (simple white text with fade)
                        let text = format!("{}", popup.value);
                        screen.draw_text(font, &text, sx, sy);
                    }
                }
            }
        }

        // ── HUD (NES-style four-column layout) ──
        let hud_font = ctx.assets.font("hud");
        let ui_font = ctx.assets.font("ui");
        let title_font = ctx.assets.font("title");

        match self.state {
            GameState::Menu => {
                // NES-style HUD on menu too
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, "1-1", 320.0, 20.0);
                    screen.draw_text(font, "TIME", 432.0, 8.0);
                }
                if let Some(font) = title_font {
                    screen.draw_text_centered(font, "MARI0", 100.0);
                }
                if let Some(font) = ui_font {
                    screen.draw_text_centered(font, "Mario + Portal", 140.0);
                    screen.draw_text_centered(font, "A tribute to Stabyourself.net", 165.0);
                    screen.draw_text_centered(font, "WASD/Arrows: Move  Space: Jump", 200.0);
                    screen.draw_text_centered(font, "Mouse: Aim  L/R Click: Portals", 220.0);
                    screen.draw_text_centered(font, "Press SPACE to start", 280.0);
                }
            }
            GameState::Playing => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, "1-1", 320.0, 20.0);
                    screen.draw_text(font, "TIME", 432.0, 8.0);
                    screen.draw_text(
                        font,
                        &format!("{}", self.time_remaining as u32),
                        440.0,
                        20.0,
                    );
                }
            }
            GameState::Dead => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, "1-1", 320.0, 20.0);
                }
                if let Some(font) = title_font {
                    if self.lives > 0 {
                        screen.draw_text_centered(font, "YOU DIED", 150.0);
                    } else {
                        screen.draw_text_centered(font, "GAME OVER", 150.0);
                    }
                }
                if let Some(font) = ui_font {
                    let score_text = format!("Score: {}", self.score);
                    screen.draw_text_centered(font, &score_text, 200.0);
                    screen.draw_text_centered(font, "Press SPACE to continue", 250.0);
                }
            }
            GameState::LevelComplete => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, "1-1", 320.0, 20.0);
                }
                if let Some(font) = title_font {
                    screen.draw_text_centered(font, "LEVEL COMPLETE!", 150.0);
                }
                if let Some(font) = ui_font {
                    let score_text = format!("Score: {}", self.score);
                    screen.draw_text_centered(font, &score_text, 200.0);
                    screen.draw_text_centered(font, "Press SPACE to continue", 250.0);
                }
            }
        }
    }

    fn clear_color(&self) -> Color {
        Color::from_hex(0x5C94FC) // NES Mario sky blue
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        let state_str = match self.state {
            GameState::Menu => "menu",
            GameState::Playing => "playing",
            GameState::Dead => "dead",
            GameState::LevelComplete => "level_complete",
        };

        let anim_str = match self.player.anim_state {
            PlayerAnim::Idle => "idle",
            PlayerAnim::Run => "run",
            PlayerAnim::Jump => "jump",
            PlayerAnim::Fall => "fall",
        };

        let portals_json = |idx: usize| -> serde_json::Value {
            match &self.portals[idx] {
                Some(p) if p.active => serde_json::json!({
                    "x": p.x, "y": p.y,
                    "orientation": match p.orientation {
                        Orientation::Up => "up",
                        Orientation::Down => "down",
                        Orientation::Left => "left",
                        Orientation::Right => "right",
                    },
                    "active": true,
                }),
                _ => serde_json::Value::Null,
            }
        };

        let enemies: Vec<serde_json::Value> = self.enemies.iter().map(|e| {
            serde_json::json!({
                "x": e.x, "y": e.y,
                "type": match e.enemy_type { EnemyType::Goomba => "goomba", EnemyType::Koopa => "koopa" },
                "state": match e.state {
                    EnemyState::Walking => "walking",
                    EnemyState::Dead => "dead",
                    EnemyState::Shell => "shell",
                    EnemyState::ShellMoving => "shell_moving",
                },
                "facing_right": e.facing_right,
            })
        }).collect();

        let coins: Vec<serde_json::Value> = self
            .level
            .coins
            .iter()
            .map(|c| serde_json::json!({"x": c.x, "y": c.y, "collected": c.collected}))
            .collect();

        let projectiles: Vec<serde_json::Value> = self
            .projectiles
            .iter()
            .map(|p| {
                serde_json::json!({
                    "x": p.x, "y": p.y, "vx": p.vx, "vy": p.vy,
                    "color": if p.portal_index == 0 { "blue" } else { "orange" },
                })
            })
            .collect();

        let items: Vec<serde_json::Value> = self
            .items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "type": match item.item_type {
                        ItemType::Mushroom => "mushroom",
                        ItemType::Star => "star",
                        ItemType::OneUp => "1up",
                        ItemType::FireFlower => "fire_flower",
                    },
                    "x": item.x, "y": item.y,
                    "vx": item.vx, "vy": item.vy,
                    "emerging": item.emerging,
                })
            })
            .collect();

        let block_contents: Vec<serde_json::Value> = self
            .level
            .block_contents
            .iter()
            .map(|((row, col), content)| {
                serde_json::json!({
                    "row": row, "col": col,
                    "x": *col as f32 * TILE_SIZE,
                    "y": *row as f32 * TILE_SIZE,
                    "content": match content {
                        BlockContent::Coin => "coin",
                        BlockContent::MultiCoin(_) => "multi_coin",
                        BlockContent::Mushroom => "mushroom",
                        BlockContent::Star => "star",
                        BlockContent::OneUp => "1up",
                    },
                })
            })
            .collect();

        serde_json::json!({
            "state": state_str,
            "player": {
                "x": self.player.x,
                "y": self.player.y,
                "vx": self.player.vx,
                "vy": self.player.vy,
                "width": self.player.width,
                "height": self.player.height,
                "on_ground": self.player.on_ground,
                "facing_right": self.player.facing_right,
                "is_big": self.player.is_big,
                "is_fire": self.player.is_fire,
                "is_jumping": self.player.is_jumping,
                "anim_state": anim_str,
                "portal_cooldown": self.player.portal_cooldown,
                "teleport_cooldown": self.player.teleport_cooldown,
                "invincible_timer": self.player.invincible_timer,
            },
            "portals": {
                "blue": portals_json(0),
                "orange": portals_json(1),
            },
            "projectiles": projectiles,
            "crosshair_angle": self.crosshair_angle,
            "enemies": enemies,
            "coins": coins,
            "level": {
                "width": self.level.width,
                "height": self.level.height,
                "flag_x": self.level.flag_x,
            },
            "camera_x": self.camera.x,
            "score": self.score,
            "coin_count": self.coins,
            "lives": self.lives,
            "combo_index": self.combo_index,
            "time_remaining": self.time_remaining,
            "items": items,
            "block_contents": block_contents,
            "star_timer": self.star_timer,
        })
    }

    #[cfg(feature = "vdp")]
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match method {
            "game.reset" => {
                self.state = GameState::Playing;
                self.reset_level();
                self.score = 0;
                self.coins = 0;
                self.lives = 3;
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setPlayerPos" => {
                let x = params
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'x'")?;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'y'")?;
                self.player.x = x as f32;
                self.player.y = y as f32;
                if let Some(vx) = params.get("vx").and_then(|v| v.as_f64()) {
                    self.player.vx = vx as f32;
                }
                if let Some(vy) = params.get("vy").and_then(|v| v.as_f64()) {
                    self.player.vy = vy as f32;
                }
                Ok(serde_json::json!({"x": self.player.x, "y": self.player.y,
                    "vx": self.player.vx, "vy": self.player.vy}))
            }
            "game.setPlayerSize" => {
                let size = params
                    .get("size")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'size'")?;
                match size {
                    "big" => self.player.set_size(true),
                    "small" => self.player.set_size(false),
                    _ => return Err(format!("Unknown size: {}", size)),
                }
                Ok(serde_json::json!({"is_big": self.player.is_big}))
            }
            "game.setState" => {
                let state = params
                    .get("state")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'state'")?;
                match state {
                    "menu" => self.state = GameState::Menu,
                    "playing" => self.state = GameState::Playing,
                    "dead" => self.state = GameState::Dead,
                    "level_complete" => self.state = GameState::LevelComplete,
                    _ => return Err(format!("Unknown state: {}", state)),
                }
                Ok(serde_json::json!({"state": state}))
            }
            "game.setScore" => {
                if let Some(s) = params.get("score").and_then(|v| v.as_u64()) {
                    self.score = s as u32;
                }
                if let Some(c) = params.get("coins").and_then(|v| v.as_u64()) {
                    self.coins = c as u32;
                }
                if let Some(l) = params.get("lives").and_then(|v| v.as_u64()) {
                    self.lives = l as u32;
                }
                Ok(
                    serde_json::json!({"score": self.score, "coins": self.coins, "lives": self.lives}),
                )
            }
            "game.setPortal" => {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'index'")? as usize;
                if index > 1 {
                    return Err("index must be 0 or 1".into());
                }
                let x = params
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'x'")? as f32;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'y'")? as f32;
                let orient_str = params
                    .get("orientation")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'orientation'")?;
                let orientation = match orient_str {
                    "up" => Orientation::Up,
                    "down" => Orientation::Down,
                    "left" => Orientation::Left,
                    "right" => Orientation::Right,
                    _ => return Err(format!("Unknown orientation: {}", orient_str)),
                };
                let active = params
                    .get("active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.portals[index] = Some(Portal {
                    x,
                    y,
                    orientation,
                    active,
                    open_scale: 1.0,
                });
                Ok(serde_json::json!({"index": index, "x": x, "y": y,
                    "orientation": orient_str, "active": active}))
            }
            "game.clearPortals" => {
                self.portals = [None, None];
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.spawnEnemy" => {
                let etype_str = params
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'type'")?;
                let etype = match etype_str {
                    "goomba" => EnemyType::Goomba,
                    "koopa" => EnemyType::Koopa,
                    _ => return Err(format!("Unknown enemy type: {}", etype_str)),
                };
                let x = params
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'x'")? as f32;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or("Missing 'y'")? as f32;
                let facing_right = params
                    .get("facing_right")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.enemies.push(Enemy {
                    x,
                    y,
                    vx: if facing_right {
                        ENEMY_SPEED
                    } else {
                        -ENEMY_SPEED
                    },
                    vy: 0.0,
                    enemy_type: etype,
                    state: EnemyState::Walking,
                    facing_right,
                    on_ground: false,
                    activated: true, // VDP-spawned enemies are always active
                    anim_timer: 0.0,
                    death_timer: 0.0,
                    flipped_death: false,
                });
                Ok(serde_json::json!({"status": "ok", "enemy_count": self.enemies.len()}))
            }
            "game.clearEnemies" => {
                self.enemies.clear();
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setTile" => {
                let col = params
                    .get("col")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing 'col'")? as usize;
                let row = params
                    .get("row")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing 'row'")? as usize;
                if row >= self.level.height || col >= self.level.width {
                    return Err("Tile position out of bounds".into());
                }
                let type_str = params
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'type'")?;
                let tile_id: u32 = match type_str {
                    "empty" => SMB_EMPTY,
                    "ground" => SMB_GROUND,
                    "brick" => SMB_BRICK,
                    "question" => SMB_QUESTION,
                    "question_used" => SMB_QUESTION_USED,
                    "staircase" => SMB_STAIRCASE,
                    "pipe_tl" => SMB_PIPE_TL,
                    "pipe_tr" => SMB_PIPE_TR,
                    "pipe_bl" => SMB_PIPE_BL,
                    "pipe_br" => SMB_PIPE_BR,
                    _ => {
                        // Try parsing as raw tile ID number
                        type_str
                            .parse::<u32>()
                            .map_err(|_| format!("Unknown tile type: {}", type_str))?
                    }
                };
                self.level.tiles[row][col] = tile_id;
                Ok(serde_json::json!({"col": col, "row": row, "type": type_str}))
            }
            _ => Err(format!("Unknown method: {}", method)),
        }
    }
}

fn main() {
    vibe2d::run::<Mari0Game>("game.yaml");
}
