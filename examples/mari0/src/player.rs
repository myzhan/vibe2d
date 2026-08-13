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

    pub(crate) fn set_size(&mut self, big: bool) {
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
