//! The lab's signal network: buttons, doors and indicators, and the wiring between.
//!
//! ## The link direction is backwards from the intuition
//!
//! The `"link"` triple lives on the **receiving** element and points at the element
//! that drives it (`door:link()`, `door.lua:33-44`). That's because the editor's
//! gesture is to drag *from* the door *to* the button. So resolving the network means
//! walking the inputs and, for each, finding the output at the coordinates it names —
//! not the other way round.
//!
//! Each input can be driven by exactly one output (the original overwrites in place
//! while scanning for the `"link"` token), while an output may drive any number of
//! inputs. Signals are three strings — `on`, `off`, `toggle` — with **no value, no
//! priority, no debounce and no cycle detection** (`game.lua:52` lists the six output
//! kinds; there is nothing else to the protocol).
//!
//! ## What the shipped levels actually contain
//!
//! Counted across all nine lab levels, the elements that carry a link — i.e. the
//! *inputs* — are doors (24), ground lights (269), wall indicators (6), box tubes
//! (12) and timers (2). The outputs driving them are buttons (19), push buttons (3)
//! and laser detectors (6).
//!
//! Two things that follow, and that save a lot of work:
//!
//! - **No shipped level contains a NOT gate.** Its documented quirk — pushing `on`
//!   downstream on the very first frame — is unobservable in the shipped data, so it
//!   is not implemented here rather than implemented untested.
//! - **No shipped lightbridge or laser carries a link.** Since `link()` only sets
//!   `enabled = false` on a *successful* match, every one of them is permanently on.
//!   That removes the whole "linked bridges start dark" case from the shipped data.
//!
//! This module covers the button → door path plus the indicators, which is the
//! wiring the shipped levels are built out of. Lasers, bridges, gels, faith plates
//! and box dispensers are still to come.

use std::collections::HashMap;

use crate::constants::*;
use crate::enemies::enemy_height;
use crate::game::Mari0Game;
use crate::level::{self, EntityKind};
use crate::physics::aabb_overlap;
use crate::player::Orientation;

/// How fast a door opens: `doorspeed = 2` means the 0→1 timer takes half a second.
const DOOR_SPEED: f32 = 2.0;

/// Cooldown between wall-button presses (`pushbuttontime = 1`).
const PUSH_BUTTON_COOLDOWN: f32 = 1.0;

/// How far a beam is allowed to travel, in cells.
///
/// The original has no such bound; this stops a beam pointing along an open corridor
/// in a hand-made level from walking off to infinity.
const MAX_BEAM_CELLS: usize = 512;

/// The kinds of element this module models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum LabKind {
    /// Floor plate. On while a player, enemy or cube rests on it.
    Button,
    /// Wall button. Emits `toggle` when struck, rather than tracking a weight.
    PushButton,
    /// Two tiles of door, blocking while shut.
    Door,
    /// Wired floor/wall strip that lights up. Purely an indicator, but it is what
    /// makes the wiring legible, and there are 269 of them.
    GroundLight,
    /// Wall panel that latches on. Also an indicator.
    WallIndicator,

    /// A laser emitter. Unlinked ones are permanently on, which is every one in the
    /// shipped levels.
    Laser,

    // ── Recognised, but with no behaviour yet ────────────────────────
    // These exist in the graph so links resolve and the network is complete. Their
    // *behaviour* is not implemented: a laser detector with no laser to detect never
    // fires, and a timer never elapses. That is deliberately inert rather than
    // half-guessed — but it does mean a door wired to a detector currently stays shut.
    /// Fires while a laser is landing on it.
    LaserDetector,
    /// Periodic pulse (`walltimer`). Waiting on its cycle rules.
    Timer,
    /// Cube dispenser tube. An *input*: it carries the link.
    BoxTube,
    /// A placed cube. An *output* — the original lists `box` among the six
    /// (`game.lua:52`), and a tube links to the cube it is responsible for so that a
    /// cube lost off the map can push `toggle` back and be replaced.
    Box,
}

impl LabKind {
    /// Does this element drive others?
    ///
    /// The original's list is `{button, laserdetector, box, pushbutton, walltimer,
    /// notgate}` (`game.lua:52`).
    /// No shipped level has a `notgate`, so that one is absent.
    pub(crate) fn is_output(self) -> bool {
        matches!(
            self,
            LabKind::Button
                | LabKind::PushButton
                | LabKind::LaserDetector
                | LabKind::Timer
                | LabKind::Box
        )
    }

    /// Is this element's behaviour implemented, or is it only in the graph so links
    /// resolve? Reported through the VDP so a test can tell the two apart.
    #[cfg(feature = "vdp")]
    pub(crate) fn is_inert(self) -> bool {
        matches!(self, LabKind::Timer | LabKind::BoxTube | LabKind::Box)
    }

    fn from_entity(kind: EntityKind) -> Option<Self> {
        use EntityKind::*;
        Some(match kind {
            Button => LabKind::Button,
            PushButtonLeft | PushButtonRight => LabKind::PushButton,
            DoorHor | DoorVer => LabKind::Door,
            GroundLightVer | GroundLightHor | GroundLightUpRight | GroundLightRightDown
            | GroundLightDownLeft | GroundLightLeftUp => LabKind::GroundLight,
            WallIndicator => LabKind::WallIndicator,
            LaserRight | LaserDown | LaserLeft | LaserUp => LabKind::Laser,
            LaserDetectorRight | LaserDetectorDown | LaserDetectorLeft | LaserDetectorUp => {
                LabKind::LaserDetector
            }
            Timer => LabKind::Timer,
            BoxTube => LabKind::BoxTube,
            Box => LabKind::Box,
            _ => return None,
        })
    }
}

/// One element of the network.
#[derive(Debug, Clone)]
pub(crate) struct LabElement {
    pub(crate) kind: LabKind,
    /// Anchor cell, 0-based.
    pub(crate) cell: (i32, i32),
    /// Which way a door lies. `None` for everything else.
    pub(crate) axis: Option<Orientation>,
    /// The cell of the output driving this element, before resolution.
    pub(crate) link_target: Option<(i32, i32)>,
    /// Resolved index of the driving element.
    pub(crate) driver: Option<usize>,
    /// Whether this element is currently energised. For a door, "open".
    pub(crate) on: bool,
    /// Door open fraction, 0..1. A door only stops blocking at 1.
    ///
    /// Doubles as a wall button's cooldown, counting *down* from
    /// `PUSH_BUTTON_COOLDOWN`, since no element needs both.
    pub(crate) timer: f32,
    /// Detector only: may `clear()` succeed this frame? See `Lab::step_lasers`.
    pub(crate) allow_clear: bool,
    /// Detector only: the value last pushed downstream, for the one-frame delay.
    pub(crate) pushed: bool,
    /// Laser only: detectors this beam has lit, which it must keep trying to clear.
    pub(crate) lit: Vec<usize>,
}

impl LabElement {
    /// The two cells a door occupies.
    ///
    /// A horizontal door is a 2-block-wide bar lying in the row above its anchor; a
    /// vertical one is 2 blocks tall in the anchor's column. Both work out to two
    /// cells, like a portal — which is not a coincidence, they're both wall fixtures
    /// sized to a portal's footprint.
    pub(crate) fn door_cells(&self) -> Option<[(i32, i32); 2]> {
        let (c, r) = self.cell;
        match self.axis? {
            Orientation::Right | Orientation::Left => Some([(c, r), (c + 1, r)]),
            Orientation::Up | Orientation::Down => Some([(c, r - 1), (c, r)]),
        }
    }
}

/// The whole network for one level.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lab {
    pub(crate) elements: Vec<LabElement>,
    /// For each element, the indices it drives. Built once at load.
    pub(crate) consumers: Vec<Vec<usize>>,
}

impl Lab {
    /// Build the network from the parsed lab placements, resolving every link.
    pub(crate) fn build(placements: &[level::parse::LabPlacement]) -> Self {
        let mut elements: Vec<LabElement> = Vec::new();
        for p in placements {
            let Some(kind) = LabKind::from_entity(p.kind) else {
                continue;
            };
            // For a door this is which way it lies; for a laser or detector, which
            // way it points.
            let axis = match p.kind {
                EntityKind::DoorHor => Some(Orientation::Right),
                EntityKind::DoorVer => Some(Orientation::Up),
                EntityKind::LaserRight | EntityKind::LaserDetectorRight => Some(Orientation::Right),
                EntityKind::LaserLeft | EntityKind::LaserDetectorLeft => Some(Orientation::Left),
                EntityKind::LaserUp | EntityKind::LaserDetectorUp => Some(Orientation::Up),
                EntityKind::LaserDown | EntityKind::LaserDetectorDown => Some(Orientation::Down),
                _ => None,
            };
            elements.push(LabElement {
                kind,
                cell: (p.x as i32, p.y as i32),
                axis,
                // The parser hands back the raw link coordinates, which are the
                // level file's 1-based tile numbers; the grid is 0-based.
                link_target: p.link.map(|(x, y)| (x as i32 - 1, y as i32 - 1)),
                driver: None,
                // A laser is on unless something is wired to control it: `laser:link()`
                // sets `enabled = false` only on a *successful* match
                // (`laser.lua:18-27`). No shipped laser is linked, so they all start on.
                on: kind == LabKind::Laser,
                timer: 0.0,
                allow_clear: true,
                pushed: false,
                lit: Vec::new(),
            });
        }

        // Resolve links: an input names the *output* that drives it.
        let by_cell: HashMap<(i32, i32), usize> = elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind.is_output())
            .map(|(i, e)| (e.cell, i))
            .collect();

        let mut consumers = vec![Vec::new(); elements.len()];
        for (i, element) in elements.iter_mut().enumerate() {
            let Some(target) = element.link_target else {
                continue;
            };
            if let Some(&driver) = by_cell.get(&target) {
                element.driver = Some(driver);
                consumers[driver].push(i);
                // The `link()` side effect: a laser that *is* controlled starts off.
                if element.kind == LabKind::Laser {
                    element.on = false;
                }
            }
        }

        Self {
            elements,
            consumers,
        }
    }

    /// Cells that currently block movement because a door is shut across them.
    pub(crate) fn blocking_cells(&self) -> Vec<(i32, i32)> {
        let mut cells = Vec::new();
        for e in &self.elements {
            // A door only stops blocking once *fully* open (`door.lua:64`), so a
            // half-open door is still a wall.
            if e.kind == LabKind::Door
                && e.timer < 1.0
                && let Some(pair) = e.door_cells()
            {
                cells.extend_from_slice(&pair);
            }
        }
        cells
    }

    /// Drive `element` and everything downstream of it.
    ///
    /// Propagation order is by element index — the original walks Lua tables with
    /// `pairs`, which is an arbitrary hash order, so *some* order has to be chosen
    /// and written down. Index order is the level file's reading order, which at
    /// least makes it reproducible and inspectable.
    ///
    /// There is no cycle detection in the original either; the `seen` set here is
    /// purely so a cycle in hand-made level data can't hang the game.
    fn emit(&mut self, from: usize, signal: Signal) {
        let mut queue = vec![(from, signal)];
        let mut seen = vec![false; self.elements.len()];
        while let Some((index, signal)) = queue.pop() {
            if seen[index] {
                continue;
            }
            seen[index] = true;
            for &consumer in &self.consumers[index].clone() {
                let element = &mut self.elements[consumer];
                match signal {
                    Signal::On => element.on = true,
                    Signal::Off => element.on = false,
                    Signal::Toggle => element.on = !element.on,
                }
                // Indicators and doors are leaves in the shipped data; nothing
                // downstream of them exists to forward to. Kept as a queue anyway so
                // adding NOT gates later needs no restructuring.
                queue.push((consumer, signal));
            }
        }
    }

    /// Send a signal to one element directly, as an upstream output would.
    ///
    /// Exists for the VDP: it is the only way to exercise a `toggle`, and to drive an
    /// element whose upstream source isn't implemented yet — a door wired to a laser
    /// detector, say.
    #[cfg(feature = "vdp")]
    pub(crate) fn signal(&mut self, index: usize, signal: Signal) {
        if index >= self.elements.len() {
            return;
        }
        let element = &mut self.elements[index];
        match signal {
            Signal::On => element.on = true,
            Signal::Off => element.on = false,
            Signal::Toggle => element.on = !element.on,
        }
        self.emit(index, signal);
    }

    /// Set an output's state, emitting only on a change.
    ///
    /// The original compares against the previous value before pushing
    /// (`button.lua:27`), which is what keeps a held-down button from re-sending
    /// `on` every frame.
    pub(crate) fn set_output(&mut self, index: usize, on: bool) {
        if self.elements[index].on == on {
            return;
        }
        self.elements[index].on = on;
        self.emit(index, if on { Signal::On } else { Signal::Off });
    }

    /// Drive the laser detectors from the beams, reproducing the two-phase latch.
    ///
    /// The order inside a frame is what makes this work, and it is easy to get wrong
    /// in a way that only shows up as a flickering laser:
    ///
    /// 1. Every detector sets `allowclear = true` and, if its value *changed since
    ///    last frame*, pushes it downstream. That comparison against the previous
    ///    frame is the detector's inherent **one-frame delay**
    ///    (`laserdetector.lua:16-25`).
    /// 2. Each live beam walks its cells; a detector under the beam gets
    ///    `input("on")`, which sets its value **and clears `allowclear`**.
    /// 3. Each beam then calls `clear()` on every detector it has ever hit. For the
    ///    ones hit this frame that's a no-op — `allowclear` is already false. For the
    ///    ones it *stopped* hitting, `allowclear` is still true from step 1, so they
    ///    go off.
    ///
    /// A beam also **cannot clear the detector that controls it** (`laser.lua:265`:
    /// the cell is skipped when it matches the laser's own link target). Without that
    /// self-exclusion a laser wired to a detector it can see would oscillate.
    fn step_lasers(&mut self, level: &crate::world::Level) {
        // Phase 1.
        for e in &mut self.elements {
            if e.kind == LabKind::LaserDetector {
                e.allow_clear = true;
            }
        }
        let changed: Vec<(usize, bool)> = self
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == LabKind::LaserDetector && e.on != e.pushed)
            .map(|(i, e)| (i, e.on))
            .collect();
        for (index, on) in changed {
            self.elements[index].pushed = on;
            self.emit(index, if on { Signal::On } else { Signal::Off });
        }

        // Phase 2: march each live beam and light what it lands on.
        let lasers: Vec<usize> = (0..self.elements.len())
            .filter(|&i| self.elements[i].kind == LabKind::Laser && self.elements[i].on)
            .collect();
        for laser in lasers {
            let cells = self.beam_cells(laser, level);
            let own_target = self.elements[laser].link_target;
            for cell in cells {
                let hit: Vec<usize> = self
                    .elements
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.kind == LabKind::LaserDetector
                            && e.cell == cell
                            // Self-exclusion: not the detector wired to this laser.
                            && Some(e.cell) != own_target
                    })
                    .map(|(i, _)| i)
                    .collect();
                for i in hit {
                    self.elements[i].on = true;
                    self.elements[i].allow_clear = false;
                    if !self.elements[laser].lit.contains(&i) {
                        self.elements[laser].lit.push(i);
                    }
                }
            }
        }

        // Phase 3: every detector a beam has ever lit gets a `clear()` attempt.
        let attempts: Vec<usize> = self
            .elements
            .iter()
            .filter(|e| e.kind == LabKind::Laser)
            .flat_map(|e| e.lit.clone())
            .collect();
        for i in attempts {
            if self.elements[i].allow_clear {
                self.elements[i].allow_clear = false;
                self.elements[i].on = false;
            }
        }
    }

    /// The cells one beam covers, stopping at the first thing that blocks it.
    ///
    /// A straight run for now: the original's beam is a list of segments so it can
    /// bend through portals, which is not implemented here. Walls stop it, and so does
    /// a shut door — `blocks_movement` already knows about both.
    fn beam_cells(&self, laser: usize, level: &crate::world::Level) -> Vec<(i32, i32)> {
        let Some(dir) = self.elements[laser].axis else {
            return Vec::new();
        };
        let (dx, dy) = match dir {
            Orientation::Right => (1, 0),
            Orientation::Left => (-1, 0),
            Orientation::Up => (0, -1),
            Orientation::Down => (0, 1),
        };
        let (mut c, mut r) = self.elements[laser].cell;
        let mut cells = Vec::new();
        // Bounded so a beam pointing down an open corridor can't walk forever.
        for _ in 0..MAX_BEAM_CELLS {
            c += dx;
            r += dy;
            if crate::physics::blocks_movement(level, c, r) {
                break;
            }
            cells.push((c, r));
        }
        cells
    }

    /// Press a wall button, if its cooldown has lapsed.
    ///
    /// A wall button is *used*, not walked into: the original registers a small "use
    /// rect" and only fires from `pushbutton:used()`, then refuses for
    /// `pushbuttontime` seconds. Each press sends `toggle` — the only element that
    /// sends it.
    pub(crate) fn push(&mut self, index: usize) -> bool {
        if self.elements[index].timer > 0.0 {
            return false;
        }
        self.elements[index].timer = PUSH_BUTTON_COOLDOWN;
        self.emit(index, Signal::Toggle);
        true
    }

    /// Advance door animations. Returns true if any door finished opening or
    /// closing, which is when collision needs rebuilding.
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        let mut changed = false;
        for e in &mut self.elements {
            if e.kind == LabKind::PushButton {
                e.timer = (e.timer - dt).max(0.0);
                continue;
            }
            if e.kind != LabKind::Door {
                continue;
            }
            let before = e.timer;
            if e.on {
                e.timer = (e.timer + DOOR_SPEED * dt).min(1.0);
            } else {
                e.timer = (e.timer - DOOR_SPEED * dt).max(0.0);
            }
            // Only the transitions across the ends matter for collision.
            if (before < 1.0 && e.timer >= 1.0) || (before >= 1.0 && e.timer < 1.0) {
                changed = true;
            }
        }
        changed
    }
}

impl Mari0Game {
    /// Drive the lab network for one frame: sense the buttons, animate the doors,
    /// and republish the cells a shut door blocks.
    pub(crate) fn update_lab(&mut self, dt: f32, use_pressed: bool) {
        if self.lab.elements.is_empty() {
            return;
        }

        // Floor buttons sense weight. The original's probe list is
        // `{"player", "goomba", "koopa", "box"}` (`button.lua:24`) — so an enemy holds
        // a button down just as well as Mario does, which several lab levels rely on.
        let player = [
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
        ];
        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::Button {
                continue;
            }
            let sense = button_sense_rect(self.lab.elements[index].cell);
            let pressed = aabb_overlap(player, sense)
                || self.enemies.iter().any(|e| {
                    aabb_overlap(
                        [
                            e.x,
                            e.y,
                            PLAYER_SMALL_W,
                            enemy_height(e.enemy_type, e.state),
                        ],
                        sense,
                    )
                })
                || self
                    .items
                    .iter()
                    .any(|i| aabb_overlap([i.x, i.y, TILE_SIZE, TILE_SIZE], sense));
            self.lab.set_output(index, pressed);
        }

        // Wall buttons: pressed with the use key while standing at them.
        if use_pressed {
            for index in 0..self.lab.elements.len() {
                if self.lab.elements[index].kind != LabKind::PushButton {
                    continue;
                }
                if aabb_overlap(player, push_button_use_rect(self.lab.elements[index].cell)) {
                    self.lab.push(index);
                }
            }
        }

        // Beams see the door state published at the end of the *previous* frame,
        // which is the same one-frame lag the original has: a door refreshes laser
        // ranges when it finishes opening, not while it moves.
        self.lab.step_lasers(&self.level);
        self.lab.tick(dt);

        // Rebuilt wholesale each frame rather than diffed on transitions: there are a
        // handful of doors per level, and "always correct" beats "clever".
        self.level.solid_extras.clear();
        for cell in self.lab.blocking_cells() {
            self.level.solid_extras.insert(cell);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    On,
    Off,
    Toggle,
}

/// The area a floor button senses, in world pixels: `[x, y, w, h]`.
///
/// The original probes a box inset from the button's own footprint
/// (`checkrect(self.x+5/16, self.y-2/16, 20/16, 1)`, `button.lua:24`) — narrower than
/// the plate and reaching a block upward, so brushing the very edge doesn't press it.
pub(crate) fn button_sense_rect(cell: (i32, i32)) -> [f32; 4] {
    let (c, r) = (cell.0 as f32, cell.1 as f32);
    // The plate is centred on its cell and 30/16 blocks wide; the sensed strip is
    // 20/16 wide, starting 5/16 in.
    let x = (c - 15.0 / 16.0 + 5.0 / 16.0) * TILE_SIZE;
    let y = (r - 3.0 / 16.0 - 2.0 / 16.0) * TILE_SIZE - TILE_SIZE;
    [x, y, 20.0 / 16.0 * TILE_SIZE, TILE_SIZE]
}

/// The rect a wall button can be pressed from, in world pixels.
///
/// `adduserect(x-10/16, y-12/16, 4/16, 12/16)` (`pushbutton.lua:11`) — a narrow strip
/// beside the panel, so you have to be standing at it rather than anywhere nearby.
pub(crate) fn push_button_use_rect(cell: (i32, i32)) -> [f32; 4] {
    let (c, r) = (cell.0 as f32, cell.1 as f32);
    [
        (c - 10.0 / 16.0) * TILE_SIZE,
        (r - 12.0 / 16.0) * TILE_SIZE,
        4.0 / 16.0 * TILE_SIZE,
        12.0 / 16.0 * TILE_SIZE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::parse::LabPlacement;

    fn place(kind: EntityKind, x: usize, y: usize, link: Option<(u16, u16)>) -> LabPlacement {
        LabPlacement {
            x,
            y,
            kind,
            arg: None,
            link,
        }
    }

    /// The link lives on the *input* and names the output. Getting this backwards
    /// silently produces a network where nothing is wired to anything.
    #[test]
    fn a_link_on_an_input_resolves_to_the_output_it_names() {
        // Button at 0-based (5, 9) — the file would write it 1-based as (6, 10).
        let lab = Lab::build(&[
            place(EntityKind::Button, 5, 9, None),
            place(EntityKind::DoorVer, 8, 9, Some((6, 10))),
        ]);
        assert_eq!(
            lab.elements[1].driver,
            Some(0),
            "door should point at button"
        );
        assert_eq!(lab.consumers[0], vec![1], "button should drive the door");
        assert!(lab.consumers[1].is_empty(), "door drives nothing");
    }

    /// Fan-out is unlimited: one button can drive a door and a pile of indicators.
    #[test]
    fn one_output_can_drive_many_inputs() {
        let lab = Lab::build(&[
            place(EntityKind::Button, 5, 9, None),
            place(EntityKind::DoorVer, 8, 9, Some((6, 10))),
            place(EntityKind::GroundLightHor, 6, 9, Some((6, 10))),
            place(EntityKind::GroundLightHor, 7, 9, Some((6, 10))),
        ]);
        assert_eq!(lab.consumers[0], vec![1, 2, 3]);
    }

    /// An unlinked input is simply never driven — not an error.
    #[test]
    fn an_unlinked_input_has_no_driver() {
        let lab = Lab::build(&[place(EntityKind::DoorVer, 8, 9, None)]);
        assert_eq!(lab.elements[0].driver, None);
    }

    /// A link naming a cell with no output there resolves to nothing rather than
    /// panicking. Hand-edited levels do this.
    #[test]
    fn a_dangling_link_is_ignored() {
        let lab = Lab::build(&[place(EntityKind::DoorVer, 8, 9, Some((99, 99)))]);
        assert_eq!(lab.elements[0].driver, None);
    }

    #[test]
    fn a_signal_opens_and_closes_the_door_it_reaches() {
        let mut lab = Lab::build(&[
            place(EntityKind::Button, 5, 9, None),
            place(EntityKind::DoorVer, 8, 9, Some((6, 10))),
        ]);
        assert!(!lab.elements[1].on);
        lab.set_output(0, true);
        assert!(lab.elements[1].on, "on should reach the door");
        lab.set_output(0, false);
        assert!(!lab.elements[1].on, "off should reach the door");
    }

    /// A held button must not re-send every frame. The original compares to the
    /// previous value first; without that, a `toggle` consumer would chatter.
    #[test]
    fn an_unchanged_output_emits_nothing() {
        let mut lab = Lab::build(&[
            place(EntityKind::Button, 5, 9, None),
            place(EntityKind::DoorVer, 8, 9, Some((6, 10))),
        ]);
        lab.set_output(0, true);
        // Forcing the door shut behind the network's back: a re-emit would reopen it.
        lab.elements[1].on = false;
        lab.set_output(0, true);
        assert!(
            !lab.elements[1].on,
            "setting an output to the value it already has must emit nothing"
        );
    }

    /// A door blocks until it is *fully* open, and stops blocking only then.
    #[test]
    fn a_door_blocks_until_fully_open() {
        let mut lab = Lab::build(&[place(EntityKind::DoorVer, 8, 9, None)]);
        assert_eq!(lab.blocking_cells().len(), 2, "shut door blocks both cells");

        lab.elements[0].on = true;
        // Half a second at doorspeed 2 is exactly one full open.
        let mut opened = false;
        for _ in 0..60 {
            opened |= lab.tick(1.0 / 60.0);
        }
        assert!(opened, "should report the transition");
        assert_eq!(lab.elements[0].timer, 1.0);
        assert!(
            lab.blocking_cells().is_empty(),
            "a fully open door blocks nothing"
        );
    }

    #[test]
    fn a_door_half_open_still_blocks() {
        let mut lab = Lab::build(&[place(EntityKind::DoorVer, 8, 9, None)]);
        lab.elements[0].on = true;
        lab.tick(0.1); // 0.2 of the way
        assert!(lab.elements[0].timer > 0.0 && lab.elements[0].timer < 1.0);
        assert_eq!(lab.blocking_cells().len(), 2, "half-open is still a wall");
    }

    /// Door footprints are two cells, oriented by axis.
    #[test]
    fn doors_occupy_two_cells_along_their_axis() {
        let hor = Lab::build(&[place(EntityKind::DoorHor, 8, 9, None)]);
        assert_eq!(hor.elements[0].door_cells(), Some([(8, 9), (9, 9)]));
        let ver = Lab::build(&[place(EntityKind::DoorVer, 8, 9, None)]);
        assert_eq!(ver.elements[0].door_cells(), Some([(8, 8), (8, 9)]));
    }

    /// A cycle in hand-made data must not hang the propagation.
    #[test]
    fn a_cycle_terminates() {
        // Two buttons naming each other. Nonsense, but the original has no cycle
        // detection and level data is hand-made.
        let mut lab = Lab::build(&[
            place(EntityKind::Button, 5, 9, Some((7, 10))),
            place(EntityKind::Button, 6, 9, Some((6, 10))),
        ]);
        lab.set_output(0, true);
        // Reaching here at all is the assertion.
        assert!(lab.elements[0].on);
    }

    /// A bare level with nothing solid in it, for beam tests.
    fn empty_level() -> crate::world::Level {
        crate::world::load_level("portal", "2-1")
    }

    /// An unlinked laser is on from the start — `link()` only disables on a match, and
    /// no shipped laser is linked.
    #[test]
    fn an_unlinked_laser_starts_on_and_a_linked_one_starts_off() {
        let free = Lab::build(&[place(EntityKind::LaserRight, 5, 9, None)]);
        assert!(free.elements[0].on, "unlinked laser should be on");

        let wired = Lab::build(&[
            place(EntityKind::Button, 3, 9, None),
            place(EntityKind::LaserRight, 5, 9, Some((4, 10))),
        ]);
        assert!(!wired.elements[1].on, "a controlled laser starts off");
    }

    /// The latch, frame by frame. This is the sequence that flickers if the phases run
    /// in the wrong order.
    #[test]
    fn the_detector_latches_on_while_lit_and_releases_a_frame_after() {
        let level = empty_level();
        // Laser at (5, 9) pointing right, detector three cells along.
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
        ]);
        let (laser, detector) = (0, 1);

        lab.step_lasers(&level);
        assert!(lab.elements[detector].on, "beam should light the detector");
        assert!(
            !lab.elements[detector].allow_clear,
            "input() must clear the flag so the beam's own clear() no-ops"
        );

        // Still lit next frame: stays on.
        lab.step_lasers(&level);
        assert!(lab.elements[detector].on, "still lit, still on");

        // Switch the laser off. The detector releases on the *next* sweep, because
        // `clear()` only takes effect once `allowclear` has been re-armed.
        lab.elements[laser].on = false;
        lab.step_lasers(&level);
        assert!(!lab.elements[detector].on, "unlit detector goes off");
    }

    /// The detector pushes downstream on a *change*, one frame behind — it compares
    /// against the value it last pushed.
    #[test]
    fn the_detector_pushes_one_frame_after_it_changes() {
        let level = empty_level();
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
            // A door wired to the detector at 0-based (8,9) → file 1-based (9,10).
            place(EntityKind::DoorVer, 12, 9, Some((9, 10))),
        ]);
        let door = 2;
        assert_eq!(lab.elements[door].driver, Some(1), "door wired to detector");

        lab.step_lasers(&level);
        assert!(
            !lab.elements[door].on,
            "the push happens on the next sweep, not the one that lit the detector"
        );
        lab.step_lasers(&level);
        assert!(lab.elements[door].on, "one frame later, the door opens");
    }

    /// A beam cannot clear the detector that controls its own laser, or the pair
    /// oscillates.
    #[test]
    fn a_beam_excludes_the_detector_that_controls_it() {
        let level = empty_level();
        // Laser linked to the detector it is pointing at.
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, Some((9, 10))),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
        ]);
        lab.elements[0].on = true; // force it on despite being linked
        lab.step_lasers(&level);
        assert!(
            !lab.elements[1].on,
            "the beam must skip its own controlling detector"
        );
    }

    /// A wall stops the beam, so a detector behind one never lights.
    #[test]
    fn a_wall_stops_the_beam() {
        let mut level = empty_level();
        // Make the cell right of the laser solid.
        level.solid_extras.insert((6, 9));
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
        ]);
        lab.step_lasers(&level);
        assert!(
            !lab.elements[1].on,
            "a detector behind a wall should stay dark"
        );
    }

    /// Every link in every shipped lab level must resolve, or that level's wiring is
    /// dead and the level unplayable.
    #[test]
    fn every_shipped_lab_link_resolves() {
        for (pack, name, raw) in level::LEVELS {
            if *pack != *"portal" {
                continue;
            }
            let parsed = level::Level::parse(raw).expect("shipped level parses");
            let lab = Lab::build(&parsed.markers.lab);
            let linked = lab.elements.iter().filter(|e| e.link_target.is_some());
            let unresolved: Vec<_> = linked
                .clone()
                .filter(|e| e.driver.is_none())
                .map(|e| (e.kind, e.cell, e.link_target))
                .collect();
            assert!(
                unresolved.is_empty(),
                "{pack}/{name}: {} of {} links dangle: {:?}",
                unresolved.len(),
                linked.count(),
                &unresolved[..unresolved.len().min(4)]
            );
        }
    }
}
