//! Mario: his box, his animation state, and the growth transition.

use crate::constants::*;

#[derive(PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum PlayerAnim {
    Idle,
    Run,
    Jump,
    Fall,
    /// Holding a vine. Two frames, picked by [`Player::climb_frame`].
    Climb,
    /// Mid-stroke or sinking, in a water level. Two frames, picked by
    /// [`Player::swim_frame`]. Walking on the sea floor is still `Run` — the original
    /// only swaps in the swimming sprite while `jumping` or `falling`
    /// (`mario.lua:1516`).
    Swim,
    /// A big Mario crouching. Not reachable while small.
    Duck,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum Orientation {
    Up,
    Down,
    Left,
    Right,
}

impl Orientation {
    /// One cell step in this direction, as `(dcol, drow)`.
    pub(crate) fn delta(self) -> (i32, i32) {
        match self {
            Orientation::Up => (0, -1),
            Orientation::Down => (0, 1),
            Orientation::Left => (-1, 0),
            Orientation::Right => (1, 0),
        }
    }

    pub(crate) fn opposite(self) -> Self {
        match self {
            Orientation::Up => Orientation::Down,
            Orientation::Down => Orientation::Up,
            Orientation::Left => Orientation::Right,
            Orientation::Right => Orientation::Left,
        }
    }

    /// Is this direction along the x axis?
    pub(crate) fn is_horizontal(self) -> bool {
        matches!(self, Orientation::Left | Orientation::Right)
    }
}

/// Which loadout the player is carrying.
///
/// `playertypelist` has three entries (`variables.lua:7`) and the third, `minecraft`, is
/// a separate mode with its own tileset and block-breaking — not ported. These two are
/// the ones that only change what the mouse does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum PlayerType {
    /// The portal gun. What every level is designed around.
    Portal,
    /// The gel cannon: no portals at all, but unlimited blue and orange paint.
    GelCannon,
}

pub(crate) struct Player {
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
    pub(crate) run_frame: f32,
    /// Which of the two climbing frames is showing, 1 or 2. Only read while
    /// `anim_state` is [`PlayerAnim::Climb`]; the vine drives it.
    pub(crate) climb_frame: u32,
    /// Stroke animation phase, in the original's own units: it lives in `[1, 3)` and the
    /// frame is its floor, so it is 1 or 2 and never 0 (`mario.lua:126`, `:1356-1360`).
    pub(crate) swim_phase: f32,
    /// Is a big Mario crouching? Halves his box — see [`Player::set_ducking`].
    pub(crate) ducking: bool,
    /// Seconds until the next bubble, and which of the two intervals is in use.
    pub(crate) bubble_timer: f32,
    pub(crate) bubble_index: usize,
    pub(crate) invincible_timer: f32,
    pub(crate) portal_cooldown: f32,
    pub(crate) teleport_cooldown: f32,
}

impl Player {
    pub(crate) fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            width: PLAYER_SMALL_W,
            height: PLAYER_SMALL_H,
            on_ground: false,
            facing_right: true,
            is_big: false,
            is_fire: false,
            is_jumping: false,
            anim_state: PlayerAnim::Idle,
            run_frame: 0.0,
            climb_frame: 2,
            swim_phase: 1.0,
            ducking: false,
            bubble_timer: 0.0,
            bubble_index: 0,
            invincible_timer: 0.0,
            portal_cooldown: 0.0,
            teleport_cooldown: 0.0,
        }
    }

    pub(crate) fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }
    pub(crate) fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }
    pub(crate) fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Crouch or stand up, keeping his feet where they are.
    ///
    /// The box halves and its top drops by the same amount (`mario.lua:2865-2878`), so
    /// the ground under him does not move. Ignored unless he is big, since there is no
    /// small-Mario crouch at all.
    ///
    /// Standing up is refused rather than deferred if there is no room — the original
    /// never checks, because the only way to be crouched under a ceiling is to have
    /// walked there crouched, and it cannot walk while crouched. `caller_has_room` lets
    /// the caller do the check where it has the level to hand.
    pub(crate) fn set_ducking(&mut self, ducking: bool) {
        if !self.is_big || self.ducking == ducking {
            return;
        }
        self.ducking = ducking;
        if ducking {
            self.y += PLAYER_BIG_H - DUCK_HEIGHT;
            self.height = DUCK_HEIGHT;
        } else {
            self.y -= PLAYER_BIG_H - DUCK_HEIGHT;
            self.height = PLAYER_BIG_H;
        }
    }

    pub(crate) fn set_size(&mut self, big: bool) {
        // A size change while crouched would leave the box the wrong height for good, so
        // stand him up first (`mario:shrink` does exactly this, `mario.lua:1667-1669`).
        self.set_ducking(false);
        let was_big = self.is_big;
        self.is_big = big;
        if big {
            self.width = PLAYER_BIG_W;
            self.height = PLAYER_BIG_H;
        } else {
            self.width = PLAYER_SMALL_W;
            self.height = PLAYER_SMALL_H;
        }
        if was_big && !big {
            self.y += PLAYER_BIG_H - PLAYER_SMALL_H;
        } else if !was_big && big {
            self.y -= PLAYER_BIG_H - PLAYER_SMALL_H;
        }
    }
}
