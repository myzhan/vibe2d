//! The title screen: pick a mappack and a level, and remember what you reached.
//!
//! Not the original's menu — that one is a scrolling grid of mappack icons
//! (`menu.lua`) with its own art, and none of that art ships in this port's assets.
//! What it *does* reproduce is the only thing the menu is load-bearing for: the
//! original lets you choose a mappack and start it, and it remembers your progress
//! between runs. Without that the nine lab levels were unreachable except through the
//! debug protocol.
//!
//! Progress is per mappack, and only ever moves forward: reaching 1-3 unlocks 1-1
//! through 1-3, and dying back to 1-1 doesn't take it away again.

use vibe2d::prelude::*;

use crate::game::{GameState, Mari0Game};
use crate::hats::HATS;
use crate::level;
use crate::player::PlayerType;

/// The mappacks the build ships with, in menu order: the two originals and all six DLC.
///
/// Four of the DLC packs carry a `tiles.png` of their own, which is why `build.rs` decodes
/// a properties block per pack — a tilesheet is not just art, each cell's 17th column
/// holds that tile's collision and breakability. `acid_trip` additionally ships 24
/// parallax backgrounds that this port does not draw; its levels play, they just have a
/// flat backdrop.
pub(crate) const PACKS: [&str; 8] = [
    "smb",
    "portal",
    "escape_the_lab",
    "scienceandstuff",
    "a_portal_tribute",
    "smb2J",
    "the_untitled_game",
    "acid_trip",
];

/// Storage keys. Bumping these resets everyone's save, so they are written once here.
const KEY_HIGH_SCORE: &str = "high_score";
const KEY_FURTHEST: &str = "furthest";

/// Where the cursor is on the title screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
pub(crate) struct MenuCursor {
    /// Index into [`PACKS`].
    pub(crate) pack: usize,
    /// World and level within the pack, 1-based, as the level files name them.
    pub(crate) world: u32,
    pub(crate) level: u32,
}

impl Default for MenuCursor {
    fn default() -> Self {
        MenuCursor {
            pack: 0,
            world: 1,
            level: 1,
        }
    }
}

impl MenuCursor {
    pub(crate) fn pack_name(self) -> &'static str {
        PACKS[self.pack % PACKS.len()]
    }

    /// Does the pack contain the level the cursor names?
    fn exists(self) -> bool {
        level::raw_level(self.pack_name(), &format!("{}-{}", self.world, self.level)).is_some()
    }

    /// Step to the next existing level, wrapping at the end of the pack.
    ///
    /// Walks rather than computes, because a mappack's shape is not a rectangle: the
    /// original defines "the end" as *the next file not existing* and so must this.
    fn advance(mut self, forward: bool) -> Self {
        for _ in 0..64 {
            if forward {
                self.level += 1;
                if !self.exists() {
                    self.level = 1;
                    self.world += 1;
                    if !self.exists() {
                        self.world = 1;
                    }
                }
            } else if self.level > 1 {
                self.level -= 1;
            } else {
                self.world = if self.world > 1 { self.world - 1 } else { 8 };
                // Walk to the last level of that world.
                self.level = 1;
                while {
                    let next = MenuCursor {
                        level: self.level + 1,
                        ..self
                    };
                    next.exists()
                } {
                    self.level += 1;
                }
                if !self.exists() {
                    continue;
                }
            }
            if self.exists() {
                return self;
            }
        }
        MenuCursor {
            pack: self.pack,
            ..Default::default()
        }
    }
}

impl Mari0Game {
    /// Read the saved progress. Called once, at startup.
    pub(crate) fn load_progress(&mut self) {
        self.high_score = self.storage.get_or(KEY_HIGH_SCORE, 0);
        self.furthest = self
            .storage
            .get_or(KEY_FURTHEST, Vec::<(String, u32, u32)>::new());
    }

    /// The furthest level reached in a pack, or 1-1 if it has never been played.
    pub(crate) fn furthest_in(&self, pack: &str) -> (u32, u32) {
        self.furthest
            .iter()
            .find(|(p, _, _)| p == pack)
            .map(|&(_, w, l)| (w, l))
            .unwrap_or((1, 1))
    }

    /// Record reaching a level, and the score if it beats the best. Only moves forward.
    pub(crate) fn record_progress(&mut self) {
        let pack = self.current.pack.clone();
        let (world, level) = (self.current.world, self.current.level);
        let better = |a: (u32, u32), b: (u32, u32)| (a.0, a.1) > (b.0, b.1);
        match self.furthest.iter_mut().find(|(p, _, _)| *p == pack) {
            Some(entry) if better((world, level), (entry.1, entry.2)) => {
                entry.1 = world;
                entry.2 = level;
            }
            Some(_) => {}
            None => self.furthest.push((pack, world, level)),
        }
        if self.score > self.high_score {
            self.high_score = self.score;
        }
        self.storage.set(KEY_HIGH_SCORE, self.high_score);
        self.storage.set(KEY_FURTHEST, self.furthest.clone());
        // Written through immediately: a run can end with the window being closed, and
        // there is no shutdown hook to flush from.
        let _ = self.storage.save();
    }

    /// One frame of the title screen.
    /// The konami code, in the original's own order (`variables.lua:381`).
    ///
    /// Bound to this port's action names rather than raw keys, so it works on a gamepad
    /// too: the last two are B and A on a NES pad, which are `jump` and `fire` here.
    const KONAMI: [&str; 10] = [
        "climb_up",
        "climb_up",
        "crouch",
        "crouch",
        "move_left",
        "move_right",
        "move_left",
        "move_right",
        "fire",
        "jump",
    ];

    /// Watch for the konami code on the title screen, and unlock every level if it lands.
    ///
    /// `gamefinished = true` in the original (`main.lua:1275`) — it marks the whole mappack
    /// as reached, which is what the level picker gates on. Any wrong key resets the
    /// sequence to the start.
    /// Returns true if the code just completed, in which case this frame's keypress is
    /// **consumed**.
    ///
    /// The original's last two keys are B and A, which do nothing on its title screen. The
    /// nearest equivalents here are `fire` and `jump` — and `jump` is what starts a level,
    /// so without swallowing it the code can never be finished without also launching into
    /// 1-1. Eating the frame is the smaller lie than renaming the keys.
    fn check_konami(&mut self, ctx: &mut Context, input: &InputState) -> bool {
        // A key that is part of the sequence but out of order still counts as wrong, so the
        // check is "is the expected key the one pressed" rather than "was any key pressed".
        let expected = Self::KONAMI[self.konami_index];
        let pressed: Vec<&str> = Self::KONAMI
            .iter()
            .copied()
            .chain(["use", "pause"])
            .filter(|a| input.is_action_just_pressed(a))
            .collect();
        if pressed.is_empty() {
            return false;
        }
        if pressed.contains(&expected) {
            self.konami_index += 1;
            if self.konami_index == Self::KONAMI.len() {
                self.konami_index = 0;
                ctx.audio.play("konami");
                self.unlock_everything();
                return true;
            }
        } else {
            self.konami_index = 0;
        }
        false
    }

    /// Mark every world of every mappack as reached.
    ///
    /// `gamefinished = true` in the original — it is the flag the level picker gates on, so
    /// the code's whole effect is "you may go anywhere". 8-4 covers all of smb, and the
    /// lab's nine levels sit inside worlds 1-2, so the same figure unlocks both packs.
    fn unlock_everything(&mut self) {
        for pack in PACKS {
            match self.furthest.iter_mut().find(|(p, _, _)| p == pack) {
                Some(entry) => {
                    entry.1 = 8;
                    entry.2 = 4;
                }
                None => self.furthest.push((pack.to_string(), 8, 4)),
            }
        }
        self.storage.set(KEY_FURTHEST, self.furthest.clone());
        let _ = self.storage.save();
    }

    pub(crate) fn update_menu(&mut self, ctx: &mut Context, input: &InputState) {
        if self.check_konami(ctx, input) {
            return;
        }

        // Left/right walks the levels, up/down switches mappack — the pack list is
        // short and the level list is long, which is the way round the keys suggest.
        if input.is_action_just_pressed("move_right") {
            self.menu = self.menu.advance(true);
        }
        if input.is_action_just_pressed("move_left") {
            self.menu = self.menu.advance(false);
        }
        if input.is_action_just_pressed("crouch") || input.is_action_just_pressed("use") {
            self.menu.pack = (self.menu.pack + 1) % PACKS.len();
            // Back to the start of the pack rather than to where you left off.
            //
            // Resuming sounds friendlier and was the first version, but it makes "press
            // start" mean a different level depending on the save file — which is a
            // non-deterministic entry point for anything scripted, and it broke the
            // autopilot regression run the first time round. The furthest level is shown
            // on screen instead; walking to it is two keys.
            self.menu.world = 1;
            self.menu.level = 1;
        }
        // The gel cannon. In the original this is one of `playertypelist`'s three
        // entries on the same menu (`menu.lua:1867-1885`); here it is a toggle, because
        // the third entry — `minecraft` — is a whole separate mode and is not ported.
        if input.is_action_just_pressed("fire") {
            self.player_type = match self.player_type {
                PlayerType::Portal => PlayerType::GelCannon,
                PlayerType::GelCannon => PlayerType::Portal,
            };
        }
        // The rainboom toggle. In the original this is a checkbox in the options menu; it
        // shares that menu's fate for now and lives here as a key, next to the loadout.
        if input.is_action_just_pressed("pause") {
            self.sonic_rainboom = !self.sonic_rainboom;
        }
        // Hats. The original stacks them in a per-player skin editor (`menu.lua:751-761`)
        // that also recolours Mario; without that screen this cycles the one hat, and
        // walks off the end of the list into wearing none before coming back round.
        if input.is_action_just_pressed("hat") {
            let next = self.hat_selection.first().map_or(1, |&i| i as usize + 1);
            self.hat_selection = if next > HATS.len() {
                Vec::new()
            } else {
                vec![next as u8]
            };
            // Nobody is wearing anything on the title screen, but keeping the two in step
            // means the selection is what starts the level even without a reload.
            self.hats = self.hat_selection.clone();
        }
        if input.is_action_just_pressed("jump") {
            self.start_selected();
        }
    }

    /// Begin the level the cursor is on.
    pub(crate) fn start_selected(&mut self) {
        self.current =
            crate::world::LevelId::new(self.menu.pack_name(), self.menu.world, self.menu.level);
        self.state = GameState::Playing;
        self.score = 0;
        self.coins = 0;
        self.lives = 3;
        self.start_fresh();
        self.record_progress();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walking forward off the end of a world rolls into the next one, and off the end
    /// of the pack wraps to the start. Nothing in between may name a level that isn't
    /// there.
    #[test]
    fn the_cursor_only_ever_lands_on_a_level_that_exists() {
        for pack in 0..PACKS.len() {
            let mut cursor = MenuCursor {
                pack,
                ..Default::default()
            };
            for _ in 0..80 {
                cursor = cursor.advance(true);
                assert!(
                    cursor.exists(),
                    "{}: {}-{} does not exist",
                    cursor.pack_name(),
                    cursor.world,
                    cursor.level
                );
            }
            for _ in 0..80 {
                cursor = cursor.advance(false);
                assert!(cursor.exists(), "walking back left the pack");
            }
        }
    }

    /// The lab pack is 3 worlds of 4/4/1 levels, so a forward walk from 1-1 visits nine
    /// levels and comes back. That count is the pack's shape, not a guess.
    #[test]
    fn walking_the_lab_pack_visits_all_nine_levels() {
        let mut cursor = MenuCursor {
            pack: 1,
            ..Default::default()
        };
        let mut seen = vec![(cursor.world, cursor.level)];
        for _ in 0..8 {
            cursor = cursor.advance(true);
            seen.push((cursor.world, cursor.level));
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 9, "nine distinct lab levels: {seen:?}");
        assert_eq!(
            cursor.advance(true),
            MenuCursor {
                pack: 1,
                world: 1,
                level: 1
            }
        );
    }
}
