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
    /// Is the update frozen? The original's pause menu amounts to the same thing.
    pub(crate) paused: bool,
    /// Which loadout the mouse buttons carry: `portal` or `gel_cannon`.
    pub(crate) player_type: PlayerType,
    /// The launch intro, if it is running: seconds in, the logo's opacity, and how far the
    /// blood has wiped up it.
    pub(crate) intro: Option<IntroView>,
    /// Is the rainboom easter egg on, and how much shake is left?
    pub(crate) sonic_rainboom: bool,
    pub(crate) earthquake: f32,
    pub(crate) rainbooms: usize,
    /// Has the warp-zone text been revealed? Only ever true in a `haswarpzone` level.
    pub(crate) warp_text: bool,
    /// The black card being held between levels, or `null`.
    pub(crate) interlude: Option<InterludeView>,
    /// Seconds into the death throw, or `null`.
    pub(crate) death_timer: Option<f32>,
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
    /// Has the player passed `lakitoend`? Once true lakitu stops throwing and leaves.
    pub(crate) lakito_retired: bool,
    /// Is the `bulletbillstart`…`bulletbillend` stretch currently raining bills?
    pub(crate) bullet_bill_zone: bool,
    /// Is the `flyingfishstart`…`flyingfishend` stretch currently throwing fish?
    pub(crate) flying_fish_zone: bool,
    /// Has `firestart` been passed? One-way, so once true it stays true.
    pub(crate) fire_started: bool,
    /// The axe ending, if it is running: which beat and how far into it.
    pub(crate) castle: Option<CastleView>,
    /// The flagpole ending, if it is running.
    pub(crate) flag: Option<FlagView>,
    /// Firework bursts on screen right now, `[x, y]` in world pixels.
    pub(crate) fireworks_shown: Vec<[f32; 2]>,
    /// Moving platforms in the world right now.
    pub(crate) platforms: Vec<PlatformView>,
    /// Springs, with their live (compressing) collision boxes.
    pub(crate) springs: Vec<SpringView>,
    /// Is Mario mid-launch on a spring, and has he charged it?
    pub(crate) spring_ride: Option<SpringRideView>,
    /// Seesaw rigs. Nine in the game, over three levels.
    pub(crate) seesaws: Vec<SeesawView>,
    /// Bubbles Mario has breathed out, `[x, y]` in world pixels. Water levels only.
    pub(crate) bubbles: Vec<[f32; 2]>,
    /// Vines growing in the world. Empty until a vine block is hit — or already
    /// populated on frame one, in a `bonusstage`.
    pub(crate) vines: Vec<VineView>,
    /// What a vine is doing to Mario, or `null`.
    pub(crate) vine: Option<VineStateView>,
    /// Is this a coin room reached by vine? Changes what a pit does.
    pub(crate) bonusstage: bool,
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
    /// Emancipation grills, resolved to the spans they cover.
    pub(crate) grills: Vec<GrillView>,
    /// Gel blobs still in the air.
    pub(crate) gel_blobs: Vec<GelBlobView>,
    /// Every cell with paint on it, and which faces. Sparse — most cells have none.
    pub(crate) gels: Vec<GelPaintView>,
    pub(crate) items: Vec<ItemView>,
    pub(crate) block_contents: Vec<BlockContentView>,
    pub(crate) star_timer: f32,
    /// Which of the four star palettes is showing. Exposed because the flashing is the only
    /// sign a star is running, and its *rate* is the only sign it is about to stop.
    pub(crate) star_color_index: usize,
    /// A size change in progress: which way it is going and how far in. `null` otherwise.
    ///
    /// Exposed because the freeze it causes is otherwise indistinguishable from a hang —
    /// every other field in the snapshot stops moving for its whole duration.
    pub(crate) transform: Option<TransformView>,
    /// Fireworks the last flagpole earned.
    pub(crate) fireworks: u32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct TransformView {
    pub(crate) kind: crate::player::TransformKind,
    pub(crate) timer: f32,
    /// Which of the three flip frames is showing, 1..=3.
    pub(crate) frame: u32,
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
    /// Is a big Mario crouched? His box is halved while so.
    pub(crate) ducking: bool,
    pub(crate) portal_cooldown: f32,
    pub(crate) teleport_cooldown: f32,
    pub(crate) invincible_timer: f32,
    /// The hats being worn *this life*, bottom first, as 1-based indices into `hats.rs`.
    pub(crate) hats: Vec<u8>,
    /// The menu's hat pick, which a death restores `hats` from.
    pub(crate) hat_selection: Vec<u8>,
    /// Where the hat sits for the pose Mario is in this frame, as `[x, y]` in the
    /// original's unscaled pixels.
    ///
    /// Exposed because the hat is pure decoration: nothing else in the snapshot moves
    /// when it does, so a screenshot would otherwise be the only way to tell that
    /// climbing shifts it two pixels right — or that falling reads the running table.
    pub(crate) hat_offset: [f32; 2],
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
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    #[serde(rename = "type")]
    pub(crate) enemy_type: EnemyType,
    pub(crate) state: EnemyState,
    pub(crate) facing_right: bool,
    /// Seconds left before a dead enemy is removed — or, for lakitu, before he
    /// returns.
    pub(crate) death_timer: f32,
    /// Whatever cycle this kind runs on: a plant's emerge/retract position, a
    /// firebar's tick accumulator, lakitu's countdown to the next egg.
    pub(crate) cycle_timer: f32,
    /// Squid only: which beat of its three-part cycle it is on.
    pub(crate) squid_phase: SquidPhase,
    /// Bowser only: fireball hits left, and whether the player has got behind him.
    pub(crate) hp: u32,
    pub(crate) backing_off: bool,
    /// How far up `KOOPA_COMBO` a sliding shell's own kill chain has climbed. Its own
    /// counter, not Mario's stomp combo, and stopping the shell resets it.
    pub(crate) shell_combo: usize,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct PlatformView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    #[serde(rename = "type")]
    pub(crate) kind: crate::platform::PlatformKind,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct CastleView {
    pub(crate) phase: crate::castle::CastlePhase,
    pub(crate) timer: f32,
    /// The next bridge cell the sweep will take, as `[col, row]`.
    pub(crate) bridge: [i32; 2],
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct SpringView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) frame: usize,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct SpringRideView {
    pub(crate) timer: f32,
    pub(crate) charged: bool,
}

/// The launch intro.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct IntroView {
    /// Starts negative: there is a beat of black before the fade begins.
    pub(crate) timer: f32,
    pub(crate) alpha: f32,
    pub(crate) blood_wipe: f32,
    pub(crate) stabbed: bool,
}

/// A black card between levels.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct InterludeView {
    pub(crate) kind: crate::interlude::InterludeKind,
    pub(crate) timer: f32,
    /// How long this card lasts — not a constant, since the first level of a world holds
    /// 50% longer.
    pub(crate) total: f32,
    /// Is the text showing? False during the lead-in and lead-out, and *always* false for
    /// the sublevel blink, which is exactly two lead-ins long.
    pub(crate) text_visible: bool,
}

/// The flagpole ending: which beat, and the two things that move during it.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct FlagView {
    pub(crate) phase: crate::flagpole::FlagPhase,
    /// Seconds since the pole was grabbed. Never reset, so `CASTLE_MIN_TIME` is a floor
    /// on the whole sequence rather than a per-beat delay.
    pub(crate) timer: f32,
    /// The flag sprite's height, which descends with Mario.
    pub(crate) flag_y: f32,
    /// The castle flag's remaining offset above its final position; 0 once it is up.
    pub(crate) castle_flag_y: f32,
    /// How many fireworks have gone off, and how many there will be.
    pub(crate) fired: u32,
    pub(crate) total: u32,
}

/// One end of a seesaw: the box you stand on, its speed, and how many riders it counted.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct SeesawPlatformView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) vy: f32,
    /// Riders counted last frame — the figure the rig is actually acting on.
    pub(crate) riders: u32,
    /// How far below the beam it hangs, in pixels. Also the length of rope drawn.
    pub(crate) drop: f32,
    pub(crate) gone: bool,
}

/// A whole rig. `rope` is the pair's shared length: the two `drop`s always sum to it.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct SeesawView {
    pub(crate) col: i32,
    pub(crate) row: i32,
    /// Which of the nine `seesawtype` entries this is, 1-based.
    pub(crate) kind: u16,
    pub(crate) range: f32,
    pub(crate) dist1: f32,
    pub(crate) dist2: f32,
    pub(crate) rope: f32,
    pub(crate) anchor_y: f32,
    pub(crate) left: SeesawPlatformView,
    pub(crate) right: SeesawPlatformView,
    /// Which side's rope gave, or `null` while the rig is intact.
    pub(crate) falloff: Option<crate::seesaw::SeesawSide>,
}

/// A vine, with the box you can actually hold and the scissor it is drawn through.
///
/// `clip_bottom` is exposed for the same reason `pipe_clip` is: whether the tip is
/// hidden inside its brick is otherwise only checkable by eyeballing a screenshot.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct VineView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// How far up it still has to grow, and whether it is done.
    pub(crate) limit: f32,
    pub(crate) grown: bool,
    /// Sublevel the top of it leads to. 0 for the bonus-stage intro vine, which
    /// leads nowhere — you have already arrived.
    pub(crate) dest: u32,
    pub(crate) clip_bottom: f32,
    pub(crate) stems: i32,
}

/// What the vine is doing to Mario, flattened so a test can read one field.
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct VineStateView {
    /// `grip`, `leaving` or `intro`.
    pub(crate) phase: &'static str,
    /// Which side he is hanging on. Only set while gripping.
    pub(crate) side: Option<crate::vine::VineSide>,
    /// Which of the two climbing frames is showing, 1 or 2.
    pub(crate) climb_frame: u32,
    /// Does he still have the controls? False for both cut-scenes, which is also what
    /// decides whether the clock is running.
    pub(crate) has_control: bool,
    /// Sublevel a `leaving` trip is bound for.
    pub(crate) dest: Option<u32>,
    /// Seconds into the bonus-stage intro, and which of its three beats it is on.
    pub(crate) intro_timer: Option<f32>,
    pub(crate) intro_climbing: Option<bool>,
    pub(crate) intro_dropping_off: Option<bool>,
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
pub(crate) struct GrillView {
    pub(crate) cell: [i32; 2],
    pub(crate) horizontal: bool,
    /// First and last cell of the span, along the grill's own axis.
    pub(crate) start: i32,
    pub(crate) end: i32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct GelBlobView {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) gel: crate::level::Gel,
}

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
pub(crate) struct GelPaintView {
    pub(crate) cell: [i32; 2],
    pub(crate) top: Option<crate::level::Gel>,
    pub(crate) bottom: Option<crate::level::Gel>,
    pub(crate) left: Option<crate::level::Gel>,
    pub(crate) right: Option<crate::level::Gel>,
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
    /// Swaps the player's whole movement model for the swimming one.
    pub(crate) underwater: bool,
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
        BlockContent::Vine(_) => "vine",
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
pub(crate) struct SetLives {
    pub(crate) lives: u32,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetRainboom {
    pub(crate) on: bool,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetHats {
    /// 1-based indices into `hats.rs`'s table, bottom of the stack first. Empty is
    /// bare-headed.
    pub(crate) hats: Vec<u8>,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetPlayerType {
    pub(crate) player_type: String,
}

#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
pub(crate) struct SetStar {
    pub(crate) seconds: f32,
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
            paused: self.paused,
            player_type: self.player_type,
            intro: self.intro.map(|i| IntroView {
                timer: i.timer,
                alpha: i.alpha(),
                blood_wipe: i.blood_wipe(),
                stabbed: i.stabbed,
            }),
            sonic_rainboom: self.sonic_rainboom,
            earthquake: self.earthquake,
            rainbooms: self.rainbooms.len(),
            warp_text: self.warp_text,
            interlude: self.interlude.map(|c| InterludeView {
                kind: c.kind,
                timer: c.timer,
                total: c.total,
                text_visible: c.text_visible(),
            }),
            death_timer: self.death.map(|d| d.timer),
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
                ducking: self.player.ducking,
                portal_cooldown: self.player.portal_cooldown,
                teleport_cooldown: self.player.teleport_cooldown,
                invincible_timer: self.player.invincible_timer,
                hats: self.hats.clone(),
                hat_selection: self.hat_selection.clone(),
                hat_offset: {
                    let (x, y) = crate::hats::hat_offset(
                        self.player.is_big,
                        self.player.anim_state,
                        self.player.run_frame as u32,
                        self.player.climb_frame,
                        self.player.swim_phase.floor() as u32,
                    );
                    [x, y]
                },
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
                    vx: e.vx,
                    vy: e.vy,
                    enemy_type: e.enemy_type,
                    state: e.state,
                    facing_right: e.facing_right,
                    death_timer: e.death_timer,
                    cycle_timer: e.cycle_timer,
                    squid_phase: e.squid_phase,
                    hp: e.hp,
                    backing_off: e.backing_off,
                    shell_combo: e.shell_combo,
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
                underwater: self.level.underwater,
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
            lakito_retired: self.lakito_retired,
            bullet_bill_zone: self.bullet_bill_zone,
            flying_fish_zone: self.flying_fish_zone,
            fire_started: self.fire_started,
            castle: self.castle.map(|c| CastleView {
                phase: c.phase,
                timer: c.timer,
                bridge: [c.bridge.0, c.bridge.1],
            }),
            springs: self
                .springs
                .iter()
                .map(|s| {
                    let [x, y, w, h] = s.rect();
                    SpringView {
                        x,
                        y,
                        w,
                        h,
                        frame: s.frame(),
                    }
                })
                .collect(),
            spring_ride: self.spring_ride.map(|r| SpringRideView {
                timer: r.timer,
                charged: r.charged,
            }),
            flag: self.flag.map(|f| FlagView {
                phase: f.phase,
                timer: f.timer,
                flag_y: f.flag_y,
                castle_flag_y: f.castle_flag_y,
                fired: f.fired,
                total: f.total,
            }),
            fireworks_shown: self.fireworks_shown.iter().map(|f| [f.x, f.y]).collect(),
            bubbles: self.bubbles.iter().map(|b| [b.x, b.y]).collect(),
            seesaws: self
                .seesaws
                .iter()
                .map(|s| {
                    let view = |side| {
                        let p = s.platform(side);
                        let [x, y, w, h] = p.rect();
                        SeesawPlatformView {
                            x,
                            y,
                            w,
                            h,
                            vy: p.vy,
                            riders: p.riders,
                            drop: s.drop_of(side),
                            gone: p.gone,
                        }
                    };
                    SeesawView {
                        col: s.col,
                        row: s.row,
                        kind: s.kind,
                        range: s.range,
                        dist1: s.dist1,
                        dist2: s.dist2,
                        rope: s.rope(),
                        anchor_y: s.anchor_y(),
                        left: view(crate::seesaw::SeesawSide::Left),
                        right: view(crate::seesaw::SeesawSide::Right),
                        falloff: s.falloff,
                    }
                })
                .collect(),
            vines: self
                .vines
                .iter()
                .map(|v| {
                    let [x, y, w, h] = v.rect();
                    VineView {
                        x,
                        y,
                        w,
                        h,
                        limit: v.limit,
                        grown: v.grown(),
                        dest: v.dest,
                        clip_bottom: v.clip_bottom(),
                        stems: v.stem_count(),
                    }
                })
                .collect(),
            vine: self.vine.map(|s| {
                let (phase, side, dest, intro) = match s {
                    crate::vine::VineState::Grip { side, .. } => {
                        ("grip", Some(side), None, None)
                    }
                    crate::vine::VineState::Leaving { dest, .. } => {
                        ("leaving", None, Some(dest), None)
                    }
                    crate::vine::VineState::Intro {
                        timer,
                        climbing,
                        dropping_off,
                        ..
                    } => ("intro", None, None, Some((timer, climbing, dropping_off))),
                };
                VineStateView {
                    phase,
                    side,
                    climb_frame: self.player.climb_frame,
                    has_control: self.vine_has_control(),
                    dest,
                    intro_timer: intro.map(|i| i.0),
                    intro_climbing: intro.map(|i| i.1),
                    intro_dropping_off: intro.map(|i| i.2),
                }
            }),
            bonusstage: self.level.bonusstage,
            platforms: self
                .platforms
                .iter()
                .map(|p| PlatformView {
                    x: p.x,
                    y: p.y,
                    w: p.w,
                    h: PLATFORM_HEIGHT,
                    kind: p.kind,
                    vx: p.vx,
                    vy: p.vy,
                })
                .collect(),
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
            grills: self
                .grills
                .iter()
                .map(|g| GrillView {
                    cell: [g.cell.0, g.cell.1],
                    horizontal: g.horizontal,
                    start: g.start,
                    end: g.end,
                })
                .collect(),
            gel_blobs: self
                .gel_blobs
                .iter()
                .map(|b| GelBlobView {
                    x: b.x,
                    y: b.y,
                    vx: b.vx,
                    vy: b.vy,
                    gel: b.gel,
                })
                .collect(),
            gels: (0..self.level.height as i32)
                .flat_map(|row| (0..self.level.width as i32).map(move |col| (col, row)))
                .filter_map(|cell| {
                    let g = self.level.gels(cell);
                    let bare = g.top.is_none()
                        && g.bottom.is_none()
                        && g.left.is_none()
                        && g.right.is_none();
                    (!bare).then_some(GelPaintView {
                        cell: [cell.0, cell.1],
                        top: g.top,
                        bottom: g.bottom,
                        left: g.left,
                        right: g.right,
                    })
                })
                .collect(),
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
            star_color_index: self.star_color_index,
            transform: self.transform.as_ref().map(|t| TransformView {
                kind: t.kind,
                timer: t.timer,
                frame: t.frame(),
            }),
            fireworks: self.fireworks,
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

    /// Switch loadout. In the game this is a menu toggle; a test needs it directly.
    #[vdp("game.setPlayerType")]
    pub(crate) fn vdp_set_player_type(
        &mut self,
        p: SetPlayerType,
    ) -> Result<serde_json::Value, String> {
        self.player_type = match p.player_type.as_str() {
            "portal" => PlayerType::Portal,
            "gel_cannon" | "gelcannon" => PlayerType::GelCannon,
            other => return Err(format!("Unknown player type: {other}")),
        };
        Ok(serde_json::json!({"player_type": p.player_type}))
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

    /// Hand the player star invincibility for `seconds`.
    ///
    /// The probe primitive for hazards. Measuring anything that hurts — bullet bills
    /// raining down a corridor, a hammer bro's arc, Bowser's breath — otherwise ends
    /// the same way: the player dies, `update_playing` stops, the whole scene freezes,
    /// and every later assertion reads as "the feature does nothing" rather than "the
    /// probe stood in the line of fire".
    #[vdp("game.setStar")]
    pub(crate) fn vdp_set_star(&mut self, p: SetStar) -> Result<serde_json::Value, String> {
        self.star_timer = p.seconds.max(0.0);
        Ok(serde_json::json!({"star_timer": self.star_timer}))
    }

    #[vdp("game.setState")]
    pub(crate) fn vdp_set_state(&mut self, p: SetState) -> Result<serde_json::Value, String> {
        match p.state.as_str() {
            "menu" => self.state = GameState::Menu,
            "playing" => self.state = GameState::Playing,
            // Starts the throw too, not just the state — see `start_death`.
            "dead" => self.start_death(false),
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

    /// Put a stack of hats on Mario. In the game the menu only ever sets one; the stack
    /// is what the original's skin editor could build, and the draw path still honours it.
    ///
    /// Sets the selection as well as what he has on, so the choice survives a death — a
    /// test that wants the two to differ should use the rainboom, which is the only thing
    /// in the game that touches one without the other.
    #[vdp("game.setHats")]
    pub(crate) fn vdp_set_hats(&mut self, p: SetHats) -> Result<serde_json::Value, String> {
        if let Some(bad) = p
            .hats
            .iter()
            .find(|&&i| i == 0 || i as usize > crate::hats::HATS.len())
        {
            return Err(format!(
                "hat {bad} out of range: 1..={}",
                crate::hats::HATS.len()
            ));
        }
        self.hat_selection = p.hats.clone();
        self.hats = p.hats;
        Ok(serde_json::json!({"hats": self.hats}))
    }

    /// Switch the rainboom easter egg on or off.
    #[vdp("game.setRainboom")]
    pub(crate) fn vdp_set_rainboom(&mut self, p: SetRainboom) -> Result<serde_json::Value, String> {
        self.sonic_rainboom = p.on;
        Ok(serde_json::json!({"sonic_rainboom": self.sonic_rainboom}))
    }

    /// Replay the launch intro from the top.
    ///
    /// It only ever runs once per session, so without this there is no way to look at it
    /// again — or to assert anything about it.
    #[vdp("game.playIntro")]
    pub(crate) fn vdp_play_intro(&mut self) -> Result<serde_json::Value, String> {
        self.intro = Some(crate::interlude::Intro {
            timer: INTRO_START,
            stabbed: false,
        });
        Ok(serde_json::json!({"status": "ok"}))
    }

    /// Set the life count without touching anything else.
    ///
    /// `game.reset` is the only other way to restore lives, and it also clears the score,
    /// the checkpoint and the level — which is exactly what a test about checkpoints
    /// cannot afford. Death now costs a life even when triggered through
    /// `setState("dead")`, so a script that kills the player repeatedly runs out and every
    /// later assertion silently measures a game over instead.
    #[vdp("game.setLives")]
    pub(crate) fn vdp_set_lives(&mut self, p: SetLives) -> Result<serde_json::Value, String> {
        self.lives = p.lives;
        Ok(serde_json::json!({"lives": self.lives}))
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
            "lakito" => EnemyType::Lakito,
            "bullet_bill" => EnemyType::BulletBill,
            "hammer_bro" => EnemyType::HammerBro,
            "squid" => EnemyType::Squid,
            "bowser" => EnemyType::Bowser,
            "fire" => EnemyType::Fire,
            "flying_fish" => EnemyType::FlyingFish,
            "hammer" => EnemyType::Hammer,
            "bullet_bill_cannon" => EnemyType::BulletBillCannon,
            "spikey" => EnemyType::Spikey,
            "spikey_fall" => EnemyType::SpikeyFall,
            _ => return Err(format!("Unknown enemy type: {}", p.etype)),
        };
        // A bill needs its own constructor: the generic one below hands out the
        // walking speed of 2 blocks/s, and a bullet bill that crawls is not a bullet
        // bill. Its age is also load-bearing, since that's what expires it.
        // Bowser carries state the generic constructor below knows nothing about: five
        // hit points, the leg of his pace, and the world number that decides whether he
        // throws hammers. Spawned without them he is a Bowser with zero HP.
        if etype == EnemyType::Bowser {
            let mut b = Enemy::from_spawn(&crate::world::EnemySpawnPoint {
                enemy_type: EnemyType::Bowser,
                x: p.x,
                y: p.y + BOWSER_H,
                facing_right: p.facing_right,
                segment: self.current.world,
            });
            b.y = p.y;
            self.enemies.push(b);
            return Ok(serde_json::json!({"status": "ok", "enemy_count": self.enemies.len()}));
        }
        if etype == EnemyType::BulletBill {
            let dir = if p.facing_right { 1.0 } else { -1.0 };
            self.enemies.push(Enemy::bullet_bill(p.x, p.y, dir));
            return Ok(serde_json::json!({"status": "ok", "enemy_count": self.enemies.len()}));
        }
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
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            hp: 0,
            shell_combo: 0,
            target_x: 0.0,
            falling_to_lava: false,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: false,
            drop_from_y: None,
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
