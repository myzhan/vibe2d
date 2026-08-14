//! Visual Debug Protocol: the inspect snapshot and the mutating methods.
//!
//! `inspect_snapshot` is a curated, typed mirror of the game state — the field
//! names and nesting are the wire format the Python test scripts assert against,
//! so renaming one is a breaking change to `tests/`.

use crate::constants::*;
use crate::enemies::*;
use crate::game::{GameState, Mari0Game};
use crate::items::*;
use crate::lab::{LabKind, Signal};
use crate::music::MusicPhase;
use crate::pipe::PipeDir;
use crate::player::*;
use crate::portal::*;
use crate::world::*;

// ── VDP inspect snapshot views ──────────────────────────────────────
// Typed, `Serialize`-derived mirror of the curated `inspect()` payload.
// Field names / nesting reproduce the JSON shape exactly; enum → string
// mapping is handled by the `serde` derives on the enums themselves.

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct Mari0Inspect {
    pub(crate) state: GameState,
    pub(crate) player: PlayerView,
    pub(crate) portals: PortalsView,
    pub(crate) projectiles: Vec<ProjectileView>,
    pub(crate) crosshair_angle: f32,
    pub(crate) enemies: Vec<EnemyView>,
    pub(crate) coins: Vec<CoinView>,
    pub(crate) level: LevelView,
    pub(crate) camera_x: f32,
    pub(crate) score: u32,
    pub(crate) coin_count: u32,
    pub(crate) lives: u32,
    pub(crate) combo_index: usize,
    pub(crate) time_remaining: f32,
    /// Which stage of the low-time music sequence is active.
    pub(crate) music_phase: MusicPhase,
    /// Which way the player is moving through a pipe, or `null` when not in one.
    pub(crate) pipe: Option<PipeDir>,
    /// The scissor window Mario is drawn through while in a pipe, in screen space
    /// as `[x, y, w, h]`. `null` outside a pipe.
    ///
    /// Exposed because "is he actually hidden?" is otherwise only checkable by
    /// eyeballing a screenshot, and the geometry is what makes it work.
    pub(crate) pipe_clip: Option<[f32; 4]>,
    /// The checkpoint the player would respawn at, as `[column, row]`.
    pub(crate) checkpoint: Option<[i32; 2]>,
    /// Sublevel a death would reload; 0 is the main level.
    pub(crate) respawn_sublevel: u32,
    /// Maze progress, for the looping castles. `null` in levels without spans.
    pub(crate) maze: Option<MazeView>,
    /// The lab signal network. Empty outside the lab mappack.
    pub(crate) lab: Vec<LabElementView>,
    /// Solid boxes that aren't tiles: the light-bridge slabs, `[x, y, w, h]` in world
    /// pixels. This is the list the collision resolver reads, so a test can check
    /// "can he stand on it" against the same numbers the physics uses.
    pub(crate) solid_rects: Vec<[f32; 4]>,
    /// Weighted cubes, with the wiring each one answers to.
    pub(crate) cubes: Vec<CubeView>,
    pub(crate) items: Vec<ItemView>,
    pub(crate) block_contents: Vec<BlockContentView>,
    pub(crate) star_timer: f32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct PlayerView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) on_ground: bool,
    pub(crate) facing_right: bool,
    pub(crate) is_big: bool,
    pub(crate) is_fire: bool,
    pub(crate) is_jumping: bool,
    pub(crate) anim_state: PlayerAnim,
    pub(crate) portal_cooldown: f32,
    pub(crate) teleport_cooldown: f32,
    pub(crate) invincible_timer: f32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct PortalsView {
    pub(crate) blue: Option<PortalView>,
    pub(crate) orange: Option<PortalView>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct PortalView {
    /// Centre of the mouth, in world pixels. The wire format the Python scripts
    /// already assert on, so it stays even though the portal is stored as an anchor.
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// The anchor tile, normalised by face — see `PortalAnchor`.
    pub(crate) anchor: [i32; 2],
    /// The two cells the portal covers.
    pub(crate) cells: [[i32; 2]; 2],
    pub(crate) orientation: Orientation,
    pub(crate) active: bool,
}

#[cfg(feature = "vdp")]
impl PortalView {
    /// A portal slot only appears in `inspect` when it's present and active.
    pub(crate) fn from_slot(slot: &Option<Portal>) -> Option<PortalView> {
        match slot {
            Some(p) if p.active => {
                let (x, y) = p.centre();
                let cells = p.anchor.cells();
                Some(PortalView {
                    x,
                    y,
                    anchor: [p.anchor.cell.0, p.anchor.cell.1],
                    cells: [[cells[0].0, cells[0].1], [cells[1].0, cells[1].1]],
                    orientation: p.anchor.facing,
                    active: true,
                })
            }
            _ => None,
        }
    }
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct ProjectileView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) color: &'static str,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct EnemyView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    #[serde(rename = "type")]
    pub(crate) enemy_type: EnemyType,
    pub(crate) state: EnemyState,
    pub(crate) facing_right: bool,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct CoinView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) collected: bool,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct LabElementView {
    pub(crate) kind: LabKind,
    pub(crate) cell: [i32; 2],
    /// Index of the element driving this one, or `null` if unwired.
    pub(crate) driver: Option<usize>,
    pub(crate) on: bool,
    /// Door open fraction, 0..1 — or, for the elements that count instead, their
    /// countdown: a wall button's cooldown, a ground light's pulse, a timer's elapsed
    /// time.
    pub(crate) timer: f32,
    /// Timer only: how long it runs for, in seconds.
    pub(crate) duration: f32,
    /// True when the element is only in the graph so links resolve — its behaviour
    /// isn't implemented yet.
    pub(crate) inert: bool,
    /// Emitters only: the runs this element's beam makes. More than one means it is
    /// bending through a portal.
    pub(crate) beam: Vec<BeamView>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct CubeView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) held: bool,
    pub(crate) falling: bool,
    /// The `box` lab element this cube fills, or `null` for one nothing is wired to.
    pub(crate) slot: Option<usize>,
    /// The dispenser that will replace it if it is lost.
    pub(crate) dispenser: Option<usize>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct BeamView {
    pub(crate) dir: Orientation,
    pub(crate) cells: Vec<[i32; 2]>,
    /// The cell that stopped this run — probed for detectors even though the beam
    /// doesn't cover it. `null` when a body cut the beam short.
    pub(crate) end: Option<[i32; 2]>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct MazeView {
    /// Gate sequence progress. Reaching a span's `gate_counts` solves it.
    pub(crate) var: u32,
    pub(crate) solved: Vec<bool>,
    pub(crate) gate_counts: Vec<u32>,
    pub(crate) starts: Vec<i32>,
    pub(crate) ends: Vec<i32>,
    pub(crate) repeat_from: Option<i32>,
    pub(crate) in_progress: bool,
    pub(crate) last_repeat: i32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct LevelView {
    /// The loaded file's name, e.g. `1-2` or `1-2_1`. Lets a test see which
    /// sublevel a pipe led to.
    pub(crate) name: String,
    pub(crate) world: u32,
    pub(crate) level: u32,
    /// 0 for the main level.
    pub(crate) sublevel: u32,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) flag_x: f32,
    /// The level's `music` field verbatim (1 = silent … 7 = mappack-supplied).
    pub(crate) music: u8,
    /// The environment palette, so a test can tell a castle from an overworld.
    pub(crate) spriteset: u8,
    /// The level's checkpoint columns, so a test knows where to walk to.
    pub(crate) checkpoints: Vec<[i32; 2]>,
    pub(crate) intermission: bool,
    pub(crate) background: u8,
    pub(crate) time_limit: f32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct ItemView {
    #[serde(rename = "type")]
    pub(crate) item_type: ItemType,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) emerging: bool,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct BlockContentView {
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) content: &'static str,
}

/// Stable string tag for a block's content (drops `MultiCoin` payload, as the
/// original curated `inspect` did).
#[cfg(feature = "vdp")]
pub(crate) fn block_content_kind(c: &BlockContent) -> &'static str {
    match c {
        BlockContent::Coin => "coin",
        BlockContent::MultiCoin(_) => "multi_coin",
        BlockContent::Mushroom => "mushroom",
        BlockContent::Star => "star",
        BlockContent::OneUp => "1up",
    }
}

// Tile ids that exist only so `game.setTile` can take a readable name. The game
// loop never refers to these, so they live here and vanish with the feature.
const SMB_GROUND: u32 = 2;
const SMB_QUESTION_USED: u32 = 113;
const SMB_PIPE_TL: u32 = 16;
const SMB_PIPE_TR: u32 = 17;
const SMB_PIPE_BL: u32 = 38;
const SMB_PIPE_BR: u32 = 39;
const SMB_STAIRCASE: u32 = 78;

// ── VDP method params & dispatch ────────────────────────────────────
// Typed param structs (deserialized by `#[vdp_methods]`) replace the
// hand-rolled `params.get().and_then().ok_or()` extraction.

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetPlayerPos {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: Option<f32>,
    pub(crate) vy: Option<f32>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetPlayerSize {
    pub(crate) size: String,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetState {
    pub(crate) state: String,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetScore {
    pub(crate) score: Option<u32>,
    pub(crate) coins: Option<u32>,
    pub(crate) lives: Option<u32>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct LabSignal {
    pub(crate) index: usize,
    pub(crate) signal: String,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetTime {
    pub(crate) time: f32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetPortal {
    pub(crate) index: usize,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) orientation: String,
    pub(crate) active: Option<bool>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SpawnEnemy {
    #[serde(rename = "type")]
    pub(crate) etype: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    #[serde(default)]
    pub(crate) facing_right: bool,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetLevel {
    #[serde(default = "default_pack")]
    pub(crate) pack: String,
    pub(crate) world: u32,
    pub(crate) level: u32,
    /// 0 (the default) is the main level.
    #[serde(default)]
    pub(crate) sublevel: u32,
}

#[cfg(feature = "vdp")]
pub(crate) fn default_pack() -> String {
    START_PACK.to_string()
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetTile {
    pub(crate) col: i64,
    pub(crate) row: i64,
    #[serde(rename = "type")]
    pub(crate) tile_type: String,
}

#[cfg(feature = "vdp")]
impl Mari0Game {
    /// Curated snapshot of game state, in the exact shape `tests/` asserts on.
    pub(crate) fn inspect_snapshot(&self) -> serde_json::Value {
        let view = Mari0Inspect {
            state: self.state,
            player: PlayerView {
                x: self.player.x,
                y: self.player.y,
                vx: self.player.vx,
                vy: self.player.vy,
                width: self.player.width,
                height: self.player.height,
                on_ground: self.player.on_ground,
                facing_right: self.player.facing_right,
                is_big: self.player.is_big,
                is_fire: self.player.is_fire,
                is_jumping: self.player.is_jumping,
                anim_state: self.player.anim_state,
                portal_cooldown: self.player.portal_cooldown,
                teleport_cooldown: self.player.teleport_cooldown,
                invincible_timer: self.player.invincible_timer,
            },
            portals: PortalsView {
                blue: PortalView::from_slot(&self.portals[0]),
                orange: PortalView::from_slot(&self.portals[1]),
            },
            projectiles: self
                .projectiles
                .iter()
                .map(|p| ProjectileView {
                    x: p.x,
                    y: p.y,
                    vx: p.vx,
                    vy: p.vy,
                    color: if p.portal_index == 0 {
                        "blue"
                    } else {
                        "orange"
                    },
                })
                .collect(),
            crosshair_angle: self.crosshair_angle,
            enemies: self
                .enemies
                .iter()
                .map(|e| EnemyView {
                    x: e.x,
                    y: e.y,
                    enemy_type: e.enemy_type,
                    state: e.state,
                    facing_right: e.facing_right,
                })
                .collect(),
            coins: self
                .level
                .coins
                .iter()
                .map(|c| CoinView {
                    x: c.x,
                    y: c.y,
                    collected: c.collected,
                })
                .collect(),
            level: LevelView {
                name: self.current.name(),
                world: self.current.world,
                level: self.current.level,
                sublevel: self.current.sublevel,
                width: self.level.width,
                height: self.level.height,
                flag_x: self.level.flag_x,
                music: self.level.music,
                spriteset: self.level.spriteset,
                checkpoints: self
                    .level
                    .checkpoints
                    .iter()
                    .map(|(c, r)| [*c, *r])
                    .collect(),
                intermission: self.level.intermission,
                background: self.level.background,
                time_limit: self.level.time_limit,
            },
            camera_x: self.camera.x,
            score: self.score,
            coin_count: self.coins,
            lives: self.lives,
            combo_index: self.combo_index,
            time_remaining: self.time_remaining,
            music_phase: self.music_phase,
            pipe: self.pipe.as_ref().map(|p| p.dir),
            pipe_clip: self.pipe_clip_rect(self.camera.x, self.vw, self.vh),
            checkpoint: self.checkpoint.map(|(c, r)| [c, r]),
            respawn_sublevel: self.respawn_sublevel,
            lab: self
                .lab
                .elements
                .iter()
                .map(|e| LabElementView {
                    kind: e.kind,
                    cell: [e.cell.0, e.cell.1],
                    driver: e.driver,
                    on: e.on,
                    timer: e.timer,
                    duration: e.duration,
                    inert: e.kind.is_inert(),
                    beam: e
                        .beam
                        .iter()
                        .map(|s| BeamView {
                            dir: s.dir,
                            cells: s.cells.iter().map(|c| [c.0, c.1]).collect(),
                            end: s.end.map(|c| [c.0, c.1]),
                        })
                        .collect(),
                })
                .collect(),
            solid_rects: self.level.solid_rects.iter().map(|s| s.rect).collect(),
            cubes: self
                .cubes
                .iter()
                .map(|c| CubeView {
                    x: c.x,
                    y: c.y,
                    vx: c.vx,
                    vy: c.vy,
                    held: c.held,
                    falling: c.falling,
                    slot: c.slot,
                    dispenser: c.dispenser,
                })
                .collect(),
            maze: (!self.level.maze_starts.is_empty()).then(|| MazeView {
                var: self.maze.var,
                solved: self.maze.solved.clone(),
                gate_counts: self.level.maze_gate_counts.clone(),
                starts: self.level.maze_starts.clone(),
                ends: self.level.maze_ends.clone(),
                repeat_from: self.maze.repeat_from,
                in_progress: self.maze.in_progress,
                last_repeat: self.maze.last_repeat,
            }),
            items: self
                .items
                .iter()
                .map(|item| ItemView {
                    item_type: item.item_type,
                    x: item.x,
                    y: item.y,
                    vx: item.vx,
                    vy: item.vy,
                    emerging: item.emerging,
                })
                .collect(),
            block_contents: self
                .level
                .block_contents
                .iter()
                .map(|((row, col), content)| BlockContentView {
                    row: *row,
                    col: *col,
                    x: *col as f32 * TILE_SIZE,
                    y: *row as f32 * TILE_SIZE,
                    content: block_content_kind(content),
                })
                .collect(),
            star_timer: self.star_timer,
        };
        serde_json::to_value(&view).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(feature = "vdp")]
#[vibe2d::vdp::vdp_methods]
impl Mari0Game {
    #[vdp("game.reset")]
    pub(crate) fn vdp_reset(&mut self) -> Result<serde_json::Value, String> {
        self.state = GameState::Playing;
        self.start_fresh();
        self.score = 0;
        self.coins = 0;
        self.lives = 3;
        Ok(serde_json::json!({"status": "ok"}))
    }

    #[vdp("game.setPlayerPos")]
    pub(crate) fn vdp_set_player_pos(
        &mut self,
        p: SetPlayerPos,
    ) -> Result<serde_json::Value, String> {
        self.player.x = p.x;
        self.player.y = p.y;
        if let Some(vx) = p.vx {
            self.player.vx = vx;
        }
        if let Some(vy) = p.vy {
            self.player.vy = vy;
        }
        Ok(serde_json::json!({"x": self.player.x, "y": self.player.y,
            "vx": self.player.vx, "vy": self.player.vy}))
    }

    #[vdp("game.setPlayerSize")]
    pub(crate) fn vdp_set_player_size(
        &mut self,
        p: SetPlayerSize,
    ) -> Result<serde_json::Value, String> {
        match p.size.as_str() {
            "big" => self.player.set_size(true),
            "small" => self.player.set_size(false),
            _ => return Err(format!("Unknown size: {}", p.size)),
        }
        Ok(serde_json::json!({"is_big": self.player.is_big}))
    }

    #[vdp("game.setState")]
    pub(crate) fn vdp_set_state(&mut self, p: SetState) -> Result<serde_json::Value, String> {
        match p.state.as_str() {
            "menu" => self.state = GameState::Menu,
            "playing" => self.state = GameState::Playing,
            "dead" => self.state = GameState::Dead,
            "level_complete" => self.state = GameState::LevelComplete,
            _ => return Err(format!("Unknown state: {}", p.state)),
        }
        Ok(serde_json::json!({"state": p.state}))
    }

    /// Set the level clock directly.
    ///
    /// Exists so a test can reach the low-time music sequence without stepping
    /// 7200 frames to burn 300 time units off a 400-unit level. Setting the clock
    /// *above* the 99 threshold also rewinds the music phase, so a test can run
    /// the warning → fast transition more than once.
    #[vdp("game.setTime")]
    pub(crate) fn vdp_set_time(&mut self, p: SetTime) -> Result<serde_json::Value, String> {
        if p.time < 0.0 {
            return Err(format!("time must not be negative, got {}", p.time));
        }
        self.time_remaining = p.time;
        if p.time > crate::music::LOW_TIME {
            self.music_phase = MusicPhase::Normal;
            self.warning_started_at = None;
        }
        Ok(serde_json::json!({
            "time_remaining": self.time_remaining,
            "music_phase": self.music_phase,
        }))
    }

    /// Send a signal into the lab network, as an upstream output would.
    #[vdp("game.labSignal")]
    pub(crate) fn vdp_lab_signal(&mut self, p: LabSignal) -> Result<serde_json::Value, String> {
        if p.index >= self.lab.elements.len() {
            return Err(format!(
                "no lab element {} (level has {})",
                p.index,
                self.lab.elements.len()
            ));
        }
        let signal = match p.signal.as_str() {
            "on" => Signal::On,
            "off" => Signal::Off,
            "toggle" => Signal::Toggle,
            other => return Err(format!("unknown signal: {other}")),
        };
        self.lab.signal(p.index, signal);
        Ok(serde_json::json!({"index": p.index, "on": self.lab.elements[p.index].on}))
    }

    #[vdp("game.setScore")]
    pub(crate) fn vdp_set_score(&mut self, p: SetScore) -> Result<serde_json::Value, String> {
        if let Some(s) = p.score {
            self.score = s;
        }
        if let Some(c) = p.coins {
            self.coins = c;
        }
        if let Some(l) = p.lives {
            self.lives = l;
        }
        Ok(serde_json::json!({"score": self.score, "coins": self.coins, "lives": self.lives}))
    }

    #[vdp("game.setPortal")]
    pub(crate) fn vdp_set_portal(&mut self, p: SetPortal) -> Result<serde_json::Value, String> {
        if p.index > 1 {
            return Err("index must be 0 or 1".into());
        }
        let orientation = match p.orientation.as_str() {
            "up" => Orientation::Up,
            "down" => Orientation::Down,
            "left" => Orientation::Left,
            "right" => Orientation::Right,
            _ => return Err(format!("Unknown orientation: {}", p.orientation)),
        };
        let active = p.active.unwrap_or(true);
        // `x`/`y` are the mouth centre, which is what this method has always taken.
        self.portals[p.index] = Some(Portal {
            anchor: crate::portal_math::PortalAnchor::from_mouth_centre(p.x, p.y, orientation),
            active,
            open_scale: 1.0,
        });
        self.refresh_portal_holes();
        Ok(serde_json::json!({"index": p.index, "x": p.x, "y": p.y,
            "orientation": p.orientation, "active": active}))
    }

    #[vdp("game.clearPortals")]
    pub(crate) fn vdp_clear_portals(&mut self) -> Result<serde_json::Value, String> {
        self.portals = [None, None];
        self.refresh_portal_holes();
        Ok(serde_json::json!({"status": "ok"}))
    }

    #[vdp("game.spawnEnemy")]
    pub(crate) fn vdp_spawn_enemy(&mut self, p: SpawnEnemy) -> Result<serde_json::Value, String> {
        let etype = match p.etype.as_str() {
            "goomba" => EnemyType::Goomba,
            "koopa" => EnemyType::Koopa,
            "koopa_red" => EnemyType::KoopaRed,
            "beetle" => EnemyType::Beetle,
            "plant" => EnemyType::Plant,
            _ => return Err(format!("Unknown enemy type: {}", p.etype)),
        };
        self.enemies.push(Enemy {
            x: p.x,
            y: p.y,
            vx: if p.facing_right {
                ENEMY_SPEED
            } else {
                -ENEMY_SPEED
            },
            vy: 0.0,
            enemy_type: etype,
            state: EnemyState::Walking,
            facing_right: p.facing_right,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: p.y,
            cycle_timer: 0.0,
            spawn_x: p.x,
            angle_deg: 0.0,
            segment: 0,
        });
        Ok(serde_json::json!({"status": "ok", "enemy_count": self.enemies.len()}))
    }

    #[vdp("game.clearEnemies")]
    pub(crate) fn vdp_clear_enemies(&mut self) -> Result<serde_json::Value, String> {
        self.enemies.clear();
        Ok(serde_json::json!({"status": "ok"}))
    }

    /// Load any level by world/level number.
    ///
    /// The point of the whole data-layer rewrite: this used to be impossible
    /// because the loader was compiled against 1-1.
    #[vdp("game.setLevel")]
    pub(crate) fn vdp_set_level(&mut self, p: SetLevel) -> Result<serde_json::Value, String> {
        let id = LevelId::new(&p.pack, p.world, p.level).with_sublevel(p.sublevel);
        if !id.exists() {
            return Err(format!("no such level {}/{}", p.pack, id.name()));
        }
        self.current = id;
        self.start_fresh();
        self.state = GameState::Playing;
        Ok(serde_json::json!({
            "pack": self.current.pack,
            "level": self.current.name(),
            "sublevel": self.current.sublevel,
            "width": self.level.width,
            "enemies": self.enemies.len(),
        }))
    }

    /// Advance to the next level exactly as finishing one would.
    #[vdp("game.nextLevel")]
    pub(crate) fn vdp_next_level(&mut self) -> Result<serde_json::Value, String> {
        self.advance_level();
        Ok(serde_json::json!({
            "pack": self.current.pack,
            "level": self.current.name(),
            "sublevel": self.current.sublevel,
            "state": format!("{:?}", self.state),
        }))
    }

    #[vdp("game.setTile")]
    pub(crate) fn vdp_set_tile(&mut self, p: SetTile) -> Result<serde_json::Value, String> {
        let col = p.col as usize;
        let row = p.row as usize;
        if row >= self.level.height || col >= self.level.width {
            return Err("Tile position out of bounds".into());
        }
        let tile_id: u32 = match p.tile_type.as_str() {
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
                p.tile_type
                    .parse::<u32>()
                    .map_err(|_| format!("Unknown tile type: {}", p.tile_type))?
            }
        };
        self.level.tiles[row][col] = tile_id;
        Ok(serde_json::json!({"col": col, "row": row, "type": p.tile_type}))
    }
}

#[cfg(feature = "vdp")]
impl Mari0Game {
    /// Route one VDP call. Wraps the macro-generated `dispatch_vdp`, which is
    /// private to this module.
    pub(crate) fn handle_vdp_method(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.dispatch_vdp(method, params)
            .unwrap_or_else(|| Err(format!("Unknown method: {}", method)))
    }
}
