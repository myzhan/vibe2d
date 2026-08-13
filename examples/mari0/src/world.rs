//! The loaded level, the camera, and where the player is in the mappack.
//!
//! `Level` here is the *gameplay* view of a level: the tile grid the game loop
//! mutates plus the spawn table it consumes. The faithful parse of the file lives
//! in `level::Level`; `load_level` adapts one to the other, which is what let the
//! parser be finished and tested against all 73 shipped levels independently of
//! the game loop.

use std::collections::HashMap;

use crate::constants::*;
use crate::effects::CoinInstance;
use crate::enemies::EnemyType;
use crate::items::BlockContent;
use crate::level;

pub(crate) struct Level {
    pub(crate) tiles: Vec<Vec<u32>>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) coins: Vec<CoinInstance>,
    pub(crate) enemy_spawns: Vec<EnemySpawnPoint>,
    pub(crate) block_contents: HashMap<(usize, usize), BlockContent>,
    pub(crate) multi_coin_timers: HashMap<(usize, usize), f32>,
    pub(crate) player_start: (f32, f32),
    pub(crate) flag_x: f32,
    pub(crate) time_limit: f32,
    /// Backdrop palette, 1..=3. Selects the clear colour.
    pub(crate) background: u8,
    /// Environment palette, 1..=4 (overworld/underground/castle/underwater).
    ///
    /// Does **not** swap tile art — the level data already references different
    /// tile ids per environment (which is why there are three distinct "brick"
    /// tiles: 7, 49 and 122). What it *does* decide is the replacement tile a
    /// struck block turns into, which differs per environment.
    pub(crate) spriteset: u8,
}

/// A pending enemy placement produced by the loader.
///
/// A named struct rather than a tuple because firebars need a segment index —
/// each fireball on a bar is its own entity sharing the bar's pivot.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EnemySpawnPoint {
    pub(crate) enemy_type: EnemyType,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) facing_right: bool,
    /// Index along a firebar; 0 for everything else.
    pub(crate) segment: u32,
}

pub(crate) struct Camera {
    pub(crate) x: f32,
}

/// Which mappack and level are loaded, and how to walk to the next one.
///
/// Mari0 has no explicit world count: `nextlevel()` increments the level, rolls
/// over to the next world past 4, and the mappack simply *ends* when the next
/// file doesn't exist (`game.lua:3448`, `levelscreen.lua:32`).
#[derive(Debug, Clone)]
pub(crate) struct LevelId {
    pub(crate) pack: String,
    pub(crate) world: u32,
    pub(crate) level: u32,
}

impl LevelId {
    pub(crate) fn new(pack: &str, world: u32, level: u32) -> Self {
        Self {
            pack: pack.to_string(),
            world,
            level,
        }
    }

    pub(crate) fn name(&self) -> String {
        format!("{}-{}", self.world, self.level)
    }

    /// Advance to the next level, rolling worlds over past level 4.
    pub(crate) fn advance(&mut self) {
        self.level += 1;
        if self.level > 4 {
            self.level = 1;
            self.world += 1;
        }
    }

    /// Does this level exist in the shipped data?
    pub(crate) fn exists(&self) -> bool {
        level::raw_level(&self.pack, &self.name()).is_some()
    }
}

/// Mappack loaded at startup. The starting level is world 1, level 1 — see
/// `LevelId`, which now decides what loads instead of a compile-time
/// `include_str!` of a single file.
pub(crate) const START_PACK: &str = "smb";

// ── Level loading ───────────────────────────────────────────────────

/// Build the gameplay `Level` from a parsed level file.
///
/// This is an adapter: `level::Level` is the faithful, fully-parsed
/// representation (all 220 tiles, all 100 entity types, every metadata field),
/// while this `Level` is the shape the gameplay code already consumes. Keeping the
/// two separate means the parser could be finished and tested against all 73
/// shipped levels without destabilising the game loop.
pub(crate) fn load_level(pack: &str, name: &str) -> Level {
    let parsed = level::load(pack, name)
        .unwrap_or_else(|| panic!("no such level {pack}/{name}"))
        .unwrap_or_else(|e| panic!("{pack}/{name} failed to parse: {e}"));

    let width = parsed.width;
    let height = parsed.height;

    let mut tiles = vec![vec![level::tiles::TILE_EMPTY as u32; width]; height];
    let mut coins = Vec::new();
    for (row, tile_row) in tiles.iter_mut().enumerate() {
        for (col, slot) in tile_row.iter_mut().enumerate() {
            let tile = parsed.tile(col as i32, row as i32);
            *slot = tile as u32;
            // Free-standing coins become collectables rather than tiles so the
            // pickup path doesn't have to special-case the tile layer.
            if level::tiles::props(tile).coin() {
                coins.push(CoinInstance {
                    x: col as f32 * TILE_SIZE,
                    y: row as f32 * TILE_SIZE,
                    collected: false,
                });
            }
        }
    }

    // Block contents, straight from the entity pass.
    let mut block_contents: HashMap<(usize, usize), BlockContent> = HashMap::new();
    for (x, y, kind, arg) in &parsed.markers.block_contents {
        let content = match kind {
            level::EntityKind::Mushroom => BlockContent::Mushroom,
            level::EntityKind::OneUp => BlockContent::OneUp,
            level::EntityKind::Star => BlockContent::Star,
            level::EntityKind::ManyCoins => BlockContent::MultiCoin(arg.unwrap_or(5).max(1) as u32),
            _ => continue,
        };
        block_contents.insert((*y, *x), content);
    }
    // A question block with no stated contents holds a single coin.
    for row in 0..height {
        for col in 0..width {
            let tile = parsed.tile(col as i32, row as i32);
            if level::tiles::props(tile).coinblock() && !block_contents.contains_key(&(row, col)) {
                block_contents.insert((row, col), BlockContent::Coin);
            }
        }
    }

    // Only the enemy kinds this build implements are instantiated; the rest are
    // parsed and carried in `markers` for the modules still being built out.
    let mut enemy_spawns = Vec::new();
    let mut firebar_segments: Vec<(f32, f32, u32)> = Vec::new();
    for spawn in &parsed.markers.enemies {
        let px = spawn.x as f32 * TILE_SIZE;
        let py = spawn.y as f32 * TILE_SIZE;
        match spawn.kind {
            level::EntityKind::Goomba => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Goomba,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            level::EntityKind::GoombaHalf => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Goomba,
                x: px,
                y: py,
                facing_right: true,
                segment: 0,
            }),
            level::EntityKind::Koopa | level::EntityKind::KoopaHalf => {
                enemy_spawns.push(EnemySpawnPoint {
                    enemy_type: EnemyType::Koopa,
                    x: px,
                    y: py,
                    facing_right: false,
                    segment: 0,
                })
            }
            level::EntityKind::KoopaRed | level::EntityKind::KoopaRedHalf => {
                enemy_spawns.push(EnemySpawnPoint {
                    enemy_type: EnemyType::KoopaRed,
                    x: px,
                    y: py,
                    facing_right: false,
                    segment: 0,
                })
            }
            level::EntityKind::Beetle | level::EntityKind::BeetleHalf => {
                enemy_spawns.push(EnemySpawnPoint {
                    enemy_type: EnemyType::Beetle,
                    x: px,
                    y: py,
                    facing_right: false,
                    segment: 0,
                })
            }
            level::EntityKind::Plant => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Plant,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            level::EntityKind::KoopaFlying | level::EntityKind::KoopaRedFlying => enemy_spawns
                .push(EnemySpawnPoint {
                    enemy_type: EnemyType::KoopaFlying,
                    x: px,
                    y: py,
                    facing_right: false,
                    segment: 0,
                }),
            level::EntityKind::UpFire => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::UpFire,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            level::EntityKind::CheepRed => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::CheepRed,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            level::EntityKind::CheepWhite => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::CheepWhite,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            // A firebar is N fireballs sharing one pivot, so it expands into
            // `length` separate entities here rather than being one wide object.
            level::EntityKind::CastleFireCw | level::EntityKind::CastleFireCcw => {
                let length = spawn.arg.unwrap_or(6).clamp(1, 12);
                for seg in 0..length {
                    firebar_segments.push((px, py, seg as u32));
                }
            }
            _ => {}
        }
    }

    // Firebar segments ride along in the spawn list; `segment` is patched in
    // after construction since the tuple form has no room for it.
    for (px, py, seg) in &firebar_segments {
        enemy_spawns.push(EnemySpawnPoint {
            enemy_type: EnemyType::Firebar,
            x: *px,
            y: *py,
            facing_right: false,
            segment: *seg,
        });
    }

    let (sx, sy) = parsed.markers.spawn_or_default();
    let player_start = (
        sx as f32 * TILE_SIZE,
        sy as f32 * TILE_SIZE - PLAYER_SMALL_H,
    );
    let flag_x = parsed
        .markers
        .flag
        .map(|(x, _)| x as f32 * TILE_SIZE)
        .unwrap_or(0.0);

    Level {
        tiles,
        width,
        height,
        coins,
        enemy_spawns,
        block_contents,
        multi_coin_timers: HashMap::new(),
        player_start,
        flag_x,
        time_limit: parsed.meta.timelimit as f32,
        background: parsed.meta.background,
        spriteset: parsed.meta.spriteset,
    }
}
