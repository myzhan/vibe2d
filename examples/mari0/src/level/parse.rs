//! The level-file parser.
//!
//! Replaces a loader that was hardcoded to 1-1 in three independent ways: an
//! `include_str!` of one file, a compile-time `LEVEL_COLS = 224`, and a literal
//! player spawn. Width now comes from the data, all five metadata fields are read,
//! and all 100 entity types are recognised.
//!
//! ## Format (`game.lua:2237-2517`)
//!
//! One line, `;`-separated:
//!
//! ```text
//! <cells> ; background=N ; spriteset=N ; music=N ; timelimit=N ; …
//! ```
//!
//! `cells` is comma-separated and **row-major with exactly 15 rows**, so
//! `width = cells / 15` and cell `(x, y)` is at `(y - 1) * width + x`. Each cell is
//! `-`-separated: tile id, then an optional entity id, then optional arguments.
//! The literal string `"link"` may appear as an argument — it is the only
//! non-numeric token in the format.

use super::entities::{EntityKind, Gel, GelFace};
use super::tiles::{self, TILE_EMPTY, TILE_GROUND};

/// Every level is 15 tiles tall. Not a convention — the parser rejects files
/// whose cell count isn't a multiple of 15, exactly as the original does.
pub const LEVEL_HEIGHT: usize = 15;

/// How far left of column 0 the world implicitly extends.
///
/// The original pads `x` from 0 down to −30 with air over ground
/// (`game.lua:2474`) so Mario can be pushed left of the start without falling out
/// of the world.
///
/// This is deliberately **not** baked into the tile grid. Storing it as extra
/// columns would shift every world coordinate by `30 * TILE_SIZE`, breaking the
/// level's own coordinate system — and with it every absolute position in the
/// existing test suite. Instead [`Level::tile`] synthesises those columns on
/// lookup, so column 0 stays column 0.
pub const LEFT_PADDING: i32 = 30;

/// One parsed cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub tile: u16,
    pub entity: Option<EntityKind>,
    /// The entity's argument (pipe target, platform width, gel colour, …).
    pub arg: Option<u16>,
    /// The same argument, unrounded.
    ///
    /// Needed because **one** argument in the whole format isn't an integer: a
    /// platform width of `1.5` (`entity.lua:210-214` offers 1.5, 2, 3 and 5, and four
    /// of the shipped elevator shafts use the 1.5). Parsed as `u16` it silently
    /// becomes `None` and the platform comes out the default width.
    pub argf: Option<f32>,
    /// Resolved `link` target in tile coordinates, if the cell carries one.
    ///
    /// The link lives on the **receiver** and points at the **emitter** — the
    /// editor's drag goes from door to button, not the other way round
    /// (`editor.lua:962-988`).
    pub link: Option<(u16, u16)>,
}

impl Cell {
    const EMPTY: Cell = Cell {
        tile: TILE_EMPTY,
        entity: None,
        arg: None,
        argf: None,
        link: None,
    };

    const GROUND: Cell = Cell {
        tile: TILE_GROUND,
        entity: None,
        arg: None,
        argf: None,
        link: None,
    };
}

/// Per-tile gel coating, keyed by face.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gels {
    pub top: Option<Gel>,
    pub bottom: Option<Gel>,
    pub left: Option<Gel>,
    pub right: Option<Gel>,
}

impl Gels {
    pub fn face(&self, face: GelFace) -> Option<Gel> {
        match face {
            GelFace::Top => self.top,
            GelFace::Bottom => self.bottom,
            GelFace::Left => self.left,
            GelFace::Right => self.right,
        }
    }

    pub fn set(&mut self, face: GelFace, gel: Gel) {
        match face {
            GelFace::Top => self.top = Some(gel),
            GelFace::Bottom => self.bottom = Some(gel),
            GelFace::Left => self.left = Some(gel),
            GelFace::Right => self.right = Some(gel),
        }
    }
}

/// Metadata trailing the cell data.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelMeta {
    /// Background palette, 1..=3.
    pub background: u8,
    /// Tileset palette: 1 overworld, 2 underground, 3 castle, 4 underwater.
    pub spriteset: u8,
    /// 1 silent, 2 overworld, 3 underground, 4 castle, 5 underwater, 6 bonus,
    /// 7 the mappack's own track (`editor.lua:29`).
    pub music: u8,
    /// Starting timer. 0 means untimed.
    pub timelimit: u32,
    /// A 24-wide stub the player runs straight through into sublevel 1.
    pub intermission: bool,
    /// Reaching the right edge reveals the warp-zone text.
    pub has_warpzone: bool,
    pub underwater: bool,
    /// A bonus room entered by vine; dying in one returns to the main level.
    pub bonusstage: bool,
    /// Use the Portal-style background instead of a numbered one.
    pub portal_background: bool,
    pub scrollfactor: f32,
}

impl Default for LevelMeta {
    fn default() -> Self {
        Self {
            background: 1,
            spriteset: 1,
            music: 2,
            // `startlevel` sets 400 before options are parsed (`game.lua:1918`),
            // so a file with no `timelimit=` gets 400, not 0.
            timelimit: 400,
            intermission: false,
            has_warpzone: false,
            underwater: false,
            bonusstage: false,
            portal_background: false,
            scrollfactor: 1.0,
        }
    }
}

/// A parsed level: tiles, per-tile gels, metadata, and the markers extracted from
/// the entity pass.
#[derive(Debug, Clone)]
pub struct Level {
    /// Number of columns in the level's own data.
    pub width: usize,
    pub height: usize,
    cells: Vec<Cell>,
    gels: Vec<Gels>,
    pub meta: LevelMeta,
    pub markers: Markers,
}

/// Level-configuring entities pulled out during parsing.
///
/// These aren't spawned objects — they're positions and spans the rest of the game
/// consults. Kept in one struct so the parser has a single output rather than a
/// dozen out-parameters.
#[derive(Debug, Clone, Default)]
pub struct Markers {
    /// Player start tile, when the level states one.
    ///
    /// Usually `None`: most levels (1-1 included) carry no `spawn` entity and
    /// rely on the original's defaults — see [`Markers::spawn_or_default`].
    pub spawn: Option<(usize, usize)>,
    /// Flagpole base; the level-complete trigger.
    pub flag: Option<(usize, usize)>,
    /// Axe position; the castle-clear trigger.
    pub axe: Option<(usize, usize)>,
    /// Pipe entrances: position → destination sublevel.
    pub pipes: Vec<(usize, usize, u16)>,
    /// Pipe exits: position → the sublevel you arrive from.
    pub pipe_spawns: Vec<(usize, usize, u16)>,
    /// Warp pipes: position → destination world.
    pub warp_pipes: Vec<(usize, usize, u16)>,
    /// Vine blocks: position → destination sublevel.
    pub vines: Vec<(usize, usize, u16)>,
    /// Checkpoint columns, ascending, with the row to respawn on.
    pub checkpoints: Vec<(usize, usize)>,
    /// Maze spans and gate counts for the looping-castle levels.
    pub maze_starts: Vec<usize>,
    pub maze_ends: Vec<usize>,
    pub maze_gates: Vec<(usize, usize, u16)>,
    /// Lab elements, wired after parsing.
    pub lab: Vec<LabPlacement>,
    /// Enemy spawn points, consumed lazily per column as the camera scrolls.
    pub enemies: Vec<EnemySpawn>,
    /// Contents of blocks: position → what pops out.
    pub block_contents: Vec<(usize, usize, EntityKind, Option<u16>)>,
    /// Spring placements. Not enemies and not lab elements: a spring is scenery you
    /// bounce off, built at load.
    pub springs: Vec<(usize, usize)>,
    /// Elevator-shaft spawners: `(x, y, is_up, width_in_blocks)`.
    ///
    /// Not enemies and not lab elements: the original builds these in the parsing
    /// loop itself (`game.lua:2386-2389`), so they exist from load rather than being
    /// revealed by the camera.
    pub platform_spawners: Vec<(usize, usize, bool, Option<f32>)>,
    pub bullet_bill_start: Option<usize>,
    pub bullet_bill_end: Option<usize>,
    pub fire_start: Option<usize>,
    pub lakito_end: Option<usize>,
    pub flying_fish_start: Option<usize>,
    pub flying_fish_end: Option<usize>,
}

impl Markers {
    /// The player's start tile, falling back to the original's hardcoded default.
    ///
    /// `startx = 3, starty = 13` (`game.lua:2057-2058`) applies whenever a level
    /// has no `spawn` entity, which is the common case.
    pub fn spawn_or_default(&self) -> (usize, usize) {
        self.spawn.unwrap_or((DEFAULT_SPAWN_X, DEFAULT_SPAWN_Y))
    }
}

/// Default player start column, from `game.lua:2057`.
pub const DEFAULT_SPAWN_X: usize = 3;
/// Default player start row, from `game.lua:2058`.
pub const DEFAULT_SPAWN_Y: usize = 13;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemySpawn {
    pub x: usize,
    pub y: usize,
    pub kind: EntityKind,
    pub arg: Option<u16>,
    /// The unrounded argument — only a platform's 1.5-block width needs it.
    pub argf: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabPlacement {
    pub x: usize,
    pub y: usize,
    pub kind: EntityKind,
    pub arg: Option<u16>,
    pub link: Option<(u16, u16)>,
}

/// Why a level failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Cell count isn't a multiple of 15, so the width is ambiguous.
    NotMultipleOfHeight {
        cells: usize,
    },
    Empty,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NotMultipleOfHeight { cells } => write!(
                f,
                "level has {cells} cells, which is not a multiple of {LEVEL_HEIGHT}"
            ),
            ParseError::Empty => write!(f, "level has no cells"),
        }
    }
}

impl Level {
    /// Parse a level file's contents.
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        let mut sections = raw.split(';');
        let cell_data = sections.next().unwrap_or("");

        let tokens: Vec<&str> = cell_data
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return Err(ParseError::Empty);
        }
        if !tokens.len().is_multiple_of(LEVEL_HEIGHT) {
            return Err(ParseError::NotMultipleOfHeight {
                cells: tokens.len(),
            });
        }
        let width = tokens.len() / LEVEL_HEIGHT;

        let mut meta = LevelMeta::default();
        for section in sections {
            let section = section.trim();
            if section.is_empty() {
                continue;
            }
            // Boolean options are written as BARE flags — the shipped files say
            // `underwater`, not `underwater=1`. Treating a missing `=` as
            // "skip this section" silently dropped every one of them, which is
            // the same class of bug the original port had with metadata.
            let (key, value) = match section.split_once('=') {
                Some((k, v)) => (k.trim(), Some(v.trim())),
                None => (section, None),
            };
            match key {
                "background" => meta.background = num(value, 1),
                "spriteset" => meta.spriteset = num(value, 1),
                "music" => meta.music = num(value, 2),
                "timelimit" => meta.timelimit = num(value, 400),
                "scrollfactor" => {
                    meta.scrollfactor = value.and_then(|v| v.parse().ok()).unwrap_or(1.0)
                }
                "intermission" => meta.intermission = flag(value),
                "haswarpzone" => meta.has_warpzone = flag(value),
                "underwater" => meta.underwater = flag(value),
                "bonusstage" => meta.bonusstage = flag(value),
                "portalbackground" => meta.portal_background = flag(value),
                _ => {}
            }
        }

        let mut cells = vec![Cell::EMPTY; width * LEVEL_HEIGHT];
        let mut gels = vec![Gels::default(); width * LEVEL_HEIGHT];
        let mut markers = Markers::default();

        for y in 0..LEVEL_HEIGHT {
            for x in 0..width {
                let token = tokens[y * width + x];
                let cell = parse_cell(token);
                let px = x;
                let index = y * width + px;

                // Gel-painting entities write into the gel layer and leave no
                // entity behind — and only when the tile is actually solid
                // (`game.lua:2435-2450`).
                if let Some(kind) = cell.entity {
                    if let Some(face) = kind.gel_face() {
                        if tiles::is_solid(cell.tile)
                            && let Some(gel) = cell.arg.and_then(Gel::from_id)
                        {
                            gels[index].set(face, gel);
                        }
                        cells[index] = Cell {
                            tile: cell.tile,
                            ..Cell::EMPTY
                        };
                        continue;
                    }
                    collect_marker(&mut markers, px, y, &cell);
                }
                cells[index] = cell;
            }
        }

        markers.checkpoints.sort_unstable();
        markers.maze_starts.sort_unstable();
        markers.maze_ends.sort_unstable();

        Ok(Self {
            width,
            height: LEVEL_HEIGHT,
            cells,
            gels,
            meta,
            markers,
        })
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<&Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells.get(y * self.width + x)
    }

    /// Tile id at a position, with the off-map left extension synthesised.
    ///
    /// Columns in `-LEFT_PADDING..0` report floor on the bottom two rows and air
    /// above, matching the original's negative-index padding. Everywhere else out
    /// of bounds is air, so walking off the right edge or falling below the world
    /// behaves as it should.
    pub fn tile(&self, x: i32, y: i32) -> u16 {
        if y < 0 || y as usize >= self.height {
            return TILE_EMPTY;
        }
        if x < 0 {
            if x >= -LEFT_PADDING && y as usize >= self.height - 2 {
                return TILE_GROUND;
            }
            return TILE_EMPTY;
        }
        if x as usize >= self.width {
            return TILE_EMPTY;
        }
        self.cells[y as usize * self.width + x as usize].tile
    }

    pub fn set_tile(&mut self, x: usize, y: usize, tile: u16) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x].tile = tile;
        }
    }

    pub fn is_solid_at(&self, x: i32, y: i32) -> bool {
        tiles::is_solid(self.tile(x, y))
    }

    pub fn gels(&self, x: usize, y: usize) -> Gels {
        if x >= self.width || y >= self.height {
            return Gels::default();
        }
        self.gels[y * self.width + x]
    }

    pub fn gels_mut(&mut self, x: usize, y: usize) -> Option<&mut Gels> {
        if x >= self.width && y >= self.height {
            return None;
        }
        self.gels.get_mut(y * self.width + x)
    }

    /// Width of the level's own data. Same as [`Level::width`]; kept as a named
    /// accessor because the distinction used to matter and callers read better.
    pub fn data_width(&self) -> usize {
        self.width
    }

    /// Duplicate a column onto the right edge, widening the level by one.
    ///
    /// This is how the looping castle levels (3-4, 6-4) work: while the maze is
    /// unsolved the game literally extends the map so the corridor never ends
    /// (`game.lua:565-673`). Markers to the right of the insertion shift with it.
    pub fn repeat_column(&mut self, source_x: usize) {
        if source_x >= self.width {
            return;
        }
        let new_width = self.width + 1;
        let mut cells = Vec::with_capacity(new_width * self.height);
        let mut gels = Vec::with_capacity(new_width * self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                cells.push(self.cells[y * self.width + x].clone());
                gels.push(self.gels[y * self.width + x]);
            }
            cells.push(self.cells[y * self.width + source_x].clone());
            gels.push(self.gels[y * self.width + source_x]);
        }
        self.cells = cells;
        self.gels = gels;
        self.width = new_width;
    }
}

/// A bare flag (`underwater`) is true; `key=0` is false; anything else is true.
fn flag(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(v) => v != "0" && !v.is_empty(),
    }
}

/// Numeric option with a fallback, tolerating a missing or malformed value.
fn num<T: std::str::FromStr>(value: Option<&str>, default: T) -> T {
    value.and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Parse one `tile[-entity[-arg…]]` token.
fn parse_cell(token: &str) -> Cell {
    let mut parts = token.split('-');
    let tile_raw: u16 = parts
        .next()
        .and_then(|t| t.parse().ok())
        .unwrap_or(TILE_EMPTY);
    // The original coerces out-of-range ids to 1 (`game.lua:2267`).
    let tile = if tile_raw == 0 || tile_raw > tiles::MAX_TILE_ID {
        TILE_EMPTY
    } else {
        tile_raw
    };

    let mut entity = None;
    let mut arg = None;
    let mut argf = None;
    let mut link = None;

    let rest: Vec<&str> = parts.collect();
    let mut i = 0;
    while i < rest.len() {
        let part = rest[i];
        if part == "link" {
            // `link` is followed by the emitter's x and y.
            let lx = rest.get(i + 1).and_then(|v| v.parse().ok());
            let ly = rest.get(i + 2).and_then(|v| v.parse().ok());
            if let (Some(lx), Some(ly)) = (lx, ly) {
                link = Some((lx, ly));
            }
            i += 3;
            continue;
        }
        match i {
            0 => entity = part.parse().ok().and_then(EntityKind::from_id),
            1 => {
                arg = part.parse().ok();
                argf = part.parse().ok();
            }
            _ => {}
        }
        i += 1;
    }

    Cell {
        tile,
        entity,
        arg,
        argf,
        link,
    }
}

/// Route an entity into the right marker bucket.
fn collect_marker(m: &mut Markers, x: usize, y: usize, cell: &Cell) {
    use EntityKind::*;
    let Some(kind) = cell.entity else { return };
    let arg = cell.arg;

    match kind {
        Spawn => m.spawn = Some((x, y)),
        Flag => m.flag = Some((x, y)),
        Axe => m.axe = Some((x, y)),
        Pipe => m.pipes.push((x, y, arg.unwrap_or(0))),
        PipeSpawn => m.pipe_spawns.push((x, y, arg.unwrap_or(0))),
        WarpPipe => m.warp_pipes.push((x, y, arg.unwrap_or(0))),
        Vine => m.vines.push((x, y, arg.unwrap_or(0))),
        Checkpoint => m.checkpoints.push((x, y)),
        MazeStart => m.maze_starts.push(x),
        MazeEnd => m.maze_ends.push(x),
        MazeGate => m.maze_gates.push((x, y, arg.unwrap_or(0))),
        BulletBillStart => m.bullet_bill_start = Some(x),
        BulletBillEnd => m.bullet_bill_end = Some(x),
        FireStart => m.fire_start = Some(x),
        LakitoEnd => m.lakito_end = Some(x),
        FlyingFishStart => m.flying_fish_start = Some(x),
        FlyingFishEnd => m.flying_fish_end = Some(x),
        // Block contents: what a question/brick block yields when struck.
        Mushroom | OneUp | Star | ManyCoins => {
            m.block_contents.push((x, y, kind, arg));
        }
        Drain | Remove => {}
        Spring => m.springs.push((x, y)),
        PlatformSpawnerUp | PlatformSpawnerDown => {
            m.platform_spawners
                .push((x, y, kind == PlatformSpawnerUp, cell.argf))
        }
        _ if kind.is_lazy_enemy() => m.enemies.push(EnemySpawn {
            x,
            y,
            kind,
            arg,
            argf: cell.argf,
        }),
        _ if kind.is_lab() => m.lab.push(LabPlacement {
            x,
            y,
            kind,
            arg,
            link: cell.link,
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a level of `width` columns, all air, with the given trailing metadata.
    fn synth(width: usize, extra: &str) -> String {
        let cells = vec!["1"; width * LEVEL_HEIGHT].join(",");
        format!("{cells}{extra}")
    }

    #[test]
    fn width_comes_from_the_data_not_a_constant() {
        // The old loader had `LEVEL_COLS = 224` baked in, so nothing but 1-1
        // could load. Real levels range from 17 to 400 columns.
        for w in [17usize, 24, 176, 224, 400] {
            let level = Level::parse(&synth(w, "")).expect("parse");
            assert_eq!(level.data_width(), w);
            assert_eq!(level.width, w);
            assert_eq!(level.height, LEVEL_HEIGHT);
        }
    }

    #[test]
    fn cell_count_must_be_a_multiple_of_fifteen() {
        let bad = vec!["1"; 15 * 10 + 3].join(",");
        assert_eq!(
            Level::parse(&bad).unwrap_err(),
            ParseError::NotMultipleOfHeight { cells: 153 }
        );
        assert_eq!(Level::parse("").unwrap_err(), ParseError::Empty);
    }

    #[test]
    fn all_five_metadata_fields_are_read() {
        // The old loader read only `timelimit` and silently dropped the rest —
        // but background/spriteset/music are exactly what 32 levels need.
        let level = Level::parse(&synth(
            20,
            ";background=3;spriteset=4;music=5;timelimit=300",
        ))
        .unwrap();
        assert_eq!(level.meta.background, 3);
        assert_eq!(level.meta.spriteset, 4);
        assert_eq!(level.meta.music, 5);
        assert_eq!(level.meta.timelimit, 300);
    }

    #[test]
    fn missing_metadata_falls_back_to_the_originals_defaults() {
        let level = Level::parse(&synth(20, "")).unwrap();
        assert_eq!(level.meta.background, 1);
        assert_eq!(level.meta.spriteset, 1);
        assert_eq!(level.meta.music, 2);
        // 400, not 0: `startlevel` sets it before options are parsed.
        assert_eq!(level.meta.timelimit, 400);
        assert!(!level.meta.underwater);
    }

    #[test]
    fn boolean_metadata_flags_parse() {
        let level = Level::parse(&synth(
            20,
            ";intermission=1;haswarpzone=1;underwater=1;bonusstage=1",
        ))
        .unwrap();
        assert!(level.meta.intermission);
        assert!(level.meta.has_warpzone);
        assert!(level.meta.underwater);
        assert!(level.meta.bonusstage);
    }

    #[test]
    fn cells_are_row_major() {
        // Row-major with a fixed 15 rows: `(y-1)*width + x`. Getting this
        // transposed would rotate every level 90°.
        let mut tokens = vec!["1"; 3 * LEVEL_HEIGHT];
        // Put ground at row index 2, column index 1 → data offset 2*3 + 1 = 7.
        tokens[7] = "2";
        let level = Level::parse(&tokens.join(",")).unwrap();
        assert_eq!(level.cell(1, 2).unwrap().tile, TILE_GROUND);
        assert_eq!(level.cell(1, 0).unwrap().tile, TILE_EMPTY);
    }

    #[test]
    fn off_map_left_extension_is_air_over_two_rows_of_floor() {
        // Synthesised on lookup rather than stored, so column 0 stays column 0
        // and world coordinates match the level file exactly.
        let level = Level::parse(&synth(5, "")).unwrap();
        for x in -LEFT_PADDING..0 {
            assert!(
                !level.is_solid_at(x, 0),
                "row 0 left of the map should be air"
            );
            assert!(
                level.is_solid_at(x, (LEVEL_HEIGHT - 1) as i32),
                "bottom row left of the map should be floor"
            );
            assert!(level.is_solid_at(x, (LEVEL_HEIGHT - 2) as i32));
        }
        // Beyond the extension there is nothing to stand on.
        assert!(!level.is_solid_at(-LEFT_PADDING - 1, (LEVEL_HEIGHT - 1) as i32));
        // And column 0 is the level's own first column, not padding.
        assert_eq!(level.width, 5);
    }

    #[test]
    fn entity_and_argument_are_parsed_from_a_cell() {
        let c = parse_cell("8-2");
        assert_eq!(c.tile, 8);
        assert_eq!(c.entity, Some(EntityKind::Mushroom));
        assert_eq!(c.arg, None);

        let c = parse_cell("17-21-1");
        assert_eq!(c.tile, 17);
        assert_eq!(c.entity, Some(EntityKind::Pipe));
        assert_eq!(c.arg, Some(1));

        let c = parse_cell("1-100");
        assert_eq!(c.entity, Some(EntityKind::Checkpoint));
    }

    #[test]
    fn link_token_is_parsed_and_is_the_only_non_numeric_field() {
        // Receiver-side link pointing at an emitter at (12, 7).
        let c = parse_cell("140-29-link-12-7");
        assert_eq!(c.tile, 140);
        assert_eq!(c.entity, Some(EntityKind::DoorHor));
        assert_eq!(c.link, Some((12, 7)));

        // `walltimer` carries an arg before the link — the index shift that
        // trips up a naive parser.
        let c = parse_cell("140-74-4-link-3-9");
        assert_eq!(c.entity, Some(EntityKind::Timer));
        assert_eq!(c.arg, Some(4));
        assert_eq!(c.link, Some((3, 9)));
    }

    #[test]
    fn out_of_range_tile_ids_become_air() {
        assert_eq!(parse_cell("9999").tile, TILE_EMPTY);
        assert_eq!(parse_cell("0").tile, TILE_EMPTY);
        assert_eq!(parse_cell("garbage").tile, TILE_EMPTY);
    }

    #[test]
    fn markers_are_collected_and_positions_include_padding() {
        let mut tokens = vec!["1"; 10 * LEVEL_HEIGHT];
        tokens[13 * 10 + 3] = "1-8"; // spawn at data (3, 13)
        tokens[13 * 10 + 7] = "78-11"; // flagpole base
        tokens[5 * 10 + 4] = "1-6"; // goomba
        let level = Level::parse(&tokens.join(",")).unwrap();

        assert_eq!(level.markers.spawn, Some((3, 13)));
        assert_eq!(level.markers.flag, Some((7, 13)));
        assert_eq!(level.markers.enemies.len(), 1);
        assert_eq!(level.markers.enemies[0].kind, EntityKind::Goomba);
        assert_eq!(level.markers.enemies[0].x, 4);
    }

    #[test]
    fn gel_is_written_to_the_face_layer_only_on_solid_tiles() {
        let mut tokens = vec!["1"; 4 * LEVEL_HEIGHT];
        tokens[0] = "2-85-1"; // solid ground + geltop + blue
        tokens[1] = "1-85-1"; // air + geltop → must be ignored
        let level = Level::parse(&tokens.join(",")).unwrap();

        assert_eq!(level.gels(0, 0).top, Some(Gel::Blue));
        assert_eq!(
            level.gels(1, 0).top,
            None,
            "gel on a non-solid tile is dropped, as in the original"
        );
        // The gel entity itself leaves no entity behind.
        assert_eq!(level.cell(0, 0).unwrap().entity, None);
    }

    #[test]
    fn repeat_column_widens_the_level_for_maze_loops() {
        let mut tokens = vec!["1"; 4 * LEVEL_HEIGHT];
        tokens[0] = "2"; // distinctive tile in column 0, row 0
        let mut level = Level::parse(&tokens.join(",")).unwrap();
        let before = level.width;

        level.repeat_column(0);
        assert_eq!(level.width, before + 1);
        // The duplicate lands on the new right edge with the same content.
        assert_eq!(level.tile(before as i32, 0), TILE_GROUND);
    }

    #[test]
    fn out_of_bounds_reads_are_safe() {
        let level = Level::parse(&synth(5, "")).unwrap();
        assert_eq!(level.tile(-1, 0), TILE_EMPTY);
        assert_eq!(level.tile(0, -1), TILE_EMPTY);
        assert_eq!(level.tile(99999, 0), TILE_EMPTY);
        assert!(level.cell(99999, 0).is_none());
        assert_eq!(level.gels(99999, 0), Gels::default());
    }
}
