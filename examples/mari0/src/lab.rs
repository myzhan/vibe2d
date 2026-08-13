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
//! This module covers the button → door path, the indicators, and the two things
//! that travel by cell: laser beams and light bridges. Gels, faith plates and box
//! dispensers are still to come.

use std::collections::HashMap;

use vibe2d::prelude::*;

use crate::constants::*;
use crate::enemies::enemy_height;
use crate::game::Mari0Game;
use crate::level::{self, EntityKind};
use crate::physics::{aabb_overlap, is_solid};
use crate::player::Orientation;
use crate::portal_math::{PortalAnchor, portal_route};

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
    /// A light-bridge emitter. Lays a thin solid slab in every cell its beam
    /// covers, so the beam is a floor (horizontal) or a wall (vertical).
    LightBridge,
    /// Fires while a laser is landing on it.
    LaserDetector,

    // ── Recognised, but with no behaviour yet ────────────────────────
    // These exist in the graph so links resolve and the network is complete. Their
    // *behaviour* is not implemented, and a timer never elapses. That is deliberately
    // inert rather than half-guessed — but it does mean a door wired to a timer
    // currently stays shut.
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

    /// Does this element project a beam — a laser or a light bridge?
    ///
    /// The two march the same path by the same rules (`laser.lua:309` and
    /// `lightbridge.lua:59` are the same loop); what differs is what they do with the
    /// cells they cover.
    pub(crate) fn is_emitter(self) -> bool {
        matches!(self, LabKind::Laser | LabKind::LightBridge)
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
            LightBridgeRight | LightBridgeLeft | LightBridgeDown | LightBridgeUp => {
                LabKind::LightBridge
            }
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
    /// Emitters only: where the beam currently runs. Recomputed every frame.
    pub(crate) beam: Vec<BeamSegment>,
}

/// One straight run of a beam, in cells.
///
/// A beam is a *list* of runs rather than a single ray because it bends: entering a
/// portal ends one run and starts another at the other mouth, pointing whichever way
/// that mouth faces (`laser.lua:340-380`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BeamSegment {
    pub(crate) dir: Orientation,
    /// Cells the beam covers, in travel order. The first is the emitter's own cell
    /// (or the cell just outside the portal this run emerged from) — the original
    /// tests the current cell for collision *before* stepping, so the cell the
    /// emitter sits in is always part of the run.
    pub(crate) cells: Vec<(i32, i32)>,
    /// The cell that ended the run — the wall it hit, the shut door, or the portal it
    /// went into. Not covered by the beam, but **probed for detectors anyway**.
    ///
    /// That is not a detail: every laser detector in the shipped levels sits on a
    /// *solid* tile (2-3 has them on 134, 135, 141 and 154), so a beam always stops
    /// one cell short of the detector it is aimed at. The original reaches it by
    /// looping one index past the run's range (`laser.lua:240`). Without this, no
    /// detector in the game ever fires.
    pub(crate) end: Option<(i32, i32)>,
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

    /// Every cell this element's beam should be tested against for detectors: the
    /// cells it covers, plus the cell each run terminates on. See
    /// [`BeamSegment::end`] for why that last one is load-bearing.
    fn probe_cells(&self) -> Vec<(i32, i32)> {
        let mut cells = Vec::new();
        for segment in &self.beam {
            cells.extend_from_slice(&segment.cells);
            cells.extend(segment.end);
        }
        cells
    }

    /// Cut the beam short at `cell`, which is where something is standing in it.
    ///
    /// Everything from that cell on is dropped, including any run past a portal: the
    /// original rebuilds the segment list up to the blocked run and overwrites its
    /// range with the object's edge (`laser.lua:83-90`). Cell granularity here rather
    /// than the original's pixel-exact truncation — the difference is up to one cell of
    /// beam drawn past the body it stops on.
    fn truncate_beam_at(&mut self, segment: usize, cell: usize) {
        self.beam.truncate(segment + 1);
        if let Some(last) = self.beam.last_mut() {
            last.cells.truncate(cell);
            // It no longer reaches a wall, so there is nothing beyond it to probe —
            // which is exactly how a body blocks a detector.
            last.end = None;
        }
    }
}

/// Walk a beam from `start` in `dir`, bending through portals, until something stops
/// it.
///
/// The one loop shared by `laser:updaterange` (`laser.lua:309`) and
/// `lightbridge:updaterange` (`lightbridge.lua:59`), which are the same code twice.
/// Three things stop a beam: a solid tile, a shut door, or the edge of the map.
///
/// Solidity here is the **tile grid**, not `blocks_movement`: a portal has not
/// removed the wall it is mounted in as far as a beam is concerned. The beam gets
/// through by being *routed* — the cell it is about to enter is checked against the
/// portal pair first, and a beam meeting a mouth's front face comes out of the other
/// mouth pointing the way that one faces.
fn march_beam(
    level: &crate::world::Level,
    portals: Option<(PortalAnchor, PortalAnchor)>,
    shut_doors: &[(i32, i32)],
    start: (i32, i32),
    dir: Orientation,
) -> Vec<BeamSegment> {
    let stops = |cell: (i32, i32)| {
        cell.0 < 0
            || cell.1 < 0
            || cell.0 >= level.width as i32
            || cell.1 >= level.height as i32
            || is_solid(crate::physics::get_tile(level, cell.0, cell.1))
            || shut_doors.contains(&cell)
    };

    let mut segments = Vec::new();
    let mut dir = dir;
    let mut cell = start;
    let mut run = BeamSegment {
        dir,
        cells: Vec::new(),
        end: None,
    };
    // Bounded so a beam that a hand-made level routes into a loop — two portals and a
    // mirror-image corridor — can't walk forever. The original leans on a weaker
    // guard: it stops only if the beam re-enters the emitter's cell heading the same
    // way.
    for _ in 0..MAX_BEAM_CELLS {
        if stops(cell) {
            run.end = Some(cell);
            break;
        }
        run.cells.push(cell);

        let (dc, dr) = dir.delta();
        let next = (cell.0 + dc, cell.1 + dr);

        // Routed *before* the solidity test, because the cell a portal is mounted in
        // is solid — that is the whole point of it.
        if let Some(pair) = portals
            && let Some((exit_cell, exit_facing, entry_facing)) = portal_route(pair, next)
            && entry_facing == dir.opposite()
        {
            run.end = Some(next);
            segments.push(run);
            dir = exit_facing;
            let (dc, dr) = dir.delta();
            cell = (exit_cell.0 + dc, exit_cell.1 + dr);
            run = BeamSegment {
                dir,
                cells: Vec::new(),
                end: None,
            };
            continue;
        }

        cell = next;
    }
    segments.push(run);
    segments
}

/// The strip one cell of beam occupies, in world pixels: `[x, y, w, h]`.
///
/// A beam is thin and **not** centred in its cell: `y - 0.5625` with a height of 2/16
/// for a horizontal run (`laser.lua:99`), i.e. 7/16 down from the top of the cell.
/// A light bridge's slab sits at the same offset with height 1/8
/// (`lightbridge.lua:141-151`) — the extra 1/32 of a block is not worth two constants,
/// so both use the beam's 2/16 and a bridge is a hair thicker than the original's.
pub(crate) fn beam_rect(dir: Orientation, cell: (i32, i32)) -> [f32; 4] {
    const OFFSET: f32 = 7.0 / 16.0;
    const THICKNESS: f32 = 2.0 / 16.0;
    let (c, r) = (cell.0 as f32, cell.1 as f32);
    if dir.is_horizontal() {
        [
            c * TILE_SIZE,
            (r + OFFSET) * TILE_SIZE,
            TILE_SIZE,
            THICKNESS * TILE_SIZE,
        ]
    } else {
        [
            (c + OFFSET) * TILE_SIZE,
            r * TILE_SIZE,
            THICKNESS * TILE_SIZE,
            TILE_SIZE,
        ]
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
                EntityKind::LaserRight
                | EntityKind::LaserDetectorRight
                | EntityKind::LightBridgeRight => Some(Orientation::Right),
                EntityKind::LaserLeft
                | EntityKind::LaserDetectorLeft
                | EntityKind::LightBridgeLeft => Some(Orientation::Left),
                EntityKind::LaserUp | EntityKind::LaserDetectorUp | EntityKind::LightBridgeUp => {
                    Some(Orientation::Up)
                }
                EntityKind::LaserDown
                | EntityKind::LaserDetectorDown
                | EntityKind::LightBridgeDown => Some(Orientation::Down),
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
                // An emitter is on unless something is wired to control it:
                // `laser:link()` and `lightbridge:link()` set `enabled = false` only on
                // a *successful* match (`laser.lua:18-27`, `lightbridge.lua:15-27`).
                // Nothing in the shipped levels links either, so they all start on.
                on: kind.is_emitter(),
                timer: 0.0,
                allow_clear: true,
                pushed: false,
                lit: Vec::new(),
                beam: Vec::new(),
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
                // The `link()` side effect: an emitter that *is* controlled starts off.
                if element.kind.is_emitter() {
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
    fn step_lasers(&mut self) {
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

        // Phase 2: walk each live beam and light what it lands on.
        let lasers: Vec<usize> = (0..self.elements.len())
            .filter(|&i| self.elements[i].kind == LabKind::Laser && self.elements[i].on)
            .collect();
        for laser in lasers {
            let cells = self.elements[laser].probe_cells();
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

    /// Recompute every beam's geometry — the lasers and the light bridges alike.
    ///
    /// **Deviation, deliberate:** the original recomputes only on an event
    /// (`updaterange` is called from `input()`, from a door reaching either end of its
    /// travel, and from every portal change — `door.lua:51`, `mario.lua:2775`). This
    /// runs every frame and lets callers diff the result. The rules are identical; the
    /// cost is a few hundred cell tests per frame, and it removes a whole class of
    /// "forgot to invalidate" bug — the original itself needs `door:firstupdate` to
    /// paper over one such gap.
    pub(crate) fn step_beams(
        &mut self,
        level: &crate::world::Level,
        portals: Option<(PortalAnchor, PortalAnchor)>,
    ) {
        if !self.elements.iter().any(|e| e.kind.is_emitter()) {
            return;
        }
        // Sampled once: a beam is stopped by a shut door, and this is the same door
        // state every beam this frame sees.
        let shut = self.blocking_cells();
        for i in 0..self.elements.len() {
            let element = &self.elements[i];
            if !element.kind.is_emitter() {
                continue;
            }
            let beam = match (element.on, element.axis) {
                (true, Some(dir)) => march_beam(level, portals, &shut, element.cell, dir),
                _ => Vec::new(),
            };
            self.elements[i].beam = beam;
        }
    }

    /// The thin slabs every lit light bridge lays down, in world pixels.
    pub(crate) fn bridge_rects(&self) -> Vec<[f32; 4]> {
        let mut rects = Vec::new();
        for element in &self.elements {
            if element.kind != LabKind::LightBridge {
                continue;
            }
            for segment in &element.beam {
                for cell in &segment.cells {
                    rects.push(beam_rect(segment.dir, *cell));
                }
            }
        }
        rects
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
    /// Drive the lab network for one frame: sense the buttons, run the beams, animate
    /// the doors, and republish what now blocks movement.
    ///
    /// The order inside the frame is the original's, and two steps of it are not
    /// interchangeable: the beams are cut short by bodies **before** the detector latch
    /// runs (`laser:update` truncates and only then calls `updateoutputs`), which is
    /// what lets a body standing in a beam hold a detector off.
    pub(crate) fn update_lab(&mut self, ctx: &mut Context, dt: f32, use_pressed: bool) {
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

        // Beam geometry first, then what is standing in it, then the latch. Beams route
        // through the portals only as a *pair*, which `portal_pair` is what enforces —
        // a lone portal is not a hole for a beam either.
        let pair = self.portal_pair().map(|(a, b)| (a.anchor, b.anchor));
        self.lab.step_beams(&self.level, pair);
        self.block_beams(ctx);
        self.lab.step_lasers();
        self.lab.tick(dt);

        // Rebuilt wholesale each frame rather than diffed on transitions: there are a
        // handful of doors per level, and "always correct" beats "clever".
        self.level.solid_extras.clear();
        for cell in self.lab.blocking_cells() {
            self.level.solid_extras.insert(cell);
        }

        // Light-bridge slabs. A slab that wasn't there last frame shoves whatever is
        // standing in it out of the way, which is the original's `pushstuff` —
        // otherwise a bridge switching on around Mario would leave him inside it.
        let bridges = self.lab.bridge_rects();
        for rect in &bridges {
            if !self.level.solid_rects.contains(rect) {
                self.push_out_of(*rect);
            }
        }
        self.level.solid_rects = bridges;
    }

    /// Cut every laser beam at the first body standing in it, and hurt that body.
    ///
    /// The original's probe list is `{"player", "box", "goomba", "koopa"}`
    /// (`laser.lua:61`) and it picks the *nearest* hit along the run; walking the cells
    /// in travel order finds the same one. What happens next is per-object:
    /// `mario:laser` dies, `goomba:laser` and `koopa:laser` are `shotted()`
    /// (`mario.lua:2672`, `goomba.lua:284`, `koopa.lua:383`).
    ///
    /// **Not implemented:** the held-cube shield. `mario:laser` returns early when he
    /// is carrying a cube and pointing into the beam — there are no cubes yet, so
    /// there is nothing to shield him with.
    fn block_beams(&mut self, ctx: &mut Context) {
        let mut kill_player = false;
        let mut shot: Vec<usize> = Vec::new();

        for index in 0..self.lab.elements.len() {
            if self.lab.elements[index].kind != LabKind::Laser {
                continue;
            }
            let mut cut: Option<(usize, usize)> = None;
            'runs: for (s, segment) in self.lab.elements[index].beam.iter().enumerate() {
                for (c, cell) in segment.cells.iter().enumerate() {
                    let strip = beam_rect(segment.dir, *cell);
                    let hit_player = self.state == crate::game::GameState::Playing
                        && aabb_overlap(
                            [
                                self.player.x,
                                self.player.y,
                                self.player.width,
                                self.player.height,
                            ],
                            strip,
                        );
                    let hit_enemies: Vec<usize> = self
                        .enemies
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            e.state != crate::enemies::EnemyState::Dead
                                && aabb_overlap(
                                    [
                                        e.x,
                                        e.y,
                                        PLAYER_SMALL_W,
                                        enemy_height(e.enemy_type, e.state),
                                    ],
                                    strip,
                                )
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if !hit_player && hit_enemies.is_empty() {
                        continue;
                    }
                    kill_player |= hit_player;
                    shot.extend(hit_enemies);
                    cut = Some((s, c));
                    break 'runs;
                }
            }
            if let Some((s, c)) = cut {
                self.lab.elements[index].truncate_beam_at(s, c);
            }
        }

        for index in shot {
            let enemy = &mut self.enemies[index];
            enemy.state = crate::enemies::EnemyState::Dead;
            enemy.death_timer = 3.0;
            enemy.flipped_death = true;
            enemy.vy = -300.0;
        }
        if kill_player {
            self.die(ctx);
        }
    }

    /// Shove the player out of a slab that has just appeared around them.
    ///
    /// `lightbridgebody:pushstuff` (`lightbridge.lua:161`): the side chosen is the one
    /// the body is already moving towards, and if *that* side is blocked it goes out the
    /// other — so a bridge appearing in a corridor can't push you into a wall.
    fn push_out_of(&mut self, rect: [f32; 4]) {
        let body = [
            self.player.x,
            self.player.y,
            self.player.width,
            self.player.height,
        ];
        if !aabb_overlap(body, rect) {
            return;
        }
        let [rx, ry, rw, rh] = rect;
        if rw < rh {
            // A vertical slab pushes sideways, preferring the way the body is already
            // heading.
            let (near, far) = if self.player.vx >= 0.0 {
                (rx + rw, rx - self.player.width)
            } else {
                (rx - self.player.width, rx + rw)
            };
            self.player.x = if crate::physics::rect_is_clear(
                &self.level,
                near,
                self.player.y,
                self.player.width,
                self.player.height,
            ) {
                near
            } else {
                far
            };
        } else {
            let (near, far) = if self.player.vy <= 0.0 {
                (ry - self.player.height, ry + rh)
            } else {
                (ry + rh, ry - self.player.height)
            };
            self.player.y = if crate::physics::rect_is_clear(
                &self.level,
                self.player.x,
                near,
                self.player.width,
                self.player.height,
            ) {
                near
            } else {
                far
            };
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

    /// A solid, portal-accepting lab tile, for building walls in beam tests.
    const WALL: u32 = 140;

    /// A bare level with nothing solid in it, for beam tests.
    fn empty_level() -> crate::world::Level {
        crate::world::load_level("portal", "2-1")
    }

    /// One frame of the beam pipeline: geometry, then the detector latch. The game
    /// runs the two with the "cut the beam at whatever is standing in it" step
    /// between them, which needs a player and so lives on `Mari0Game`.
    fn sweep(lab: &mut Lab, level: &crate::world::Level) {
        lab.step_beams(level, None);
        lab.step_lasers();
    }

    /// The same, with a portal pair the beams can route through.
    fn sweep_with_portals(
        lab: &mut Lab,
        level: &crate::world::Level,
        pair: (PortalAnchor, PortalAnchor),
    ) {
        lab.step_beams(level, Some(pair));
        lab.step_lasers();
    }

    fn anchor(col: i32, row: i32, facing: Orientation) -> PortalAnchor {
        PortalAnchor {
            cell: (col, row),
            facing,
        }
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

        sweep(&mut lab, &level);
        assert!(lab.elements[detector].on, "beam should light the detector");
        assert!(
            !lab.elements[detector].allow_clear,
            "input() must clear the flag so the beam's own clear() no-ops"
        );

        // Still lit next frame: stays on.
        sweep(&mut lab, &level);
        assert!(lab.elements[detector].on, "still lit, still on");

        // Switch the laser off. The detector releases on the *next* sweep, because
        // `clear()` only takes effect once `allowclear` has been re-armed.
        lab.elements[laser].on = false;
        sweep(&mut lab, &level);
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

        sweep(&mut lab, &level);
        assert!(
            !lab.elements[door].on,
            "the push happens on the next sweep, not the one that lit the detector"
        );
        sweep(&mut lab, &level);
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
        sweep(&mut lab, &level);
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
        level.tiles[9][6] = WALL;
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
        ]);
        sweep(&mut lab, &level);
        assert!(
            !lab.elements[1].on,
            "a detector behind a wall should stay dark"
        );
    }

    /// **The case every shipped level is built on.** A detector is mounted *in* a
    /// wall, so the beam stops one cell short of it — and it still has to fire, which
    /// only works because the cell that terminated the run is probed too.
    #[test]
    fn a_detector_sunk_into_the_wall_it_is_mounted_in_still_fires() {
        let mut level = empty_level();
        level.tiles[9][8] = WALL;
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 8, 9, None),
        ]);
        sweep(&mut lab, &level);
        let beam = &lab.elements[0].beam;
        assert_eq!(beam.len(), 1, "no portals, so one straight run");
        assert_eq!(
            beam[0].cells,
            vec![(5, 9), (6, 9), (7, 9)],
            "the run covers the emitter's own cell and stops before the wall"
        );
        assert_eq!(beam[0].end, Some((8, 9)), "and remembers the wall it hit");
        assert!(
            lab.elements[1].on,
            "the detector in that wall must still be lit"
        );
    }

    /// A shut door stops a beam; opening it lets the beam through. This is the
    /// laser-and-door puzzle in 2-3.
    #[test]
    fn a_shut_door_stops_a_beam_and_an_open_one_does_not() {
        let level = empty_level();
        let mut lab = Lab::build(&[
            place(EntityKind::LaserRight, 5, 9, None),
            place(EntityKind::LaserDetectorLeft, 10, 9, None),
            // A vertical door spanning (7,8)-(7,9), across the beam's path.
            place(EntityKind::DoorVer, 7, 9, None),
        ]);
        sweep(&mut lab, &level);
        assert!(!lab.elements[1].on, "shut door stops the beam");

        lab.elements[2].on = true;
        for _ in 0..40 {
            lab.tick(1.0 / 60.0);
        }
        assert_eq!(lab.elements[2].timer, 1.0, "door is fully open");
        sweep(&mut lab, &level);
        sweep(&mut lab, &level);
        assert!(lab.elements[1].on, "an open door lets the beam past");
    }

    /// A beam entering a portal's front face comes out of the other mouth, pointing
    /// the way *that* mouth faces. This is what a bridge or a laser routed through the
    /// gun looks like.
    #[test]
    fn a_beam_bends_through_a_portal_pair() {
        let mut level = empty_level();
        // A wall for each portal to be mounted in, plus the wall the beam ends on.
        level.tiles[9][8] = WALL; // entry portal's backing
        level.tiles[5][20] = WALL; // exit portal's backing
        level.tiles[4][23] = WALL; // what the redirected beam stops against

        let mut lab = Lab::build(&[place(EntityKind::LaserRight, 5, 9, None)]);
        // Entry mounted on the left face of the wall at (8,9): it faces left, back
        // towards the laser. Exit faces up out of the wall at (20,5).
        let entry = anchor(8, 9, Orientation::Left);
        let exit = anchor(20, 5, Orientation::Up);
        sweep_with_portals(&mut lab, &level, (entry, exit));

        let beam = &lab.elements[0].beam;
        assert_eq!(beam.len(), 2, "one run into the portal, one out of it");
        assert_eq!(beam[0].dir, Orientation::Right);
        assert_eq!(beam[0].end, Some((8, 9)), "first run ends in the mouth");
        assert_eq!(
            beam[1].dir,
            Orientation::Up,
            "the second run points the way the exit mouth faces"
        );
        assert!(
            beam[1].cells.iter().all(|c| c.1 < 5),
            "and travels upward out of the exit: {:?}",
            beam[1].cells
        );
    }

    /// The beam is stopped by the wall a portal is mounted in when it arrives from
    /// *behind* — a portal is a hole one way only, as far as a beam is concerned.
    #[test]
    fn a_beam_hitting_a_portal_from_behind_is_stopped() {
        let mut level = empty_level();
        level.tiles[9][8] = WALL;
        level.tiles[5][20] = WALL;
        let mut lab = Lab::build(&[place(EntityKind::LaserRight, 5, 9, None)]);
        // Same wall, but the mouth faces *right* — away from the laser.
        let entry = anchor(8, 9, Orientation::Right);
        let exit = anchor(20, 5, Orientation::Up);
        sweep_with_portals(&mut lab, &level, (entry, exit));
        assert_eq!(lab.elements[0].beam.len(), 1, "no bend");
    }

    /// A light bridge lays one slab per cell it covers, starting in its own cell.
    #[test]
    fn a_light_bridge_lays_a_slab_in_every_cell_it_covers() {
        let mut level = empty_level();
        level.tiles[9][8] = WALL;
        let mut lab = Lab::build(&[place(EntityKind::LightBridgeRight, 5, 9, None)]);
        assert!(lab.elements[0].on, "an unlinked bridge is on");
        lab.step_beams(&level, None);
        let rects = lab.bridge_rects();
        assert_eq!(rects.len(), 3, "cells (5,9), (6,9), (7,9)");
        // Thin and lying flat, since the beam runs horizontally.
        for r in &rects {
            assert_eq!(r[2], TILE_SIZE, "a slab spans its cell");
            assert!(r[3] < TILE_SIZE / 4.0, "and is thin: {r:?}");
        }
        assert_eq!(rects[0][0], 5.0 * TILE_SIZE, "starting in its own cell");
    }

    /// A vertical bridge is a wall, not a floor: the slabs stand on edge.
    #[test]
    fn a_vertical_light_bridge_lays_slabs_on_edge() {
        let mut level = empty_level();
        level.tiles[3][6] = WALL;
        let mut lab = Lab::build(&[place(EntityKind::LightBridgeUp, 6, 9, None)]);
        lab.step_beams(&level, None);
        let rects = lab.bridge_rects();
        assert!(!rects.is_empty());
        for r in &rects {
            assert!(r[2] < TILE_SIZE / 4.0, "thin across");
            assert_eq!(r[3], TILE_SIZE, "full cell tall");
        }
    }

    /// A bridge that is switched off has no slabs at all — that is how a wired bridge
    /// stops being a floor.
    #[test]
    fn a_bridge_that_is_off_lays_nothing() {
        let level = empty_level();
        let mut lab = Lab::build(&[place(EntityKind::LightBridgeRight, 5, 9, None)]);
        lab.elements[0].on = false;
        lab.step_beams(&level, None);
        assert!(lab.bridge_rects().is_empty());
    }

    /// The shipped levels, end to end: every laser that is aimed at a detector must
    /// actually light it on load.
    ///
    /// This is the test that would have caught the beam stopping one cell short. Both
    /// of these levels wire a door to the detector, so a dark detector means a door
    /// that never opens and a level that can't be finished.
    #[test]
    fn shipped_lasers_light_the_detectors_they_are_aimed_at() {
        for name in ["1-2", "2-4"] {
            let level = crate::world::load_level("portal", name);
            let parsed =
                level::Level::parse(level::raw_level("portal", name).unwrap()).expect("parses");
            let mut lab = Lab::build(&parsed.markers.lab);
            // Two sweeps: the first lights the detector, the second is when it pushes
            // downstream.
            sweep(&mut lab, &level);
            sweep(&mut lab, &level);
            let lit = lab
                .elements
                .iter()
                .filter(|e| e.kind == LabKind::LaserDetector && e.on)
                .count();
            assert!(lit > 0, "portal/{name}: no detector is lit by its laser");
        }
    }

    /// No shipped beam may run away: each one has to end on something.
    #[test]
    fn every_shipped_beam_terminates() {
        for (pack, name, raw) in level::LEVELS {
            if *pack != *"portal" {
                continue;
            }
            let level = crate::world::load_level(pack, name);
            let parsed = level::Level::parse(raw).expect("parses");
            let mut lab = Lab::build(&parsed.markers.lab);
            lab.step_beams(&level, None);
            for element in lab.elements.iter().filter(|e| e.kind.is_emitter()) {
                let total: usize = element.beam.iter().map(|s| s.cells.len()).sum();
                assert!(
                    total < MAX_BEAM_CELLS,
                    "{pack}/{name}: beam at {:?} hit the length cap",
                    element.cell
                );
                assert!(
                    element.beam.last().and_then(|s| s.end).is_some(),
                    "{pack}/{name}: beam at {:?} ends nowhere",
                    element.cell
                );
            }
        }
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
