//! The enemy roster: what each kind is, how tall it is, and how it moves.
//!
//! Scripted kinds (firebars, geysers, cheep-cheeps) ignore the walker logic
//! entirely — see `EnemyType::is_scripted`.

use std::collections::HashMap;

use vibe2d::prelude::*;

use crate::constants::*;
use crate::game::Mari0Game;
use crate::physics::*;
use crate::portal::{PortalBody, portal_carry};
use crate::world::EnemySpawnPoint;

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum EnemyType {
    Goomba,
    Koopa,
    /// Red koopa: identical to a green one except it refuses to walk off ledges.
    KoopaRed,
    /// Buzzy beetle: a koopa that shrugs off fireballs.
    Beetle,
    /// Piranha plant. Rises and retracts on a timer, never moves horizontally,
    /// and cannot be stomped — only fire or a star kills it.
    Plant,
    /// Winged koopa. Hovers on a cosine path; stomping it removes the wings and
    /// leaves an ordinary koopa behind.
    KoopaFlying,
    /// One fireball of a rotating firebar. Indestructible; contact hurts.
    ///
    /// Each segment is its own entity carrying the bar's pivot and its index, so
    /// the whole bar is just N of these sharing a pivot.
    Firebar,
    /// Lava geyser: leaps out of the bottom of the world and falls back.
    UpFire,
    /// Cheep-cheep. Red swims fast and level; white drifts slower and bobs.
    CheepRed,
    CheepWhite,
    /// Lakitu: rides a cloud above the level, matches the player's pace and lobs
    /// spiny eggs at him. Never touched by gravity or terrain.
    Lakito,
    /// A spiny, walking. Mechanically a goomba that cannot be stomped — the
    /// original literally builds it as one (`goomba.lua:48`, `t = "spikey"`), which
    /// is why it shares the walker path and the goomba's speed and animation rate.
    Spikey,
    /// A spiny still in its egg, arcing through the air after lakitu throws it.
    ///
    /// Its own kind rather than a flag because three things differ: it falls at
    /// 30 blocks/s² instead of 80, it drifts with no horizontal speed, and it ignores
    /// the lakitu who threw it until it has dropped two blocks below the release
    /// point. Landing turns it into a [`EnemyType::Spikey`].
    SpikeyFall,
    /// A bullet bill in flight: constant speed, no gravity, and **no terrain at all**.
    ///
    /// The last part is not a simplification. Every one of its collision handlers
    /// returns `false` (`bulletbill.lua:158-179`), and returning `false` is how the
    /// original's physics is told *not* to resolve a contact (`physics.lua:288-296`),
    /// so a bill flies through walls, floors and pipes alike. Only its 20-second
    /// lifetime stops it.
    BulletBill,
    /// Hammer bro: shuffles inside one block, throws hammers, and hops between
    /// floors by switching off his own tile collision for the duration of the hop.
    HammerBro,
    /// A hammer in flight. Arcs, ignores terrain, hurts from every side, and cannot
    /// be destroyed — a fireball bursts on one without stopping it.
    Hammer,
    /// A bloober. Drifts down, lunges diagonally up two blocks, sinks one, repeats —
    /// and cannot be stomped, so the only way past is round it.
    Squid,
    /// A flying fish, leaping out of the water on the player's own horizontal speed
    /// plus a random nudge. Ignores terrain; stomping one kills it.
    FlyingFish,
    /// Bowser. Nearly two blocks square, five fireballs to kill, and he retreats
    /// faster than he advances.
    Bowser,
    /// One breath of Bowser's fire — or of a `firestart` zone's, which uses the same
    /// object with no Bowser attached.
    Fire,
    /// The cannon, which is a timer rather than a creature.
    ///
    /// It lives in the enemy list only to inherit the lazy per-column reveal and the
    /// off-screen cull — the original's `rocketlauncher` isn't in `objects` at all and
    /// has no hitbox (`bulletbill.lua:1-20`). What you can stand on and bump into is
    /// the tile art underneath it, tiles 42 and 64. See [`EnemyType::harmless`].
    BulletBillCannon,
}

impl EnemyType {
    /// Does this behave as a koopa (shell mechanics, 24px-tall sprite)?
    pub(crate) fn is_koopa_like(self) -> bool {
        matches!(
            self,
            EnemyType::Koopa | EnemyType::KoopaRed | EnemyType::Beetle
        )
    }

    /// Red koopas turn around at a ledge instead of walking off.
    pub(crate) fn avoids_ledges(self) -> bool {
        self == EnemyType::KoopaRed
    }

    /// Buzzy beetles are immune to fireballs (that's their whole point).
    pub(crate) fn fireball_immune(self) -> bool {
        self == EnemyType::Beetle
    }

    /// Can the player kill this by landing on it?
    ///
    /// Plants, firebars and geysers hurt from every direction — jumping on a
    /// firebar is how you die, not how you win. So does a spiny, and that is the
    /// whole point of one: the original's test is a single inequality on the
    /// goomba's subtype, `a == "goomba" and b.t ~= "goomba"` → kill
    /// (`mario.lua:1778`), so anything built as a goomba that *isn't* a goomba
    /// hurts from above as well.
    pub(crate) fn stompable(self) -> bool {
        !matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::Spikey
                | EnemyType::SpikeyFall
                | EnemyType::Hammer
                | EnemyType::Squid
                | EnemyType::Bowser
                | EnemyType::Fire
        )
    }

    /// Enemies that ignore gravity and terrain and follow their own path.
    ///
    /// Lakitu belongs here on the original's own terms, not as a liberty: his
    /// `mask[2] = true` (`lakito.lua:16`) and Mari0's mask is an **exclusion** table —
    /// the physics collides when `mask[category] ~= true` (`physics.lua:113`) — so
    /// setting the tile category means he passes through walls. Same for the bullet
    /// bill. (It's also unobservable for lakitu either way: all four rows he flies in
    /// are empty of solid tiles in all three of his levels.)
    pub(crate) fn is_scripted(self) -> bool {
        matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::CheepRed
                | EnemyType::CheepWhite
                | EnemyType::KoopaFlying
                | EnemyType::Lakito
                | EnemyType::BulletBill
                | EnemyType::BulletBillCannon
                | EnemyType::Hammer
                | EnemyType::Squid
                | EnemyType::FlyingFish
                | EnemyType::Fire
        )
    }

    /// Ground speed this kind starts with.
    ///
    /// Only the hammer bro differs, and noticeably: 1.5 blocks/s against everyone
    /// else's 2, which is why he reads as shuffling rather than walking.
    pub(crate) fn walk_speed(self) -> f32 {
        match self {
            EnemyType::HammerBro => HAMMERBRO_SPEED,
            _ => ENEMY_SPEED,
        }
    }

    /// Downward acceleration while walking or falling.
    ///
    /// Only the thrown spiny egg differs from the world's gravity, and it differs a
    /// lot — see [`SPIKEY_FALL_GRAVITY`].
    pub(crate) fn gravity(self) -> f32 {
        match self {
            EnemyType::SpikeyFall => SPIKEY_FALL_GRAVITY,
            EnemyType::HammerBro => HAMMERBRO_GRAVITY,
            EnemyType::Hammer => HAMMER_GRAVITY,
            EnemyType::Bowser => BOWSER_GRAVITY,
            _ => GRAVITY,
        }
    }

    /// Can this kind travel through a portal?
    ///
    /// Two separate reasons a kind can't, both from the original:
    ///
    /// - **`static = true`** — plants, firebars and lava geysers are fixtures. They
    ///   have a position but never move, so the mover code that would carry them
    ///   through a portal never runs (`plant.lua:15`, `castlefire.lua:84`,
    ///   `upfire.lua:16`).
    /// - **`portalable = false`** — cheep-cheeps opt out explicitly even though they
    ///   do move (`cheepcheep.lua:33`), and so does lakitu (`lakito.lua:24`).
    pub(crate) fn portalable(self) -> bool {
        !matches!(
            self,
            EnemyType::Plant
                | EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::CheepRed
                | EnemyType::CheepWhite
                | EnemyType::Lakito
                // A fixture like the plants: it has a position and a timer, nothing
                // that could be carried anywhere.
                | EnemyType::BulletBillCannon
        )
    }

    /// Indestructible hazards: fire and stars don't remove them either.
    pub(crate) fn indestructible(self) -> bool {
        matches!(
            self,
            EnemyType::Firebar
                | EnemyType::UpFire
                | EnemyType::BulletBillCannon
                | EnemyType::Hammer
                | EnemyType::Fire
        )
    }

    /// Not a creature at all: no hitbox, no sprite, cannot hurt or be hurt.
    ///
    /// Only the bullet-bill cannon. It rides in the enemy list for the lazy reveal and
    /// nothing else, so every interaction pass has to skip it — otherwise the player
    /// dies to a cannon he is standing on, which is precisely the spot the original
    /// makes safe by refusing to fire at close range.
    pub(crate) fn harmless(self) -> bool {
        self == EnemyType::BulletBillCannon
    }

    /// Points for killing this with fire or a star (`firepoints`, `variables.lua:28-37`).
    ///
    /// A flat table, unlike stomping, which runs up the combo ladder. A goomba is the
    /// only cheap one; everything else in Super Mario Bros is worth 200 except the
    /// hammer bro at 1000 and Bowser at 5000.
    pub(crate) fn fire_points(self) -> u32 {
        match self {
            EnemyType::Goomba | EnemyType::Spikey | EnemyType::SpikeyFall => 100,
            EnemyType::HammerBro => 1000,
            EnemyType::Bowser => BOWSER_SCORE,
            _ => 200,
        }
    }

    /// Skips the portal system's third, catch-all entry test.
    ///
    /// The original expresses this as `mask[2]`, which the portal code reads as "don't
    /// run `inportal` on this" (`game.lua`'s portal pass). Swept entry still applies —
    /// a bill crossing a portal plane goes through — but it is never grabbed just for
    /// *overlapping* a mouth. Without the exemption a bill travelling at 8 blocks/s
    /// past a floor portal gets yanked sideways by a mouth it was never aimed at.
    pub(crate) fn exempt_from_containment(self) -> bool {
        matches!(self, EnemyType::BulletBill | EnemyType::Hammer)
    }
}

/// The deterministic stand-in for `math.random`.
///
/// The original picks cannon delays and bullet-bill altitudes at random. A real PRNG
/// would make every VDP probe and every autopilot run irreproducible, so this is a
/// plain 32-bit LCG (the Numerical Recipes constants) seeded per level. It looks
/// random, and the same level played twice plays the same way.
#[derive(Clone)]
pub(crate) struct Rng(u32);

impl Rng {
    pub(crate) fn new(seed: u32) -> Self {
        // Never zero: an LCG seeded at 0 with these constants is fine, but a nonzero
        // seed keeps the first few draws from clustering.
        Rng(seed | 1)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    /// A float in `min..=max`, quantised to tenths the way the original's
    /// `math.random(min*10, max*10)/10` is.
    pub(crate) fn tenths(&mut self, min: f32, max: f32) -> f32 {
        let lo = (min * 10.0).round() as u32;
        let hi = (max * 10.0).round() as u32;
        let span = hi - lo + 1;
        (lo + self.next_u32() % span) as f32 / 10.0
    }

    /// An integer in `min..=max`.
    pub(crate) fn range(&mut self, min: i32, max: i32) -> i32 {
        let span = (max - min + 1) as u32;
        min + (self.next_u32() % span) as i32
    }
}

/// Where a squid is in its three-beat cycle (`squid.lua:76-132`).
#[derive(Debug, PartialEq, Clone, Copy, Default)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum SquidPhase {
    /// Drifting down, waiting for the player to come level with it.
    #[default]
    Idle,
    /// Lunging: accelerating up and sideways until it has covered two blocks across.
    Lunge,
    /// Settling: sinking one block, then back to idle.
    Sink,
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum EnemyState {
    Walking,
    Dead,
    Shell,
    ShellMoving,
}

#[derive(Clone)]
pub(crate) struct Enemy {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) enemy_type: EnemyType,
    pub(crate) state: EnemyState,
    pub(crate) facing_right: bool,
    pub(crate) on_ground: bool,
    pub(crate) anim_timer: f32,
    pub(crate) death_timer: f32,
    pub(crate) flipped_death: bool, // true = star/fireball kill (flip + fly off)
    /// Y position the enemy spawned at. Plants oscillate around it.
    pub(crate) spawn_y: f32,
    /// Plant emerge/retract cycle position, in seconds. Also the firebar's
    /// accumulator and the flying koopa's hover phase.
    pub(crate) cycle_timer: f32,
    /// X position the enemy spawned at. Firebars rotate around it.
    pub(crate) spawn_x: f32,
    /// Current firebar angle in degrees.
    pub(crate) angle_deg: f32,
    /// Index of this fireball along its firebar (0 = at the pivot).
    pub(crate) segment: u32,
    /// Seconds until this cannon's next shot. Unused by everything else.
    pub(crate) fire_delay: f32,
    /// Hammer bro: seconds since his last hop between floors.
    pub(crate) jump_timer: f32,
    /// Bowser: fireball hits left before he goes down. Nothing else has hit points.
    pub(crate) hp: u32,
    /// Bowser: the end of the current leg of his pace, re-drawn at each turn.
    pub(crate) target_x: f32,
    /// Bowser: is the player behind him? Then he is scrambling backwards, and while he
    /// is he neither breathes fire nor throws hammers.
    pub(crate) backing_off: bool,
    /// Squid: which beat of its cycle it is on.
    pub(crate) squid_phase: SquidPhase,
    /// Squid: where the current beat began — the x a lunge started from, or the y a
    /// sink started from, depending on the phase. One field because only one is ever
    /// live at a time, and the original likewise keeps `upx` and `downy` apart.
    pub(crate) beat_from: f32,
    /// Hammer bro: is he mid-hop, and therefore passing through floors?
    ///
    /// This is `mask[2]`, switched on for the duration of a hop (`hammerbro.lua:110`,
    /// `:114`) — and since Mari0's mask *excludes*, switching it on means he stops
    /// colliding with tiles. That is the whole trick behind him climbing between the
    /// floors of a castle: he doesn't jump *onto* the next floor, he jumps *through*
    /// the ceiling and then turns collision back on so he lands on top of it.
    pub(crate) ignore_tiles: bool,
    /// Hammer bro: for a *downward* hop, the height it began at. A downward hop keeps
    /// falling through floors until it is [`HAMMERBRO_DROP_THROUGH`] below this.
    /// `None` during an upward hop, which instead ends the moment he starts falling.
    pub(crate) drop_from_y: Option<f32>,
    /// Has this been through a portal?
    ///
    /// Only a bullet bill cares, and what it earns is the right to kill: a bill that
    /// has been portaled sets `killstuff` and thereafter shoots down any goomba or
    /// koopa it touches (`bulletbill.lua:181-194`). Before that it passes through
    /// them harmlessly.
    pub(crate) portaled: bool,
}

/// Collision height of an enemy in its current state.
///
/// Koopa-likes stand 24px tall (48 at 2x) while walking but shrink to a shell; a
/// piranha plant is shorter than a block; everything else is one small-Mario tall.
pub(crate) fn enemy_height(enemy_type: EnemyType, state: EnemyState) -> f32 {
    if enemy_type == EnemyType::Bowser {
        BOWSER_H
    } else if enemy_type == EnemyType::Fire {
        FIRE_H
    } else if enemy_type.is_koopa_like() && state == EnemyState::Walking {
        48.0
    } else if enemy_type == EnemyType::Plant {
        PLANT_HEIGHT
    } else {
        PLAYER_SMALL_H
    }
}

/// Where a piranha plant's hitbox sits when fully retracted, given its spawn cell.
///
/// Its own function because a plant is the one enemy whose placement is not "stand
/// it on the cell": it is a fixture, so nothing corrects a bad y afterwards. Every
/// other enemy is dropped a tile high and lands in the right place within a few
/// frames, which is why this was wrong for a long time without being obvious.
pub(crate) fn plant_rest_y(spawn_y: f32) -> f32 {
    spawn_y + PLANT_REST_DROP
}

impl Enemy {
    /// Instantiate one spawn point.
    pub(crate) fn from_spawn(sp: &EnemySpawnPoint) -> Self {
        let h = enemy_height(sp.enemy_type, EnemyState::Walking);
        // Spawn coordinates name the cell the enemy stands *on*, so a taller
        // enemy has to be lifted by its own height to rest on that surface. Two
        // exceptions: a plant hangs *below* its cell, tucked into the pipe, and a
        // cannon simply *is* its cell — it doesn't stand anywhere, and lifting it
        // sends its bills out a row above the barrel.
        let y = match sp.enemy_type {
            EnemyType::Plant => plant_rest_y(sp.y),
            EnemyType::BulletBillCannon => sp.y,
            _ => sp.y - h,
        };
        // A plant is a block wide but its cell is the pipe's *left* column, so it
        // straddles both (`plant.lua:9`, `self.x = x - 8/16`) — that half-block is
        // what centres it on a two-wide pipe. All 102 plants in the shipped levels
        // sit one row above a pipe's rim, which is what makes this safe to assume.
        let x = if sp.enemy_type == EnemyType::Plant {
            sp.x + TILE_SIZE / 2.0
        } else {
            sp.x
        };
        Enemy {
            x,
            y,
            vx: if sp.facing_right {
                sp.enemy_type.walk_speed()
            } else {
                -sp.enemy_type.walk_speed()
            },
            vy: 0.0,
            enemy_type: sp.enemy_type,
            state: EnemyState::Walking,
            facing_right: sp.facing_right,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y,
            // Bowser's five hit points, and the leg of his pace he starts on. `segment`
            // carries the world number for him, since that is what decides whether he
            // throws hammers at all.
            hp: if sp.enemy_type == EnemyType::Bowser {
                BOWSER_HEALTH
            } else {
                0
            },
            target_x: if sp.enemy_type == EnemyType::Bowser {
                // `newtargetx("right")` at birth (`bowser.lua:58`).
                x - BOWSER_TURN_NEAR - TILE_SIZE
            } else {
                0.0
            },
            backing_off: false,
            // A hammer bro's throw countdown. The original picks between 0.6 and 1.6 at
            // birth; a fixed first gap keeps `from_spawn` free of the RNG, and every
            // gap after this one is drawn properly.
            cycle_timer: if sp.enemy_type == EnemyType::HammerBro {
                HAMMERBRO_TIME[0]
            } else {
                0.0
            },
            spawn_x: x,
            // Each firebar segment starts at the same angle; its distance
            // from the pivot is what differs.
            angle_deg: 0.0,
            segment: sp.segment,
            // A cannon opens fire half a second after it appears. The original gets
            // there by starting its timer at `time - 0.5` with a random `time`
            // (`bulletbill.lua:7-8`); a fixed first delay is the same thing observed,
            // and it means `from_spawn` needs no access to the RNG.
            fire_delay: BULLET_BILL_FIRST_SHOT,
            portaled: false,
            jump_timer: 0.0,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: false,
            drop_from_y: None,
        }
    }

    /// Killed by fire, a star or a kicked shell: flips over and sails off screen
    /// (`goomba.lua:177-189`).
    ///
    /// Worth a method rather than four copies because lakitu turns it into something
    /// else entirely: for him "dead" is a 16-second absence, after which he sails
    /// back in from the right edge as if nothing happened.
    pub(crate) fn shotted(&mut self) {
        self.state = EnemyState::Dead;
        self.flipped_death = true;
        self.vy = -SHOT_JUMP_FORCE;
        self.vx = if self.facing_right {
            SHOT_SPEED_X
        } else {
            -SHOT_SPEED_X
        };
        self.death_timer = if self.enemy_type == EnemyType::Lakito {
            LAKITO_RESPAWN
        } else {
            SHOT_DEATH_TIME
        };
    }

    /// A bullet bill leaving a cannon, or dropped in by the `bulletbillstart` zone.
    ///
    /// `cycle_timer` is its age; at [`BULLET_BILL_LIFETIME`] the cull takes it, which
    /// is the only thing that ever will — it has no terrain to stop it.
    pub(crate) fn bullet_bill(x: f32, y: f32, dir: f32) -> Self {
        Enemy {
            x,
            y,
            vx: dir * BULLET_BILL_SPEED,
            vy: 0.0,
            enemy_type: EnemyType::BulletBill,
            state: EnemyState::Walking,
            facing_right: dir > 0.0,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            hp: 0,
            target_x: 0.0,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: false,
            drop_from_y: None,
        }
    }

    /// A flying fish, just out of the water.
    ///
    /// Its sideways speed is **the player's own** plus a random nudge of -4..=5
    /// blocks/s (`flyingfish.lua:11`), which is why a school of them tracks you as you
    /// run rather than being dodgeable by moving: outrunning them is impossible by
    /// construction. A zero result is bumped to 1 so a stationary player still gets
    /// fish that drift.
    pub(crate) fn flying_fish(x: f32, y: f32, vx: f32) -> Self {
        let vx = if vx == 0.0 { TILE_SIZE } else { vx };
        Enemy {
            x,
            y,
            vx,
            vy: -FLYING_FISH_FORCE,
            enemy_type: EnemyType::FlyingFish,
            state: EnemyState::Walking,
            facing_right: vx > 0.0,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            hp: 0,
            target_x: 0.0,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: true,
            drop_from_y: None,
        }
    }

    /// One breath of fire.
    ///
    /// `spawn_y` is the height it is *aimed* at, which it drifts towards rather than
    /// flying straight to — Bowser aims a random couple of blocks around his own
    /// starting row (`fire.lua:11`), so a volley fans out vertically.
    pub(crate) fn fire(x: f32, y: f32, target_y: f32) -> Self {
        Enemy {
            x,
            y,
            vx: -FIRE_SPEED,
            vy: 0.0,
            enemy_type: EnemyType::Fire,
            state: EnemyState::Walking,
            facing_right: false,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: target_y,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
            hp: 0,
            target_x: 0.0,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            ignore_tiles: true,
            drop_from_y: None,
        }
    }

    /// A hammer, just released. Thrown up and forward, then it arcs down through
    /// everything — no terrain, no lifetime, gone when it leaves the screen.
    pub(crate) fn hammer(x: f32, y: f32, dir: f32) -> Self {
        Enemy {
            x,
            // Released a block above the bro's own box (`hammerbro.lua:274`), which is
            // what puts the arc's peak over your head rather than at his waist.
            y: y - TILE_SIZE,
            vx: dir * HAMMER_SPEED,
            vy: -HAMMER_TOSS_SPEED,
            enemy_type: EnemyType::Hammer,
            state: EnemyState::Walking,
            facing_right: dir > 0.0,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y - TILE_SIZE,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            hp: 0,
            target_x: 0.0,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: true,
            drop_from_y: None,
        }
    }

    /// A spiny egg, mid-throw. `spawn_y` is the release height, which is what the
    /// two-block window for hitting lakitu is measured from.
    fn spiny_egg(x: f32, y: f32) -> Self {
        Enemy {
            x,
            y,
            vx: 0.0,
            vy: -SPIKEY_TOSS_SPEED,
            enemy_type: EnemyType::SpikeyFall,
            state: EnemyState::Walking,
            facing_right: false,
            on_ground: false,
            anim_timer: 0.0,
            death_timer: 0.0,
            flipped_death: false,
            spawn_y: y,
            cycle_timer: 0.0,
            spawn_x: x,
            angle_deg: 0.0,
            segment: 0,
            fire_delay: 0.0,
            portaled: false,
            jump_timer: 0.0,
            hp: 0,
            target_x: 0.0,
            backing_off: false,
            squid_phase: SquidPhase::Idle,
            beat_from: 0.0,
            ignore_tiles: false,
            drop_from_y: None,
        }
    }
}

/// Which spawns one tile column reveals, cluster rule included.
///
/// Split out from the game struct so the rule is testable without a window: it
/// takes only the cell index and the "already spawned" flags, marks what it
/// claims, and returns the indices to instantiate.
///
/// The original recurses into `x-2, x-1, x+1, x+2` at the *same* row whenever a
/// cell actually yields an enemy, with the already-spawned list as the base case —
/// its own comment reads "spawn enemies in 5x1 line so they spawn as a unit and
/// not alone" (`game.lua:3795-3798`). So a horizontal run of goombas arrives
/// together instead of trickling in one column at a time, and the chain can reach
/// well past the five cells the comment suggests, because each newly spawned cell
/// spreads in turn. Written as a work stack rather than recursion for that reason.
///
/// A cell that yields nothing does **not** spread: in the original the recursive
/// calls sit inside the `if enemy then` branch.
pub(crate) fn column_spawn_indices(
    by_cell: &HashMap<(i32, i32), Vec<usize>>,
    spawned: &mut [bool],
    col: i32,
) -> Vec<usize> {
    let mut claimed = Vec::new();
    let mut pending: Vec<(i32, i32)> = by_cell.keys().filter(|(c, _)| *c == col).copied().collect();
    // Sorted so the order enemies enter the world is deterministic. Lua's `pairs`
    // gives an arbitrary hash order here; anything stable is closer to the intent
    // than "whatever the allocator did".
    pending.sort_unstable();

    while let Some(cell) = pending.pop() {
        let Some(indices) = by_cell.get(&cell) else {
            continue;
        };
        let fresh: Vec<usize> = indices.iter().copied().filter(|i| !spawned[*i]).collect();
        if fresh.is_empty() {
            continue;
        }
        for i in fresh {
            spawned[i] = true;
            claimed.push(i);
        }
        for d in [-2, -1, 1, 2] {
            pending.push((cell.0 + d, cell.1));
        }
    }
    claimed
}

impl Mari0Game {
    /// Instantiate everything the camera has revealed since the last call.
    ///
    /// Mari0 does not create enemies at load; it walks the columns the camera has
    /// uncovered and spawns what it finds (`game.lua:681-686`, `spawnenemy` at
    /// `:3687`). This matters for more than memory: 8-1 is **400 tiles wide**, and
    /// an enemy that existed from frame one would have walked off its ledge long
    /// before the player arrived. Spawning on reveal is the original's feel, not
    /// an optimisation.
    ///
    /// The frontier sits one screen-width plus one column ahead of the camera, so
    /// enemies come into being just off the right edge.
    pub(crate) fn spawn_revealed_columns(&mut self) {
        let screen_cols = (self.vw / TILE_SIZE).ceil() as i32;
        let target = (self.camera.x / TILE_SIZE).floor() as i32 + screen_cols + 1;
        while self.spawn_frontier < target {
            self.spawn_frontier += 1;
            for i in column_spawn_indices(
                &self.level.spawns_by_cell,
                &mut self.spawned,
                self.spawn_frontier,
            ) {
                self.enemies
                    .push(Enemy::from_spawn(&self.level.enemy_spawns[i]));
            }
            // Platforms come through the same sweep, and so inherit the cluster rule:
            // the original creates them in `spawnenemy` alongside the creatures, which
            // is also why their oscillation phase starts when the camera reveals them
            // rather than at load — you meet a lift at the bottom of its travel.
            for i in column_spawn_indices(
                &self.level.platform_spawns_by_cell,
                &mut self.platforms_spawned,
                self.spawn_frontier,
            ) {
                let sp = self.level.platform_spawns[i];
                self.platforms.push(crate::platform::Platform::new(
                    sp.cell.0,
                    sp.cell.1,
                    sp.kind,
                    sp.size_blocks,
                ));
            }
        }
    }

    pub(crate) fn update_enemies(&mut self, dt: f32, ctx: &mut Context) {
        let cam_x = self.camera.x;
        // Cloned up front: the loop below holds `&mut self.enemies`.
        let portals = self.portal_pair();
        let retired = self.lakito_retired;
        // Lakitu holds his fire while three spinies are already out. Counted once,
        // before anything moves, so two lakitus in one level (no shipped level has
        // any) would both see the same tally rather than racing each other.
        let spinies_out = self
            .enemies
            .iter()
            .filter(|e| {
                matches!(e.enemy_type, EnemyType::Spikey | EnemyType::SpikeyFall)
                    && e.state != EnemyState::Dead
            })
            .count();
        // Eggs and bullet bills can't be pushed onto `self.enemies` from inside the
        // loop that borrows it, so they queue here and join at the end of the frame.
        let mut thrown: Vec<Enemy> = Vec::new();
        let mut fired: Vec<(f32, f32, f32)> = Vec::new();
        // Cannons share one cap on live bills (`maximumbulletbills`). Counted before
        // anything moves, so two cannons in range can't both slip past the fifth slot.
        let bills_out = self
            .enemies
            .iter()
            .filter(|e| e.enemy_type == EnemyType::BulletBill && e.state == EnemyState::Walking)
            .count();
        // Borrowed out of `self` so the loop below can draw from it while holding
        // `&mut self.enemies`.
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        // Where lakitu aims: the player's position `LAKITO_DISTANCE_TIME` seconds
        // from now at his current speed (`lakito.lua:80`). Chasing where the player
        // *is* would let you shake him off by just holding a direction.
        let lead_x = self.player.x + self.player.vx * LAKITO_DISTANCE_TIME;

        for enemy in &mut self.enemies {
            let ew = PLAYER_SMALL_W;
            let eh = enemy_height(enemy.enemy_type, enemy.state);

            // Portals, before the scripted/walking split rather than inside the
            // walking branch. Being scripted and being portable are independent
            // properties — a winged koopa and a bullet bill are both scripted and
            // both travel — and while the check sat inside the walker path neither
            // of them ever could.
            if enemy.enemy_type.portalable()
                && enemy.state != EnemyState::Dead
                && let Some((nx, ny, nvx, nvy)) = portal_carry(
                    &self.level,
                    portals.as_ref(),
                    PortalBody {
                        x: enemy.x,
                        y: enemy.y,
                        w: ew,
                        h: eh,
                        vx: enemy.vx,
                        vy: enemy.vy,
                    },
                    dt,
                    !enemy.enemy_type.exempt_from_containment(),
                )
            {
                enemy.x = nx;
                enemy.y = ny;
                enemy.vx = nvx;
                enemy.vy = nvy;
                enemy.facing_right = nvx > 0.0;
                // A bill that has been through a portal comes out lethal to other
                // enemies. Nothing else acts on this.
                enemy.portaled = true;
                continue;
            }

            // Scripted enemies follow their own path and ignore gravity and
            // terrain entirely, so they bypass the walking/collision path below.
            if enemy.enemy_type.is_scripted() && enemy.state == EnemyState::Walking {
                enemy.anim_timer += dt;
                enemy.cycle_timer += dt;
                let start_y = enemy.spawn_y;
                match enemy.enemy_type {
                    EnemyType::Plant => {
                        if enemy.cycle_timer < PLANT_OUT_TIME {
                            // Emerging.
                            enemy.y =
                                (enemy.y - PLANT_MOVE_SPEED * dt).max(start_y - PLANT_MOVE_DIST);
                        } else if enemy.cycle_timer < PLANT_OUT_TIME + PLANT_IN_TIME {
                            // Retracting.
                            enemy.y = (enemy.y + PLANT_MOVE_SPEED * dt).min(start_y);
                        } else {
                            // Fully retracted: hold while the player is near the
                            // pipe, which is what makes waiting on top safe.
                            let player_cx = self.player.center_x();
                            let plant_cx = enemy.x + ew / 2.0;
                            if (player_cx - plant_cx).abs() > PLANT_PLAYER_NEAR {
                                enemy.cycle_timer = 0.0;
                            }
                        }
                    }
                    EnemyType::KoopaFlying => {
                        // Cosine hover: `(-cos(t*2pi)+1)/2` over the cycle
                        // (`koopa.lua:72`), which starts and ends at rest rather
                        // than snapping at the turnaround.
                        let t = (enemy.cycle_timer / KOOPA_FLYING_TIME).fract();
                        let eased = (-(t * std::f32::consts::TAU).cos() + 1.0) / 2.0;
                        enemy.y = start_y + eased * KOOPA_FLYING_DISTANCE;
                    }
                    EnemyType::Firebar => {
                        // The bar advances in fixed 11.25-degree ticks rather than
                        // continuously — 32 discrete positions per revolution, and
                        // reproducing the stepping matters for dodge timing.
                        while enemy.cycle_timer >= FIREBAR_DELAY {
                            enemy.cycle_timer -= FIREBAR_DELAY;
                            enemy.angle_deg = (enemy.angle_deg + FIREBAR_ANGLE_STEP) % 360.0;
                        }
                        let radius = enemy.segment as f32 * FIREBAR_SEGMENT_SPACING;
                        let rad = enemy.angle_deg.to_radians();
                        enemy.x = enemy.spawn_x + rad.cos() * radius;
                        enemy.y = start_y + rad.sin() * radius;
                    }
                    EnemyType::UpFire => {
                        // Leaps from below the world, arcs up, falls back, and
                        // relaunches after a random delay.
                        enemy.vy += UPFIRE_GRAVITY * dt;
                        enemy.y += enemy.vy * dt;
                        let floor = (self.level.height as f32) * TILE_SIZE;
                        if enemy.y > floor && enemy.vy > 0.0 {
                            enemy.y = floor;
                            enemy.vy = -UPFIRE_FORCE;
                        }
                    }
                    EnemyType::BulletBill => {
                        // Straight line, forever, through everything. The lifetime
                        // check that eventually removes it lives in the cull below.
                        enemy.x += enemy.vx * dt;
                    }
                    EnemyType::Squid => {
                        // Three beats, and the shape of them is what makes a bloober
                        // awkward: it never chases you, it *intercepts*. It sinks until
                        // you are level with it, throws itself up and across two
                        // blocks, then settles one block and waits again.
                        match enemy.squid_phase {
                            SquidPhase::Idle => {
                                enemy.vy = SQUID_FALL_SPEED;
                                // "Level with the player" measured against where a *big*
                                // Mario's head would be, whatever size he actually is
                                // (`squid.lua:80` subtracts his height from 24/16), so a
                                // small Mario is lunged at from further above.
                                let head =
                                    self.player.y - (SQUID_TRIGGER_HEAD - self.player.height);
                                if enemy.y + enemy.vy * dt + eh + SQUID_TRIGGER_SLACK >= head {
                                    enemy.squid_phase = SquidPhase::Lunge;
                                    enemy.beat_from = enemy.x;
                                    enemy.vx = 0.0;
                                    enemy.vy = 0.0;
                                    // Turn towards the player if it is facing away. The
                                    // original wraps this in `if true then` with a
                                    // commented-out `math.random(2) == 1`
                                    // (`squid.lua:87`) — the coin flip was disabled, so
                                    // it turns every time.
                                    if enemy.facing_right && enemy.x > self.player.x {
                                        enemy.facing_right = false;
                                    } else if !enemy.facing_right && enemy.x < self.player.x {
                                        enemy.facing_right = true;
                                    }
                                }
                            }
                            SquidPhase::Lunge => {
                                let dir = if enemy.facing_right { 1.0 } else { -1.0 };
                                enemy.vx = (enemy.vx + dir * SQUID_ACCELERATION * dt)
                                    .clamp(-SQUID_X_SPEED, SQUID_X_SPEED);
                                enemy.vy =
                                    (enemy.vy - SQUID_ACCELERATION * dt).max(-SQUID_UP_SPEED);
                                // Ends on *sideways* distance covered, not on time or
                                // height — so a lunge is always two blocks across
                                // however far up it got.
                                if (enemy.x - enemy.beat_from).abs() >= SQUID_LUNGE_DIST {
                                    enemy.squid_phase = SquidPhase::Sink;
                                    enemy.beat_from = enemy.y;
                                    enemy.vx = 0.0;
                                }
                            }
                            SquidPhase::Sink => {
                                enemy.vy = SQUID_FALL_SPEED;
                                if enemy.y > enemy.beat_from + SQUID_DOWN_DIST {
                                    enemy.squid_phase = SquidPhase::Idle;
                                }
                            }
                        }
                        enemy.x += enemy.vx * dt;
                        enemy.y += enemy.vy * dt;
                    }
                    EnemyType::FlyingFish => {
                        // A ballistic leap through everything, lighter than the world.
                        enemy.vy += FLYING_FISH_GRAVITY * dt;
                        enemy.x += enemy.vx * dt;
                        enemy.y += enemy.vy * dt;
                    }
                    EnemyType::Fire => {
                        // Sideways at a constant speed, but *drifting* vertically
                        // towards the height it was aimed at (`fire.lua:68-79`) — which
                        // is what makes ducking under one unreliable, since it comes
                        // down to meet you.
                        enemy.x += enemy.vx * dt;
                        let target = enemy.spawn_y;
                        if enemy.y > target {
                            enemy.y = (enemy.y - FIRE_VER_SPEED * dt).max(target);
                        } else if enemy.y < target {
                            enemy.y = (enemy.y + FIRE_VER_SPEED * dt).min(target);
                        }
                    }
                    EnemyType::Hammer => {
                        // A ballistic arc that ignores terrain (`mask[2]`), so it can
                        // be thrown across a gap or down through the floor you are
                        // standing on. The off-screen cull is what disposes of it.
                        enemy.vy += HAMMER_GRAVITY * dt;
                        enemy.x += enemy.vx * dt;
                        enemy.y += enemy.vy * dt;
                    }
                    EnemyType::BulletBillCannon => {
                        // The timer runs whether or not the cannon is on screen; only
                        // *firing* is gated on being visible (`bulletbill.lua:12-19`).
                        if enemy.cycle_timer <= enemy.fire_delay {
                            continue;
                        }
                        let on_screen =
                            enemy.x > cam_x && enemy.x < cam_x + self.vw + 2.0 * TILE_SIZE;
                        if !on_screen {
                            continue;
                        }
                        if bills_out >= MAX_BULLET_BILLS {
                            // Timer keeps climbing, so it fires the instant a slot
                            // frees rather than waiting out a fresh delay.
                            continue;
                        }
                        // Fires *away from* the player's right edge, and only once he
                        // is more than `BULLET_BILL_RANGE` clear — which is what makes
                        // standing on a cannon safe (`bulletbill.lua:35-41`).
                        let player_edge = self.player.x + self.player.width;
                        let dir = if player_edge > enemy.x + BULLET_BILL_RANGE {
                            1.0
                        } else if player_edge < enemy.x - BULLET_BILL_RANGE {
                            -1.0
                        } else {
                            continue;
                        };
                        fired.push((enemy.x, enemy.y, dir));
                        enemy.cycle_timer = 0.0;
                        enemy.fire_delay = rng.tenths(BULLET_BILL_TIME_MIN, BULLET_BILL_TIME_MAX);
                    }
                    EnemyType::Lakito => {
                        if retired {
                            // Past `lakitoend` he stops caring: no more eggs, no more
                            // tracking, just a steady drift left until the cull takes
                            // him (`lakito.lua:59-60`, `:106-108`).
                            enemy.x -= LAKITO_PASSIVE_SPEED * dt;
                            enemy.facing_right = false;
                            continue;
                        }

                        if spinies_out < LAKITO_MAX_SPINIES && enemy.cycle_timer > LAKITO_THROW_TIME
                        {
                            // Released from just above him, tossed straight up. The
                            // egg carries no sideways speed at all — the arc you dodge
                            // comes from lakitu's own motion at the moment of release.
                            thrown.push(Enemy::spiny_egg(enemy.x, enemy.y - PLAYER_SMALL_H));
                            enemy.cycle_timer = 0.0;
                        }

                        // Turning is hysteretic: he only reverses once he is a full
                        // `LAKITO_SPACE` blocks past the lead point, so he oscillates
                        // slowly around the player instead of jittering on top of him.
                        let space = LAKITO_SPACE * TILE_SIZE;
                        if !enemy.facing_right && enemy.x < lead_x - space {
                            enemy.facing_right = true;
                        } else if enemy.facing_right && enemy.x > lead_x + space {
                            enemy.facing_right = false;
                        }

                        // The two directions are not mirror images, and that
                        // asymmetry is the whole character: heading right he closes
                        // at a speed proportional to the gap, so he always catches
                        // up; heading left he only ever manages 2 blocks/s, so you
                        // can outrun him going forward but never leave him behind.
                        enemy.vx = if enemy.facing_right {
                            let blocks = (enemy.x - lead_x).abs() / TILE_SIZE;
                            ((blocks - 3.0) * 2.0).round().max(2.0) * TILE_SIZE
                        } else {
                            -2.0 * TILE_SIZE
                        };
                        enemy.x += enemy.vx * dt;
                    }
                    EnemyType::CheepRed | EnemyType::CheepWhite => {
                        let speed = if enemy.enemy_type == EnemyType::CheepRed {
                            CHEEP_RED_SPEED
                        } else {
                            CHEEP_WHITE_SPEED
                        };
                        enemy.x += if enemy.facing_right { speed } else { -speed } * dt;
                        // White cheeps bob; red ones swim level.
                        if enemy.enemy_type == EnemyType::CheepWhite {
                            let bob = (enemy.cycle_timer * CHEEP_Y_SPEED).sin();
                            enemy.y = start_y + bob * CHEEP_HEIGHT;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            match enemy.state {
                EnemyState::Walking | EnemyState::ShellMoving => {
                    enemy.anim_timer += dt;

                    if enemy.enemy_type == EnemyType::Bowser {
                        // **The retreat is the whole fight.** If the player has got
                        // past him he turns and scrambles backwards at more than twice
                        // his advancing speed (`bowser.lua:139-142`), and while
                        // `backwards` is set he neither breathes nor throws — so
                        // getting behind him is not just an escape, it disarms him.
                        enemy.backing_off = self.player.x > enemy.x + BOWSER_W / 2.0;
                        if enemy.backing_off {
                            enemy.facing_right = false;
                            enemy.vx = BOWSER_SPEED_BACKWARDS;
                            enemy.jump_timer = 0.0;
                        } else {
                            enemy.facing_right = false;
                            // Pace towards the current target, then draw a new one on
                            // the other side. Both ends are randomised, so he never
                            // paces the same beat twice.
                            if enemy.x < enemy.target_x {
                                enemy.vx = BOWSER_SPEED_FORWARDS;
                                if enemy.x + enemy.vx * dt >= enemy.target_x {
                                    enemy.target_x = enemy.spawn_x
                                        - BOWSER_TURN_FAR
                                        - rng.range(1, 2) as f32 * TILE_SIZE;
                                }
                            } else {
                                enemy.vx = -BOWSER_SPEED_FORWARDS;
                                if enemy.x + enemy.vx * dt <= enemy.target_x {
                                    enemy.target_x = enemy.spawn_x
                                        - BOWSER_TURN_NEAR
                                        - rng.range(1, 2) as f32 * TILE_SIZE;
                                }
                            }
                            // Hops on a fixed cadence while he has the player in front
                            // of him, and only while grounded.
                            enemy.jump_timer += dt;
                            if enemy.jump_timer > BOWSER_JUMP_DELAY && enemy.on_ground {
                                enemy.vy = -BOWSER_JUMP_FORCE;
                                enemy.jump_timer -= BOWSER_JUMP_DELAY;
                            }
                            // Hammers, from world 6 on. `hp` doubles as the "does this
                            // one throw" flag via the level it was built for.
                            if enemy.segment >= BOWSER_HAMMER_WORLD {
                                enemy.cycle_timer += dt;
                                while enemy.cycle_timer > enemy.fire_delay {
                                    enemy.cycle_timer -= enemy.fire_delay;
                                    thrown.push(Enemy::hammer(enemy.x, enemy.y, -1.0));
                                    let i = rng.range(0, BOWSER_HAMMER_TABLE.len() as i32 - 1);
                                    enemy.fire_delay = BOWSER_HAMMER_TABLE[i as usize];
                                }
                            }
                        }
                        // His fall speed is capped for the animation's sake
                        // (`bowser.lua:74`), which is also why he drops so slowly.
                        enemy.vy = enemy.vy.min(BOWSER_FALL_SPEED);
                    }

                    if enemy.enemy_type == EnemyType::HammerBro {
                        // Faces whichever way the player is, every frame, independent
                        // of which way he happens to be shuffling — so he throws
                        // towards you even while stepping away (`hammerbro.lua:144`).
                        enemy.facing_right = self.player.x > enemy.x;

                        // The patrol is one block wide, between `startx - 1` and
                        // `startx`. Not a walk: a shuffle in place.
                        if enemy.vx < 0.0 && enemy.x < enemy.spawn_x - HAMMERBRO_PATROL {
                            enemy.vx = HAMMERBRO_SPEED;
                        } else if enemy.vx > 0.0 && enemy.x > enemy.spawn_x {
                            enemy.vx = -HAMMERBRO_SPEED;
                        }

                        enemy.cycle_timer -= dt;
                        if enemy.cycle_timer <= 0.0 {
                            let dir = if enemy.facing_right { 1.0 } else { -1.0 };
                            thrown.push(Enemy::hammer(enemy.x, enemy.y, dir));
                            enemy.cycle_timer = HAMMERBRO_TIME[rng.range(0, 1) as usize];
                        }

                        enemy.jump_timer += dt;
                        if enemy.jump_timer > HAMMERBRO_JUMP_TIME {
                            enemy.jump_timer -= HAMMERBRO_JUMP_TIME;
                            // Hemmed in at the top and the bottom, random in between.
                            // The comparison is against raw y, so "low" means far down
                            // the screen (`hammerbro.lua:96-106`).
                            let up = if enemy.y > HAMMERBRO_LOW_ROW {
                                true
                            } else if enemy.y < HAMMERBRO_HIGH_ROW {
                                false
                            } else {
                                rng.range(0, 1) == 0
                            };
                            // Both directions start with an upward kick and both switch
                            // tile collision *off*: he leaves by going through the
                            // ceiling or the floor, not by clearing it.
                            enemy.ignore_tiles = true;
                            if up {
                                enemy.vy = -HAMMERBRO_JUMP_FORCE;
                                enemy.drop_from_y = None;
                            } else {
                                enemy.vy = -HAMMERBRO_JUMP_FORCE_DOWN;
                                enemy.drop_from_y = Some(enemy.y);
                            }
                        }
                        if enemy.ignore_tiles {
                            let landed = match enemy.drop_from_y {
                                // Upward: the moment he stops rising, collision comes
                                // back and he settles onto the floor he just crossed.
                                None => enemy.vy > 0.0,
                                // Downward: keep falling through until two blocks below
                                // the start, which selects the next floor down.
                                Some(from) => enemy.y > from + HAMMERBRO_DROP_THROUGH,
                            };
                            if landed {
                                enemy.ignore_tiles = false;
                                enemy.drop_from_y = None;
                            }
                        }
                    }

                    // Gravity. Per-kind because a thrown spiny egg is the one thing
                    // in the game that falls slower than everything else.
                    enemy.vy += enemy.enemy_type.gravity() * dt;
                    if enemy.vy > MAX_Y_SPEED {
                        enemy.vy = MAX_Y_SPEED;
                    }

                    // Horizontal movement + wall collision
                    let old_x = enemy.x;
                    enemy.x += enemy.vx * dt;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + eh - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            // A hammer bro mid-hop has switched his tile collision off
                            // (`mask[2]`), which is how he crosses a floor at all.
                            if !enemy.ignore_tiles && blocks_movement(&self.level, col, row) {
                                let (tx, _ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap([enemy.x, enemy.y, ew, eh], [tx, _ty, tw, th]) {
                                    if enemy.vx > 0.0 {
                                        enemy.x = tx - ew;
                                    } else if enemy.vx < 0.0 {
                                        enemy.x = tx + tw;
                                    }
                                    enemy.vx = -enemy.vx;
                                    if enemy.state == EnemyState::Walking {
                                        enemy.facing_right = !enemy.facing_right;
                                    }
                                }
                            }
                        }
                    }

                    // Red koopas refuse to walk off a ledge: if the tile ahead
                    // and below is empty while they're grounded, turn around.
                    // Checked before the vertical step so the turn happens on the
                    // last solid tile rather than mid-fall.
                    if enemy.enemy_type.avoids_ledges()
                        && enemy.on_ground
                        && enemy.state == EnemyState::Walking
                    {
                        let ahead_x = if enemy.vx > 0.0 {
                            enemy.x + ew + 1.0
                        } else {
                            enemy.x - 1.0
                        };
                        let ahead_col = (ahead_x / TILE_SIZE).floor() as i32;
                        let below_row = ((enemy.y + eh + 2.0) / TILE_SIZE).floor() as i32;
                        if !is_solid(get_tile(&self.level, ahead_col, below_row)) {
                            enemy.vx = -enemy.vx;
                            enemy.facing_right = !enemy.facing_right;
                        }
                    }

                    // Vertical movement + ground/ceiling collision
                    enemy.y += enemy.vy * dt;
                    enemy.on_ground = false;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + eh - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            if !enemy.ignore_tiles && blocks_movement(&self.level, col, row) {
                                let (tx, ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap([enemy.x, enemy.y, ew, eh], [tx, ty, tw, th]) {
                                    if enemy.vy > 0.0 {
                                        enemy.y = ty - eh;
                                        enemy.on_ground = true;
                                    } else if enemy.vy < 0.0 {
                                        enemy.y = ty + th;
                                    }
                                    enemy.vy = 0.0;
                                }
                            }
                        }
                    }

                    // An egg that has touched down hatches (`goomba.lua:250-272`): it
                    // becomes an ordinary walking spiny and sets off *towards* the
                    // player, which is why a spiny always greets you head-on rather
                    // than wandering off.
                    if enemy.enemy_type == EnemyType::SpikeyFall && enemy.on_ground {
                        enemy.enemy_type = EnemyType::Spikey;
                        enemy.facing_right = enemy.x < self.player.x;
                        enemy.vx = if enemy.facing_right {
                            ENEMY_SPEED
                        } else {
                            -ENEMY_SPEED
                        };
                    }

                    // Ledge detection (only for walking enemies on ground, not shells)
                    // Not for a hammer bro: his one-block patrol decides where he
                    // goes, and this would fight it every time he shuffles to the
                    // edge of the ledge he is standing on.
                    if enemy.state == EnemyState::Walking
                        && enemy.on_ground
                        && enemy.enemy_type != EnemyType::HammerBro
                    {
                        let foot_col = if enemy.vx > 0.0 {
                            ((enemy.x + ew) / TILE_SIZE).floor() as i32
                        } else {
                            (enemy.x / TILE_SIZE).floor() as i32
                        };
                        let ground_row = ((enemy.y + eh) / TILE_SIZE).floor() as i32;
                        if !is_solid(get_tile(&self.level, foot_col, ground_row)) {
                            enemy.vx = -enemy.vx;
                            enemy.facing_right = !enemy.facing_right;
                            // Undo horizontal movement to prevent walking off
                            enemy.x = old_x;
                        }
                    }
                }
                EnemyState::Dead => {
                    enemy.death_timer -= dt;
                    if enemy.flipped_death {
                        // `shotgravity`, not the world's — a shot enemy hangs a beat
                        // longer at the top of its arc (`variables.lua:164`).
                        enemy.vy += SHOT_GRAVITY * dt;
                        enemy.y += enemy.vy * dt;
                        enemy.x += enemy.vx * dt;
                    }
                }
                EnemyState::Shell => {
                    // Gravity for stationary shell too
                    enemy.vy += GRAVITY * dt;
                    if enemy.vy > MAX_Y_SPEED {
                        enemy.vy = MAX_Y_SPEED;
                    }
                    enemy.y += enemy.vy * dt;
                    let left_col = (enemy.x / TILE_SIZE).floor() as i32;
                    let right_col = ((enemy.x + ew - 0.01) / TILE_SIZE).floor() as i32;
                    let top_row = (enemy.y / TILE_SIZE).floor() as i32;
                    let bottom_row = ((enemy.y + PLAYER_SMALL_H - 0.01) / TILE_SIZE).floor() as i32;
                    for row in top_row..=bottom_row {
                        for col in left_col..=right_col {
                            if blocks_movement(&self.level, col, row) {
                                let (tx, ty, tw, th) = tile_rect(col, row);
                                if aabb_overlap(
                                    [enemy.x, enemy.y, ew, PLAYER_SMALL_H],
                                    [tx, ty, tw, th],
                                ) {
                                    if enemy.vy > 0.0 {
                                        enemy.y = ty - PLAYER_SMALL_H;
                                        enemy.on_ground = true;
                                    }
                                    enemy.vy = 0.0;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Player-enemy interaction
        let mut player_bounce = false;
        for enemy in &mut self.enemies {
            if enemy.state == EnemyState::Dead || enemy.enemy_type.harmless() {
                continue;
            }

            // The same height the movement code used. This was a second, subtly
            // different copy that only recognised a green koopa as tall and gave
            // every plant a full block — so a red koopa and a beetle were fought
            // with a shorter box than they were drawn at, and a plant's box did not
            // match where it actually is.
            let eh = enemy_height(enemy.enemy_type, enemy.state);

            if !aabb_overlap(
                [
                    self.player.x,
                    self.player.y,
                    self.player.width,
                    self.player.height,
                ],
                [enemy.x, enemy.y, PLAYER_SMALL_W, eh],
            ) {
                continue;
            }

            // Check if stomping (player feet above enemy top half)
            let player_feet = self.player.bottom();
            let enemy_mid = enemy.y + eh / 2.0;

            if self.player.vy > 0.0 && player_feet < enemy_mid + 8.0 && enemy.enemy_type.stompable()
            {
                // Stomp!
                match enemy.state {
                    EnemyState::Walking if enemy.enemy_type == EnemyType::KoopaFlying => {
                        // Stomping a winged koopa knocks the wings off and leaves
                        // a walking koopa, rather than killing it outright.
                        enemy.enemy_type = EnemyType::Koopa;
                        enemy.vy = -KOOPA_FLYING_GRAVITY * dt_hint();
                        enemy.vx = if enemy.facing_right {
                            ENEMY_SPEED
                        } else {
                            -ENEMY_SPEED
                        };
                    }
                    EnemyState::Walking if enemy.enemy_type == EnemyType::Lakito => {
                        // Stomping lakitu doesn't finish him, it evicts him: he drops
                        // out of his cloud upside-down and is back at the right edge
                        // of the screen 16 seconds later (`lakito.lua:45-56`,
                        // `:130-133`). Straight down, because `stomp` zeroes the
                        // upward kick `shotted` had just given him.
                        enemy.state = EnemyState::Dead;
                        enemy.flipped_death = true;
                        enemy.death_timer = LAKITO_RESPAWN;
                        enemy.vx = 0.0;
                        enemy.vy = 0.0;
                    }
                    EnemyState::Walking => {
                        if enemy.enemy_type.is_koopa_like() {
                            // Koopas, red koopas and beetles all retreat into a
                            // shell rather than dying outright.
                            enemy.state = EnemyState::Shell;
                            enemy.vx = 0.0;
                        } else {
                            enemy.state = EnemyState::Dead;
                            enemy.death_timer = ENEMY_DEATH_TIME;
                        }
                    }
                    EnemyState::Shell => {
                        // Kick shell
                        enemy.state = EnemyState::ShellMoving;
                        enemy.vx = if self.player.center_x() < enemy.x + PLAYER_SMALL_W / 2.0 {
                            SHELL_SPEED
                        } else {
                            -SHELL_SPEED
                        };
                    }
                    _ => {}
                }

                let combo_score = COMBO_SCORES[self.combo_index.min(COMBO_SCORES.len() - 1)];
                self.score += combo_score;
                self.combo_index += 1;
                self.combo_active = true;
                player_bounce = true;
                ctx.audio.play("stomp");
            } else if self.star_timer > 0.0 && !enemy.enemy_type.indestructible() {
                // Star invincibility: kill enemy on contact (flip + fly off).
                // A star does not clear a firebar or a lava geyser — those are
                // level geometry with a hitbox, not enemies.
                enemy.shotted();
                let combo_score = COMBO_SCORES[self.combo_index.min(COMBO_SCORES.len() - 1)];
                self.score += combo_score;
                self.combo_index += 1;
                self.combo_active = true;
                ctx.audio.play("stomp");
            } else if self.player.invincible_timer <= 0.0 && enemy.state != EnemyState::Shell {
                // Hit by enemy from side
                if self.player.is_fire {
                    self.player.is_fire = false;
                    self.player.invincible_timer = 2.0;
                    ctx.audio.play("shrink");
                } else if self.player.is_big {
                    self.player.set_size(false);
                    self.player.invincible_timer = 2.0;
                    ctx.audio.play("shrink");
                } else {
                    self.die(ctx);
                    return;
                }
            }
        }

        if player_bounce {
            self.player.vy = STOMP_BOUNCE;
            self.player.on_ground = false;
        }

        self.rng = rng;
        self.enemies.append(&mut thrown);
        for (x, y, dir) in fired {
            self.enemies.push(Enemy::bullet_bill(x, y, dir));
            ctx.audio.play("bulletbill");
        }
        self.egg_may_hit_its_thrower();
        self.portaled_bills_shoot_things_down();
        self.respawn_shot_lakitos();

        // Remove dead enemies after timer, or enemies that fell off the map
        self.enemies.retain(|e| {
            if e.state == EnemyState::Dead && e.death_timer <= 0.0 {
                return false;
            }
            // A live bill expires on a clock, because nothing else can stop one: it
            // ignores terrain, so without this it would fly to the end of the world
            // and sit off the right edge for the rest of the level.
            if e.enemy_type == EnemyType::BulletBill
                && e.state == EnemyState::Walking
                && e.cycle_timer >= BULLET_BILL_LIFETIME
            {
                return false;
            }
            if e.y > (self.level.height as f32) * TILE_SIZE + 100.0 {
                // A shot lakitu who has not yet been retired is *waiting*, not gone:
                // he has to survive falling out of the world to make it back for his
                // respawn. Once retired the timer runs down and this catches him.
                if e.enemy_type == EnemyType::Lakito && e.state == EnemyState::Dead && !retired {
                    return true;
                }
                return false;
            }
            // Scrolled well off the left edge. It does not come back: the
            // spawn record is never cleared, exactly as `enemiesspawned` isn't.
            if e.x < cam_x - 200.0 {
                return false;
            }
            true
        });
    }

    /// An egg can hit a lakitu, but only one it has already fallen well past.
    ///
    /// `self.mask[21] = true` (`goomba.lua:54`) — and Mari0's `mask` is an
    /// **exclusion** table, not an inclusion one: the physics collides when
    /// `mask[category] ~= true` (`physics.lua:113`). So a fresh egg *ignores* lakitu,
    /// and only starts colliding with him once it has dropped
    /// [`SPIKEY_IGNORES_LAKITO_WITHIN`] blocks below where it was released
    /// (`goomba.lua:132`).
    ///
    /// Read the mask the other way round — as "collides with lakitu until it has
    /// fallen two blocks" — and you invert the rule into something that fires often:
    /// the egg is thrown *upward* with no sideways speed, so it comes straight back
    /// down through lakitu's altitude, and he only clears the spot because he is
    /// moving. Catch him mid-turnaround and he'd shoot himself down. That is exactly
    /// what the exclusion is there to prevent, which is why it exists at all. What
    /// remains is a guard that essentially never fires, since lakitu holds one
    /// altitude and the egg has to get two blocks *below* its release point to
    /// qualify — but it is cheap and it is what the original does.
    fn egg_may_hit_its_thrower(&mut self) {
        let eggs: Vec<[f32; 4]> = self
            .enemies
            .iter()
            .filter(|e| {
                e.enemy_type == EnemyType::SpikeyFall
                    && e.state == EnemyState::Walking
                    && e.y > e.spawn_y + SPIKEY_IGNORES_LAKITO_WITHIN
            })
            .map(|e| [e.x, e.y, PLAYER_SMALL_W, PLAYER_SMALL_H])
            .collect();
        if eggs.is_empty() {
            return;
        }
        let mut struck = Vec::new();
        for (i, enemy) in self.enemies.iter_mut().enumerate() {
            if enemy.enemy_type != EnemyType::Lakito || enemy.state != EnemyState::Walking {
                continue;
            }
            let box_ = [enemy.x, enemy.y, PLAYER_SMALL_W, PLAYER_SMALL_H];
            if eggs.iter().any(|egg| aabb_overlap(box_, *egg)) {
                enemy.shotted();
                struck.push(i);
            }
        }
        for i in struck {
            let (x, y) = (self.enemies[i].x, self.enemies[i].y);
            self.score += LAKITO_SCORE;
            self.score_popups.push(crate::effects::ScorePopup {
                x,
                y,
                value: LAKITO_SCORE,
                timer: 0.0,
            });
        }
    }

    /// A bullet bill that has been through a portal mows down goombas and koopas.
    ///
    /// `bulletbill:portaled()` sets `killstuff`, and only then does its collision
    /// handler shoot anything down (`bulletbill.lua:181-194`). Before that a bill and a
    /// goomba pass through each other. So the bill is harmless to other enemies as
    /// fired and becomes a weapon once you route it through a portal — which is the
    /// one place Mari0's own mechanics reach into Super Mario Bros' bestiary.
    ///
    /// The kill is always `shotted("left")` regardless of which way the bill is
    /// travelling; that asymmetry is the original's, not a slip here.
    fn portaled_bills_shoot_things_down(&mut self) {
        let bills: Vec<[f32; 4]> = self
            .enemies
            .iter()
            .filter(|e| {
                e.enemy_type == EnemyType::BulletBill
                    && e.portaled
                    && e.state == EnemyState::Walking
            })
            .map(|e| [e.x, e.y, PLAYER_SMALL_W, PLAYER_SMALL_H])
            .collect();
        if bills.is_empty() {
            return;
        }
        for enemy in &mut self.enemies {
            let victim = enemy.enemy_type == EnemyType::Goomba
                || enemy.enemy_type.is_koopa_like()
                || matches!(enemy.enemy_type, EnemyType::Spikey | EnemyType::SpikeyFall);
            if !victim || enemy.state == EnemyState::Dead {
                continue;
            }
            let box_ = [
                enemy.x,
                enemy.y,
                PLAYER_SMALL_W,
                enemy_height(enemy.enemy_type, enemy.state),
            ];
            if bills.iter().any(|b| aabb_overlap(box_, *b)) {
                enemy.facing_right = false;
                enemy.shotted();
            }
        }
    }

    /// Bring a shot lakitu back at the right edge of the screen.
    ///
    /// He re-enters at the altitude he first appeared at, not where he fell from
    /// (`lakito.lua:48`), so a level's lakitu always flies the same lane. Retired
    /// lakitus are left to expire — being past `lakitoend` is permanent.
    fn respawn_shot_lakitos(&mut self) {
        if self.lakito_retired {
            return;
        }
        let (cam_x, vw) = (self.camera.x, self.vw);
        for enemy in &mut self.enemies {
            if enemy.enemy_type != EnemyType::Lakito
                || enemy.state != EnemyState::Dead
                || enemy.death_timer > 0.0
            {
                continue;
            }
            enemy.state = EnemyState::Walking;
            enemy.flipped_death = false;
            enemy.x = cam_x + vw;
            enemy.y = enemy.spawn_y;
            enemy.vx = 0.0;
            enemy.vy = 0.0;
            enemy.facing_right = false;
            enemy.cycle_timer = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level;
    use crate::world::load_level;

    /// Build a cell → indices map from a list of `(col, row)` placements.
    fn by_cell(cells: &[(i32, i32)]) -> HashMap<(i32, i32), Vec<usize>> {
        let mut map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, c) in cells.iter().enumerate() {
            map.entry(*c).or_default().push(i);
        }
        map
    }

    #[test]
    fn a_lone_enemy_spawns_with_its_own_column() {
        let cells = [(10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert!(column_spawn_indices(&map, &mut spawned, 9).is_empty());
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
    }

    /// The cluster rule: reaching one of a group drags in the neighbours within
    /// two columns, so a row of goombas arrives as a unit.
    #[test]
    fn reaching_one_of_a_group_pulls_in_neighbours_within_two_columns() {
        // 10, 11, 12 on the same row; column 10 is revealed first.
        let cells = [(10, 5), (11, 5), (12, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1, 2], "all three should arrive together");
        assert!(spawned.iter().all(|s| *s));
    }

    /// The chain keeps going: each newly spawned cell spreads in turn, so a long
    /// unbroken run comes in all at once even though each hop is only two columns.
    #[test]
    fn the_cluster_chains_along_a_long_run() {
        let cells: Vec<(i32, i32)> = (10..30).map(|c| (c, 5)).collect();
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let got = column_spawn_indices(&map, &mut spawned, 10);
        assert_eq!(got.len(), 20, "the whole run should arrive at once");
    }

    /// A gap wider than two columns stops the chain — that's what makes the rule
    /// "this group" rather than "the whole level".
    #[test]
    fn a_gap_of_three_columns_breaks_the_chain() {
        // 10, 11 … then nothing until 15.
        let cells = [(10, 5), (11, 5), (15, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "15 is four columns past 11, out of reach");
        assert!(!spawned[2]);
    }

    /// Different rows are independent: the recursion only walks sideways.
    #[test]
    fn the_cluster_does_not_spread_vertically() {
        let cells = [(10, 5), (11, 9)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
        assert!(!spawned[1], "a different row is a different group");
    }

    /// Nothing spawns twice. This is what stops a killed enemy from returning
    /// when the camera revisits its column.
    #[test]
    fn a_column_never_spawns_the_same_enemy_twice() {
        let cells = [(10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        assert_eq!(column_spawn_indices(&map, &mut spawned, 10), vec![0]);
        assert!(
            column_spawn_indices(&map, &mut spawned, 10).is_empty(),
            "second sweep of the same column yields nothing"
        );
    }

    /// A firebar puts one spawn per segment on the same pivot cell, so a cell can
    /// legitimately hold several.
    #[test]
    fn one_cell_can_hold_several_spawns() {
        let cells = [(10, 5), (10, 5), (10, 5)];
        let map = by_cell(&cells);
        let mut spawned = vec![false; cells.len()];
        let mut got = column_spawn_indices(&map, &mut spawned, 10);
        got.sort_unstable();
        assert_eq!(got, vec![0, 1, 2]);
    }

    /// The portal exemption table, which has two independent reasons in it.
    ///
    /// Worth pinning because the two reasons look the same from the outside but
    /// aren't: plants/firebars/geysers are `static = true` fixtures, while
    /// cheep-cheeps move perfectly well and opt out with `portalable = false`. Anyone
    /// "simplifying" this to `!is_scripted()` would quietly make flying koopas
    /// non-portable, since they're scripted but do travel.
    #[test]
    fn the_portal_exemption_table_has_two_distinct_reasons() {
        for kind in [
            EnemyType::Goomba,
            EnemyType::Koopa,
            EnemyType::KoopaRed,
            EnemyType::Beetle,
            EnemyType::KoopaFlying,
        ] {
            assert!(kind.portalable(), "{kind:?} should travel through portals");
        }
        for kind in [
            // `static = true`: fixtures that never move.
            EnemyType::Plant,
            EnemyType::Firebar,
            EnemyType::UpFire,
            // `portalable = false`: moves, but opts out.
            EnemyType::CheepRed,
            EnemyType::CheepWhite,
            EnemyType::Lakito,
        ] {
            assert!(!kind.portalable(), "{kind:?} should not travel");
        }
        assert!(
            EnemyType::KoopaFlying.is_scripted() && EnemyType::KoopaFlying.portalable(),
            "scripted and portalable are independent; a flying koopa is both"
        );
    }

    /// A spiny hurts from above, which is the one thing that makes it not a goomba.
    ///
    /// The original expresses this as `b.t ~= "goomba"` rather than a per-type flag
    /// (`mario.lua:1778`), so it is easy to port a spiny as "a goomba with a different
    /// sprite" and quietly hand the player a free stomp.
    #[test]
    fn a_spiny_cannot_be_stomped_but_a_goomba_can() {
        assert!(EnemyType::Goomba.stompable());
        assert!(!EnemyType::Spikey.stompable());
        assert!(!EnemyType::SpikeyFall.stompable());
        // Lakitu, by contrast, is on the stomp list (`mario.lua:1761`).
        assert!(EnemyType::Lakito.stompable());
    }

    /// The egg falls at 30 blocks/s², nothing else does.
    #[test]
    fn only_a_thrown_spiny_egg_falls_slower_than_the_world() {
        assert_eq!(EnemyType::SpikeyFall.gravity(), SPIKEY_FALL_GRAVITY);
        const { assert!(SPIKEY_FALL_GRAVITY < GRAVITY) };
        for kind in [
            EnemyType::Goomba,
            EnemyType::Spikey,
            EnemyType::Koopa,
            EnemyType::Lakito,
        ] {
            assert_eq!(kind.gravity(), GRAVITY, "{kind:?} should fall normally");
        }
    }

    /// Lakitu opts out of portals explicitly, like a cheep-cheep — he is not a
    /// fixture, he simply refuses.
    #[test]
    fn lakito_refuses_portals_without_being_a_fixture() {
        assert!(!EnemyType::Lakito.portalable());
        assert!(EnemyType::Lakito.is_scripted());
        // The spinies he throws have no such exemption: they are goombas.
        assert!(EnemyType::Spikey.portalable());
        assert!(EnemyType::SpikeyFall.portalable());
    }

    /// "Dead" means something different for lakitu: a 16-second absence, not removal.
    #[test]
    fn a_downed_lakito_is_scheduled_to_return() {
        let mut lakito = Enemy::spiny_egg(0.0, 0.0);
        lakito.enemy_type = EnemyType::Lakito;
        lakito.shotted();
        assert_eq!(lakito.state, EnemyState::Dead);
        assert_eq!(lakito.death_timer, LAKITO_RESPAWN);

        let mut goomba = Enemy::spiny_egg(0.0, 0.0);
        goomba.enemy_type = EnemyType::Goomba;
        goomba.shotted();
        assert_eq!(goomba.death_timer, SHOT_DEATH_TIME);
        const { assert!(SHOT_DEATH_TIME < LAKITO_RESPAWN) };
    }

    /// The egg is tossed upward, which is why it can come back down onto its thrower.
    #[test]
    fn a_spiny_egg_leaves_lakitos_hands_going_up() {
        let egg = Enemy::spiny_egg(100.0, 200.0);
        assert_eq!(egg.enemy_type, EnemyType::SpikeyFall);
        assert!(egg.vy < 0.0, "thrown up, not dropped");
        assert_eq!(egg.vx, 0.0, "no sideways speed of its own");
        assert_eq!(
            egg.spawn_y, 200.0,
            "the release height is what the lakitu-hit window is measured from"
        );
    }

    /// Sweeping every column of a real level must claim every spawn exactly once.
    ///
    /// The invariant that matters for play: lazy spawning must not *lose* enemies.
    #[test]
    fn sweeping_all_columns_claims_every_spawn_exactly_once() {
        for (pack, name, _) in level::LEVELS {
            let level = load_level(pack, name);
            let mut spawned = vec![false; level.enemy_spawns.len()];
            let mut total = 0;
            // Well past both ends, since the cluster rule can reach outside the
            // level's own column range.
            for col in -4..(level.width as i32 + 4) {
                total += column_spawn_indices(&level.spawns_by_cell, &mut spawned, col).len();
            }
            assert_eq!(
                total,
                level.enemy_spawns.len(),
                "{pack}/{name}: swept {total} of {} spawns",
                level.enemy_spawns.len()
            );
            assert!(
                spawned.iter().all(|s| *s),
                "{pack}/{name}: some spawns were never claimed"
            );
        }
    }

    /// A fully grown plant's feet land exactly on the pipe's rim.
    ///
    /// This is the check that pins the whole placement, because it is exact rather
    /// than approximate: the plant's cell is always one row above a pipe's top tile,
    /// so the rim is at `(row + 1) * TILE_SIZE`, and the original's numbers put the
    /// extended hitbox's bottom edge precisely there. Get the 1-based row shift or
    /// the `9/16` wrong and this misses by whole tiles — which is exactly how the
    /// plant ended up hovering a block and a half above its pipe.
    #[test]
    fn a_fully_grown_plant_stands_on_the_pipes_rim() {
        for row in [4, 9, 10, 12] {
            let cell_top = row as f32 * TILE_SIZE;
            let rim = cell_top + TILE_SIZE;
            let extended_top = plant_rest_y(cell_top) - PLANT_MOVE_DIST;
            let extended_bottom = extended_top + PLANT_HEIGHT;
            assert!(
                (extended_bottom - rim).abs() < 0.01,
                "row {row}: extended plant bottom {extended_bottom} should sit on the rim {rim}"
            );
        }
    }

    /// A retracted plant's sprite starts at the rim, so the clip window hides it all.
    ///
    /// The other exact landing. It is what makes the two-block scissor sufficient:
    /// the window's bottom edge *is* the rim, so "retracted" and "invisible" are the
    /// same condition rather than two things that have to be kept in step.
    #[test]
    fn a_retracted_plants_sprite_begins_exactly_at_the_rim() {
        let cell_top = 9.0 * TILE_SIZE;
        let rim = cell_top + TILE_SIZE;
        let sprite_top = plant_rest_y(cell_top) - PLANT_SPRITE_RISE;
        assert!(
            (sprite_top - rim).abs() < 0.01,
            "retracted sprite top {sprite_top} should be the rim {rim}"
        );
        // Extended, the sprite ends on the rim too and sits wholly inside the
        // window, so the plant grows *out of* the pipe rather than through its wall.
        let window_top = cell_top - TILE_SIZE;
        let extended_sprite_top = plant_rest_y(cell_top) - PLANT_MOVE_DIST - PLANT_SPRITE_RISE;
        assert!(
            (extended_sprite_top + PLANT_SPRITE_H - rim).abs() < 0.01,
            "extended sprite bottom {} should be the rim {rim}",
            extended_sprite_top + PLANT_SPRITE_H
        );
        assert!(
            extended_sprite_top >= window_top,
            "extended sprite top {extended_sprite_top} escapes the window top {window_top}"
        );
    }

    /// A plant is shorter than a block, and one code path used to think otherwise.
    #[test]
    fn a_plant_is_shorter_than_every_other_enemy() {
        assert_eq!(
            enemy_height(EnemyType::Plant, EnemyState::Walking),
            PLANT_HEIGHT
        );
        const { assert!(PLANT_HEIGHT < PLAYER_SMALL_H) };
        // All three koopa-likes are tall while walking, not just the green one.
        for kind in [EnemyType::Koopa, EnemyType::KoopaRed, EnemyType::Beetle] {
            assert_eq!(enemy_height(kind, EnemyState::Walking), 48.0, "{kind:?}");
            assert_eq!(
                enemy_height(kind, EnemyState::Shell),
                PLAYER_SMALL_H,
                "{kind:?} in a shell"
            );
        }
    }

    /// Every plant hangs one row above something solid, nearly always a pipe's rim.
    ///
    /// The placement maths only needs "solid below", since the drop is measured from
    /// the plant's own cell — but the tile ids are worth pinning too. 14 and 16 are
    /// the **left** halves of the two spritesets' pipe-top pairs, and that all 99
    /// pipe-mounted plants land on a left half is the evidence for the half-block
    /// shift that centres a plant on its two-block-wide pipe. The other three are in
    /// 8-4, growing out of a castle wall instead.
    #[test]
    fn every_plant_hangs_over_something_solid() {
        let mut on_pipe = 0;
        let mut elsewhere = 0;
        for (pack, name, _) in level::LEVELS {
            let parsed = level::load(pack, name)
                .expect("shipped level")
                .expect("parses");
            for spawn in &parsed.markers.enemies {
                if spawn.kind != level::EntityKind::Plant {
                    continue;
                }
                let below = parsed.tile(spawn.x as i32, spawn.y as i32 + 1);
                assert!(
                    level::tiles::props(below).collision(),
                    "{pack}/{name}: plant at ({}, {}) has nothing to hide in — tile {below}",
                    spawn.x,
                    spawn.y
                );
                if matches!(below, 14 | 16) {
                    on_pipe += 1;
                } else {
                    elsewhere += 1;
                }
            }
        }
        assert_eq!(on_pipe, 99, "plants mounted on a pipe's left rim tile");
        assert_eq!(elsewhere, 3, "8-4's three wall-mounted plants");
    }

    /// A cannon is scenery with a timer, not an enemy.
    ///
    /// It is in the enemy list for the lazy reveal and the off-screen cull, and every
    /// interaction pass has to skip it. If it didn't, the player would die to the
    /// cannon he is standing on — the exact spot the original makes safe by refusing
    /// to fire within three blocks.
    #[test]
    fn the_cannon_is_scenery_and_the_bill_is_not() {
        assert!(EnemyType::BulletBillCannon.harmless());
        assert!(EnemyType::BulletBillCannon.indestructible());
        assert!(!EnemyType::BulletBillCannon.portalable());
        assert!(!EnemyType::BulletBill.harmless());
        assert!(EnemyType::BulletBill.stompable());
        assert!(EnemyType::BulletBill.portalable());
        // But exempt from the catch-all containment test, so a mouth it flies past
        // doesn't swallow it.
        assert!(EnemyType::BulletBill.exempt_from_containment());
        assert!(!EnemyType::Goomba.exempt_from_containment());
    }

    /// A bill flies flat and fast, and only a clock stops it.
    #[test]
    fn a_bill_flies_level_at_full_speed() {
        let right = Enemy::bullet_bill(0.0, 0.0, 1.0);
        assert_eq!(right.vx, BULLET_BILL_SPEED);
        assert_eq!(right.vy, 0.0);
        assert!(right.facing_right);
        let left = Enemy::bullet_bill(0.0, 0.0, -1.0);
        assert_eq!(left.vx, -BULLET_BILL_SPEED);
        assert!(!left.facing_right);
        assert!(!left.portaled, "only a portal trip sets that");
    }

    /// The RNG has to be a *reproducible* stand-in for `math.random`.
    ///
    /// Two runs of the same level must play out identically or every VDP probe and the
    /// autopilot become flaky. Also checks the quantisation: the original draws
    /// `math.random(min*10, max*10)/10`, so the values land on tenths.
    #[test]
    fn the_random_stand_in_repeats_and_stays_in_range() {
        let draw = |seed| {
            let mut rng = Rng::new(seed);
            (0..64)
                .map(|_| rng.tenths(BULLET_BILL_TIME_MIN, BULLET_BILL_TIME_MAX))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(7), draw(7), "same seed must replay exactly");
        assert_ne!(draw(7), draw(8), "different seeds should diverge");
        for v in draw(7) {
            assert!(
                (BULLET_BILL_TIME_MIN..=BULLET_BILL_TIME_MAX).contains(&v),
                "{v} outside 1.0..=4.5"
            );
            assert!(
                ((v * 10.0) - (v * 10.0).round()).abs() < 1e-4,
                "{v} not a tenth"
            );
        }

        let mut rng = Rng::new(3);
        let rows: Vec<i32> = (0..200)
            .map(|_| rng.range(BULLET_BILL_ZONE_ROWS.0, BULLET_BILL_ZONE_ROWS.1))
            .collect();
        assert!(rows.iter().all(|r| (3..=11).contains(r)));
        assert!(
            rows.contains(&3) && rows.contains(&11),
            "both ends of the row range should come up"
        );
    }

    /// The two bullet-bill sources appear in different levels, and 6-3 has no end.
    ///
    /// Worth pinning because the asymmetry looks like missing data: 5-3 fences off a
    /// stretch with both markers, while 6-3 opens the tap and never closes it.
    #[test]
    fn the_cannon_levels_and_the_zone_levels_are_different_levels() {
        let mut cannons = Vec::new();
        let mut zones = Vec::new();
        for (pack, name, _) in level::LEVELS {
            let parsed = level::load(pack, name)
                .expect("shipped level")
                .expect("parses");
            if parsed
                .markers
                .enemies
                .iter()
                .any(|s| s.kind == level::EntityKind::BulletBill)
            {
                cannons.push(name);
            }
            if parsed.markers.bullet_bill_start.is_some() {
                zones.push((name, parsed.markers.bullet_bill_end.is_some()));
            }
        }
        assert_eq!(cannons, ["5-1", "5-2", "7-1", "8-2", "8-3"]);
        assert_eq!(zones, [("5-3", true), ("6-3", false)]);
    }

    /// A hammer bro shuffles, floats and throws — three constants apart from everyone.
    #[test]
    fn a_hammer_bro_is_slower_and_lighter_than_everything_else() {
        assert_eq!(EnemyType::HammerBro.walk_speed(), HAMMERBRO_SPEED);
        const { assert!(HAMMERBRO_SPEED < ENEMY_SPEED) };
        assert_eq!(EnemyType::HammerBro.gravity(), HAMMERBRO_GRAVITY);
        const { assert!(HAMMERBRO_GRAVITY < GRAVITY) };
        // The most valuable thing in Super Mario Bros short of Bowser himself.
        assert_eq!(EnemyType::HammerBro.fire_points(), 1000);
        assert!(EnemyType::HammerBro.stompable());
    }

    /// A hammer can't be stopped by anything: not a stomp, not a fireball, not a wall.
    #[test]
    fn a_hammer_is_a_hazard_rather_than_a_creature() {
        assert!(!EnemyType::Hammer.stompable());
        assert!(EnemyType::Hammer.indestructible());
        assert!(EnemyType::Hammer.is_scripted(), "it flies its own arc");
        assert!(EnemyType::Hammer.exempt_from_containment());
        let h = Enemy::hammer(100.0, 200.0, 1.0);
        assert!(h.vy < 0.0, "thrown up and forward");
        assert_eq!(h.vx, HAMMER_SPEED);
        assert_eq!(
            h.y,
            200.0 - TILE_SIZE,
            "released a block above the bro, which is what puts the arc over your head"
        );
        assert!(
            h.ignore_tiles,
            "a hammer never collides with terrain, so it starts and stays exempt"
        );
    }

    /// Both kinds of hop start by going *up*, and both switch tile collision off.
    ///
    /// The downward hop is the counter-intuitive one: `speedy = -6` is still a jump.
    /// He leaves the floor he is on by rising off it, then falls through it because
    /// collision is off, and lands two blocks lower.
    #[test]
    fn a_downward_hop_still_starts_upward() {
        // Both are positive kicks; the down-hop is just the gentler of the two.
        const { assert!(HAMMERBRO_JUMP_FORCE_DOWN > 0.0) };
        const { assert!(HAMMERBRO_JUMP_FORCE_DOWN < HAMMERBRO_JUMP_FORCE) };
        // And the drop-through distance is what picks the next floor rather than the
        // one after it.
        assert_eq!(HAMMERBRO_DROP_THROUGH, 2.0 * TILE_SIZE);
    }

    /// Bowser is the only thing in the game with hit points, and the only thing a
    /// single fireball doesn't finish.
    #[test]
    fn only_bowser_has_hit_points() {
        let sp = crate::world::EnemySpawnPoint {
            enemy_type: EnemyType::Bowser,
            x: 100.0 * TILE_SIZE,
            y: 9.0 * TILE_SIZE,
            facing_right: false,
            segment: 1,
        };
        let b = Enemy::from_spawn(&sp);
        assert_eq!(b.hp, BOWSER_HEALTH);
        assert!(!b.backing_off);
        // His first target is on the near side of his pace (`newtargetx("right")`).
        assert!(b.target_x < b.x, "he starts by pacing towards the player");
        // Everyone else has none, which is what makes the "one hit" path the default.
        for kind in [
            EnemyType::Goomba,
            EnemyType::HammerBro,
            EnemyType::Squid,
            EnemyType::BulletBill,
        ] {
            let other = Enemy::from_spawn(&crate::world::EnemySpawnPoint {
                enemy_type: kind,
                ..sp
            });
            assert_eq!(other.hp, 0, "{kind:?}");
        }
    }

    /// He retreats faster than he advances, which is the whole shape of the fight.
    #[test]
    fn bowser_runs_away_faster_than_he_comes_at_you() {
        const { assert!(BOWSER_SPEED_BACKWARDS > 2.0 * BOWSER_SPEED_FORWARDS) };
        // And he is lighter than anything else that falls.
        assert_eq!(EnemyType::Bowser.gravity(), BOWSER_GRAVITY);
        const { assert!(BOWSER_GRAVITY < GRAVITY / 4.0) };
        // Neither he nor his breath can be landed on.
        assert!(!EnemyType::Bowser.stompable());
        assert!(!EnemyType::Fire.stompable());
        assert!(EnemyType::Fire.indestructible());
        assert_eq!(EnemyType::Bowser.fire_points(), BOWSER_SCORE);
    }

    /// He is the biggest hitbox in the game — bigger than big Mario.
    #[test]
    fn bowser_is_the_biggest_thing_on_screen() {
        assert_eq!(
            enemy_height(EnemyType::Bowser, EnemyState::Walking),
            BOWSER_H
        );
        const { assert!(BOWSER_H > PLAYER_BIG_H / 2.0) };
        const { assert!(BOWSER_W > PLAYER_SMALL_W) };
        // His breath is wide and flat, the opposite shape.
        const { assert!(FIRE_W > FIRE_H) };
    }

    /// A squid can't be stomped, and a flying fish can.
    ///
    /// Two enemies that arrive together and differ on the one thing that matters when
    /// you meet one: `mario.lua:1778` lists `squid` under KILL, while
    /// `flyingfish:stomp` exists (`flyingfish.lua:139`).
    #[test]
    fn the_squid_is_the_one_you_cannot_land_on() {
        assert!(!EnemyType::Squid.stompable());
        assert!(EnemyType::FlyingFish.stompable());
        // Both swim their own path through terrain.
        assert!(EnemyType::Squid.is_scripted() && EnemyType::FlyingFish.is_scripted());
        assert_eq!(EnemyType::Squid.fire_points(), 200);
        assert_eq!(EnemyType::FlyingFish.fire_points(), 200);
    }

    /// A fish never leaves the water with zero sideways speed.
    ///
    /// `if self.speedx == 0 then self.speedx = 1` (`flyingfish.lua:38`) — without it a
    /// standing player gets fish that go straight up and straight back down through the
    /// same spot, which is both easy and wrong.
    #[test]
    fn a_flying_fish_always_drifts() {
        let still = Enemy::flying_fish(0.0, 0.0, 0.0);
        assert_ne!(still.vx, 0.0);
        assert!(still.vy < 0.0, "it leaps up");
        // Otherwise the speed passed in is kept verbatim, sign and all.
        assert_eq!(Enemy::flying_fish(0.0, 0.0, -100.0).vx, -100.0);
        assert!(!Enemy::flying_fish(0.0, 0.0, -100.0).facing_right);
    }

    /// No level places a spiny, and every level with a lakitu says where he stops.
    ///
    /// Both halves of this are why lakitu and the spiny had to be built together. The
    /// entity ids exist (98 and 99) and the editor offers them, but nothing ships one:
    /// a walking spiny is only ever reached by an egg landing, so a port that adds
    /// `spikey` as a spawn point and stops there has added an enemy the player can
    /// never meet. The `lakitoend` half is the other side of the same coin — without
    /// it lakitu would follow the player into the flagpole.
    #[test]
    fn spinies_are_never_placed_and_every_lakito_has_somewhere_to_stop() {
        let mut with_lakito = Vec::new();
        for (pack, name, _) in level::LEVELS {
            let parsed = level::load(pack, name)
                .expect("shipped level")
                .expect("parses");
            for spawn in &parsed.markers.enemies {
                assert_ne!(
                    spawn.kind,
                    level::EntityKind::Spikey,
                    "{pack}/{name} places a spiny; the roster assumed none did"
                );
                assert_ne!(spawn.kind, level::EntityKind::SpikeyHalf, "{pack}/{name}");
                if spawn.kind == level::EntityKind::Lakito {
                    with_lakito.push((name, parsed.markers.lakito_end));
                }
            }
        }
        assert_eq!(
            with_lakito.len(),
            3,
            "expected 4-1, 6-1 and 8-2 to be the only lakitu levels, got {with_lakito:?}"
        );
        for (name, end) in with_lakito {
            assert!(end.is_some(), "{name} has a lakitu but no lakitoend");
        }
    }

    /// 8-1 is the width stress case the lazy spawner exists for.
    #[test]
    fn the_widest_level_holds_its_enemies_back_until_revealed() {
        let level = load_level("smb", "8-1");
        assert!(
            level.width >= 400,
            "8-1 should be ~400 tiles wide, got {}",
            level.width
        );
        let mut spawned = vec![false; level.enemy_spawns.len()];
        // One screen plus a column, exactly what `spawn_revealed_columns` opens with.
        let mut opening = 0;
        for col in 0..=17 {
            opening += column_spawn_indices(&level.spawns_by_cell, &mut spawned, col).len();
        }
        assert!(
            opening < level.enemy_spawns.len(),
            "the opening screen claimed all {} spawns; nothing was left to reveal",
            level.enemy_spawns.len()
        );
    }
}
