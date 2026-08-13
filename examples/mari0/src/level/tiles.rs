//! Tile properties, decoded from the tilesheets at build time.
//!
//! Mari0 keeps no tile table in source: each 17×17 cell's 17th column encodes six
//! flags as alpha values (`quad.lua:10-56`). `build.rs` decodes all 220 tiles into
//! `TILE_PROPS`; this module gives that raw bitfield a typed API.
//!
//! The previous port hardcoded a 10-id `is_solid` whitelist — which is why only
//! 1-1 was playable. SMB alone uses **62** colliding tiles.

include!(concat!(env!("OUT_DIR"), "/tile_props.rs"));

const FLAG_COLLISION: u8 = 1 << 0;
const FLAG_INVISIBLE: u8 = 1 << 1;
const FLAG_BREAKABLE: u8 = 1 << 2;
const FLAG_COINBLOCK: u8 = 1 << 3;
const FLAG_COIN: u8 = 1 << 4;
const FLAG_PORTALABLE: u8 = 1 << 5;

/// The empty tile. 73% of every SMB level.
pub const TILE_EMPTY: u16 = 1;
/// Solid ground — the auto-filled floor of the off-map left padding.
pub const TILE_GROUND: u16 = 2;
/// The coin tile picked up by walking through it.
pub const TILE_COIN: u16 = 116;

/// Highest valid tile id (`SMB_TILE_COUNT + PORTAL_TILE_COUNT`).
pub const MAX_TILE_ID: u16 = (SMB_TILE_COUNT + PORTAL_TILE_COUNT) as u16;

/// Properties of one tile id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileProps(u8);

impl TileProps {
    /// Blocks movement.
    pub const fn collision(self) -> bool {
        self.0 & FLAG_COLLISION != 0
    }
    /// Not drawn. Air is invisible; so are hidden coin blocks until struck.
    pub const fn invisible(self) -> bool {
        self.0 & FLAG_INVISIBLE != 0
    }
    /// A brick: destroyed by a big Mario's head or a shell.
    pub const fn breakable(self) -> bool {
        self.0 & FLAG_BREAKABLE != 0
    }
    /// A question/hidden block that yields contents when struck.
    pub const fn coinblock(self) -> bool {
        self.0 & FLAG_COINBLOCK != 0
    }
    /// A free-standing coin collected by touch.
    pub const fn coin(self) -> bool {
        self.0 & FLAG_COIN != 0
    }
    /// Accepts a portal. Every SMB tile does; 22 of the lab tiles refuse.
    pub const fn portalable(self) -> bool {
        self.0 & FLAG_PORTALABLE != 0
    }
}

/// Look up a tile id's properties.
///
/// Out-of-range ids report as empty rather than panicking: Mari0 itself coerces
/// them to 1 at load (`game.lua:2267`), and a malformed level should degrade, not
/// crash.
pub fn props(tile_id: u16) -> TileProps {
    TILE_PROPS
        .get(tile_id as usize)
        .copied()
        .map(TileProps)
        .unwrap_or(TileProps(FLAG_INVISIBLE))
}

pub fn is_solid(tile_id: u16) -> bool {
    props(tile_id).collision()
}

/// Replacement tile after a block with contents has been struck.
///
/// The **only** hardcoded tile ids in all of Mari0 (`mario.lua:2383-2432`). Note
/// the visible and invisible branches are asymmetric — visible uses 113/114/117
/// while invisible uses 113/118/112. That is faithfully reproduced, not a typo.
pub fn used_block_tile(spriteset: u8, was_invisible: bool) -> u16 {
    if was_invisible {
        match spriteset {
            1 => 113,
            2 => 118,
            _ => 112,
        }
    } else {
        match spriteset {
            1 => 113,
            2 => 114,
            _ => 117,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_sizes_match_the_source_art() {
        // smbtiles.png is 374×102 → 22×6; portaltiles.png 374×68 → 22×4.
        // If either sheet is ever replaced, this catches a silent id shift that
        // would misplace every tile in every level.
        assert_eq!(SMB_TILE_COUNT, 132);
        assert_eq!(PORTAL_TILE_COUNT, 88);
        assert_eq!(MAX_TILE_ID, 220);
        assert_eq!(TILE_PROPS.len(), 221, "index 0 is unused padding");
    }

    #[test]
    fn known_smb_tiles_decode_correctly() {
        // Spot-checks against the marker pixels, cross-verified independently
        // against the shipped 1-1 layout.
        let air = props(TILE_EMPTY);
        assert!(!air.collision() && air.invisible());

        let ground = props(TILE_GROUND);
        assert!(ground.collision() && !ground.breakable() && !ground.coinblock());

        let brick = props(7);
        assert!(brick.collision() && brick.breakable());

        let question = props(8);
        assert!(question.collision() && question.coinblock() && !question.breakable());

        let hidden = props(115);
        assert!(hidden.collision() && hidden.coinblock() && hidden.invisible());

        let coin = props(TILE_COIN);
        assert!(coin.coin() && !coin.collision());
    }

    #[test]
    fn smb_has_sixty_two_solid_tiles() {
        // The number that mattered: the old whitelist had 10.
        let solid = (1..=SMB_TILE_COUNT as u16).filter(|&t| is_solid(t)).count();
        assert_eq!(solid, 62);
    }

    #[test]
    fn breakable_and_coinblock_sets_are_exactly_as_decoded() {
        let breakable: Vec<u16> = (1..=MAX_TILE_ID)
            .filter(|&t| props(t).breakable())
            .collect();
        assert_eq!(breakable, vec![7, 49, 122]);
        let coinblocks: Vec<u16> = (1..=MAX_TILE_ID)
            .filter(|&t| props(t).coinblock())
            .collect();
        assert_eq!(coinblocks, vec![8, 115]);
        let coins: Vec<u16> = (1..=MAX_TILE_ID).filter(|&t| props(t).coin()).collect();
        assert_eq!(coins, vec![116]);
    }

    #[test]
    fn every_smb_tile_is_portalable_but_some_lab_tiles_are_not() {
        for t in 1..=SMB_TILE_COUNT as u16 {
            assert!(props(t).portalable(), "smb tile {t} should be portalable");
        }
        let refused: Vec<u16> = ((SMB_TILE_COUNT as u16 + 1)..=MAX_TILE_ID)
            .filter(|&t| !props(t).portalable())
            .collect();
        assert_eq!(
            refused,
            vec![
                134, 135, 136, 137, 138, 139, 142, 143, 144, 145, 152, 153, 156, 157, 174, 175,
                196, 197, 210, 211, 218, 219
            ]
        );
    }

    #[test]
    fn out_of_range_ids_degrade_to_empty() {
        assert!(!props(0).collision());
        assert!(!props(9999).collision());
        assert!(props(9999).invisible());
    }

    #[test]
    fn used_block_replacement_reproduces_the_asymmetric_original() {
        // Visible branch.
        assert_eq!(used_block_tile(1, false), 113);
        assert_eq!(used_block_tile(2, false), 114);
        assert_eq!(used_block_tile(3, false), 117);
        assert_eq!(used_block_tile(4, false), 117);
        // Invisible branch — deliberately different for spritesets 2+.
        assert_eq!(used_block_tile(1, true), 113);
        assert_eq!(used_block_tile(2, true), 118);
        assert_eq!(used_block_tile(3, true), 112);
        // And the replacements are themselves solid, so you can stand on them.
        for ss in 1..=4 {
            assert!(is_solid(used_block_tile(ss, false)));
            assert!(is_solid(used_block_tile(ss, true)));
        }
    }
}
