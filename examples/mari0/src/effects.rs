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

/// A bubble out of Mario's mouth, in a water level.
///
/// Purely decorative, and the only reason it is worth the twenty lines is that without
/// them a water level reads as a blue room rather than as water. It rises at
/// [`BUBBLE_SPEED`] with a speed that wanders inside ±[`BUBBLE_MARGIN`], and pops at the
/// surface (`bubble.lua`).
pub(crate) struct Bubble {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vy: f32,
}
