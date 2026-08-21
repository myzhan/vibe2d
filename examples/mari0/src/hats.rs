//! Hats — the cosmetic layer drawn on top of Mario, and the only thing in the game
//! that is purely decoration.
//!
//! Ported from `hatconfigs.lua` (small Mario) and `bighatconfigs.lua` (big), with the
//! draw arithmetic from `game.lua:1306-1352`. The original lets you stack them in a skin
//! editor this port does not have, so the stack is kept but only ever one deep in
//! practice — the menu picks one, and [`crate::portal`]'s rainboom awards one.

use crate::player::PlayerAnim;

/// How a hat sits on one of Mario's two sizes, in the original's unscaled pixels.
pub(crate) struct HatFit {
    /// Where the image goes against the sprite cell. Larger `x` is further right, larger
    /// `y` further *down* — both are subtracted from the draw origin in the Lua, so the
    /// signs read backwards there.
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// How far the *next* hat in a stack is lifted (`yadd`, `game.lua:1346`). Deliberately
    /// not the image height: a hat's brim is meant to overlap what it sits on.
    pub(crate) height: f32,
    /// The image's own size. Each hat is its own texture, so this is the whole of it.
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// One hat at both sizes, plus the names the texture keys and the menu are built from.
pub(crate) struct HatDef {
    /// Matches the original's file name, and so the `hat_*` / `bighat_*` texture keys.
    pub(crate) name: &'static str,
    /// What the title screen calls it.
    pub(crate) label: &'static str,
    pub(crate) small: HatFit,
    pub(crate) big: HatFit,
}

/// Every hat, in `hatconfigs.lua`'s order. That order is the wire format — a hat is
/// referred to by its 1-based index everywhere, including over VDP — so entries may be
/// appended but never reordered.
pub(crate) const HATS: [HatDef; 33] = [
    // 1
    HatDef {
        name: "standard",
        label: "STANDARD",
        small: HatFit {
            x: 7.0,
            y: 2.0,
            height: 2.0,
            w: 9.0,
            h: 2.0,
        },
        big: HatFit {
            x: 0.0,
            y: 0.0,
            height: 4.0,
            w: 11.0,
            h: 4.0,
        },
    },
    // 2
    HatDef {
        name: "tyrolean",
        label: "TYROLEAN",
        small: HatFit {
            x: 5.0,
            y: -3.0,
            height: 4.0,
            w: 11.0,
            h: 7.0,
        },
        big: HatFit {
            x: -2.0,
            y: -3.0,
            height: 5.0,
            w: 13.0,
            h: 7.0,
        },
    },
    // 3
    HatDef {
        name: "towering1",
        label: "TOWERING 1",
        small: HatFit {
            x: 5.0,
            y: -1.0,
            height: 4.0,
            w: 11.0,
            h: 5.0,
        },
        big: HatFit {
            x: -2.0,
            y: -2.0,
            height: 5.0,
            w: 13.0,
            h: 6.0,
        },
    },
    // 4
    HatDef {
        name: "towering2",
        label: "TOWERING 2",
        small: HatFit {
            x: 5.0,
            y: -6.0,
            height: 8.0,
            w: 11.0,
            h: 10.0,
        },
        big: HatFit {
            x: -2.0,
            y: -8.0,
            height: 9.0,
            w: 14.0,
            h: 12.0,
        },
    },
    // 5
    HatDef {
        name: "towering3",
        label: "TOWERING 3",
        small: HatFit {
            x: 5.0,
            y: 1.0,
            height: 2.0,
            w: 11.0,
            h: 6.0,
        },
        big: HatFit {
            x: -3.0,
            y: -1.0,
            height: 2.0,
            w: 15.0,
            h: 9.0,
        },
    },
    // 6
    HatDef {
        name: "drseuss",
        label: "DR SEUSS",
        small: HatFit {
            x: 5.0,
            y: -7.0,
            height: 10.0,
            w: 11.0,
            h: 11.0,
        },
        big: HatFit {
            x: -1.0,
            y: -7.0,
            height: 10.0,
            w: 11.0,
            h: 11.0,
        },
    },
    // 7
    HatDef {
        name: "bird",
        label: "BIRD",
        small: HatFit {
            x: 4.0,
            y: -7.0,
            height: 8.0,
            w: 14.0,
            h: 11.0,
        },
        big: HatFit {
            x: -3.0,
            y: -8.0,
            height: 8.0,
            w: 16.0,
            h: 12.0,
        },
    },
    // 8
    HatDef {
        name: "banana",
        label: "BANANA",
        small: HatFit {
            x: 4.0,
            y: -1.0,
            height: 3.0,
            w: 14.0,
            h: 5.0,
        },
        big: HatFit {
            x: -3.0,
            y: -2.0,
            height: 3.0,
            w: 15.0,
            h: 6.0,
        },
    },
    // 9
    HatDef {
        name: "beanie",
        label: "BEANIE",
        small: HatFit {
            x: 7.0,
            y: -2.0,
            height: 3.0,
            w: 7.0,
            h: 6.0,
        },
        big: HatFit {
            x: 1.0,
            y: -3.0,
            height: 3.0,
            w: 7.0,
            h: 6.0,
        },
    },
    // 10
    HatDef {
        name: "toilet",
        label: "TOILET",
        small: HatFit {
            x: 7.0,
            y: -5.0,
            height: 8.0,
            w: 8.0,
            h: 9.0,
        },
        big: HatFit {
            x: -1.0,
            y: -5.0,
            height: 8.0,
            w: 10.0,
            h: 9.0,
        },
    },
    // 11
    HatDef {
        name: "indian",
        label: "INDIAN",
        small: HatFit {
            x: 5.0,
            y: -4.0,
            height: 5.0,
            w: 11.0,
            h: 8.0,
        },
        big: HatFit {
            x: -1.0,
            y: -4.0,
            height: 5.0,
            w: 11.0,
            h: 8.0,
        },
    },
    // 12
    HatDef {
        name: "officerhat",
        label: "OFFICER HAT",
        small: HatFit {
            x: 6.0,
            y: -1.0,
            height: 3.0,
            w: 9.0,
            h: 5.0,
        },
        big: HatFit {
            x: -1.0,
            y: -2.0,
            height: 4.0,
            w: 11.0,
            h: 6.0,
        },
    },
    // 13
    HatDef {
        name: "crown",
        label: "CROWN",
        small: HatFit {
            x: 5.0,
            y: -3.0,
            height: 6.0,
            w: 11.0,
            h: 6.0,
        },
        big: HatFit {
            x: -2.0,
            y: -3.0,
            height: 7.0,
            w: 13.0,
            h: 7.0,
        },
    },
    // 14
    HatDef {
        name: "tophat",
        label: "TOP HAT",
        small: HatFit {
            x: 5.0,
            y: -5.0,
            height: 9.0,
            w: 11.0,
            h: 9.0,
        },
        big: HatFit {
            x: -2.0,
            y: -7.0,
            height: 10.0,
            w: 14.0,
            h: 11.0,
        },
    },
    // 15
    HatDef {
        name: "batter",
        label: "BATTER",
        small: HatFit {
            x: 6.0,
            y: 1.0,
            height: 2.0,
            w: 10.0,
            h: 7.0,
        },
        big: HatFit {
            x: -2.0,
            y: 1.0,
            height: 3.0,
            w: 13.0,
            h: 9.0,
        },
    },
    // 16
    HatDef {
        name: "bonk",
        label: "BONK",
        small: HatFit {
            x: 6.0,
            y: 0.0,
            height: 2.0,
            w: 10.0,
            h: 8.0,
        },
        big: HatFit {
            x: -2.0,
            y: 0.0,
            height: 3.0,
            w: 13.0,
            h: 10.0,
        },
    },
    // 17
    HatDef {
        name: "bakerboy",
        label: "BAKER BOY",
        small: HatFit {
            x: 6.0,
            y: 0.0,
            height: 3.0,
            w: 10.0,
            h: 4.0,
        },
        big: HatFit {
            x: 0.0,
            y: 0.0,
            height: 4.0,
            w: 11.0,
            h: 4.0,
        },
    },
    // 18
    HatDef {
        name: "troublemaker",
        label: "TROUBLEMAKER",
        small: HatFit {
            x: 5.0,
            y: 1.0,
            height: 2.0,
            w: 9.0,
            h: 7.0,
        },
        big: HatFit {
            x: -3.0,
            y: 0.0,
            height: 3.0,
            w: 12.0,
            h: 11.0,
        },
    },
    // 19
    HatDef {
        name: "whoopee",
        label: "WHOOPEE",
        small: HatFit {
            x: 7.0,
            y: 1.0,
            height: 3.0,
            w: 7.0,
            h: 3.0,
        },
        big: HatFit {
            x: 0.0,
            y: 0.0,
            height: 4.0,
            w: 9.0,
            h: 4.0,
        },
    },
    // 20
    HatDef {
        name: "milkman",
        label: "MILKMAN",
        small: HatFit {
            x: 6.0,
            y: -1.0,
            height: 4.0,
            w: 10.0,
            h: 5.0,
        },
        big: HatFit {
            x: -1.0,
            y: -1.0,
            height: 4.0,
            w: 12.0,
            h: 5.0,
        },
    },
    // 21
    HatDef {
        name: "bombingrun",
        label: "BOMBING RUN",
        small: HatFit {
            x: 6.0,
            y: 1.0,
            height: 2.0,
            w: 8.0,
            h: 9.0,
        },
        big: HatFit {
            x: -2.0,
            y: 1.0,
            height: 3.0,
            w: 12.0,
            h: 11.0,
        },
    },
    // 22
    HatDef {
        name: "bonkboy",
        label: "BONK BOY",
        small: HatFit {
            x: 4.0,
            y: 3.0,
            height: 0.0,
            w: 10.0,
            h: 3.0,
        },
        big: HatFit {
            x: -4.0,
            y: 3.0,
            height: 0.0,
            w: 13.0,
            h: 4.0,
        },
    },
    // 23
    HatDef {
        name: "flippedtrilby",
        label: "FLIPPED TRILBY",
        small: HatFit {
            x: 6.0,
            y: 0.0,
            height: 3.0,
            w: 9.0,
            h: 4.0,
        },
        big: HatFit {
            x: -2.0,
            y: -1.0,
            height: 4.0,
            w: 13.0,
            h: 5.0,
        },
    },
    // 24
    HatDef {
        name: "superfan",
        label: "SUPERFAN",
        small: HatFit {
            x: 7.0,
            y: 0.0,
            height: 3.0,
            w: 10.0,
            h: 4.0,
        },
        big: HatFit {
            x: 0.0,
            y: -1.0,
            height: 3.0,
            w: 12.0,
            h: 5.0,
        },
    },
    // 25
    HatDef {
        name: "familiarfez",
        label: "FAMILIAR FEZ",
        small: HatFit {
            x: 6.0,
            y: -2.0,
            height: 4.0,
            w: 8.0,
            h: 8.0,
        },
        big: HatFit {
            x: -1.0,
            y: -2.0,
            height: 4.0,
            w: 10.0,
            h: 9.0,
        },
    },
    // 26
    HatDef {
        name: "santahat",
        label: "SANTA HAT",
        small: HatFit {
            x: 3.0,
            y: 0.0,
            height: 4.0,
            w: 12.0,
            h: 6.0,
        },
        big: HatFit {
            x: -3.0,
            y: -1.0,
            height: 4.0,
            w: 13.0,
            h: 7.0,
        },
    },
    // 27
    HatDef {
        name: "sailor",
        label: "SAILOR",
        small: HatFit {
            x: 6.0,
            y: 0.0,
            height: 2.0,
            w: 10.0,
            h: 5.0,
        },
        big: HatFit {
            x: -1.0,
            y: 0.0,
            height: 3.0,
            w: 13.0,
            h: 5.0,
        },
    },
    // 28
    HatDef {
        name: "koopa",
        label: "KOOPA",
        small: HatFit {
            x: 3.0,
            y: -3.0,
            height: 5.0,
            w: 16.0,
            h: 14.0,
        },
        big: HatFit {
            x: -3.0,
            y: 0.0,
            height: 5.0,
            w: 16.0,
            h: 14.0,
        },
    },
    // 29
    HatDef {
        name: "blooper",
        label: "BLOOPER",
        small: HatFit {
            x: 5.0,
            y: -5.0,
            height: 5.0,
            w: 12.0,
            h: 17.0,
        },
        big: HatFit {
            x: -2.0,
            y: -5.0,
            height: 5.0,
            w: 14.0,
            h: 16.0,
        },
    },
    // 30
    HatDef {
        name: "shyguy",
        label: "SHY GUY",
        small: HatFit {
            x: 7.0,
            y: 1.0,
            height: 2.0,
            w: 11.0,
            h: 10.0,
        },
        big: HatFit {
            x: -1.0,
            y: 1.0,
            height: 3.0,
            w: 13.0,
            h: 12.0,
        },
    },
    // 31
    HatDef {
        name: "goodnewseverybody",
        label: "GOOD NEWS EVERYBODY",
        small: HatFit {
            x: 6.0,
            y: 4.0,
            height: 4.0,
            w: 10.0,
            h: 2.0,
        },
        big: HatFit {
            x: -1.0,
            y: 3.0,
            height: 4.0,
            w: 10.0,
            h: 3.0,
        },
    },
    // 32
    HatDef {
        name: "jetset",
        label: "JET SET",
        small: HatFit {
            x: 5.0,
            y: 1.0,
            height: 4.0,
            w: 6.0,
            h: 7.0,
        },
        big: HatFit {
            x: -3.0,
            y: -1.0,
            height: 5.0,
            w: 7.0,
            h: 9.0,
        },
    },
    // 33
    HatDef {
        name: "bestpony",
        label: "BEST PONY",
        small: HatFit {
            x: 6.0,
            y: 1.0,
            height: 4.0,
            w: 12.0,
            h: 13.0,
        },
        big: HatFit {
            x: -2.0,
            y: 0.0,
            height: 5.0,
            w: 14.0,
            h: 19.0,
        },
    },
];

/// Hat 1, the standard cap, is the one hat tinted to Mario's shirt (`game.lua:1339-1342`)
/// — which is why a fire Mario's cap comes out white without a second image.
pub(crate) const HAT_STANDARD: u8 = 1;

/// What a sonic rainboom leaves you wearing (`mario.lua:3133`). It is hat 33 for a reason.
pub(crate) const HAT_BEST_PONY: u8 = 33;

/// Where the hat sits for the pose Mario is in, in unscaled pixels, or `None` if this
/// pose wears no hat at all.
///
/// The original keys `hatoffsets` by animation state and bails when the entry is `false`
/// (`game.lua:1306`), which only `dead` is. Three of its states have no counterpart here
/// — `sliding`, `grow` and big Mario's `fire` throw pose are animations this port does
/// not have — and their offsets are dropped rather than guessed at.
pub(crate) fn hat_offset(
    is_big: bool,
    anim: PlayerAnim,
    run_frame: u32,
    climb_frame: u32,
    swim_frame: u32,
) -> (f32, f32) {
    // `falling` deliberately reads the *running* table, indexed by whichever run frame
    // Mario stopped on (`game.lua:1318`, `:1328`). It looks like a typo and behaves like
    // one — a hat can shift mid-fall depending on how you took off — but both size
    // branches do it, so `hatoffsets["falling"]` is dead data in the original too.
    let run = (run_frame % 3) as usize;
    let climb = climb_frame.clamp(1, 2) as usize - 1;
    let swim = swim_frame.clamp(1, 2) as usize - 1;
    if is_big {
        match anim {
            PlayerAnim::Idle => (-4.0, -2.0),
            PlayerAnim::Run | PlayerAnim::Fall => [(-5.0, -4.0), (-4.0, -3.0), (-3.0, -2.0)][run],
            PlayerAnim::Jump => (-4.0, -4.0),
            PlayerAnim::Climb => [(-4.0, -4.0), (-4.0, -4.0)][climb],
            PlayerAnim::Swim => [(-5.0, -4.0), (-5.0, -4.0)][swim],
            PlayerAnim::Duck => (-5.0, -12.0),
        }
    } else {
        match anim {
            PlayerAnim::Idle => (0.0, 0.0),
            PlayerAnim::Run | PlayerAnim::Fall => [(0.0, 0.0), (0.0, 0.0), (-1.0, -1.0)][run],
            PlayerAnim::Jump => (0.0, -1.0),
            PlayerAnim::Climb => [(2.0, 0.0), (2.0, -1.0)][climb],
            PlayerAnim::Swim => [(1.0, -1.0), (1.0, -1.0)][swim],
            // A small Mario cannot crouch, so there is no `hatoffsets["ducking"]` to port.
            PlayerAnim::Duck => (0.0, 0.0),
        }
    }
}
