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
    /// `None` means draw the "1UP" graphic instead of a number.
    ///
    /// The original stores the two in the same list and switches on the *type* of the
    /// field — `type(i) == "number"` against `i == "1up"` (`game.lua:1585-1588`) — so an
    /// extra life floats up on exactly the same track as a score.
    pub(crate) value: Option<u32>,
    pub(crate) timer: f32,
}

pub(crate) struct BrickDebris {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) timer: f32,
}

/// Dust out of an open portal's mouth.
///
/// Purely decorative, and the one detail worth keeping is that an **upward**-facing
/// portal's particles are stopped from ever falling back (`portalparticle.lua:30-34`):
/// their downward speed is clamped to zero, so the plume above a floor portal keeps
/// rising instead of raining back into it.
pub(crate) struct PortalParticle {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) timer: f32,
    /// Which portal it came from, so it takes that portal's colour.
    pub(crate) portal: usize,
    /// Was the portal facing up? Then it never falls back.
    pub(crate) facing_up: bool,
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
