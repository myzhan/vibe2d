//! The loaded level, the camera, and where the player is in the mappack.
//!
//! `Level` here is the *gameplay* view of a level: the tile grid the game loop
//! mutates plus the spawn table it consumes. The faithful parse of the file lives
//! in `level::Level`; `load_level` adapts one to the other, which is what let the
//! parser be finished and tested against all 73 shipped levels independently of
//! the game loop.

use std::collections::{HashMap, HashSet};

use crate::constants::*;
use crate::effects::CoinInstance;
use crate::enemies::EnemyType;
use crate::items::BlockContent;
use crate::level;

/// One of the lab's non-tile solids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SolidRect {
    /// `[x, y, w, h]` in world pixels.
    pub(crate) rect: [f32; 4],
    /// Cubes pass straight through this one.
    ///
    /// A cube dispenser is solid to Mario but transparent to cubes — its collision mask
    /// lists the box category as ignored (`cubedispenser.lua:16`), and that is exactly
    /// what lets the cube it produces fall *out* of the tube instead of being ejected
    /// on top of it.
    pub(crate) cubes_pass: bool,
}

pub(crate) struct Level {
    pub(crate) tiles: Vec<Vec<u32>>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) coins: Vec<CoinInstance>,
    /// Every enemy the level *can* produce. Nothing is instantiated at load — the
    /// camera reveals them column by column. See `Mari0Game::spawn_revealed_columns`.
    pub(crate) enemy_spawns: Vec<EnemySpawnPoint>,
    /// `enemy_spawns` indices bucketed by tile cell, so both the per-column sweep
    /// and the `x±2` cluster cascade are lookups rather than scans.
    ///
    /// A `Vec` per cell because a firebar puts one spawn per segment on the same
    /// pivot cell.
    pub(crate) spawns_by_cell: HashMap<(i32, i32), Vec<usize>>,
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
    /// Which theme to play, 1..=7. See `music::MusicPhase` for the encoding.
    pub(crate) music: u8,
    /// Pipe entrances, keyed by tile cell → destination sublevel (0 = back to the
    /// main level).
    pub(crate) pipes: HashMap<(i32, i32), u32>,
    /// Where the player emerges, keyed by the sublevel they came *from*.
    ///
    /// The original's `pipespawn` entity carries the sublevel number it pairs with,
    /// and both directions of a trip consult it: going in matches
    /// `mariosublevel == arg`, coming back matches `prevsublevel == arg`
    /// (`game.lua:2306`).
    pub(crate) pipe_spawns: HashMap<u32, (i32, i32)>,
    /// Warp pipes, keyed by tile cell → destination world.
    pub(crate) warp_pipes: HashMap<(i32, i32), u32>,
    /// Checkpoints as `(column, row)`, ascending by column.
    ///
    /// The row matters: the original respawns you standing on the checkpoint's own
    /// row, falling back to 13 (`game.lua:2147`).
    pub(crate) checkpoints: Vec<(i32, i32)>,
    /// A 24-wide stub the player runs straight through into sublevel 1.
    ///
    /// Load-bearing for respawns: taking a pipe out of an intermission sets the
    /// sublevel to come back to on death, so dying in 1-2_1 doesn't dump you back
    /// in the stub (`mario.lua:2891-2893`).
    pub(crate) intermission: bool,

    // ── Maze spans (the looping castles) ────────────────────────────
    /// First and last column of each maze span. **Mutable at runtime**: splicing a
    /// repeat into the map shifts every column to its right, these included.
    pub(crate) maze_starts: Vec<i32>,
    pub(crate) maze_ends: Vec<i32>,
    /// How many gates each span requires, i.e. the highest gate number inside it.
    ///
    /// Floors at 1 (`game.lua:2120`), which is why a span with **no** gates can
    /// never be solved. That's not an oversight: 8-4's spans have no gates at all
    /// and are meant to loop forever — you leave by pipe, not by walking.
    pub(crate) maze_gate_counts: Vec<u32>,
    /// Gate cells → gate number. Walking your centre through them in order is what
    /// solves a span.
    pub(crate) maze_gates: HashMap<(i32, i32), u32>,
    /// Cells that block movement *in addition* to the tile grid — currently shut
    /// doors. Rebuilt each frame from the lab network.
    pub(crate) solid_extras: HashSet<(i32, i32)>,
    /// Gel painted on tile faces, one entry per cell, row-major with `width` stride.
    ///
    /// **Mutable at runtime**: this is where a gel blob's splat lands, and it is what
    /// the movement code and the portal-placement rules read back. The level's own
    /// `geltop`/`gelleft`/… entities seed it at load — only where the tile is actually
    /// solid (`game.lua:2435-2450`).
    pub(crate) gels: Vec<level::Gels>,
    /// Solid boxes that aren't cells: light-bridge slabs and cube dispensers. Rebuilt
    /// each frame from the lab network.
    ///
    /// A separate list rather than more `solid_extras` because a bridge is 1/8 of a
    /// block thick and sits *inside* its cell — rounding it up to the whole cell would
    /// wall off the row a horizontal bridge is supposed to let you walk along.
    pub(crate) solid_rects: Vec<SolidRect>,
    /// Moving platforms, as solids. **A second list, deliberately.**
    ///
    /// `solid_rects` is *assigned* wholesale by the lab each frame — but only when the
    /// level has lab elements at all (`update_lab` returns early otherwise). So in
    /// every SMB level nothing ever clears it, and appending platforms to it leaves one
    /// stale rectangle per frame lying around forever. The first symptom was a falling
    /// platform that dropped a few pixels and stopped: the player was standing on the
    /// ghost of where it had been on frame one.
    pub(crate) platform_rects: Vec<SolidRect>,

    /// Cells whose collision is suppressed because a portal has opened there.
    ///
    /// Populated only while **both** portals exist: a lone portal is not a hole
    /// (`modifyportaltiles` requires the pair). Rendering and `getTile` ignore this
    /// set — see `physics::blocks_movement`.
    pub(crate) portal_holes: HashSet<(i32, i32)>,

    /// Platforms the camera has yet to reveal, and the same cell index the enemies use.
    ///
    /// A separate pair of lists rather than more `enemy_spawns`, because a platform is
    /// a solid rather than a creature — but revealed by exactly the same sweep, since
    /// the original routes both through `spawnenemy` and so gives platforms the
    /// `x±2` cluster rule too.
    pub(crate) platform_spawns: Vec<PlatformSpawnPoint>,
    pub(crate) platform_spawns_by_cell: HashMap<(i32, i32), Vec<usize>>,
    /// Springs, placed at load. They never move, so unlike the platforms there is no
    /// reveal to schedule — but their collision box *changes shape* as they compress,
    /// so they get a published list of their own for the same reason the platforms do.
    pub(crate) springs: Vec<(i32, i32)>,
    pub(crate) spring_rects: Vec<SolidRect>,
    /// Elevator-shaft spawners, built at load rather than revealed.
    pub(crate) platform_spawners: Vec<crate::platform::PlatformSpawner>,
    /// The stretch in which bullet bills rain in from the right edge, as columns.
    ///
    /// Both are compared against the *player*, and the pair is not symmetric in use:
    /// 5-3 sets both and so fences off one run, while 6-3 sets only a start and never
    /// turns it off again.
    pub(crate) bullet_bill_start: Option<i32>,
    pub(crate) bullet_bill_end: Option<i32>,
    /// Column past which Bowser (or the level itself) starts breathing fire.
    ///
    /// A **one-way** latch, unlike the bullet-bill and flying-fish pairs: there is no
    /// `fireend` entity, and `mario.lua:973` only ever sets it true.
    pub(crate) fire_start: Option<i32>,
    /// The stretch in which flying fish leap out of the water. Same shape as the bullet
    /// bill pair, and the same two-way latch on the player's x (`mario.lua:977-983`).
    pub(crate) flying_fish_start: Option<i32>,
    pub(crate) flying_fish_end: Option<i32>,
    /// The axe's cell, if this level has one. The castle ending's trigger and the
    /// anchor for the bridge sweep.
    pub(crate) axe: Option<(usize, usize)>,
    /// Column past which lakitu gives up and drifts away, if this level has one.
    ///
    /// A single column rather than a cell: the marker is "place anywhere — defines a
    /// right border for lakito" (`entity.lua:189`) and the loader keeps only its x
    /// (`game.lua:2412`). All three levels that use it park it on row 13 or 14, where
    /// it means nothing.
    pub(crate) lakito_end: Option<i32>,
    /// Columns holding a `mazeend`. Copying one marks the end of a repetition.
    ///
    /// A column set, not cells: the original's test is "does this column contain a
    /// mazeend in any row" (`game.lua:651-660`), and the parser only records the
    /// column anyway.
    pub(crate) maze_end_cols: HashSet<i32>,
}

impl Level {
    /// The gel coating a cell's faces. Cells outside the level are bare.
    pub(crate) fn gels(&self, cell: (i32, i32)) -> level::Gels {
        if cell.0 < 0
            || cell.1 < 0
            || cell.0 as usize >= self.width
            || cell.1 as usize >= self.height
        {
            return level::Gels::default();
        }
        self.gels[cell.1 as usize * self.width + cell.0 as usize]
    }

    /// Paint one face of one cell.
    pub(crate) fn paint_gel(&mut self, cell: (i32, i32), face: level::GelFace, gel: level::Gel) {
        if cell.0 < 0
            || cell.1 < 0
            || cell.0 as usize >= self.width
            || cell.1 as usize >= self.height
        {
            return;
        }
        let index = cell.1 as usize * self.width + cell.0 as usize;
        self.gels[index].set(face, gel);
    }

    /// Splice a copy of column `source` in at `at`, widening the level by one.
    ///
    /// This is the whole trick behind the looping castles. The original does
    /// `table.insert(map, x, …)` at the camera frontier (`game.lua:606-627`), which
    /// pushes the rest of the level — including everything past the maze — one
    /// column right. The corridor never loops back on itself; the map simply grows,
    /// so the player can walk forward forever.
    ///
    /// Everything keyed by column and at or past `at` shifts with it. The original
    /// only shifts `flagx`/`axex`/`mazestarts`/`mazeends` by hand because those are
    /// the only positions it hoists out of the tile grid — pipes, checkpoints and
    /// enemies live *in* `map[x][y]` there and move for free. This port lifted them
    /// into their own tables at load time, so shifting them here is what keeps them
    /// equivalent, not an embellishment.
    pub(crate) fn insert_column(&mut self, at: i32, source: i32) {
        if at < 0 || source < 0 || (source as usize) >= self.width {
            return;
        }
        let at_usize = (at as usize).min(self.width);

        for row in &mut self.tiles {
            let copied = row[source as usize];
            row.insert(at_usize, copied);
        }
        // The gel layer is indexed by the same grid, so it grows with it.
        let mut gels = Vec::with_capacity((self.width + 1) * self.height);
        for row in 0..self.height {
            for col in 0..self.width {
                if col == at_usize {
                    gels.push(self.gels[row * self.width + source as usize]);
                }
                gels.push(self.gels[row * self.width + col]);
            }
            if at_usize >= self.width {
                gels.push(self.gels[row * self.width + source as usize]);
            }
        }
        self.gels = gels;
        self.width += 1;

        let bump_col = |c: i32| if c >= at { c + 1 } else { c };
        let bump_px = |x: f32| {
            if x >= at as f32 * TILE_SIZE {
                x + TILE_SIZE
            } else {
                x
            }
        };

        for coin in &mut self.coins {
            coin.x = bump_px(coin.x);
        }
        for spawn in &mut self.enemy_spawns {
            spawn.x = bump_px(spawn.x);
        }
        self.spawns_by_cell = HashMap::new();
        for (i, sp) in self.enemy_spawns.iter().enumerate() {
            self.spawns_by_cell.entry(sp.cell()).or_default().push(i);
        }

        self.block_contents = self
            .block_contents
            .drain()
            .map(|((row, col), v)| ((row, bump_col(col as i32) as usize), v))
            .collect();
        self.multi_coin_timers = self
            .multi_coin_timers
            .drain()
            .map(|((row, col), v)| ((row, bump_col(col as i32) as usize), v))
            .collect();

        self.flag_x = bump_px(self.flag_x);
        self.pipes = self
            .pipes
            .drain()
            .map(|((c, r), v)| ((bump_col(c), r), v))
            .collect();
        self.warp_pipes = self
            .warp_pipes
            .drain()
            .map(|((c, r), v)| ((bump_col(c), r), v))
            .collect();
        self.pipe_spawns = self
            .pipe_spawns
            .drain()
            .map(|(k, (c, r))| (k, (bump_col(c), r)))
            .collect();
        for (c, _) in &mut self.checkpoints {
            *c = bump_col(*c);
        }
        for c in &mut self.maze_starts {
            *c = bump_col(*c);
        }
        for c in &mut self.maze_ends {
            *c = bump_col(*c);
        }
        self.maze_gates = self
            .maze_gates
            .drain()
            .map(|((c, r), v)| ((bump_col(c), r), v))
            .collect();
        self.maze_end_cols = self.maze_end_cols.drain().map(bump_col).collect();
    }
}

/// A platform waiting to be revealed: its cell, which of the six behaviours, and how
/// wide the level said to make it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlatformSpawnPoint {
    pub(crate) cell: (i32, i32),
    pub(crate) kind: crate::platform::PlatformKind,
    pub(crate) size_blocks: f32,
}

/// The width a platform's argument asks for, in blocks.
///
/// Defaults to 2 when absent (`platform.lua:5`), which one `platformright` in the
/// shipped data relies on.
pub(crate) fn platform_width(arg: Option<f32>) -> f32 {
    arg.unwrap_or(2.0)
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

impl EnemySpawnPoint {
    /// The tile cell this spawn sits in.
    ///
    /// Exact rather than approximate: the loader builds `x`/`y` as
    /// `tile * TILE_SIZE`, so the division recovers the original cell. The lazy
    /// spawner works in cells because the original does — it walks columns as the
    /// camera reveals them.
    pub(crate) fn cell(&self) -> (i32, i32) {
        (
            (self.x / TILE_SIZE).floor() as i32,
            (self.y / TILE_SIZE).floor() as i32,
        )
    }
}

pub(crate) struct Camera {
    pub(crate) x: f32,
}

/// Which mappack and level are loaded, and how to walk to the next one.
///
/// Mari0 has no explicit world count: `nextlevel()` increments the level, rolls
/// over to the next world past 4, and the mappack simply *ends* when the next
/// file doesn't exist (`game.lua:3448`, `levelscreen.lua:32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LevelId {
    pub(crate) pack: String,
    pub(crate) world: u32,
    pub(crate) level: u32,
    /// Which sublevel is loaded; 0 is the main level.
    ///
    /// Sublevels are separate files named `W-L_N` — the underground coin rooms,
    /// the bonus stages, the warp zones. `startlevel` takes a *number* for a
    /// sublevel and a *string* for a fresh level, and the two branches differ in
    /// more than the filename: only the fresh-level branch resets the clock
    /// (`game.lua:1898-1918`, `:2111`).
    pub(crate) sublevel: u32,
}

impl LevelId {
    pub(crate) fn new(pack: &str, world: u32, level: u32) -> Self {
        Self {
            pack: pack.to_string(),
            world,
            level,
            sublevel: 0,
        }
    }

    pub(crate) fn name(&self) -> String {
        if self.sublevel == 0 {
            format!("{}-{}", self.world, self.level)
        } else {
            format!("{}-{}_{}", self.world, self.level, self.sublevel)
        }
    }

    /// The same level with a different sublevel selected.
    pub(crate) fn with_sublevel(&self, sublevel: u32) -> Self {
        Self {
            sublevel,
            ..self.clone()
        }
    }

    /// Advance to the next level, rolling worlds over past level 4.
    pub(crate) fn advance(&mut self) {
        self.level += 1;
        self.sublevel = 0;
        if self.level > 4 {
            self.level = 1;
            self.world += 1;
        }
    }

    /// Jump to the first level of `world`, as a warp pipe does.
    pub(crate) fn warp_to_world(&mut self, world: u32) {
        self.world = world;
        self.level = 1;
        self.sublevel = 0;
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
/// The world number a level's filename names, for the handful of rules that care.
///
/// Only Bowser does: he throws hammers from world 6 on (`bowser.lua:49`). `M-1`, the
/// minus world, has no numeric world and no Bowser, so it falls back to 1.
fn world_of(name: &str) -> u32 {
    name.split('-')
        .next()
        .and_then(|w| w.parse().ok())
        .unwrap_or(1)
}

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
        // The four `…half` ids differ from their plain form in **x and nothing else**:
        // `goomba:new(x-0.5, …)` against `goomba:new(x, …)` (`game.lua:3702-3719`).
        // "Half" is half a tile to the right — the editor's own description reads
        // "more to the right" (`entity.lua:196`) — and both forms start walking
        // *left*, because `speedx = -goombaspeed` is in the constructor with no
        // parameter to change it. Reading the distinction as a facing had 64 enemies
        // across the game marching away from the player instead of towards him.
        let half = matches!(
            spawn.kind,
            level::EntityKind::GoombaHalf
                | level::EntityKind::KoopaHalf
                | level::EntityKind::KoopaRedHalf
                | level::EntityKind::BeetleHalf
        );
        let px = spawn.x as f32 * TILE_SIZE + if half { TILE_SIZE / 2.0 } else { 0.0 };
        let py = spawn.y as f32 * TILE_SIZE;
        match spawn.kind {
            level::EntityKind::Goomba | level::EntityKind::GoombaHalf => {
                enemy_spawns.push(EnemySpawnPoint {
                    enemy_type: EnemyType::Goomba,
                    x: px,
                    y: py,
                    facing_right: false,
                    segment: 0,
                })
            }
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
            // Bowser. `segment` carries the world number, which is what decides
            // whether he throws hammers (`bowser.lua:49` — world 6 and up).
            level::EntityKind::Bowser => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Bowser,
                x: px,
                y: py,
                facing_right: false,
                segment: world_of(name),
            }),
            // A squid starts facing left and drifting down.
            level::EntityKind::Squid => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Squid,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            // A hammer bro starts shuffling left, and `spawn_x` becomes the anchor of
            // his one-block patrol.
            level::EntityKind::HammerBro => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::HammerBro,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            // Entity 60 is the *cannon*, not a bullet. Its own barrel and base are
            // tiles 42 and 64 in the level data, so it draws and collides as terrain;
            // what this spawns is only the timer that fires out of it.
            level::EntityKind::BulletBill => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::BulletBillCannon,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            // Lakitu starts drifting left, which is what sends him back over the
            // player the moment he's revealed.
            level::EntityKind::Lakito => enemy_spawns.push(EnemySpawnPoint {
                enemy_type: EnemyType::Lakito,
                x: px,
                y: py,
                facing_right: false,
                segment: 0,
            }),
            // `spikey`/`spikeyhalf` are deliberately absent: neither id appears in
            // any of the 73 shipped level files. A spiny only ever enters the world
            // as lakitu's ammunition (`lakito.lua:72`), so the walking form is
            // reached by a thrown egg landing, never by a spawn point.
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

    // Platforms: revealed by the same column sweep as the enemies, but kept in their
    // own list because what they become is a solid, not a creature.
    let mut platform_spawns: Vec<PlatformSpawnPoint> = Vec::new();
    for spawn in &parsed.markers.enemies {
        use crate::platform::PlatformKind;
        let kind = match spawn.kind {
            level::EntityKind::PlatformUp => PlatformKind::Vertical,
            level::EntityKind::PlatformRight => PlatformKind::Horizontal,
            level::EntityKind::PlatformFall => PlatformKind::Fall,
            level::EntityKind::PlatformBonus => PlatformKind::Bonus,
            _ => continue,
        };
        // The size argument is an index into a table of widths, not a width
        // (`entity.lua:209-231`). The bonus-stage platform ignores it and is always 3
        // (`game.lua:3749`).
        let size_blocks = if kind == PlatformKind::Bonus {
            3.0
        } else {
            platform_width(spawn.argf)
        };
        platform_spawns.push(PlatformSpawnPoint {
            cell: (spawn.x as i32, spawn.y as i32),
            kind,
            size_blocks,
        });
    }
    let mut platform_spawns_by_cell: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, p) in platform_spawns.iter().enumerate() {
        platform_spawns_by_cell.entry(p.cell).or_default().push(i);
    }
    // The shaft spawners exist from load, unlike everything else here.
    let platform_spawners: Vec<crate::platform::PlatformSpawner> = parsed
        .markers
        .platform_spawners
        .iter()
        .map(|(x, y, up, arg)| crate::platform::PlatformSpawner {
            cell: (*x as i32, *y as i32),
            up: *up,
            size_blocks: platform_width(*arg),
            timer: 0.0,
        })
        .collect();

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

    let mut spawns_by_cell: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, sp) in enemy_spawns.iter().enumerate() {
        spawns_by_cell.entry(sp.cell()).or_default().push(i);
    }

    let pipes = parsed
        .markers
        .pipes
        .iter()
        .map(|(x, y, dest)| ((*x as i32, *y as i32), *dest as u32))
        .collect();
    let warp_pipes = parsed
        .markers
        .warp_pipes
        .iter()
        .map(|(x, y, world)| ((*x as i32, *y as i32), *world as u32))
        .collect();
    // Keyed by sublevel, so a level with two exits from different sublevels keeps
    // both. Later entries win on a duplicate, which no shipped level has.
    let pipe_spawns = parsed
        .markers
        .pipe_spawns
        .iter()
        .map(|(x, y, from)| (*from as u32, (*x as i32, *y as i32)))
        .collect();
    let maze_gates: HashMap<(i32, i32), u32> = parsed
        .markers
        .maze_gates
        .iter()
        .map(|(x, y, gate)| ((*x as i32, *y as i32), *gate as u32))
        .collect();
    let maze_end_cols: HashSet<i32> = parsed.markers.maze_ends.iter().map(|c| *c as i32).collect();
    // Gate count per span: the highest gate number between its start and end,
    // never below 1 (`game.lua:2118-2131`).
    let maze_gate_counts: Vec<u32> = parsed
        .markers
        .maze_starts
        .iter()
        .zip(&parsed.markers.maze_ends)
        .map(|(start, end)| {
            maze_gates
                .iter()
                .filter(|((col, _), _)| *col >= *start as i32 && *col <= *end as i32)
                .map(|(_, gate)| *gate)
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();

    // Already sorted by the parser; the pass-detection walks them in order.
    let checkpoints = parsed
        .markers
        .checkpoints
        .iter()
        .map(|(x, y)| (*x as i32, *y as i32))
        .collect();

    Level {
        tiles,
        width,
        height,
        coins,
        enemy_spawns,
        spawns_by_cell,
        block_contents,
        multi_coin_timers: HashMap::new(),
        player_start,
        flag_x,
        time_limit: parsed.meta.timelimit as f32,
        background: parsed.meta.background,
        spriteset: parsed.meta.spriteset,
        music: parsed.meta.music,
        pipes,
        pipe_spawns,
        warp_pipes,
        checkpoints,
        intermission: parsed.meta.intermission,
        maze_starts: parsed
            .markers
            .maze_starts
            .iter()
            .map(|c| *c as i32)
            .collect(),
        maze_ends: parsed.markers.maze_ends.iter().map(|c| *c as i32).collect(),
        maze_gate_counts,
        maze_gates,
        maze_end_cols,
        platform_spawns,
        platform_spawns_by_cell,
        platform_spawners,
        springs: parsed
            .markers
            .springs
            .iter()
            .map(|(x, y)| (*x as i32, *y as i32))
            .collect(),
        spring_rects: Vec::new(),
        lakito_end: parsed.markers.lakito_end.map(|c| c as i32),
        bullet_bill_start: parsed.markers.bullet_bill_start.map(|c| c as i32),
        bullet_bill_end: parsed.markers.bullet_bill_end.map(|c| c as i32),
        axe: parsed.markers.axe,
        fire_start: parsed.markers.fire_start.map(|c| c as i32),
        flying_fish_start: parsed.markers.flying_fish_start.map(|c| c as i32),
        flying_fish_end: parsed.markers.flying_fish_end.map(|c| c as i32),
        portal_holes: HashSet::new(),
        solid_extras: HashSet::new(),
        solid_rects: Vec::new(),
        platform_rects: Vec::new(),
        gels: (0..height)
            .flat_map(|row| (0..width).map(move |col| (col, row)))
            .map(|(col, row)| parsed.gels(col, row))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sublevel_changes_only_the_filename() {
        let id = LevelId::new("smb", 1, 2);
        assert_eq!(id.name(), "1-2");
        assert_eq!(id.with_sublevel(1).name(), "1-2_1");
        assert_eq!(id.with_sublevel(3).name(), "1-2_3");
    }

    /// Advancing past level 4 rolls the world over and drops any sublevel.
    #[test]
    fn advancing_rolls_worlds_and_clears_the_sublevel() {
        let mut id = LevelId::new("smb", 1, 4).with_sublevel(2);
        id.advance();
        assert_eq!((id.world, id.level, id.sublevel), (2, 1, 0));
    }

    #[test]
    fn a_warp_lands_on_the_first_level_of_the_target_world() {
        let mut id = LevelId::new("smb", 1, 2).with_sublevel(1);
        id.warp_to_world(4);
        assert_eq!(id.name(), "4-1");
    }

    /// Every sublevel a pipe points at must exist, or the trip would strand the
    /// player. Checks the whole shipped mappack, both directions.
    #[test]
    fn every_pipe_destination_exists() {
        for (pack, name, _) in level::LEVELS {
            let lv = load_level(pack, name);
            // Level names are `W-L` or `W-L_N`; only the former can host pipes into
            // sublevels, and `LevelId` is what resolves the target.
            let Some((world, rest)) = name.split_once('-') else {
                continue;
            };
            let (level_num, _) = rest.split_once('_').unwrap_or((rest, ""));
            let (Ok(world), Ok(level_num)) = (world.parse::<u32>(), level_num.parse::<u32>())
            else {
                continue; // "M-1" and friends aren't numeric worlds
            };
            let id = LevelId::new(pack, world, level_num);
            for dest in lv.pipes.values() {
                let target = id.with_sublevel(*dest);
                assert!(
                    target.exists(),
                    "{pack}/{name}: pipe leads to {} which doesn't exist",
                    target.name()
                );
            }
        }
    }

    /// Checkpoints arrive sorted by column, which the pass-detector relies on to
    /// only ever look at the next one.
    #[test]
    fn checkpoints_are_sorted_by_column() {
        for (pack, name, _) in level::LEVELS {
            let lv = load_level(pack, name);
            let cols: Vec<i32> = lv.checkpoints.iter().map(|(c, _)| *c).collect();
            let mut sorted = cols.clone();
            sorted.sort_unstable();
            assert_eq!(cols, sorted, "{pack}/{name}: checkpoints out of order");
        }
    }

    /// A checkpoint must sit inside the level and above the floor, or respawning
    /// there would drop the player out of the world.
    #[test]
    fn checkpoints_are_inside_their_level() {
        for (pack, name, _) in level::LEVELS {
            let lv = load_level(pack, name);
            for (col, row) in &lv.checkpoints {
                assert!(
                    *col >= 0 && (*col as usize) < lv.width,
                    "{pack}/{name}: checkpoint column {col} outside width {}",
                    lv.width
                );
                assert!(
                    *row >= 0 && (*row as usize) <= lv.height,
                    "{pack}/{name}: checkpoint row {row} outside height {}",
                    lv.height
                );
            }
        }
    }

    /// The 21 levels that have one, have exactly one. Worth pinning: the respawn
    /// index logic is simple only because no shipped level has two.
    #[test]
    fn no_shipped_level_has_more_than_one_checkpoint() {
        let mut with_checkpoints = 0;
        for (pack, name, _) in level::LEVELS {
            let lv = load_level(pack, name);
            assert!(
                lv.checkpoints.len() <= 1,
                "{pack}/{name} has {} checkpoints",
                lv.checkpoints.len()
            );
            if !lv.checkpoints.is_empty() {
                with_checkpoints += 1;
            }
        }
        assert_eq!(with_checkpoints, 21, "expected 21 levels with a checkpoint");
    }

    /// The intermission stubs are the levels whose pipe sets a respawn sublevel.
    #[test]
    fn the_intermission_stubs_are_the_narrow_ones() {
        for name in ["1-2", "2-2", "4-2", "7-2"] {
            let lv = load_level("smb", name);
            assert!(lv.intermission, "{name} should be flagged intermission");
            assert_eq!(lv.width, 24, "{name} should be the 24-wide stub");
            assert!(!lv.pipes.is_empty(), "{name} should hold the pipe onward");
        }
    }
}
