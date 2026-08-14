//! The 100 entity types a level file can place, in Mari0's exact order.
//!
//! A level cell is `tile[-entity[-arg…]]`. The entity field is an index into
//! `entitylist` (`entity.lua:3-104`), so **the order below is load-bearing**: one
//! shifted line silently turns every goomba in every level into something else.
//! `entities.png` only supplies the level editor's icons — gameplay never needs it.

/// Entity kinds, indexed 1..=100 exactly as the level format encodes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum EntityKind {
    Remove,
    Mushroom,
    OneUp,
    Star,
    ManyCoins,
    Goomba,
    Koopa,
    Spawn,
    GoombaHalf,
    KoopaHalf,
    Flag,
    KoopaRed,
    KoopaRedHalf,
    Vine,
    HammerBro,
    CheepRed,
    CheepWhite,
    PlatformUp,
    PlatformRight,
    Box,
    Pipe,
    Lakito,
    MazeStart,
    MazeEnd,
    MazeGate,
    EmanceHor,
    EmanceVer,
    DoorVer,
    DoorHor,
    WallIndicator,
    PipeSpawn,
    PlatformFall,
    BulletBillStart,
    BulletBillEnd,
    Drain,
    LightBridgeRight,
    LightBridgeLeft,
    LightBridgeDown,
    LightBridgeUp,
    Button,
    PlatformSpawnerDown,
    PlatformSpawnerUp,
    GroundLightVer,
    GroundLightHor,
    GroundLightUpRight,
    GroundLightRightDown,
    GroundLightDownLeft,
    GroundLightLeftUp,
    FaithPlateUp,
    FaithPlateRight,
    FaithPlateLeft,
    LaserRight,
    LaserDown,
    LaserLeft,
    LaserUp,
    LaserDetectorRight,
    LaserDetectorDown,
    LaserDetectorLeft,
    LaserDetectorUp,
    BulletBill,
    BlueGelDown,
    BlueGelRight,
    BlueGelLeft,
    OrangeGelDown,
    OrangeGelRight,
    OrangeGelLeft,
    BoxTube,
    PushButtonLeft,
    PushButtonRight,
    Plant,
    WhiteGelDown,
    WhiteGelRight,
    WhiteGelLeft,
    Timer,
    Beetle,
    BeetleHalf,
    KoopaRedFlying,
    KoopaFlying,
    CastleFireCcw,
    Seesaw,
    WarpPipe,
    CastleFireCw,
    LakitoEnd,
    NotGate,
    GelTop,
    GelLeft,
    GelBottom,
    GelRight,
    FireStart,
    Bowser,
    Axe,
    PlatformBonus,
    Spring,
    Squid,
    FlyingFishStart,
    FlyingFishEnd,
    UpFire,
    Spikey,
    SpikeyHalf,
    Checkpoint,
}

/// Ordered exactly as `entity.lua:3-104`. Index `i` is entity id `i + 1`.
const ENTITY_ORDER: [EntityKind; 100] = {
    use EntityKind::*;
    [
        Remove,
        Mushroom,
        OneUp,
        Star,
        ManyCoins,
        Goomba,
        Koopa,
        Spawn,
        GoombaHalf,
        KoopaHalf,
        Flag,
        KoopaRed,
        KoopaRedHalf,
        Vine,
        HammerBro,
        CheepRed,
        CheepWhite,
        PlatformUp,
        PlatformRight,
        Box,
        Pipe,
        Lakito,
        MazeStart,
        MazeEnd,
        MazeGate,
        EmanceHor,
        EmanceVer,
        DoorVer,
        DoorHor,
        WallIndicator,
        PipeSpawn,
        PlatformFall,
        BulletBillStart,
        BulletBillEnd,
        Drain,
        LightBridgeRight,
        LightBridgeLeft,
        LightBridgeDown,
        LightBridgeUp,
        Button,
        PlatformSpawnerDown,
        PlatformSpawnerUp,
        GroundLightVer,
        GroundLightHor,
        GroundLightUpRight,
        GroundLightRightDown,
        GroundLightDownLeft,
        GroundLightLeftUp,
        FaithPlateUp,
        FaithPlateRight,
        FaithPlateLeft,
        LaserRight,
        LaserDown,
        LaserLeft,
        LaserUp,
        LaserDetectorRight,
        LaserDetectorDown,
        LaserDetectorLeft,
        LaserDetectorUp,
        BulletBill,
        BlueGelDown,
        BlueGelRight,
        BlueGelLeft,
        OrangeGelDown,
        OrangeGelRight,
        OrangeGelLeft,
        BoxTube,
        PushButtonLeft,
        PushButtonRight,
        Plant,
        WhiteGelDown,
        WhiteGelRight,
        WhiteGelLeft,
        Timer,
        Beetle,
        BeetleHalf,
        KoopaRedFlying,
        KoopaFlying,
        CastleFireCcw,
        Seesaw,
        WarpPipe,
        CastleFireCw,
        LakitoEnd,
        NotGate,
        GelTop,
        GelLeft,
        GelBottom,
        GelRight,
        FireStart,
        Bowser,
        Axe,
        PlatformBonus,
        Spring,
        Squid,
        FlyingFishStart,
        FlyingFishEnd,
        UpFire,
        Spikey,
        SpikeyHalf,
        Checkpoint,
    ]
};

impl EntityKind {
    /// Resolve a level file's entity id (1-based). `None` for 0 or out of range.
    pub fn from_id(id: u16) -> Option<Self> {
        if id == 0 {
            return None;
        }
        ENTITY_ORDER.get((id - 1) as usize).copied()
    }

    /// The 1-based id this kind occupies.
    pub fn id(self) -> u16 {
        // Linear scan over 100 entries; only used for diagnostics and tests.
        ENTITY_ORDER.iter().position(|&k| k == self).unwrap() as u16 + 1
    }

    /// Is this an enemy spawned lazily by column as the camera scrolls?
    ///
    /// Mari0 does not create enemies at load — `spawnenemy` (`game.lua:3687`) runs
    /// per newly-revealed column, which is both a performance requirement (8-1 is
    /// 400 tiles wide) and the original's feel: groups appear together because a
    /// spawn also spawns `x±1` and `x±2`.
    pub fn is_lazy_enemy(self) -> bool {
        use EntityKind::*;
        matches!(
            self,
            Goomba
                | GoombaHalf
                | Koopa
                | KoopaHalf
                | KoopaRed
                | KoopaRedHalf
                | KoopaFlying
                | KoopaRedFlying
                | Beetle
                | BeetleHalf
                | Spikey
                | SpikeyHalf
                | HammerBro
                | Lakito
                | CheepRed
                | CheepWhite
                | Squid
                | Plant
                | Bowser
                | BulletBill
                | UpFire
                | CastleFireCw
                | CastleFireCcw
                | PlatformUp
                | PlatformRight
                | PlatformFall
                | PlatformBonus
        )
    }

    /// Is this a Portal-side lab element (built at load, wired by `link`)?
    pub fn is_lab(self) -> bool {
        use EntityKind::*;
        matches!(
            self,
            EmanceHor
                | EmanceVer
                | DoorVer
                | DoorHor
                | WallIndicator
                | Button
                | PushButtonLeft
                | PushButtonRight
                | NotGate
                | Timer
                | BoxTube
                | Box
                | LaserRight
                | LaserDown
                | LaserLeft
                | LaserUp
                | LaserDetectorRight
                | LaserDetectorDown
                | LaserDetectorLeft
                | LaserDetectorUp
                | LightBridgeRight
                | LightBridgeLeft
                | LightBridgeDown
                | LightBridgeUp
                | FaithPlateUp
                | FaithPlateRight
                | FaithPlateLeft
                | GroundLightVer
                | GroundLightHor
                | GroundLightUpRight
                | GroundLightRightDown
                | GroundLightDownLeft
                | GroundLightLeftUp
                // Gel dispensers: lab fixtures like the cube tubes, and the only
                // source of the paint the three gels are made of.
                | BlueGelDown
                | BlueGelRight
                | BlueGelLeft
                | OrangeGelDown
                | OrangeGelRight
                | OrangeGelLeft
                | WhiteGelDown
                | WhiteGelRight
                | WhiteGelLeft
        )
    }

    /// The colour and direction of a gel dispenser, if this is one.
    ///
    /// The entity encodes both: nine ids, three colours × three nozzle directions.
    /// There is no upward-facing one.
    pub fn gel_dispenser(self) -> Option<(Gel, crate::player::Orientation)> {
        use crate::player::Orientation::*;
        use EntityKind::*;
        Some(match self {
            BlueGelDown => (Gel::Blue, Down),
            BlueGelRight => (Gel::Blue, Right),
            BlueGelLeft => (Gel::Blue, Left),
            OrangeGelDown => (Gel::Orange, Down),
            OrangeGelRight => (Gel::Orange, Right),
            OrangeGelLeft => (Gel::Orange, Left),
            WhiteGelDown => (Gel::White, Down),
            WhiteGelRight => (Gel::White, Right),
            WhiteGelLeft => (Gel::White, Left),
            _ => return None,
        })
    }

    /// Gel painted directly onto a tile face by the level, with its face and
    /// the gel id carried in the cell's argument.
    ///
    /// Returns which face of the tile the gel coats.
    pub fn gel_face(self) -> Option<GelFace> {
        match self {
            EntityKind::GelTop => Some(GelFace::Top),
            EntityKind::GelBottom => Some(GelFace::Bottom),
            EntityKind::GelLeft => Some(GelFace::Left),
            EntityKind::GelRight => Some(GelFace::Right),
            _ => None,
        }
    }
}

/// Which face of a tile a gel coats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GelFace {
    Top,
    Bottom,
    Left,
    Right,
}

/// Gel colour. 1 blue (bounce), 2 orange (speed), 3 white (portalable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub enum Gel {
    Blue,
    Orange,
    White,
}

impl Gel {
    pub fn from_id(id: u16) -> Option<Self> {
        match id {
            1 => Some(Gel::Blue),
            2 => Some(Gel::Orange),
            3 => Some(Gel::White),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_the_documented_types() {
        // Anchors taken from real level data: `1-6` is 1-1's first goomba,
        // `8-2` is a question block holding a mushroom, `78-11` is the flagpole
        // base, `17-21-1` is a pipe warp entrance.
        assert_eq!(EntityKind::from_id(2), Some(EntityKind::Mushroom));
        assert_eq!(EntityKind::from_id(6), Some(EntityKind::Goomba));
        assert_eq!(EntityKind::from_id(8), Some(EntityKind::Spawn));
        assert_eq!(EntityKind::from_id(11), Some(EntityKind::Flag));
        assert_eq!(EntityKind::from_id(21), Some(EntityKind::Pipe));
        assert_eq!(EntityKind::from_id(31), Some(EntityKind::PipeSpawn));
        assert_eq!(EntityKind::from_id(81), Some(EntityKind::WarpPipe));
        assert_eq!(EntityKind::from_id(91), Some(EntityKind::Axe));
        assert_eq!(EntityKind::from_id(100), Some(EntityKind::Checkpoint));
    }

    #[test]
    fn the_ground_light_run_is_in_the_original_order() {
        // Six consecutive ids whose order is easy to transpose; the loader maps
        // them to directions 1..6 positionally.
        assert_eq!(EntityKind::from_id(43), Some(EntityKind::GroundLightVer));
        assert_eq!(EntityKind::from_id(44), Some(EntityKind::GroundLightHor));
        assert_eq!(
            EntityKind::from_id(45),
            Some(EntityKind::GroundLightUpRight)
        );
        assert_eq!(
            EntityKind::from_id(46),
            Some(EntityKind::GroundLightRightDown)
        );
        assert_eq!(
            EntityKind::from_id(47),
            Some(EntityKind::GroundLightDownLeft)
        );
        assert_eq!(EntityKind::from_id(48), Some(EntityKind::GroundLightLeftUp));
    }

    #[test]
    fn table_is_exactly_one_hundred_entries_and_bounds_are_respected() {
        assert_eq!(ENTITY_ORDER.len(), 100);
        assert_eq!(EntityKind::from_id(0), None);
        assert_eq!(EntityKind::from_id(101), None);
        assert_eq!(EntityKind::from_id(u16::MAX), None);
    }

    #[test]
    fn id_round_trips_for_every_kind() {
        for id in 1..=100u16 {
            let kind = EntityKind::from_id(id).expect("all ids resolve");
            assert_eq!(kind.id(), id, "round-trip failed for id {id}");
        }
    }

    #[test]
    fn no_duplicate_kinds_in_the_table() {
        // A copy-paste slip would make two ids resolve to the same kind and
        // silently misplace one of them in every level.
        let mut seen = std::collections::HashSet::new();
        for kind in ENTITY_ORDER {
            assert!(seen.insert(kind), "{kind:?} appears twice");
        }
    }

    #[test]
    fn gel_faces_and_ids() {
        assert_eq!(EntityKind::GelTop.gel_face(), Some(GelFace::Top));
        assert_eq!(EntityKind::GelRight.gel_face(), Some(GelFace::Right));
        assert_eq!(EntityKind::Goomba.gel_face(), None);
        assert_eq!(Gel::from_id(1), Some(Gel::Blue));
        assert_eq!(Gel::from_id(2), Some(Gel::Orange));
        assert_eq!(Gel::from_id(3), Some(Gel::White));
        assert_eq!(Gel::from_id(4), None);
    }

    #[test]
    fn enemies_are_lazy_and_lab_parts_are_not() {
        assert!(EntityKind::Goomba.is_lazy_enemy());
        assert!(EntityKind::Bowser.is_lazy_enemy());
        assert!(!EntityKind::Goomba.is_lab());
        assert!(EntityKind::DoorHor.is_lab());
        assert!(!EntityKind::DoorHor.is_lazy_enemy());
        // Markers are neither: they configure the level, not spawn anything.
        assert!(!EntityKind::Spawn.is_lazy_enemy() && !EntityKind::Spawn.is_lab());
        assert!(!EntityKind::Checkpoint.is_lazy_enemy() && !EntityKind::Checkpoint.is_lab());
    }
}
