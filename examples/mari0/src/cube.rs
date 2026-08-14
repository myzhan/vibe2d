//! Weighted cubes: the thing the lab is actually about.
//!
//! A cube is the only body in the game that the player can *place*, and nearly every
//! lab puzzle is built on that: it holds a floor button down, it blocks a laser, and
//! carrying it in front of you shields you from one. It is also the reason the cube
//! dispenser exists — a cube dropped down a pit has to come back, or the level becomes
//! unfinishable.
//!
//! Three things about it are worth stating up front, because they are not what you'd
//! write from scratch:
//!
//! - **The player walks straight through a cube and shoves it aside.** The cube's own
//!   `leftcollide`/`rightcollide` return false for the player and set a `pushed` flag
//!   (`box.lua:127-143`); the displacement comes from `passivecollide`, which teleports
//!   the cube to the player's edge. There is no push force and no mass.
//! - **A held cube is not attached to Mario's hands but to his aim**: its position is
//!   the player's corner plus 0.3 blocks along the pointing angle (`box.lua:86-88`), so
//!   it swings around him as the mouse moves. That is what makes it a shield.
//! - **A cube lost off the bottom of the map notifies its dispenser** by pushing
//!   `toggle` back up the wire (`box.lua:120-127`), which is what makes a fresh one
//!   appear. The wiring is the cube's, not the dispenser's: the tube carries the link
//!   and it points *at* the cube.

use crate::constants::*;
use crate::game::{GameState, Mari0Game};
use crate::lab::{LabKind, Signal};
use crate::physics::*;
use crate::portal::{PortalBody, portal_carry};

/// A cube is 12/16 of a block on a side (`box.lua:9-10`).
pub(crate) const CUBE_SIZE: f32 = 12.0 / 16.0 * TILE_SIZE;

/// `boxfriction = 20` blocks/s², `boxfrictionair = 8` — a cube slides much further
/// through the air than along the ground.
const CUBE_FRICTION: f32 = 20.0 * TILE_SIZE;
const CUBE_FRICTION_AIR: f32 = 8.0 * TILE_SIZE;

/// How fast a cube's rotation returns to square after a trip through a portal
/// (`portalrotationalignmentspeed = 15`).
const ROTATION_ALIGN_SPEED: f32 = 15.0;

/// How far the cube floats from the player while carried, in blocks (`box.lua:86`).
const CARRY_DISTANCE: f32 = 0.3;

/// The `use` probe: a one-block square, one block from the player's centre along the
/// aim (`userange = 1`, `usesquaresize = 1`, `mario.lua:2793-2797`).
const USE_RANGE: f32 = TILE_SIZE;
const USE_SQUARE: f32 = TILE_SIZE;

/// One weighted cube.
pub(crate) struct Cube {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    /// Accumulated portal rotation, easing back to zero.
    pub(crate) rotation: f32,
    /// In the air. Decides which friction applies, and is set by a fall or a stomp.
    pub(crate) falling: bool,
    /// Shoved by the player this frame, which suspends friction (`box.lua:56-66`).
    pub(crate) pushed: bool,
    pub(crate) held: bool,
    /// The `box` element in the lab graph this cube fills, if any. Destroying the cube
    /// pushes `toggle` from that element, which is how its dispenser hears about it.
    pub(crate) slot: Option<usize>,
    /// The dispenser responsible for this cube, if it came from one.
    pub(crate) dispenser: Option<usize>,
}

impl Cube {
    /// A cube resting in cell `(c, r)`, as a level's `box` entity places it.
    ///
    /// `self.x = cox - 14/16, self.y = coy - 12/16` (`box.lua:5-6`) — not centred in
    /// the cell: 2/16 in from the left and flush with the bottom.
    pub(crate) fn in_cell(cell: (i32, i32)) -> Self {
        Self::at(
            (cell.0 as f32 + 2.0 / 16.0) * TILE_SIZE,
            (cell.1 as f32 + 4.0 / 16.0) * TILE_SIZE,
        )
    }

    pub(crate) fn at(x: f32, y: f32) -> Self {
        Cube {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            rotation: 0.0,
            falling: true,
            pushed: false,
            held: false,
            slot: None,
            dispenser: None,
        }
    }

    pub(crate) fn rect(&self) -> [f32; 4] {
        [self.x, self.y, CUBE_SIZE, CUBE_SIZE]
    }

    fn centre(&self) -> (f32, f32) {
        (self.x + CUBE_SIZE / 2.0, self.y + CUBE_SIZE / 2.0)
    }
}

impl Mari0Game {
    /// Put a cube in every `box` the level places.
    ///
    /// Called from level load, after the lab graph is built, because each cube needs
    /// the index of the element it stands for.
    pub(crate) fn spawn_level_cubes(&mut self) {
        self.cubes.clear();
        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::Box {
                continue;
            }
            let mut cube = Cube::in_cell(self.lab.elements[index].cell);
            cube.slot = Some(index);
            // The tube that links to this box is the dispenser that owns it.
            cube.dispenser = self.lab.consumers[index]
                .iter()
                .copied()
                .find(|&c| self.lab.elements[c].kind == LabKind::BoxTube);
            self.cubes.push(cube);
        }
    }

    /// One frame of cube physics, carrying and destruction.
    pub(crate) fn update_cubes(&mut self, dt: f32) {
        let pair = self.portal_pair();
        let player = [
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
        ];
        let map_bottom = self.level.height as f32 * TILE_SIZE;
        let mut destroyed: Vec<usize> = Vec::new();

        for i in 0..self.cubes.len() {
            // Rotation eases back to square whether held or not.
            let align = ROTATION_ALIGN_SPEED * dt;
            let cube = &mut self.cubes[i];
            cube.rotation = if cube.rotation > 0.0 {
                (cube.rotation - align).max(0.0)
            } else {
                (cube.rotation + align).min(0.0)
            };

            if cube.held {
                // Carried: the cube tracks the *aim*, not the hands. Centred on the aim
                // ray rather than hung off the player's corner as the original does —
                // this port's Mario is a full tile wide where the original's is 12/16,
                // so the corner offset would leave the cube inside him.
                let (ax, ay) = (self.crosshair_angle.cos(), self.crosshair_angle.sin());
                cube.x = self.player.center_x() - CUBE_SIZE / 2.0 + ax * CARRY_DISTANCE * TILE_SIZE;
                cube.y = self.player.center_y() - CUBE_SIZE / 2.0 + ay * CARRY_DISTANCE * TILE_SIZE;
                cube.vx = 0.0;
                cube.vy = 0.0;
                continue;
            }

            // ── Friction, then gravity ──
            let friction = if cube.falling {
                CUBE_FRICTION_AIR
            } else {
                CUBE_FRICTION
            };
            if cube.pushed {
                cube.pushed = false;
            } else if cube.vx > 0.0 {
                cube.vx = (cube.vx - friction * dt).max(0.0);
            } else if cube.vx < 0.0 {
                cube.vx = (cube.vx + friction * dt).min(0.0);
            }
            cube.vy = (cube.vy + GRAVITY * dt).min(MAX_Y_SPEED);

            // ── Portals, before the move, exactly as every other mover does ──
            let body = PortalBody {
                x: cube.x,
                y: cube.y,
                w: CUBE_SIZE,
                h: CUBE_SIZE,
                vx: cube.vx,
                vy: cube.vy,
            };
            if let Some((x, y, vx, vy)) = portal_carry(&self.level, pair.as_ref(), body, dt, true) {
                let cube = &mut self.cubes[i];
                cube.x = x;
                cube.y = y;
                cube.vx = vx;
                cube.vy = vy;
                cube.falling = true;
                continue;
            }

            let cube = &mut self.cubes[i];
            cube.vx = move_and_collide_x(
                &mut cube.x,
                cube.y,
                CUBE_SIZE,
                CUBE_SIZE,
                cube.vx,
                &self.level,
                dt,
                Body::Cube,
            );
            let (vy, on_ground) = move_and_collide_y(
                cube.x,
                &mut cube.y,
                CUBE_SIZE,
                CUBE_SIZE,
                cube.vy,
                &self.level,
                dt,
                Body::Cube,
            );
            cube.vy = vy;
            cube.falling = !on_ground;

            // ── The player shoves it aside ──
            // No force and no mass: he walks through and it is moved to his edge
            // (`box.lua:146-152`). Which side is decided by which side it is already
            // on, so a cube half-overlapped doesn't jump across him.
            if self.state == GameState::Playing && aabb_overlap(player, self.cubes[i].rect()) {
                let cube = &mut self.cubes[i];
                let (cx, _) = cube.centre();
                cube.x = if cx > player[0] + player[2] / 2.0 {
                    player[0] + player[2]
                } else {
                    player[0] - CUBE_SIZE
                };
                cube.pushed = true;
                // Resolve again so he can't shove it into a wall.
                let mut x = cube.x;
                move_and_collide_x(
                    &mut x,
                    cube.y,
                    CUBE_SIZE,
                    CUBE_SIZE,
                    0.0,
                    &self.level,
                    0.0,
                    Body::Cube,
                );
                self.cubes[i].x = x;
            }

            // ── Landing on an enemy stomps it ──
            // A dropped cube is a weapon: `box:floorcollide` stomps and scores 200
            // (`box.lua:155-165`), and the cube bounces into a fall again.
            if !self.cubes[i].falling {
                let rect = self.cubes[i].rect();
                let mut stomped = false;
                for enemy in &mut self.enemies {
                    if enemy.state == crate::enemies::EnemyState::Dead
                        || !enemy.enemy_type.stompable()
                    {
                        continue;
                    }
                    let height = crate::enemies::enemy_height(enemy.enemy_type, enemy.state);
                    if aabb_overlap(rect, [enemy.x, enemy.y, PLAYER_SMALL_W, height]) {
                        enemy.state = crate::enemies::EnemyState::Dead;
                        enemy.death_timer = ENEMY_DEATH_TIME;
                        stomped = true;
                    }
                }
                if stomped {
                    self.score += 200;
                    self.cubes[i].falling = true;
                }
            }

            // Off the bottom of the map. The original's threshold is a block past the
            // 15-row map (`box.lua:112`).
            if self.cubes[i].y > map_bottom + TILE_SIZE {
                destroyed.push(i);
            }
        }

        // Destroying a cube pushes `toggle` from the slot it filled, which is how its
        // dispenser learns it needs to make another.
        for i in destroyed.iter().rev() {
            if let Some(slot) = self.cubes[*i].slot {
                self.lab.signal(slot, Signal::Toggle);
            }
            self.cubes.remove(*i);
        }
    }

    /// Handle the `use` key: drop a carried cube, or pick up / press whatever the aim
    /// is pointing at.
    ///
    /// The original registers a "use rect" per usable object and probes a one-block
    /// square a block out along the aim (`mario.lua:2785-2800`) — *not* a box around
    /// the player, so where you are looking decides what you use. Dropping takes
    /// priority over using: with a cube in hand the key can only put it down.
    ///
    /// Cubes are checked before wall buttons. The original walks its `userects` table
    /// in hash order, so there is no defined precedence to copy; picking the cube means
    /// a cube resting against a button panel can still be picked up.
    pub(crate) fn use_pressed(&mut self) {
        if let Some(held) = self.cubes.iter().position(|c| c.held) {
            self.drop_cube(held);
            return;
        }
        let (ax, ay) = (self.crosshair_angle.cos(), self.crosshair_angle.sin());
        let probe = [
            self.player.center_x() + ax * USE_RANGE - USE_SQUARE / 2.0,
            self.player.center_y() + ay * USE_RANGE - USE_SQUARE / 2.0,
            USE_SQUARE,
            USE_SQUARE,
        ];

        if let Some(i) = self
            .cubes
            .iter()
            .position(|c| aabb_overlap(probe, c.rect()))
        {
            self.cubes[i].held = true;
            return;
        }
        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::PushButton {
                continue;
            }
            if aabb_overlap(
                probe,
                crate::lab::push_button_use_rect(self.lab.elements[index].cell),
            ) {
                self.lab.push(index);
                return;
            }
        }
    }

    /// Put a carried cube down beside the player.
    ///
    /// Six candidate spots in order (`mario:dropbox`, `mario.lua:2806-2846`): the side
    /// he is aiming at, then the other side, then below, then above, and finally on top
    /// of himself. The last one always "succeeds" — the original would rather leave the
    /// cube inside Mario than not drop it.
    fn drop_cube(&mut self, index: usize) {
        let (px, py, pw, ph) = (
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
        );
        let feet = py + ph - CUBE_SIZE;
        let aiming_right = self.crosshair_angle.cos() >= 0.0;
        let right = (px + pw, feet);
        let left = (px - CUBE_SIZE, feet);
        let candidates = if aiming_right {
            [right, left, (px, py + ph), (px, py - CUBE_SIZE), (px, py)]
        } else {
            [left, right, (px, py + ph), (px, py - CUBE_SIZE), (px, py)]
        };
        let (x, y) = candidates
            .into_iter()
            .find(|&(x, y)| rect_is_clear(&self.level, x, y, CUBE_SIZE, CUBE_SIZE))
            // The fallback is the last candidate, i.e. inside the player.
            .unwrap_or((px, py));
        let cube = &mut self.cubes[index];
        cube.held = false;
        cube.x = x;
        cube.y = y;
        cube.vx = 0.0;
        cube.vy = 0.0;
        cube.falling = true;
    }

    /// Does the player's carried cube shield him from a beam arriving from `from`?
    ///
    /// `mario:laser` returns early when he is carrying a cube and aiming into the beam
    /// (`mario.lua:2672-2684`) — four sign tests, one per side, which together say
    /// "the aim has a component towards the beam". A held cube blocks by geometry too,
    /// but only when it happens to be the nearest body along the run; this rule is what
    /// makes holding it up reliable.
    pub(crate) fn cube_shields_from(&self, from: crate::player::Orientation) -> bool {
        use crate::player::Orientation::*;
        if !self.cubes.iter().any(|c| c.held) {
            return false;
        }
        let (ax, ay) = (self.crosshair_angle.cos(), self.crosshair_angle.sin());
        match from {
            Right => ax > 0.0,
            Left => ax < 0.0,
            Up => ay < 0.0,
            Down => ay > 0.0,
        }
    }
}
