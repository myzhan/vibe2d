//! Drawing the lab: buttons, doors, indicators, laser beams and light bridges.
//!
//! ## Reading the original's coordinates
//!
//! Every one of these sprites is placed by hand in `love.graphics.draw` calls full of
//! sixteenths, and the conversion is not obvious, so it is worth stating once.
//!
//! The original draws a tile of 1-based row `y` at pixel `(y-1)*16 - 8`, and objects
//! at `by*16 - offsetY` with the origin usually at the image's centre `(8, 8)`. So for
//! a draw at love position `(Lx, Ly)` with origin `(ox, oy)`, the sprite's top-left in
//! **block** coordinates is
//!
//! ```text
//! bx = (Lx - ox) / 16          by = (Ly - oy + 8) / 16
//! ```
//!
//! — note the `+8` on `y` only. Miss it and everything lab-related sits half a tile
//! off. Block coordinates then carry straight over to this port (a 0-based cell `c`
//! spans blocks `[c, c+1]`, the same numbers the original's 1-based cell `c+1` spans),
//! and one block is `TILE_SIZE`.
//!
//! ## Where this deliberately differs
//!
//! The original rotates about odd pivots — `(8, 1)` for a vertical beam, `(4, 0)` for a
//! door panel — because Löve lets it pick any origin. This engine rotates a sprite
//! about its own centre, so rotated sprites here are positioned by *where they should
//! end up* rather than by porting the pivot arithmetic. For the beams that lands them
//! exactly on the collision strip's centre line, which is arguably better than the
//! original's 3/16-block lean.

use vibe2d::prelude::*;

use crate::atlas::*;
use crate::constants::*;
use crate::game::Mari0Game;
use crate::lab::{LabKind, beam_rect, button_plate_rect};
use crate::player::Orientation;

/// Quarter turns for a wall fixture facing each way, matching `laser:draw`'s `rot`.
fn facing_rotation(dir: Orientation) -> f32 {
    use std::f32::consts::PI;
    match dir {
        Orientation::Right => 0.0,
        Orientation::Down => PI * 0.5,
        Orientation::Left => PI,
        Orientation::Up => PI * 1.5,
    }
}

/// A ground light's colour: orange when energised, blue when not
/// (`groundlight.lua:36-39`).
fn indicator_colour(lit: bool) -> Color {
    let (r, g, b) = if lit {
        (255.0, 122.0, 66.0)
    } else {
        (60.0, 188.0, 252.0)
    };
    Color {
        r: srgb_to_linear(r / 255.0),
        g: srgb_to_linear(g / 255.0),
        b: srgb_to_linear(b / 255.0),
        a: 1.0,
    }
}

impl Mari0Game {
    /// Draw the whole lab, back to front: wall fixtures, then doors, then the beams.
    ///
    /// Beams last because they read as light: a laser crossing in front of a door is
    /// how you see that it is a laser and not a rail.
    pub(crate) fn draw_lab(&self, screen: &mut Screen) {
        if self.lab.elements.is_empty() {
            return;
        }
        let cam_x = self.camera.x;
        // One tile of slack each side so a sprite half off-screen still draws.
        let visible = |c: i32| {
            let x = c as f32 * TILE_SIZE - cam_x;
            x > -2.0 * TILE_SIZE && x < self.vw + TILE_SIZE
        };

        for element in &self.lab.elements {
            let (c, r) = element.cell;
            if !visible(c) {
                continue;
            }
            let cell = [
                c as f32 * TILE_SIZE - cam_x,
                r as f32 * TILE_SIZE,
                TILE_SIZE,
                TILE_SIZE,
            ];
            match element.kind {
                LabKind::GroundLight => {
                    // Six variants of pipework, and the entity id is what says which:
                    // ids 43..48 in `entities.png`, tinted by state.
                    let id = element.entity.id() as u32;
                    let (col, row) = ((id - 1) % 10, (id - 1) / 10);
                    screen.draw_sprite_region_tinted(
                        self.tex_entities,
                        entity_uv(col, row),
                        cell,
                        indicator_colour(element.on),
                    );
                }
                LabKind::WallIndicator => {
                    // 32×16 sheet, two 16×16 frames: dark, then lit.
                    let frame = if element.on { 1.0 } else { 0.0 };
                    screen.draw_sprite_region(
                        self.tex_wall_indicator,
                        [frame * 0.5, 0.0, 0.5, 1.0],
                        cell,
                    );
                }
                LabKind::Button => {
                    // The cap sinks 1px (2 at this scale) while held
                    // (`button.lua:42-49`), which is the only feedback a floor button
                    // gives.
                    let plate = button_plate_rect((c, r));
                    let pressed = if element.on { 2.0 } else { 0.0 };
                    screen.draw_sprite(
                        self.tex_button_cap,
                        plate[0] - cam_x + 10.0,
                        r as f32 * TILE_SIZE + 22.0 + pressed,
                        40.0,
                        4.0,
                    );
                    screen.draw_sprite(
                        self.tex_button_base,
                        c as f32 * TILE_SIZE - cam_x,
                        r as f32 * TILE_SIZE + 26.0,
                        64.0,
                        6.0,
                    );
                }
                LabKind::PushButton => {
                    // Held down for the whole cooldown, which is what `timer` counts.
                    let frame = if element.timer > 0.0 { 1.0 } else { 0.0 };
                    let src = [frame * 0.5, 0.0, 0.5, 1.0];
                    if element.axis == Some(Orientation::Right) {
                        screen.draw_sprite_region_flipped(
                            self.tex_push_button,
                            src,
                            cell,
                            true,
                            false,
                        );
                    } else {
                        screen.draw_sprite_region(self.tex_push_button, src, cell);
                    }
                }
                LabKind::LaserDetector => {
                    let rot = facing_rotation(element.axis.unwrap_or(Orientation::Right));
                    screen.rotated(rot, |screen| {
                        screen.draw_sprite(
                            self.tex_laser_detector,
                            cell[0],
                            cell[1],
                            TILE_SIZE,
                            TILE_SIZE,
                        );
                    });
                }
                LabKind::Door => self.draw_door(screen, element, cam_x),
                _ => {}
            }
        }

        // Beams and their emitters, on top.
        for element in &self.lab.elements {
            if !element.kind.is_emitter() {
                continue;
            }
            let (beam_tex, side_tex) = match element.kind {
                LabKind::Laser => (self.tex_laser, self.tex_laser_side),
                _ => (self.tex_light_bridge, self.tex_light_bridge_side),
            };
            for segment in &element.beam {
                // Rotating the 32×16 strip a quarter turn about its centre — the cell
                // centre — gives the 16×32 upright strip, on the same centre line the
                // collision strip uses.
                let rot = if segment.dir.is_horizontal() {
                    0.0
                } else {
                    std::f32::consts::FRAC_PI_2
                };
                screen.rotated(rot, |screen| {
                    for cell in &segment.cells {
                        if !visible(cell.0) {
                            continue;
                        }
                        // The slab's rect *is* the sprite's rect for a horizontal run;
                        // for a vertical one the same rect rotates into place.
                        let [x, y, ..] = beam_rect(Orientation::Right, *cell);
                        screen.draw_sprite(
                            beam_tex,
                            x - cam_x,
                            y - THICKNESS_PAD,
                            TILE_SIZE,
                            TILE_SIZE / 2.0,
                        );
                    }
                });
            }
            let (c, r) = element.cell;
            if visible(c) {
                let rot = facing_rotation(element.axis.unwrap_or(Orientation::Right));
                screen.rotated(rot, |screen| {
                    screen.draw_sprite(
                        side_tex,
                        c as f32 * TILE_SIZE - cam_x,
                        r as f32 * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    );
                });
            }
        }
    }

    /// One door: two panels retracting into the frame, with the little hinge pieces
    /// folding as they go.
    ///
    /// The animation is two phases, not one (`door.lua:84-90`): for the first half of
    /// the timer the hinge pieces rotate a quarter turn and nothing moves; for the
    /// second half the panels slide apart by up to a whole block. Everything is clipped
    /// to the door's own two cells, so a panel on its way out disappears into the frame
    /// rather than poking through the wall.
    fn draw_door(&self, screen: &mut Screen, element: &crate::lab::LabElement, cam_x: f32) {
        use std::f32::consts::PI;
        let Some(cells) = element.door_cells() else {
            return;
        };
        let (slide, fold) = if element.timer > 0.5 {
            ((element.timer - 0.5) * 2.0 * TILE_SIZE, PI / 2.0)
        } else {
            (0.0, element.timer * PI)
        };

        // The span, in screen pixels, and the line the two panels part along.
        let x0 = cells[0].0 as f32 * TILE_SIZE - cam_x;
        let y0 = cells[0].1 as f32 * TILE_SIZE;
        let vertical = element.axis == Some(Orientation::Up);
        let (span_w, span_h) = if vertical {
            (TILE_SIZE, TILE_SIZE * 2.0)
        } else {
            (TILE_SIZE * 2.0, TILE_SIZE)
        };
        let mid_x = x0 + span_w / 2.0;
        let mid_y = y0 + span_h / 2.0;

        // Panel: 8×14 source, so 16×28 here — a half-block wide, most of a block long.
        const PANEL_W: f32 = 16.0;
        const PANEL_L: f32 = 28.0;
        const HINGE: f32 = 8.0;

        screen.clipped(x0, y0, span_w, span_h, |screen| {
            if vertical {
                for (sign, panel_rot) in [(-1.0_f32, PI), (1.0, 0.0)] {
                    let top = if sign < 0.0 {
                        mid_y - PANEL_L - slide
                    } else {
                        mid_y + slide
                    };
                    screen.rotated(panel_rot, |screen| {
                        screen.draw_sprite(
                            self.tex_door_piece,
                            mid_x - PANEL_W / 2.0,
                            top,
                            PANEL_W,
                            PANEL_L,
                        );
                    });
                    let hinge_y = if sign < 0.0 {
                        mid_y - HINGE - slide
                    } else {
                        mid_y + slide
                    };
                    screen.rotated(fold + panel_rot, |screen| {
                        screen.draw_sprite(
                            self.tex_door_centre,
                            mid_x - HINGE / 2.0,
                            hinge_y,
                            HINGE,
                            HINGE,
                        );
                    });
                }
            } else {
                // Same door turned on its side: the panel art rotates a quarter turn,
                // which with centre-rotation means placing the 16×28 rect on the centre
                // the 28×16 result should have.
                for (sign, panel_rot) in [(-1.0_f32, PI * 1.5), (1.0, PI * 0.5)] {
                    let centre_x = if sign < 0.0 {
                        mid_x - PANEL_L / 2.0 - slide
                    } else {
                        mid_x + PANEL_L / 2.0 + slide
                    };
                    screen.rotated(panel_rot, |screen| {
                        screen.draw_sprite(
                            self.tex_door_piece,
                            centre_x - PANEL_W / 2.0,
                            mid_y - PANEL_L / 2.0,
                            PANEL_W,
                            PANEL_L,
                        );
                    });
                    let hinge_x = if sign < 0.0 {
                        mid_x - HINGE - slide
                    } else {
                        mid_x + slide
                    };
                    screen.rotated(fold + panel_rot, |screen| {
                        screen.draw_sprite(
                            self.tex_door_centre,
                            hinge_x,
                            mid_y - HINGE / 2.0,
                            HINGE,
                            HINGE,
                        );
                    });
                }
            }
        });
    }
}

/// Half the difference between the beam sprite's height and the collision strip's.
///
/// The beam is drawn 8 source pixels tall (16 here) but only collides over 2/16 of a
/// block, both centred on the same line — so the sprite starts this much above the
/// strip. `laser.lua:99` versus `laser.lua:277`.
const THICKNESS_PAD: f32 = (16.0 - 4.0) / 2.0;
