//! The black screens between levels — and the four-second death they follow from.
//!
//! `levelscreen.lua` is one function driving every gap in the game: the "world 1-1" card,
//! the flicker between a level and its sublevel, "game over", and the mappack-finished
//! message. They differ only in how long they last and what is printed in the middle, so
//! they are one state here too.
//!
//! Two details are the whole reason it feels like the original rather than a delay:
//!
//! - **The text is inset from the black.** Nothing is drawn in the first or last
//!   [`BLACKTIME_SUB`] of the screen (`levelscreen.lua:106`), so every card fades in out
//!   of black and back out into it. It also means the 0.2-second sublevel screen — whose
//!   whole duration is `2 × BLACKTIME_SUB` — **never draws anything at all**. That is why
//!   taking a pipe reads as a blink and taking a flagpole reads as a card.
//! - **The first level of a world lingers 50% longer** (`levelscreen.lua:60-62`), which
//!   is what gives "world 2-1" its extra beat over "world 2-2".
//!
//! The level is already loaded by the time an interlude starts. That keeps this module a
//! timer and a draw rather than a scheduler: whoever begins the interlude has already
//! done the work, and the screen is just held over the top of it.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::{GameState, Mari0Game};

/// The launch intro: Stabyourself's logo, stabbed.
///
/// Not one of the cards — it has no HUD, its own fade, and it only ever runs once, at
/// launch. It lives here because it is the same kind of thing: a timed screen that owns
/// the frame and hands over to something else.
///
/// The timer starts **negative** (`introprogress = -0.2`), so there is a beat of black
/// before the fade begins.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Intro {
    pub(crate) timer: f32,
    /// Has the stab sound played? Once only.
    pub(crate) stabbed: bool,
}

impl Intro {
    /// Opacity of the logo: fading in at the start, out at the end, solid between.
    pub(crate) fn alpha(&self) -> f32 {
        if self.timer < 0.0 || self.timer >= INTRO_DURATION {
            return 0.0;
        }
        if self.timer < INTRO_FADE_TIME {
            self.timer / INTRO_FADE_TIME
        } else if self.timer >= INTRO_DURATION - INTRO_FADE_TIME {
            1.0 - (self.timer - (INTRO_DURATION - INTRO_FADE_TIME)) / INTRO_FADE_TIME
        } else {
            1.0
        }
    }

    /// How far the blood has wiped up the logo, in pixels — 0 before the stab, and the
    /// full height once the fade-out begins.
    ///
    /// The wipe is a scissor whose *height* grows from the logo's bottom edge upward
    /// (`intro.lua:53-56`), which is why it reads as blood running up rather than the
    /// image cross-fading.
    pub(crate) fn blood_wipe(&self) -> f32 {
        if self.timer >= INTRO_DURATION - INTRO_FADE_TIME {
            return INTRO_BLOOD_SPAN;
        }
        if self.timer <= INTRO_FADE_TIME + 0.3 {
            return 0.0;
        }
        (self.timer - 0.2 - INTRO_FADE_TIME) / (INTRO_DURATION - 2.0 * INTRO_FADE_TIME)
            * INTRO_BLOOD_SPAN
    }
}

/// Which card is being held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum InterludeKind {
    /// "world 1-1", the puppet and the life count. Before every level and every respawn.
    LevelScreen,
    /// The blink between a level and one of its sublevels. Prints nothing — see above.
    Sublevel,
    /// "game over". Ends at the title screen rather than in a level.
    GameOver,
    /// "congratulations!". Same, and the only place the princess theme plays.
    MappackFinished,
}

impl InterludeKind {
    /// Does this one hand back to the level, or to the title screen?
    fn returns_to_menu(self) -> bool {
        matches!(
            self,
            InterludeKind::GameOver | InterludeKind::MappackFinished
        )
    }
}

/// A black screen in progress.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Interlude {
    pub(crate) kind: InterludeKind,
    pub(crate) timer: f32,
    /// How long it lasts. Not a constant: the level screen stretches for the first level
    /// of a world.
    pub(crate) total: f32,
}

impl Interlude {
    /// Is the card's text showing? False during the lead-in and lead-out.
    pub(crate) fn text_visible(&self) -> bool {
        self.timer > BLACKTIME_SUB && self.timer < self.total - BLACKTIME_SUB
    }
}

/// Mario's death throw.
///
/// Four seconds, and the shape of it is the point: he hangs still for
/// [`DEATH_JUMP_TIME`], then is thrown upward and falls out of the level under a gravity
/// of his own — [`DEATH_GRAVITY`] is half the world's, so the arc is slow and readable.
/// A death **down a pit** skips the throw entirely (`mario.lua:596`): he is already
/// falling, and throwing him would bounce him back up out of the hole.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeathAnim {
    pub(crate) timer: f32,
    pub(crate) vy: f32,
    /// Did he go down a hole? Then there is no throw.
    pub(crate) pit: bool,
}

impl Mari0Game {
    /// Run the launch intro. Returns true while it owns the frame.
    ///
    /// Skippable by any key — but only after the first frame (`allowskip`), so the key
    /// that launched the game cannot dismiss it before it has been seen.
    pub(crate) fn update_intro(&mut self, ctx: &mut Context, dt: f32, any_key: bool) -> bool {
        let Some(mut intro) = self.intro else {
            return false;
        };
        let skippable = intro.timer > INTRO_START;
        intro.timer += dt;
        if !intro.stabbed && intro.timer > INTRO_STAB_TIME {
            intro.stabbed = true;
            ctx.audio.play("stab");
        }
        if (skippable && any_key) || intro.timer >= INTRO_DURATION + INTRO_BLACK_AFTER {
            self.intro = None;
            self.state = GameState::Menu;
            return true;
        }
        self.intro = Some(intro);
        true
    }

    /// Begin a black screen. The level behind it must already be loaded.
    ///
    /// Takes no `Context`, because two of its four callers are level loads that have none
    /// — so the one card with a soundtrack raises a flag instead and `update_interlude`
    /// starts the track on its first frame.
    pub(crate) fn begin_interlude(&mut self, kind: InterludeKind) {
        let total = match kind {
            InterludeKind::LevelScreen => {
                // The first level of a world holds 50% longer. `mariolevel == 1` covers
                // 1-1 through 8-1; `marioworld == 1` covers the whole of world 1.
                let stretch = self.current.world == 1 || self.current.level == 1;
                LEVELSCREEN_TIME * if stretch { 1.5 } else { 1.0 }
            }
            InterludeKind::Sublevel => SUBLEVELSCREEN_TIME,
            InterludeKind::GameOver | InterludeKind::MappackFinished => GAMEOVER_TIME,
        };
        self.interlude = Some(Interlude {
            kind,
            timer: 0.0,
            total,
        });
        // The princess theme is this card's whole soundtrack and plays nowhere else in the
        // game (`levelscreen.lua:40`). Every other card is silent — a level's own theme is
        // stopped by the load and only restarts when the card lifts.
        self.pending_music = Some(match kind {
            InterludeKind::MappackFinished => Some("princess"),
            _ => None,
        });
        self.state = GameState::Interlude;
    }

    /// Hold the black screen, then hand back to the level or to the title.
    pub(crate) fn update_interlude(&mut self, ctx: &mut Context, dt: f32) {
        let Some(mut card) = self.interlude else {
            // No card but the state says there is one: recover rather than freeze.
            self.state = GameState::Playing;
            return;
        };
        // First frame of the card: start (or silence) its soundtrack.
        if let Some(track) = self.pending_music.take() {
            match track {
                Some(name) => ctx.audio.play_music(name),
                None => ctx.audio.stop_music(),
            }
        }
        card.timer += dt;
        // `-epsilon` in the original, to keep the delay from drifting with float error
        // (`levelscreen.lua:88`). Comparing against the accumulated timer directly is the
        // same guard here.
        if card.timer >= card.total {
            self.interlude = None;
            if card.kind.returns_to_menu() {
                self.state = GameState::Menu;
            } else {
                self.state = GameState::Playing;
                // Deferred to here rather than to the load, so the theme starts with the
                // level and not underneath the card.
                self.start_music(ctx);
            }
            return;
        }
        self.interlude = Some(card);
        // The coin in the HUD keeps spinning behind the card, as it does in the original.
        self.coin_spin += dt;
    }

    /// Drive the death throw. Returns true while it owns the frame.
    pub(crate) fn update_death(&mut self, ctx: &mut Context, dt: f32) -> bool {
        let Some(mut d) = self.death else {
            return false;
        };
        let before = d.timer;
        d.timer += dt;

        if !d.pit {
            // The throw lands once, on the frame the timer crosses the mark.
            if before <= DEATH_JUMP_TIME && d.timer > DEATH_JUMP_TIME {
                d.vy = -DEATH_JUMP_FORCE;
            }
            if d.timer > DEATH_JUMP_TIME {
                d.vy += DEATH_GRAVITY * dt;
                self.player.y += d.vy * dt;
            }
        }

        if d.timer > DEATH_TOTAL_TIME {
            self.death = None;
            if self.lives > 0 {
                // Reload first, then hold the card over it.
                self.respawn_after_death();
                self.begin_interlude(InterludeKind::LevelScreen);
            } else {
                // A game over clears the run-scoped progress (`levelscreen.lua:49`).
                self.checkpoint = None;
                self.respawn_sublevel = 0;
                ctx.audio.play("gameover");
                self.begin_interlude(InterludeKind::GameOver);
            }
            return true;
        }

        self.death = Some(d);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The timings are the original's.
    #[test]
    fn the_timings_are_the_originals() {
        assert_eq!(LEVELSCREEN_TIME, 2.4);
        assert_eq!(SUBLEVELSCREEN_TIME, 0.2);
        assert_eq!(GAMEOVER_TIME, 7.0);
        assert_eq!(BLACKTIME_SUB, 0.1);
        assert_eq!(DEATH_TOTAL_TIME, 4.0);
        assert_eq!(DEATH_JUMP_TIME, 0.3);
    }

    /// The sublevel blink is exactly two lead-ins long, so it can never show text. That
    /// is not a coincidence to be tidied up — it is what makes a pipe a blink.
    #[test]
    fn the_sublevel_blink_never_shows_anything() {
        let card = Interlude {
            kind: InterludeKind::Sublevel,
            timer: 0.0,
            total: SUBLEVELSCREEN_TIME,
        };
        assert_eq!(SUBLEVELSCREEN_TIME, 2.0 * BLACKTIME_SUB);
        for i in 0..=20 {
            let mut c = card;
            c.timer = SUBLEVELSCREEN_TIME * i as f32 / 20.0;
            assert!(!c.text_visible(), "showed text at t={}", c.timer);
        }
    }

    /// A level card, by contrast, shows its text for all but the two ends.
    #[test]
    fn a_level_card_shows_its_text_in_the_middle() {
        let card = Interlude {
            kind: InterludeKind::LevelScreen,
            timer: 0.0,
            total: LEVELSCREEN_TIME,
        };
        let mut c = card;
        c.timer = 0.05;
        assert!(!c.text_visible(), "still fading in");
        c.timer = LEVELSCREEN_TIME / 2.0;
        assert!(c.text_visible());
        c.timer = LEVELSCREEN_TIME - 0.05;
        assert!(!c.text_visible(), "already fading out");
    }

    /// Only the two terminal cards end at the title screen.
    #[test]
    fn only_the_terminal_cards_go_back_to_the_menu() {
        assert!(InterludeKind::GameOver.returns_to_menu());
        assert!(InterludeKind::MappackFinished.returns_to_menu());
        assert!(!InterludeKind::LevelScreen.returns_to_menu());
        assert!(!InterludeKind::Sublevel.returns_to_menu());
    }

    /// The throw is upward and the fall that follows is gentler than the world's, which is
    /// what makes the arc readable rather than a snap.
    #[test]
    fn the_death_throw_is_slower_than_falling() {
        const { assert!(DEATH_GRAVITY < GRAVITY) };
        const { assert!(DEATH_JUMP_FORCE > 0.0) };
        // And it happens well inside the four seconds, so there is time to watch it.
        const { assert!(DEATH_JUMP_TIME < DEATH_TOTAL_TIME / 2.0) };
    }
}
