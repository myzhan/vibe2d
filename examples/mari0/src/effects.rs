//! Short-lived visual effects: struck-block bounces, coin pops, floating
//! scores, and brick debris.
//!
//! Each is a plain record with a timer; the update and draw passes live with the
//! systems that spawn them.

#[derive(Clone)]
pub(crate) struct CoinInstance {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) collected: bool,
}

pub(crate) struct BlockBounce {
    pub(crate) col: i32,
    pub(crate) row: i32,
    pub(crate) timer: f32,
}

pub(crate) struct CoinPopup {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vy: f32,
    pub(crate) timer: f32,
}

pub(crate) struct ScorePopup {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) value: u32,
    pub(crate) timer: f32,
}

pub(crate) struct BrickDebris {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) timer: f32,
}
