use std::collections::VecDeque;
use vibe2d::prelude::*;

// ── Layout constants ────────────────────────────────────────────────
const CELL: f32 = 32.0;
const COLS: usize = 10;
const VISIBLE_ROWS: usize = 20;
const TOTAL_ROWS: usize = 40;
const FIELD_X: f32 = 198.0;
const FIELD_Y: f32 = 30.0;
const HOLD_X: f32 = 52.0;
const HOLD_Y: f32 = 195.0;
const NEXT_X: f32 = 592.0;
const NEXT_Y: f32 = 96.0;
const NEXT_SPACING: f32 = 96.0;
const STATS_X: f32 = 20.0;
const STATS_Y: f32 = 330.0;

// ── Gameplay constants ──────────────────────────────────────────────
const DAS_DELAY: f32 = 0.167;
const ARR_RATE: f32 = 0.033;
const LOCK_DELAY: f32 = 0.5;
const MAX_LOCK_RESETS: u32 = 15;
const SPAWN_ROW: i32 = 19; // row index where piece pivot spawns
const SPAWN_COL: i32 = 4;

// ── Piece types ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
#[allow(dead_code)]
enum PieceType {
    I = 0,
    O = 1,
    T = 2,
    S = 3,
    Z = 4,
    J = 5,
    L = 6,
}

const ALL_PIECES: [PieceType; 7] = [
    PieceType::I,
    PieceType::O,
    PieceType::T,
    PieceType::S,
    PieceType::Z,
    PieceType::J,
    PieceType::L,
];

impl PieceType {
    #[allow(dead_code)]
    fn texture_name(self) -> &'static str {
        match self {
            PieceType::I => "block_i",
            PieceType::O => "block_o",
            PieceType::T => "block_t",
            PieceType::S => "block_s",
            PieceType::Z => "block_z",
            PieceType::J => "block_j",
            PieceType::L => "block_l",
        }
    }

    #[cfg(feature = "vdp")]
    fn name(self) -> &'static str {
        match self {
            PieceType::I => "I",
            PieceType::O => "O",
            PieceType::T => "T",
            PieceType::S => "S",
            PieceType::Z => "Z",
            PieceType::J => "J",
            PieceType::L => "L",
        }
    }

    #[cfg(feature = "vdp")]
    fn from_name(s: &str) -> Option<Self> {
        match s {
            "I" => Some(PieceType::I),
            "O" => Some(PieceType::O),
            "T" => Some(PieceType::T),
            "S" => Some(PieceType::S),
            "Z" => Some(PieceType::Z),
            "J" => Some(PieceType::J),
            "L" => Some(PieceType::L),
            _ => None,
        }
    }
}

// ── Rotation state ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Rot {
    N = 0,
    E = 1,
    S = 2,
    W = 3,
}

impl Rot {
    fn cw(self) -> Self {
        match self {
            Rot::N => Rot::E,
            Rot::E => Rot::S,
            Rot::S => Rot::W,
            Rot::W => Rot::N,
        }
    }
    fn ccw(self) -> Self {
        match self {
            Rot::N => Rot::W,
            Rot::E => Rot::N,
            Rot::S => Rot::E,
            Rot::W => Rot::S,
        }
    }
    fn idx(self) -> usize {
        self as usize
    }
}

// ── Piece shape data (SRS) ─────────────────────────────────────────
// Each entry: [4 cells as (row_offset, col_offset)] relative to pivot.
// Row increases downward (toward bottom of grid).
type Shape = [(i32, i32); 4];

const SHAPES: [[Shape; 4]; 7] = [
    // I
    [
        [(0, -1), (0, 0), (0, 1), (0, 2)], // N
        [(-1, 1), (0, 1), (1, 1), (2, 1)], // E
        [(1, -1), (1, 0), (1, 1), (1, 2)], // S
        [(-1, 0), (0, 0), (1, 0), (2, 0)], // W
    ],
    // O
    [
        [(0, 0), (0, 1), (1, 0), (1, 1)], // N
        [(0, 0), (0, 1), (1, 0), (1, 1)], // E
        [(0, 0), (0, 1), (1, 0), (1, 1)], // S
        [(0, 0), (0, 1), (1, 0), (1, 1)], // W
    ],
    // T
    [
        [(-1, 0), (0, -1), (0, 0), (0, 1)], // N: nub up
        [(-1, 0), (0, 0), (0, 1), (1, 0)],  // E: nub right
        [(0, -1), (0, 0), (0, 1), (1, 0)],  // S: nub down
        [(-1, 0), (0, -1), (0, 0), (1, 0)], // W: nub left
    ],
    // S
    [
        [(-1, 0), (-1, 1), (0, -1), (0, 0)], // N
        [(-1, 0), (0, 0), (0, 1), (1, 1)],   // E
        [(0, 0), (0, 1), (1, -1), (1, 0)],   // S
        [(-1, -1), (0, -1), (0, 0), (1, 0)], // W
    ],
    // Z
    [
        [(-1, -1), (-1, 0), (0, 0), (0, 1)], // N
        [(-1, 1), (0, 0), (0, 1), (1, 0)],   // E
        [(0, -1), (0, 0), (1, 0), (1, 1)],   // S
        [(-1, 0), (0, -1), (0, 0), (1, -1)], // W
    ],
    // J
    [
        [(-1, -1), (0, -1), (0, 0), (0, 1)], // N
        [(-1, 0), (-1, 1), (0, 0), (1, 0)],  // E
        [(0, -1), (0, 0), (0, 1), (1, 1)],   // S
        [(-1, 0), (0, 0), (1, -1), (1, 0)],  // W
    ],
    // L
    [
        [(-1, 1), (0, -1), (0, 0), (0, 1)],  // N
        [(-1, 0), (0, 0), (1, 0), (1, 1)],   // E
        [(0, -1), (0, 0), (0, 1), (1, -1)],  // S
        [(-1, -1), (-1, 0), (0, 0), (1, 0)], // W
    ],
];

// ── SRS Wall-kick data ──────────────────────────────────────────────
// Index: 0=N->E, 1=E->N, 2=E->S, 3=S->E, 4=S->W, 5=W->S, 6=W->N, 7=N->W
// Each entry has 5 (dx, dy) offsets to test. (dx=col offset, dy=row offset)
// Positive dx = right, positive dy = down.
const KICKS_JLSTZ: [[(i32, i32); 5]; 8] = [
    // 0: N -> E
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    // 1: E -> N
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
    // 2: E -> S
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    // 3: S -> E
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    // 4: S -> W
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    // 5: W -> S
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    // 6: W -> N
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    // 7: N -> W
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
];

const KICKS_I: [[(i32, i32); 5]; 8] = [
    // 0: N -> E
    [(0, 0), (-2, 0), (1, 0), (-2, 1), (1, -2)],
    // 1: E -> N
    [(0, 0), (2, 0), (-1, 0), (2, -1), (-1, 2)],
    // 2: E -> S
    [(0, 0), (-1, 0), (2, 0), (-1, -2), (2, 1)],
    // 3: S -> E
    [(0, 0), (1, 0), (-2, 0), (1, 2), (-2, -1)],
    // 4: S -> W
    [(0, 0), (2, 0), (-1, 0), (2, -1), (-1, 2)],
    // 5: W -> S
    [(0, 0), (-2, 0), (1, 0), (-2, 1), (1, -2)],
    // 6: W -> N
    [(0, 0), (1, 0), (-2, 0), (1, 2), (-2, -1)],
    // 7: N -> W
    [(0, 0), (-1, 0), (2, 0), (-1, -2), (2, 1)],
];

fn kick_index(from: Rot, to: Rot) -> usize {
    match (from, to) {
        (Rot::N, Rot::E) => 0,
        (Rot::E, Rot::N) => 1,
        (Rot::E, Rot::S) => 2,
        (Rot::S, Rot::E) => 3,
        (Rot::S, Rot::W) => 4,
        (Rot::W, Rot::S) => 5,
        (Rot::W, Rot::N) => 6,
        (Rot::N, Rot::W) => 7,
        _ => 0,
    }
}

// ── Active piece ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Piece {
    piece_type: PieceType,
    rotation: Rot,
    x: i32, // column of pivot
    y: i32, // row of pivot (0 = top of buffer, 39 = bottom)
}

impl Piece {
    fn cells(&self) -> [(i32, i32); 4] {
        let shape = SHAPES[self.piece_type as usize][self.rotation.idx()];
        let mut out = [(0i32, 0i32); 4];
        for (i, &(dr, dc)) in shape.iter().enumerate() {
            out[i] = (self.y + dr, self.x + dc);
        }
        out
    }
}

// ── Grid helpers ────────────────────────────────────────────────────
type Grid = [[Option<PieceType>; COLS]; TOTAL_ROWS];

fn empty_grid() -> Grid {
    [[None; COLS]; TOTAL_ROWS]
}

fn collides(piece: &Piece, grid: &Grid) -> bool {
    for (r, c) in piece.cells() {
        if c < 0 || c >= COLS as i32 {
            return true;
        }
        if r >= TOTAL_ROWS as i32 {
            return true;
        }
        if r >= 0 && grid[r as usize][c as usize].is_some() {
            return true;
        }
    }
    false
}

fn ghost_y(piece: &Piece, grid: &Grid) -> i32 {
    let mut p = piece.clone();
    while !collides(&p, grid) {
        p.y += 1;
    }
    p.y - 1
}

fn lock_piece(piece: &Piece, grid: &mut Grid) {
    for (r, c) in piece.cells() {
        if r >= 0 && r < TOTAL_ROWS as i32 && c >= 0 && c < COLS as i32 {
            grid[r as usize][c as usize] = Some(piece.piece_type);
        }
    }
}

fn clear_lines(grid: &mut Grid) -> u32 {
    let mut cleared = 0u32;
    // Scan from bottom to top
    let mut rows_to_keep: Vec<usize> = Vec::new();
    for r in (0..TOTAL_ROWS).rev() {
        let full = grid[r].iter().all(|c| c.is_some());
        if !full {
            rows_to_keep.push(r);
        } else {
            cleared += 1;
        }
    }
    // Rebuild grid from bottom
    let mut new_grid = empty_grid();
    let mut write = TOTAL_ROWS as i32 - 1;
    for &r in &rows_to_keep {
        if write >= 0 {
            new_grid[write as usize] = grid[r];
            write -= 1;
        }
    }
    *grid = new_grid;
    cleared
}

// ── T-Spin detection ────────────────────────────────────────────────
fn is_t_spin(piece: &Piece, grid: &Grid) -> bool {
    if piece.piece_type != PieceType::T {
        return false;
    }
    let py = piece.y;
    let px = piece.x;
    // Check 4 corners around T pivot
    let corners = [
        (py - 1, px - 1),
        (py - 1, px + 1),
        (py + 1, px - 1),
        (py + 1, px + 1),
    ];
    let mut filled = 0;
    for (r, c) in corners {
        if r < 0 || r >= TOTAL_ROWS as i32 || c < 0 || c >= COLS as i32 {
            filled += 1;
        } else if grid[r as usize][c as usize].is_some() {
            filled += 1;
        }
    }
    filled >= 3
}

// ── Scoring ─────────────────────────────────────────────────────────
fn gravity_interval(level: u32) -> f32 {
    let l = (level.max(1) - 1) as f32;
    (0.8 - l * 0.007).max(0.001).powf(l)
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum ClearKind {
    None,
    Single,
    Double,
    Triple,
    Tetris,
    TSpinMini,
    TSpin,
    TSpinSingle,
    TSpinDouble,
    TSpinTriple,
}

fn classify_clear(lines: u32, t_spin: bool, _t_spin_mini: bool) -> ClearKind {
    match (lines, t_spin) {
        (0, true) => ClearKind::TSpin,
        (0, false) => ClearKind::None,
        (1, true) => ClearKind::TSpinSingle,
        (1, false) => ClearKind::Single,
        (2, true) => ClearKind::TSpinDouble,
        (2, false) => ClearKind::Double,
        (3, true) => ClearKind::TSpinTriple,
        (3, false) => ClearKind::Triple,
        (4, _) => ClearKind::Tetris,
        _ => ClearKind::None,
    }
}

fn base_score(kind: ClearKind) -> u32 {
    match kind {
        ClearKind::None => 0,
        ClearKind::Single => 100,
        ClearKind::Double => 300,
        ClearKind::Triple => 500,
        ClearKind::Tetris => 800,
        ClearKind::TSpinMini => 100,
        ClearKind::TSpin => 400,
        ClearKind::TSpinSingle => 800,
        ClearKind::TSpinDouble => 1200,
        ClearKind::TSpinTriple => 1600,
    }
}

fn is_difficult(kind: ClearKind) -> bool {
    matches!(
        kind,
        ClearKind::Tetris
            | ClearKind::TSpin
            | ClearKind::TSpinSingle
            | ClearKind::TSpinDouble
            | ClearKind::TSpinTriple
    )
}

// ── Game phase ──────────────────────────────────────────────────────
#[derive(PartialEq, Clone, Copy, Debug)]
enum GamePhase {
    Playing,
    GameOver,
}

// ── Main game struct ────────────────────────────────────────────────
struct TetrisGame {
    grid: Grid,
    current: Option<Piece>,
    ghost: i32,

    hold_piece: Option<PieceType>,
    hold_used: bool,
    next_queue: VecDeque<PieceType>,
    bag: Vec<PieceType>,

    gravity_timer: f32,
    lock_timer: f32,
    lock_active: bool,
    lock_resets: u32,
    last_was_rotation: bool,

    das_direction: i32,
    das_timer: f32,
    arr_timer: f32,

    score: u32,
    level: u32,
    lines_cleared: u32,
    combo: i32,
    back_to_back: bool,

    phase: GamePhase,

    block_tex: [TextureId; 7],
    ghost_tex: TextureId,
    bg_tex: TextureId,
}

impl TetrisGame {
    fn tex(ctx: &Context, name: &str) -> TextureId {
        ctx.assets
            .texture_id(name)
            .unwrap_or_else(|| panic!("Missing texture: {name}"))
    }

    fn fill_bag(&mut self) {
        let mut set = ALL_PIECES;
        // Fisher-Yates shuffle
        for i in (1..set.len()).rev() {
            let j = (rand::random::<f32>() * (i + 1) as f32) as usize % (i + 1);
            set.swap(i, j);
        }
        self.bag.extend_from_slice(&set);
    }

    fn pop_piece(&mut self) -> PieceType {
        if self.bag.is_empty() {
            self.fill_bag();
        }
        self.bag.pop().unwrap()
    }

    fn ensure_next(&mut self) {
        while self.next_queue.len() < 5 {
            let p = self.pop_piece();
            self.next_queue.push_back(p);
        }
    }

    fn spawn_piece(&mut self) -> bool {
        self.ensure_next();
        let pt = self.next_queue.pop_front().unwrap();
        self.ensure_next();
        let piece = Piece {
            piece_type: pt,
            rotation: Rot::N,
            x: SPAWN_COL,
            y: SPAWN_ROW,
        };
        if collides(&piece, &self.grid) {
            // Try one row up
            let piece_up = Piece {
                y: SPAWN_ROW - 1,
                ..piece.clone()
            };
            if collides(&piece_up, &self.grid) {
                self.current = None;
                return false; // game over
            }
            self.current = Some(piece_up);
        } else {
            self.current = Some(piece);
        }
        self.ghost = self
            .current
            .as_ref()
            .map(|p| ghost_y(p, &self.grid))
            .unwrap_or(0);
        self.gravity_timer = gravity_interval(self.level);
        self.lock_active = false;
        self.lock_timer = LOCK_DELAY;
        self.lock_resets = 0;
        self.last_was_rotation = false;
        self.hold_used = false;
        true
    }

    fn try_move(&mut self, dx: i32, dy: i32) -> bool {
        if let Some(ref mut p) = self.current {
            let mut test = p.clone();
            test.x += dx;
            test.y += dy;
            if !collides(&test, &self.grid) {
                p.x = test.x;
                p.y = test.y;
                self.ghost = ghost_y(p, &self.grid);
                self.last_was_rotation = false;
                if self.lock_active && self.lock_resets < MAX_LOCK_RESETS {
                    self.lock_timer = LOCK_DELAY;
                    self.lock_resets += 1;
                }
                return true;
            }
        }
        false
    }

    fn try_rotate(&mut self, cw: bool) -> bool {
        if let Some(ref mut piece) = self.current {
            if piece.piece_type == PieceType::O {
                return false; // O doesn't rotate
            }
            let new_rot = if cw {
                piece.rotation.cw()
            } else {
                piece.rotation.ccw()
            };
            let ki = kick_index(piece.rotation, new_rot);
            let kicks = if piece.piece_type == PieceType::I {
                &KICKS_I[ki]
            } else {
                &KICKS_JLSTZ[ki]
            };

            for &(dx, dy) in kicks {
                let test = Piece {
                    piece_type: piece.piece_type,
                    rotation: new_rot,
                    x: piece.x + dx,
                    y: piece.y + dy,
                };
                if !collides(&test, &self.grid) {
                    piece.x = test.x;
                    piece.y = test.y;
                    piece.rotation = new_rot;
                    self.ghost = ghost_y(piece, &self.grid);
                    self.last_was_rotation = true;
                    if self.lock_active && self.lock_resets < MAX_LOCK_RESETS {
                        self.lock_timer = LOCK_DELAY;
                        self.lock_resets += 1;
                    }
                    return true;
                }
            }
        }
        false
    }

    fn hard_drop(&mut self) -> u32 {
        if let Some(ref mut p) = self.current {
            let gy = ghost_y(p, &self.grid);
            let cells_dropped = (gy - p.y).max(0) as u32;
            p.y = gy;
            return cells_dropped;
        }
        0
    }

    fn do_lock(&mut self) {
        let piece = match self.current.take() {
            Some(p) => p,
            None => return,
        };

        // Check T-Spin before locking
        let t_spin = self.last_was_rotation && is_t_spin(&piece, &self.grid);

        // Lock into grid
        lock_piece(&piece, &mut self.grid);

        // Clear lines
        let lines = clear_lines(&mut self.grid);

        // Classify and score
        let kind = classify_clear(lines, t_spin, false);
        if kind != ClearKind::None {
            let mut pts = base_score(kind) * self.level;
            // Back-to-back bonus
            if is_difficult(kind) && self.back_to_back {
                pts = (pts as f32 * 1.5) as u32;
            }
            // Combo bonus
            if lines > 0 {
                self.combo += 1;
                if self.combo > 0 {
                    pts += 50 * self.combo as u32 * self.level;
                }
            }
            self.score += pts;

            // Update B2B
            if lines > 0 {
                self.back_to_back = is_difficult(kind);
            }
        } else if lines == 0 {
            self.combo = -1;
        }

        // Update lines and level
        self.lines_cleared += lines;
        let new_level = (self.lines_cleared / 10) + 1;
        if new_level > self.level {
            self.level = new_level;
        }

        // Spawn next
        if !self.spawn_piece() {
            self.phase = GamePhase::GameOver;
        }
    }

    fn do_hold(&mut self) {
        if self.hold_used {
            return;
        }
        if let Some(ref piece) = self.current {
            let cur_type = piece.piece_type;
            if let Some(held) = self.hold_piece {
                // Swap
                self.hold_piece = Some(cur_type);
                let new_piece = Piece {
                    piece_type: held,
                    rotation: Rot::N,
                    x: SPAWN_COL,
                    y: SPAWN_ROW,
                };
                if collides(&new_piece, &self.grid) {
                    self.current = None;
                    self.phase = GamePhase::GameOver;
                    return;
                }
                self.current = Some(new_piece);
            } else {
                // First hold
                self.hold_piece = Some(cur_type);
                self.current = None;
                if !self.spawn_piece() {
                    self.phase = GamePhase::GameOver;
                    return;
                }
            }
            self.hold_used = true;
            self.ghost = self
                .current
                .as_ref()
                .map(|p| ghost_y(p, &self.grid))
                .unwrap_or(0);
            self.gravity_timer = gravity_interval(self.level);
            self.lock_active = false;
            self.lock_timer = LOCK_DELAY;
            self.lock_resets = 0;
            self.last_was_rotation = false;
        }
    }

    fn reset(&mut self) {
        self.grid = empty_grid();
        self.current = None;
        self.hold_piece = None;
        self.hold_used = false;
        self.next_queue.clear();
        self.bag.clear();
        self.score = 0;
        self.level = 1;
        self.lines_cleared = 0;
        self.combo = -1;
        self.back_to_back = false;
        self.phase = GamePhase::Playing;
        self.das_direction = 0;
        self.das_timer = 0.0;
        self.arr_timer = 0.0;
        self.spawn_piece();
    }

    // ── Drawing helpers ─────────────────────────────────────────────

    fn draw_block(&self, screen: &mut Screen, tex: TextureId, row: i32, col: i32) {
        // Only draw visible rows (20..40 maps to screen rows 0..20)
        let visible_row = row as i32 - (TOTAL_ROWS as i32 - VISIBLE_ROWS as i32);
        if visible_row < 0 || visible_row >= VISIBLE_ROWS as i32 {
            return;
        }
        let x = FIELD_X + col as f32 * CELL;
        let y = FIELD_Y + visible_row as f32 * CELL;
        screen.draw_sprite(tex, x, y, CELL, CELL);
    }

    fn draw_piece_at(&self, screen: &mut Screen, pt: PieceType, rot: Rot, px: f32, py: f32) {
        let shape = SHAPES[pt as usize][rot.idx()];
        let tex = self.block_tex[pt as usize];
        for &(dr, dc) in &shape {
            let x = px + dc as f32 * CELL;
            let y = py + dr as f32 * CELL;
            screen.draw_sprite(tex, x, y, CELL, CELL);
        }
    }

    fn draw_piece_preview(&self, screen: &mut Screen, pt: PieceType, cx: f32, cy: f32) {
        self.draw_piece_at(screen, pt, Rot::N, cx, cy);
    }
}

impl Game for TetrisGame {
    fn new(ctx: &mut Context, _renderer: &Renderer) -> Self {
        let block_tex = [
            Self::tex(ctx, "block_i"),
            Self::tex(ctx, "block_o"),
            Self::tex(ctx, "block_t"),
            Self::tex(ctx, "block_s"),
            Self::tex(ctx, "block_z"),
            Self::tex(ctx, "block_j"),
            Self::tex(ctx, "block_l"),
        ];
        let ghost_tex = Self::tex(ctx, "block_ghost");
        let bg_tex = Self::tex(ctx, "bg");

        let mut game = TetrisGame {
            grid: empty_grid(),
            current: None,
            ghost: 0,
            hold_piece: None,
            hold_used: false,
            next_queue: VecDeque::new(),
            bag: Vec::new(),
            gravity_timer: 0.0,
            lock_timer: LOCK_DELAY,
            lock_active: false,
            lock_resets: 0,
            last_was_rotation: false,
            das_direction: 0,
            das_timer: 0.0,
            arr_timer: 0.0,
            score: 0,
            level: 1,
            lines_cleared: 0,
            combo: -1,
            back_to_back: false,
            phase: GamePhase::Playing,
            block_tex,
            ghost_tex,
            bg_tex,
        };
        game.spawn_piece();
        game
    }

    fn update(&mut self, _ctx: &mut Context, dt: f32, input: &InputState) {
        if self.phase == GamePhase::GameOver {
            // Press space to restart
            if input.is_action_just_pressed("hard_drop") {
                self.reset();
            }
            return;
        }

        if self.current.is_none() {
            return;
        }

        // ── Hold ──
        if input.is_action_just_pressed("hold") {
            self.do_hold();
            if self.phase == GamePhase::GameOver || self.current.is_none() {
                return;
            }
        }

        // ── Rotation ──
        if input.is_action_just_pressed("rotate_cw") {
            self.try_rotate(true);
        }
        if input.is_action_just_pressed("rotate_ccw") {
            self.try_rotate(false);
        }

        // ── Hard drop ──
        if input.is_action_just_pressed("hard_drop") {
            let cells = self.hard_drop();
            self.score += cells * 2;
            self.do_lock();
            return;
        }

        // ── DAS horizontal movement ──
        let left = input.is_action_pressed("move_left");
        let right = input.is_action_pressed("move_right");
        let left_just = input.is_action_just_pressed("move_left");
        let right_just = input.is_action_just_pressed("move_right");

        if left_just {
            self.das_direction = -1;
            self.das_timer = 0.0;
            self.arr_timer = 0.0;
            self.try_move(-1, 0);
        } else if right_just {
            self.das_direction = 1;
            self.das_timer = 0.0;
            self.arr_timer = 0.0;
            self.try_move(1, 0);
        }

        if self.das_direction == -1 && left {
            self.das_timer += dt;
            if self.das_timer >= DAS_DELAY {
                self.arr_timer += dt;
                while self.arr_timer >= ARR_RATE {
                    self.arr_timer -= ARR_RATE;
                    self.try_move(-1, 0);
                }
            }
        } else if self.das_direction == 1 && right {
            self.das_timer += dt;
            if self.das_timer >= DAS_DELAY {
                self.arr_timer += dt;
                while self.arr_timer >= ARR_RATE {
                    self.arr_timer -= ARR_RATE;
                    self.try_move(1, 0);
                }
            }
        } else {
            // Reset DAS if direction key released
            if self.das_direction == -1 && !left {
                self.das_direction = 0;
            }
            if self.das_direction == 1 && !right {
                self.das_direction = 0;
            }
        }

        // ── Soft drop ──
        let soft = input.is_action_pressed("soft_drop");
        if soft {
            // Speed up gravity by 20x
            self.gravity_timer -= dt * 19.0; // extra 19x on top of normal
            if self.gravity_timer <= 0.0 {
                if self.try_move(0, 1) {
                    self.score += 1; // soft drop point
                }
                self.gravity_timer += gravity_interval(self.level);
            }
        }

        // ── Gravity ──
        self.gravity_timer -= dt;
        if self.gravity_timer <= 0.0 {
            self.gravity_timer += gravity_interval(self.level);
            if !self.try_move(0, 1) {
                // Can't move down — start or continue lock
                if !self.lock_active {
                    self.lock_active = true;
                    self.lock_timer = LOCK_DELAY;
                }
            } else {
                // Moved down successfully — cancel lock
                self.lock_active = false;
            }
        }

        // ── Lock delay ──
        if self.lock_active {
            // Check if piece is still on surface
            if let Some(ref p) = self.current {
                let mut test = p.clone();
                test.y += 1;
                if !collides(&test, &self.grid) {
                    // Piece can move down — cancel lock
                    self.lock_active = false;
                } else {
                    self.lock_timer -= dt;
                    if self.lock_timer <= 0.0 || self.lock_resets >= MAX_LOCK_RESETS {
                        self.do_lock();
                    }
                }
            }
        }
    }

    fn draw(&self, ctx: &Context, screen: &mut Screen) {
        // ── Background (includes grid lines, hold/next boxes) ──
        screen.draw_sprite(self.bg_tex, 0.0, 0.0, 800.0, 700.0);

        // ── Locked blocks ──
        let base = TOTAL_ROWS - VISIBLE_ROWS;
        for r in base..TOTAL_ROWS {
            for c in 0..COLS {
                if let Some(pt) = self.grid[r][c] {
                    self.draw_block(screen, self.block_tex[pt as usize], r as i32, c as i32);
                }
            }
        }

        // ── Ghost piece ──
        if let Some(ref piece) = self.current {
            let ghost_piece = Piece {
                y: self.ghost,
                ..piece.clone()
            };
            for (r, c) in ghost_piece.cells() {
                self.draw_block(screen, self.ghost_tex, r, c);
            }
        }

        // ── Current piece ──
        if let Some(ref piece) = self.current {
            let tex = self.block_tex[piece.piece_type as usize];
            for (r, c) in piece.cells() {
                self.draw_block(screen, tex, r, c);
            }
        }

        // ── Hold piece ──
        if let Some(pt) = self.hold_piece {
            self.draw_piece_preview(screen, pt, HOLD_X + CELL, HOLD_Y + CELL);
        }

        // ── Next queue ──
        for (i, &pt) in self.next_queue.iter().take(5).enumerate() {
            let y = NEXT_Y + i as f32 * NEXT_SPACING;
            self.draw_piece_preview(screen, pt, NEXT_X + CELL, y + CELL);
        }

        // ── Stats ──
        let ui_font = ctx.assets.font("ui");
        if let Some(font) = ui_font {
            screen.draw_text(font, "SCORE", STATS_X, STATS_Y);
            screen.draw_text(font, &format!("{}", self.score), STATS_X, STATS_Y + 22.0);
            screen.draw_text(font, "LEVEL", STATS_X, STATS_Y + 56.0);
            screen.draw_text(font, &format!("{}", self.level), STATS_X, STATS_Y + 78.0);
            screen.draw_text(font, "LINES", STATS_X, STATS_Y + 112.0);
            screen.draw_text(
                font,
                &format!("{}", self.lines_cleared),
                STATS_X,
                STATS_Y + 134.0,
            );
        }

        // ── Game over overlay ──
        if self.phase == GamePhase::GameOver {
            if let Some(font) = ctx.assets.font("title") {
                screen.draw_text_centered(font, "GAME OVER", 300.0);
            }
            if let Some(font) = ui_font {
                let text = format!("Score: {}  Level: {}", self.score, self.level);
                screen.draw_text_centered(font, &text, 340.0);
                screen.draw_text_centered(font, "Press SPACE to restart", 380.0);
            }
        }
    }

    fn clear_color(&self) -> Color {
        Color::from_hex(0x0F0F1A)
    }

    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value {
        let phase_str = match self.phase {
            GamePhase::Playing => "playing",
            GamePhase::GameOver => "game_over",
        };

        // Serialize grid
        let grid_json: Vec<Vec<serde_json::Value>> = self
            .grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        Some(pt) => serde_json::Value::String(pt.name().to_string()),
                        None => serde_json::Value::Null,
                    })
                    .collect()
            })
            .collect();

        let current_json = self.current.as_ref().map(|p| {
            let cells: Vec<Vec<i32>> = p.cells().iter().map(|&(r, c)| vec![r, c]).collect();
            serde_json::json!({
                "type": p.piece_type.name(),
                "rotation": p.rotation.idx(),
                "x": p.x,
                "y": p.y,
                "cells": cells,
            })
        });

        let next_json: Vec<&str> = self.next_queue.iter().map(|pt| pt.name()).collect();

        serde_json::json!({
            "phase": phase_str,
            "grid": grid_json,
            "current": current_json,
            "ghost_y": self.ghost,
            "hold": self.hold_piece.map(|pt| pt.name()),
            "hold_used": self.hold_used,
            "next": next_json,
            "score": self.score,
            "level": self.level,
            "lines": self.lines_cleared,
            "combo": self.combo,
            "back_to_back": self.back_to_back,
            "gravity_timer": self.gravity_timer,
            "lock_active": self.lock_active,
            "lock_resets": self.lock_resets,
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
                self.reset();
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setGrid" => {
                let grid_arr = params
                    .get("grid")
                    .and_then(|v| v.as_array())
                    .ok_or("Missing 'grid' array")?;
                if grid_arr.len() != TOTAL_ROWS {
                    return Err(format!("Grid must have {} rows", TOTAL_ROWS));
                }
                let mut new_grid = empty_grid();
                for (r, row_val) in grid_arr.iter().enumerate() {
                    let row = row_val.as_array().ok_or("Each row must be an array")?;
                    if row.len() != COLS {
                        return Err(format!("Each row must have {} columns", COLS));
                    }
                    for (c, cell) in row.iter().enumerate() {
                        if cell.is_null() {
                            new_grid[r][c] = None;
                        } else {
                            let name = cell.as_str().ok_or("Cell must be null or string")?;
                            new_grid[r][c] =
                                Some(PieceType::from_name(name).ok_or("Invalid piece type")?);
                        }
                    }
                }
                self.grid = new_grid;
                // Update ghost
                if let Some(ref p) = self.current {
                    self.ghost = ghost_y(p, &self.grid);
                }
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.clearGrid" => {
                self.grid = empty_grid();
                if let Some(ref p) = self.current {
                    self.ghost = ghost_y(p, &self.grid);
                }
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setPiece" => {
                let pt_name = params
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'type'")?;
                let pt = PieceType::from_name(pt_name).ok_or("Invalid piece type")?;
                let rot_idx = params.get("rotation").and_then(|v| v.as_u64()).unwrap_or(0);
                let rot = match rot_idx {
                    0 => Rot::N,
                    1 => Rot::E,
                    2 => Rot::S,
                    3 => Rot::W,
                    _ => return Err("Rotation must be 0-3".to_string()),
                };
                let x = params
                    .get("x")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing 'x'")? as i32;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing 'y'")? as i32;
                let piece = Piece {
                    piece_type: pt,
                    rotation: rot,
                    x,
                    y,
                };
                self.ghost = ghost_y(&piece, &self.grid);
                self.current = Some(piece);
                self.lock_active = false;
                self.lock_timer = LOCK_DELAY;
                self.lock_resets = 0;
                self.last_was_rotation = false;
                self.gravity_timer = gravity_interval(self.level);
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setNextQueue" => {
                let queue_arr = params
                    .get("queue")
                    .and_then(|v| v.as_array())
                    .ok_or("Missing 'queue' array")?;
                self.next_queue.clear();
                for val in queue_arr {
                    let name = val.as_str().ok_or("Queue elements must be strings")?;
                    let pt = PieceType::from_name(name).ok_or("Invalid piece type")?;
                    self.next_queue.push_back(pt);
                }
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setHoldPiece" => {
                let piece_val = params.get("piece").ok_or("Missing 'piece'")?;
                if piece_val.is_null() {
                    self.hold_piece = None;
                } else {
                    let name = piece_val.as_str().ok_or("'piece' must be null or string")?;
                    self.hold_piece = Some(PieceType::from_name(name).ok_or("Invalid piece type")?);
                }
                self.hold_used = params
                    .get("hold_used")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(serde_json::json!({"status": "ok"}))
            }
            "game.setScore" => {
                if let Some(v) = params.get("score").and_then(|v| v.as_u64()) {
                    self.score = v as u32;
                }
                if let Some(v) = params.get("level").and_then(|v| v.as_u64()) {
                    self.level = v.max(1) as u32;
                }
                if let Some(v) = params.get("lines").and_then(|v| v.as_u64()) {
                    self.lines_cleared = v as u32;
                }
                if let Some(v) = params.get("combo").and_then(|v| v.as_i64()) {
                    self.combo = v as i32;
                }
                if let Some(v) = params.get("back_to_back").and_then(|v| v.as_bool()) {
                    self.back_to_back = v;
                }
                Ok(serde_json::json!({
                    "score": self.score,
                    "level": self.level,
                    "lines": self.lines_cleared,
                }))
            }
            "game.setPhase" => {
                let phase = params
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'phase'")?;
                match phase {
                    "playing" => self.phase = GamePhase::Playing,
                    "game_over" => self.phase = GamePhase::GameOver,
                    _ => return Err(format!("Unknown phase: {}", phase)),
                }
                Ok(serde_json::json!({"phase": phase}))
            }
            _ => Err(format!("Unknown method: {}", method)),
        }
    }
}

fn main() {
    vibe2d::run::<TetrisGame>("game.yaml");
}
