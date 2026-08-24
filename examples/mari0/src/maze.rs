//! The looping castles: 4-4, 7-4 and the 8-4 rooms.
//!
//! These levels appear to loop, but nothing loops. When the camera reaches the end
//! of an unsolved maze span, the game splices a copy of that span's columns into
//! the map at the camera frontier (`game.lua:565-672`), pushing the rest of the
//! level right. The player walks forward the whole time; the corridor simply never
//! runs out. Solve the span and the splicing stops, letting you out.
//!
//! ## What "solving" means
//!
//! `mazegate` cells carry a number. Walking your centre through them in ascending
//! order advances a counter; the wrong one resets it to zero (`mario.lua:888-899`).
//! A span is solved when the counter reaches the highest gate number inside it.
//!
//! The three levels use this very differently:
//!
//! | level | gates | how you get through |
//! |---|---|---|
//! | 4-4 | all numbered `1` | one gate is enough — the gates *mark the correct path* |
//! | 7-4 | `1`, `2`, `3` | walk them in order, wrong turn resets you |
//! | 8-4 | **none at all** | the span can never be solved; you leave by pipe |
//!
//! 8-4 having no gates is deliberate, not a data bug. The gate count floors at 1
//! (`game.lua:2120`) and nothing can raise the counter, so the corridor repeats
//! forever — which is exactly the famous water-room loop, escaped through the pipe
//! at column 82, not by walking.

use crate::constants::*;
use crate::game::Mari0Game;

/// Progress through the maze spans of the current level.
#[derive(Debug, Clone, Default)]
pub(crate) struct MazeState {
    /// Highest gate number walked in sequence so far.
    pub(crate) var: u32,
    /// Which spans have been solved, indexed as `maze_starts`.
    pub(crate) solved: Vec<bool>,
    /// Column being copied next. `None` until a repetition starts.
    pub(crate) repeat_from: Option<i32>,
    /// Rightmost column the splicer has considered.
    pub(crate) last_repeat: i32,
    /// A repetition is mid-flight; it runs to the span's end even if the player
    /// solves the maze partway through.
    pub(crate) in_progress: bool,
}

impl MazeState {
    pub(crate) fn for_level(span_count: usize) -> Self {
        Self {
            var: 0,
            solved: vec![false; span_count],
            repeat_from: None,
            last_repeat: -1,
            in_progress: false,
        }
    }
}

impl Mari0Game {
    /// Advance the player's gate sequence for the cell they're standing in.
    ///
    /// Three outcomes, all from `mario.lua:891-898`: the next gate in order
    /// advances the counter, the gate you're already inside changes nothing, and
    /// anything else resets to zero.
    pub(crate) fn check_maze_gate(&mut self) {
        if self.level.maze_gates.is_empty() {
            return;
        }
        let cell = (
            (self.player.center_x() / TILE_SIZE).floor() as i32,
            (self.player.center_y() / TILE_SIZE).floor() as i32,
        );
        let Some(gate) = self.level.maze_gates.get(&cell).copied() else {
            return;
        };
        if gate == self.maze.var + 1 {
            self.maze.var += 1;
        } else if gate != self.maze.var {
            self.maze.var = 0;
        }
    }

    /// Extend the corridor for every column the camera has newly reached.
    pub(crate) fn update_maze(&mut self) {
        if self.level.maze_starts.is_empty()
            || self.level.maze_starts.len() != self.level.maze_ends.len()
        {
            return;
        }
        let target = (self.camera.x / TILE_SIZE).floor() as i32 + 2;
        // Bounded per frame. In play the camera advances a column at a time and the
        // budget is never touched; it only bites when the camera *jumps* — a VDP
        // teleport, or a respawn at a checkpoint far into the level. The trade-off
        // is that a jump silently skips the columns it flew over rather than
        // splicing all of them, which costs a few repeats nobody was going to see
        // and avoids a multi-thousand-column stall.
        let mut budget = 64;
        while self.maze.last_repeat < target && budget > 0 {
            self.maze.last_repeat += 1;
            self.maze_step(self.maze.last_repeat);
            budget -= 1;
        }
        self.maze.last_repeat = self.maze.last_repeat.max(target);
    }

    /// One column's worth of maze bookkeeping.
    fn maze_step(&mut self, current: i32) {
        let screen_cols = (self.vw / TILE_SIZE).ceil() as i32;
        let frontier = current + screen_cols;

        // The span in play is the last one whose *end* is already behind the
        // frontier. Before you reach a maze's exit there's nothing to repeat, which
        // is why the original seeds `mazesolved[0] = true` for "no span yet".
        let Some(span) =
            (0..self.level.maze_ends.len()).rfind(|i| self.level.maze_ends[*i] < frontier)
        else {
            return;
        };

        // Solved? The counter has to reach the gate count of the span the *player*
        // is standing in, which needn't be the one being spliced.
        if self.maze.var == self.level.maze_gate_counts[span] {
            let player_col = (self.player.x / TILE_SIZE).floor() as i32;
            let standing_in = (0..self.level.maze_starts.len())
                .rfind(|i| player_col > self.level.maze_starts[*i]);
            if let Some(i) = standing_in {
                self.maze.solved[i] = true;
            }
            self.maze.var = 0;
        }

        let solved = self.maze.solved[span];
        if solved && !self.maze.in_progress {
            return;
        }
        if !solved {
            self.maze.in_progress = true;
        }

        let source = *self
            .maze
            .repeat_from
            .get_or_insert(self.level.maze_starts[span]);
        self.level.insert_column(frontier, source);

        // A copied `mazeend` closes the repetition. Solving mid-flight moves the
        // source to the next span; otherwise we wrap to this span's start.
        if self.level.maze_end_cols.contains(&source) {
            self.maze.in_progress = false;
            self.maze.repeat_from = if self.maze.solved[span] {
                self.level.maze_starts.get(span + 1).copied()
            } else {
                // Wrapping is a deliberate correction. The original only ever
                // rewinds `repeatX` on the solved branch, so an unsolved span walks
                // its source pointer off the end and starts copying whatever
                // follows — which happens to look maze-like in 4-4 and 7-4 but
                // isn't the intended loop. Wrapping keeps the corridor made of the
                // span the player is actually stuck in.
                Some(self.level.maze_starts[span])
            };
        } else {
            self.maze.repeat_from = Some(source + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::level;
    use crate::world::load_level;

    /// The maze levels are 4-4, 7-4 and the 8-4 rooms — *not* 3-4 and 6-4, which is
    /// what the plan assumed before the data was checked.
    #[test]
    fn only_these_levels_have_maze_spans() {
        let mut found: Vec<&str> = Vec::new();
        for (pack, name, _) in level::original_levels() {
            if !load_level(pack, name).maze_starts.is_empty() {
                found.push(name);
            }
        }
        found.sort_unstable();
        assert_eq!(found, ["4-4", "7-4", "8-4", "8-4_2", "8-4_3"]);
    }

    /// Every span must be a matched, non-empty, forward range, or the splicer would
    /// copy from nowhere.
    #[test]
    fn spans_are_matched_and_ordered() {
        for (pack, name, _) in level::original_levels() {
            let lv = load_level(pack, name);
            assert_eq!(
                lv.maze_starts.len(),
                lv.maze_ends.len(),
                "{pack}/{name}: unmatched maze spans"
            );
            for (start, end) in lv.maze_starts.iter().zip(&lv.maze_ends) {
                assert!(
                    start < end,
                    "{pack}/{name}: span {start}..{end} not forward"
                );
                assert!(
                    (*end as usize) < lv.width,
                    "{pack}/{name}: span end {end} past width {}",
                    lv.width
                );
            }
        }
    }

    /// 4-4 needs one gate, 7-4 needs three. These are the numbers that decide
    /// whether the corridor ever lets you out.
    #[test]
    fn gate_counts_match_the_level_design() {
        let four = load_level("smb", "4-4");
        assert_eq!(four.maze_gate_counts, vec![1, 1], "4-4 gates are all `1`");

        let seven = load_level("smb", "7-4");
        assert_eq!(
            seven.maze_gate_counts,
            vec![3, 3],
            "7-4 wants gates 1, 2 and 3 in order"
        );
    }

    /// 8-4's spans have no gates, so the floor-of-1 makes them unsolvable. That is
    /// the intended design — the exit is a pipe.
    #[test]
    fn the_8_4_rooms_are_unsolvable_by_design_and_exit_by_pipe() {
        for name in ["8-4", "8-4_2", "8-4_3"] {
            let lv = load_level("smb", name);
            assert!(
                lv.maze_gates.is_empty(),
                "{name} should have no maze gates at all"
            );
            assert_eq!(
                lv.maze_gate_counts,
                vec![1],
                "{name}: the count floors at 1, which nothing can reach"
            );
            assert!(
                !lv.pipes.is_empty(),
                "{name} must offer a pipe, since walking can never solve it"
            );
        }
    }

    /// Splicing a column widens the level and shifts everything to its right —
    /// including the tables this port hoisted out of the tile grid.
    #[test]
    fn inserting_a_column_shifts_everything_to_its_right() {
        let mut lv = load_level("smb", "8-4");
        let width_before = lv.width;
        let pipes_before: Vec<(i32, i32)> = {
            let mut v: Vec<(i32, i32)> = lv.pipes.keys().copied().collect();
            v.sort_unstable();
            v
        };
        let ends_before = lv.maze_ends.clone();

        // Splice at column 60: the pipe at 82 moves, the one at 52 does not.
        let at = 60;
        let source = lv.maze_starts[0];
        let source_column: Vec<u32> = lv.tiles.iter().map(|row| row[source as usize]).collect();
        lv.insert_column(at, source);

        assert_eq!(lv.width, width_before + 1);
        for (row, expected) in lv.tiles.iter().zip(&source_column) {
            assert_eq!(row[at as usize], *expected, "spliced column is a copy");
        }

        let mut pipes_after: Vec<(i32, i32)> = lv.pipes.keys().copied().collect();
        pipes_after.sort_unstable();
        let expected: Vec<(i32, i32)> = pipes_before
            .iter()
            .map(|(c, r)| (if *c >= at { c + 1 } else { *c }, *r))
            .collect();
        assert_eq!(pipes_after, expected, "pipes right of the splice must move");

        for (before, after) in ends_before.iter().zip(&lv.maze_ends) {
            let want = if *before >= at { before + 1 } else { *before };
            assert_eq!(*after, want, "span ends shift with the map");
        }
    }

    /// Splicing must never leave a row shorter than the level claims to be, or the
    /// tile lookups would read out of bounds.
    #[test]
    fn every_row_stays_as_wide_as_the_level() {
        let mut lv = load_level("smb", "4-4");
        for i in 0..20 {
            let source = lv.maze_starts[0] + i;
            lv.insert_column(lv.maze_ends[0], source);
        }
        for row in &lv.tiles {
            assert_eq!(row.len(), lv.width);
        }
    }
}
