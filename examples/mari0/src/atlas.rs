//! Texture-atlas UV math.
//!
//! Every sheet is addressed by cell, but the cell sizes differ per sheet and
//! several sheets mix frame sizes (see `fireball_uv`), so each accessor carries
//! the sheet's dimensions in its arithmetic rather than sharing a generic helper.

// ── sRGB → Linear conversion (for tint colors with sRGB textures) ───
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// ── UV Helper Functions ──────────────────────────────────────────────

/// Get UV rect for a tile in smbtiles.png (374×102, 22-col grid, 17×17 cells)
pub(crate) fn smb_tile_uv(tile_id: u32) -> [f32; 4] {
    let col = ((tile_id - 1) % 22) as f32;
    let row = ((tile_id - 1) / 22) as f32;
    [
        col * 17.0 / 374.0,
        row * 17.0 / 102.0,
        16.0 / 374.0,
        16.0 / 102.0,
    ]
}

/// UV rect for a tile in `portaltiles.png` (374×68, 22-col grid, 17×17 cells).
///
/// Tile ids continue straight on from the SMB sheet: 133 is this sheet's first cell
/// (`main.lua:218-245` walks one sheet then the other into a single list), which is
/// why a lab level's tiles come out as garbage if you feed them to [`smb_tile_uv`] —
/// id 133 lands one row *past* the bottom of a six-row sheet.
pub(crate) fn portal_tile_uv(tile_id: u32) -> [f32; 4] {
    let index = tile_id - FIRST_PORTAL_TILE;
    let col = (index % 22) as f32;
    let row = (index / 22) as f32;
    [
        col * 17.0 / 374.0,
        row * 17.0 / 68.0,
        16.0 / 374.0,
        16.0 / 68.0,
    ]
}

/// The first tile id that lives on `portaltiles.png`. `smbtiles.png` is 22×6 = 132
/// cells, so the lab sheet starts at 133.
pub(crate) const FIRST_PORTAL_TILE: u32 = 133;

/// Get UV rect for a mario animation frame (512×128, 20×20 cells)
pub(crate) fn mario_uv(col: u32, row: u32) -> [f32; 4] {
    [
        (col * 20) as f32 / 512.0,
        (row * 20) as f32 / 128.0,
        20.0 / 512.0,
        20.0 / 128.0,
    ]
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

/// Piranha plant frame UV. `plant.png` is 32x128: 16x24 cells, 2 frames wide.
pub(crate) fn plant_uv(frame: u32) -> [f32; 4] {
    [(frame * 16) as f32 / 32.0, 0.0, 16.0 / 32.0, 24.0 / 128.0]
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

/// Get UV rect for coin animation frame (16×32, 2 vertical frames)
pub(crate) fn coin_frame_uv(frame: u32) -> [f32; 4] {
    [0.0, (frame * 16) as f32 / 32.0, 1.0, 16.0 / 32.0]
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
