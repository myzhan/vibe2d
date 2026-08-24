//! Parallax backdrops: which images a level gets, and how fast each one scrolls.
//!
//! A level opts in with `custombackground` or `portalbackground` — the original treats the
//! two keys as the same switch (`game.lua:2503`). 41 of the shipped levels set one,
//! including every level of the lab, and none of them had a backdrop here until now.
//!
//! Where the images come from is a three-step search (`main.lua:1653-1684`): the pack's
//! `<level>background1.png`, `2`, … first; failing that its pack-wide `background1.png`, …;
//! failing that the stock `portalbackground.png`. Only `acid_trip` ships any of its own, so
//! for the other 32 levels this resolves to the stock strip — which is the whole point of
//! the fallback, and why the lab has a moving backdrop at all.

include!(concat!(env!("OUT_DIR"), "/backgrounds.rs"));

/// The stock backdrop every opted-in level falls back to.
///
/// `portalbackground.png` is 16x224 — one block wide and fourteen tall, tiled sideways
/// across the screen. Its narrowness is what makes the lab's backdrop read as vertical
/// stripes sliding past.
pub(crate) const STOCK_LAYER: BackgroundLayer = BackgroundLayer {
    texture: "portal_background",
    w: 1.0,
    h: 14.0,
};

/// The layer set a level should draw, or `None` if it asked for no backdrop.
///
/// Level-specific beats pack-wide beats stock, and an empty `layers` can never be returned
/// — the stock strip is always a valid answer.
pub(crate) fn layers_for(pack: &str, level: &str) -> &'static [BackgroundLayer] {
    let find = |lvl: &str| {
        BACKGROUND_SETS
            .iter()
            .find(|s| s.pack == pack && s.level == lvl)
            .map(|s| s.layers)
    };
    find(level)
        .or_else(|| find(""))
        .unwrap_or(std::slice::from_ref(&STOCK_LAYER))
}

/// How far layer `index` (0-based, nearest first) has scrolled when the camera is at
/// `camera_x` blocks.
///
/// `xscroll / (i * scrollfactor + 1)` with `i` the original's 1-based layer number
/// (`game.lua:905`), so the nearest layer already lags the world by a factor of
/// `scrollfactor + 1` and each one behind it lags more.
///
/// The exception is a level whose `scrollfactor` is exactly 9: `reversescrollfactor()` is
/// `sqrt(scrollfactor)/3`, the draw loop pins the backdrop when that equals 1
/// (`game.lua:906-908`), and 9 is the only value that satisfies it. Presumably a way for a
/// level to ask for a still backdrop without a separate flag.
pub(crate) fn layer_scroll(camera_blocks: f32, index: usize, scrollfactor: f32) -> f32 {
    if (scrollfactor.sqrt() / 3.0 - 1.0).abs() < f32::EPSILON {
        return 0.0;
    }
    camera_blocks / ((index + 1) as f32 * scrollfactor + 1.0)
}
