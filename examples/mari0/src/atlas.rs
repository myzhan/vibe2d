//! Texture-atlas UV math.
//!
//! Every sheet is addressed by cell, but the cell sizes differ per sheet and
//! several sheets mix frame sizes (see `fireball_uv`), so each accessor carries
//! the sheet's dimensions in its arithmetic rather than sharing a generic helper.

use crate::constants::{RAINBOOM_CELL, RAINBOOM_FRAMES, RAINBOOM_SHEET};

// ── sRGB → Linear conversion (for tint colors with sRGB textures) ───
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// ── UV Helper Functions ──────────────────────────────────────────────

/// UV rect for a tile on a mappack's own sheet.
///
/// `tile_id` is **pack-relative** (1-based) and `sheet_h` is that sheet's pixel height.
/// Every sheet is 374 wide with a 22-column grid of 17x17 cells, but they differ in how
/// many *rows* they have: the stock one is 6, `the untitled game`'s is 17 and
/// `acid trip`'s is 24. Dividing by a hardcoded 102 samples off the bottom of the taller
/// ones, which is the same class of bug that once made every lab level render as garbage.
pub(crate) fn pack_tile_uv(tile_id: u32, sheet_h: f32) -> [f32; 4] {
    let col = ((tile_id - 1) % 22) as f32;
    let row = ((tile_id - 1) / 22) as f32;
    [
        col * 17.0 / 374.0,
        row * 17.0 / sheet_h,
        16.0 / 374.0,
        16.0 / sheet_h,
    ]
}

/// The stock sheet's height, for the few places that draw an `smb` tile directly.
pub(crate) const SMB_SHEET_H: f32 = 102.0;

/// UV rect for a tile on the stock `smbtiles.png` (374x102).
pub(crate) fn smb_tile_uv(tile_id: u32) -> [f32; 4] {
    pack_tile_uv(tile_id, SMB_SHEET_H)
}

/// UV rect for a tile in `portaltiles.png` (374×68, 22-col grid, 17×17 cells).
///
/// Tile ids continue straight on from the pack's own sheet — `main.lua:218-245` walks one
/// sheet then the other into a single list — so where they start depends on how tall that
/// sheet is. 133 for a six-row sheet, 529 for `acid trip`'s twenty-four. Hence the
/// parameter: a hardcoded 133 puts a DLC pack's ordinary ground tiles on the lab sheet.
pub(crate) fn portal_tile_uv(tile_id: u32, first_portal_tile: u32) -> [f32; 4] {
    let index = tile_id - first_portal_tile;
    let col = (index % 22) as f32;
    let row = (index / 22) as f32;
    [
        col * 17.0 / 374.0,
        row * 17.0 / 68.0,
        16.0 / 374.0,
        16.0 / 68.0,
    ]
}

/// Get UV rect for a mario animation frame (512×128, 20×20 cells)
pub(crate) fn mario_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 20) as f32 / 512.0,
        (row * 20) as f32 / 128.0,
        20.0 / 512.0,
        20.0 / 128.0,
    ]
}

/// The transform cell used while growing or shrinking: 20x24 at x=260 on the small sheet.
///
/// Always row 0 — `mariogrow[i]` ignores its own loop variable (`main.lua:552`), so unlike
/// every other pose this one does not follow the gun's aim. Four pixels taller than a
/// standing frame, which is the whole point: it is Mario caught halfway.
pub(crate) fn mario_grow_uv() -> [f32; 4] {
    [260.0 / 512.0, 0.0, 20.0 / 512.0, 24.0 / 128.0]
}

/// Get UV rect for a big mario animation frame (512×256, 20×36 cells)
pub(crate) fn mario_big_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 20) as f32 / 512.0,
        (row * 36) as f32 / 256.0,
        20.0 / 512.0,
        36.0 / 256.0,
    ]
}

/// Get UV rect for goomba frame (32×64, 16×16 cells)
pub(crate) fn goomba_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 16) as f32 / 32.0,
        (row * 16) as f32 / 64.0,
        16.0 / 32.0,
        16.0 / 64.0,
    ]
}

/// Lakitu frame. `lakito.png` is 32x24: two 16x24 frames, the second the crouch
/// he ducks into just before letting an egg go.
pub(crate) fn lakito_uv(frame: u32) -> [f32; 4] {
    [(frame % 2) as f32 * 0.5, 0.0, 0.5, 1.0]
}

/// Spiny frame. `spikey.png` is 64x16: four 16x16 frames, **the first two walking
/// and the last two the egg** (`goomba.lua:51-61` picks `quadi` 1/2 vs 3/4), so the
/// sheet holds two animations rather than one four-frame cycle.
pub(crate) fn spikey_uv(frame: u32) -> [f32; 4] {
    [(frame % 4) as f32 * 0.25, 0.0, 0.25, 1.0]
}

/// Bullet bill. `bulletbill.png` is 16x64: one 16x16 frame per spriteset, stacked.
/// Only the first is used — the port doesn't swap enemy art per environment.
pub(crate) fn bullet_bill_uv() -> [f32; 4] {
    [0.0, 0.0, 1.0, 16.0 / 64.0]
}

/// Hammer bro frame. `hammerbros.png` is 64x256 with **16x34** cells
/// (`main.lua:446`), four columns per spriteset row. Columns 0/1 are the shuffle,
/// 2/3 the same pose with the hammer raised over his head.
pub(crate) fn hammer_bro_uv(frame: u32) -> [f32; 4] {
    [(frame % 4) as f32 * 0.25, 0.0, 0.25, 34.0 / 256.0]
}

/// A thrown hammer. `hammer.png` is 64x64: four 16x16 frames of it tumbling.
pub(crate) fn hammer_uv(frame: u32) -> [f32; 4] {
    [(frame % 4) as f32 * 0.25, 0.0, 0.25, 0.25]
}

/// Bowser frame. `bowser.png` is 64x64: 32x32 cells, two columns (the walk) by two
/// rows — the **second row is mouth-open**, shown for the half second before he
/// breathes (`bowser.lua:128-135`).
pub(crate) fn bowser_uv(walk: u32, breathing: bool) -> [f32; 4] {
    [
        (walk % 2) as f32 * 0.5,
        if breathing { 0.5 } else { 0.0 },
        0.5,
        0.5,
    ]
}

/// One breath of fire. `fire.png` is 48x8: two 24x8 frames.
pub(crate) fn fire_uv(frame: u32) -> [f32; 4] {
    [(frame % 2) as f32 * 0.5, 0.0, 0.5, 1.0]
}

/// The "false Bowser" underneath. `decoys.png` is 64x256: one 32x32 cell per world,
/// revealed when the Bowser of worlds 1-7 goes down (`bowser.lua:196-199`) — the joke
/// being that he was a painted goomba all along.
pub(crate) fn decoy_uv(world: u32) -> [f32; 4] {
    [
        0.0,
        (world.clamp(1, 8) - 1) as f32 * 32.0 / 256.0,
        0.5,
        32.0 / 256.0,
    ]
}

/// Spring frame. `spring.png` is 48x124 — three 16x31 columns of compression, and the
/// rows are the spritesets.
pub(crate) fn spring_uv(frame: usize) -> [f32; 4] {
    [
        (frame % 3) as f32 * 16.0 / 48.0,
        0.0,
        16.0 / 48.0,
        31.0 / 124.0,
    ]
}

/// One of the rainboom's 49 frames. `rainboom.png` is a 7x7 grid of 204x182 cells.
pub(crate) fn rainboom_uv(frame: u32) -> [f32; 4] {
    let frame = frame.min(RAINBOOM_FRAMES - 1);
    let (cw, ch) = RAINBOOM_CELL;
    let (sw, sh) = RAINBOOM_SHEET;
    [
        (frame % 7) as f32 * cw / sw,
        (frame / 7) as f32 * ch / sh,
        cw / sw,
        ch / sh,
    ]
}

/// Seesaw piece. `seesaw.png` is 64x16 — four 16x16 cells in a row, and unlike most of
/// these sheets there is no spriteset dimension (`main.lua:342-345`).
///
/// In order: the left pulley, the right pulley, a length of rope, and a length of beam.
/// The two pulleys are halves of one wheel, which is why they are separate cells rather
/// than one mirrored sprite.
pub(crate) fn seesaw_uv(cell: u32) -> [f32; 4] {
    [(cell % 4) as f32 * 16.0 / 64.0, 0.0, 16.0 / 64.0, 1.0]
}

/// Vine piece. `vine.png` is 32x64: two 16x16 columns — the curled tip and a length of
/// stem — and one row per spriteset (`main.lua:373-378`).
///
/// A vine is drawn as one tip with as many stems as it is tall stacked below it, so
/// these two cells are the whole sheet a vine of any length needs.
pub(crate) fn vine_uv(spriteset: u8, stem: bool) -> [f32; 4] {
    let row = (spriteset.clamp(1, 4) - 1) as f32;
    [
        if stem { 0.5 } else { 0.0 },
        row * 16.0 / 64.0,
        0.5,
        16.0 / 64.0,
    ]
}

/// Squid frame. `squid.png` is 32x32: two 16x16 frames — arms up while it drifts and
/// lunges, arms spread while it sinks.
pub(crate) fn squid_uv(frame: u32) -> [f32; 4] {
    [(frame % 2) as f32 * 0.5, 0.0, 0.5, 0.5]
}

/// Piranha plant frame UV. `plant.png` is 32x128: **16x23** cells, 2 frames wide.
///
/// 23, not 24 (`main.lua:464`): the rows butt up against each other with no
/// padding, so a 24 pulls one row of the next spriteset's plant into this one.
pub(crate) fn plant_uv(frame: u32) -> [f32; 4] {
    [(frame % 2) as f32 * 0.5, 0.0, 0.5, 23.0 / 128.0]
}

/// One firebar/geyser fireball. `fireball.png` is 80x16 and its **first four
/// frames are 8x8** (`main.lua:363-366`) — the larger 16x16 frames later in the
/// sheet are the explosion, not the fireball.
///
/// Getting this wrong is what made the firebar look twice as thick as it should:
/// an 8x8 source drawn at 2x is 16x16, which exactly matches the 0.5-block
/// spacing between segments, so the bar reads as a chain of touching fireballs
/// rather than a slab of overlapping ones.
pub(crate) fn fireball_uv(frame: u32) -> [f32; 4] {
    [(frame % 4) as f32 * 8.0 / 80.0, 0.0, 8.0 / 80.0, 8.0 / 16.0]
}

/// Cheep-cheep frame. `cheepcheep.png` is 32x32: 16x16 cells, two frames wide.
pub(crate) fn cheep_uv(frame: u32) -> [f32; 4] {
    [(frame % 2) as f32 * 0.5, 0.0, 0.5, 0.5]
}

/// Koopa frame UV. `koopa.png` is 128x128 with 16x24 cells.
pub(crate) fn koopa_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 16) as f32 / 128.0,
        (row * 24) as f32 / 128.0,
        16.0 / 128.0,
        24.0 / 128.0,
    ]
}

/// A collectible coin's frame. `coin.png` is 64x64 with **16x16** cells: four columns
/// of animation by four rows of spriteset (`main.lua:311`).
///
/// Not to be confused with `coinanimation.png`, which is 16x32 of **5x8** cells and is
/// the tiny HUD icon (`game.lua:1030`). Feeding a 16x16 region of *that* sheet to a
/// coin is what put six little rings on screen where one coin belonged — a 16x16 window
/// onto 5x8 cells shows a 3x2 grid of them.
pub(crate) fn coin_uv(frame: u32) -> [f32; 4] {
    [
        (frame % 3) as f32 * 16.0 / 64.0,
        0.0,
        16.0 / 64.0,
        16.0 / 64.0,
    ]
}

/// The HUD's little coin icon. `coinanimation.png` is 16x32 of **5x8** cells
/// (`main.lua:291`) — three columns of animation by four rows of spriteset. It is only
/// ever drawn beside the coin counter (`game.lua:1030`), never in the world.
pub(crate) fn coin_hud_uv(frame: u32) -> [f32; 4] {
    [(frame % 3) as f32 * 5.0 / 16.0, 0.0, 5.0 / 16.0, 8.0 / 32.0]
}

/// Which frame the shared coin spin is on, from a time in seconds.
///
/// The original runs one global counter for every coin on screen so they spin in
/// unison (`game.lua:149-160`): `coinanimation += dt*6.75`, wrapping by 5, and then the
/// frame is **ping-ponged** — floor 1,2,3,4,5 maps to frames 1,2,3,2,1. So it is three
/// pictures played out and back (wide, narrow, edge-on, narrow), not a two-frame
/// alternation, and a full spin takes 5/6.75 ≈ 0.74s.
pub(crate) fn coin_spin_frame(seconds: f32) -> u32 {
    // Same wrap as the original: the counter lives in 1..6 and steps back by 5.
    let t = 1.0 + (seconds * 6.75) % 5.0;
    match t.floor() as u32 {
        4 => 1,
        5 => 0,
        n => n.saturating_sub(1).min(2),
    }
}

/// Get UV rect for entity in entities.png (170×170, 17px cells, 16px sprites)
pub(crate) fn entity_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 17) as f32 / 170.0,
        (row * 17) as f32 / 170.0,
        16.0 / 170.0,
        16.0 / 170.0,
    ]
}

/// Get UV rect for star frame in star.png (64×16, 4 frames)
pub(crate) fn star_frame_uv(frame: u32) -> [f32; 4] {
    [(frame * 16) as f32 / 64.0, 0.0, 16.0 / 64.0, 1.0]
}

/// Get UV rect for flower frame in flower.png (64×16, 4 frames)
pub(crate) fn flower_frame_uv(frame: u32) -> [f32; 4] {
    [(frame * 16) as f32 / 64.0, 0.0, 16.0 / 64.0, 1.0]
}

/// Get UV rect for fireball explosion in fireball.png (80×16, frames at offset 32, 16×16)
pub(crate) fn fireball_explode_uv(frame: u32) -> [f32; 4] {
    [(32 + frame * 16) as f32 / 80.0, 0.0, 16.0 / 80.0, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coin spin is a three-frame **ping-pong**, not a two-frame alternation.
    ///
    /// `game.lua:154-160` maps the counter's floor 1,2,3,4,5 onto frames 1,2,3,2,1 —
    /// wide, narrow, edge-on, narrow, wide. Getting this wrong is subtle on screen but
    /// it is the difference between a coin spinning and a coin flickering.
    #[test]
    fn the_coin_spin_goes_out_and_back() {
        // One full cycle is 5 counter units at 6.75 units/s.
        let period = 5.0 / 6.75;
        let seen: Vec<u32> = (0..40)
            .map(|i| coin_spin_frame(period * i as f32 / 40.0))
            .collect();
        assert_eq!(
            seen.iter().copied().max(),
            Some(2),
            "all three frames should appear: {seen:?}"
        );
        assert_eq!(seen[0], 0, "a cycle opens on the wide frame");
        // Ping-pong: frame 2 is reached once and only in the middle, never at an end.
        let peak = seen.iter().position(|f| *f == 2).unwrap();
        assert!(
            peak > 0 && peak < seen.len() - 1,
            "edge-on is the middle of the cycle: {seen:?}"
        );
        assert_eq!(
            *seen.last().unwrap(),
            0,
            "and it closes back on the wide frame: {seen:?}"
        );
        // It repeats. Sampled off the frame boundaries: *on* a boundary the counter
        // lands exactly on an integer and which side of it you get is down to float
        // rounding — arbitrary in the original too, since it steps an accumulator.
        for i in 0..10 {
            let t = period * (i as f32 + 0.5) / 10.0;
            assert_eq!(coin_spin_frame(t), coin_spin_frame(t + period), "at t={t}");
        }
    }

    /// The two coin sheets have different cell sizes, and mixing them is what put six
    /// little rings on screen where one coin belonged.
    #[test]
    fn the_two_coin_sheets_are_addressed_differently() {
        // World coin: 16x16 out of 64x64, so a quarter of the sheet each way.
        let [x, y, w, h] = coin_uv(0);
        assert_eq!((x, y), (0.0, 0.0));
        assert_eq!((w, h), (0.25, 0.25));
        // Three columns — the sheet's fourth is empty — and it wraps rather than
        // sampling that empty column.
        assert_eq!(coin_uv(3), coin_uv(0));
        assert!(coin_uv(2)[0] + coin_uv(2)[2] <= 48.0 / 64.0);

        // HUD icon: 5x8 out of 16x32. In *pixels* that is a much smaller cell, but the
        // sheet is smaller too, so as UV fractions it comes out wider — which is
        // exactly the trap. Compare the pixel sizes the fractions imply.
        let [hx, hy, hw, hh] = coin_hud_uv(0);
        assert_eq!((hx, hy), (0.0, 0.0));
        assert_eq!((hw * 16.0, hh * 32.0), (5.0, 8.0), "5x8 px out of 16x32");
        assert_eq!((w * 64.0, h * 64.0), (16.0, 16.0), "16x16 px out of 64x64");
        // Three columns on this one, not four.
        assert_eq!(coin_hud_uv(3), coin_hud_uv(0));
    }
}
