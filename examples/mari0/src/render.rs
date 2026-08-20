//! Everything the frame draws, in one pass over the world.

use vibe2d::prelude::*;

use crate::atlas::*;
use crate::constants::*;
use crate::enemies::*;
use crate::game::{GameState, Mari0Game};
use crate::items::*;
use crate::physics::*;
use crate::player::*;

/// Lakitu's cloud is taller than he is.
///
/// His hitbox is one small-Mario square like a goomba's, but the sprite is 16x24 —
/// the extra half-block is cloud, and it belongs *above* him, so the two rectangles
/// share a bottom edge rather than a top one.
fn lakito_dst(hitbox: [f32; 4]) -> [f32; 4] {
    const SPRITE_H: f32 = 24.0 * 2.0;
    [
        hitbox[0],
        hitbox[1] + hitbox[3] - SPRITE_H,
        hitbox[2],
        SPRITE_H,
    ]
}

/// Bowser's sprite is a 32x32 cell for a 30x28 hitbox — so nearly the same, but the
/// two share a bottom edge like everything else that overhangs.
fn bowser_dst(hitbox: [f32; 4]) -> [f32; 4] {
    const SPRITE: f32 = 32.0 * 2.0;
    [hitbox[0], hitbox[1] + hitbox[3] - SPRITE, SPRITE, SPRITE]
}

/// A hammer bro is more than twice as tall as his hitbox.
///
/// The cell is 16x34 for a 12/16-block box, and the extra is all head and hammer —
/// so like lakitu's cloud the two rectangles share a bottom edge, not a top one.
fn hammer_bro_dst(hitbox: [f32; 4]) -> [f32; 4] {
    const SPRITE_H: f32 = 34.0 * 2.0;
    [
        hitbox[0],
        hitbox[1] + hitbox[3] - SPRITE_H,
        hitbox[2],
        SPRITE_H,
    ]
}

/// Mario's three tinted layers, in draw order: shirt, overalls, skin.
///
/// Layer 0 is the outline and is drawn untinted on top. Fire Mario swaps the shirt for
/// white and the overalls for red; everything else is the same sheet.
fn mario_palette(fire: bool) -> [Color; 3] {
    let rgb = |r: f32, g: f32, b: f32| Color {
        r: srgb_to_linear(r / 255.0),
        g: srgb_to_linear(g / 255.0),
        b: srgb_to_linear(b / 255.0),
        a: 1.0,
    };
    if fire {
        [
            Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            rgb(224.0, 32.0, 0.0),
            rgb(252.0, 152.0, 56.0),
        ]
    } else {
        [
            rgb(224.0, 32.0, 0.0),
            rgb(136.0, 112.0, 0.0),
            rgb(252.0, 152.0, 56.0),
        ]
    }
}

impl Mari0Game {
    /// The window a piranha plant is drawn through, in screen space.
    ///
    /// `customscissor = {x-1, y-2, 2, 2}` (`plant.lua:33`) — two blocks square,
    /// spanning the pipe's own two columns and ending **exactly at the pipe's rim**.
    /// That last part is the whole trick: a retracted plant's sprite starts at the
    /// rim, so the window clips all of it and the plant is invisible until it rises.
    /// Without this a plant is on screen the entire time, snapping away inside a
    /// pipe you can see through.
    ///
    /// Derived from the plant's rest position rather than stored, so it stays put
    /// while the plant slides.
    fn plant_clip_rect(&self, enemy: &Enemy, cam_x: f32) -> [f32; 4] {
        // Back out the cell from the rest height, then the window is anchored on it.
        let cell_top = enemy.spawn_y - PLANT_REST_DROP;
        [
            enemy.x - TILE_SIZE / 2.0 - cam_x,
            cell_top - TILE_SIZE,
            TILE_SIZE * 2.0,
            TILE_SIZE * 2.0,
        ]
    }

    /// Draw every platform, one block-wide segment at a time.
    ///
    /// `platform.png` is a single 16x8 tile repeated across the width — there is no
    /// wide sprite (`platform.lua:146-157`). A fractional width draws `floor(size)`
    /// segments and then **one more at the right edge**, overlapping the last, which is
    /// how a 1.5-block platform gets a solid-looking right end.
    fn draw_platforms(&self, screen: &mut Screen, cam_x: f32) {
        for p in &self.platforms {
            let tex = if p.kind == crate::platform::PlatformKind::Bonus {
                self.tex_platform_bonus
            } else {
                self.tex_platform
            };
            let whole = (p.w / TILE_SIZE).floor() as i32;
            for i in 0..whole {
                let x = p.x + i as f32 * TILE_SIZE - cam_x;
                screen.draw_sprite(tex, x, p.y, TILE_SIZE, PLATFORM_HEIGHT);
            }
            if p.w % TILE_SIZE != 0.0 {
                let x = p.x + p.w - TILE_SIZE - cam_x;
                screen.draw_sprite(tex, x, p.y, TILE_SIZE, PLATFORM_HEIGHT);
            }
        }
    }

    /// Draw every seesaw: two pulleys, the beam between them, the two ropes, and the
    /// platforms hanging on the ends.
    ///
    /// The ropes are the interesting part, and they need the original's scissor. Rope is
    /// drawn as whole 16px segments stacked downward, so a platform hanging 3.4 blocks
    /// down needs 4 of them — one too many. `seesaw:draw` clips the column to exactly
    /// how far the platform has dropped, which is what makes the rope end *at* the
    /// platform instead of poking through it.
    ///
    /// Once the rig has given, the side that dropped away draws its rope at full length
    /// instead: it is no longer holding anything, so there is nothing to clip it to.
    fn draw_seesaws(&self, screen: &mut Screen, cam_x: f32) {
        for s in &self.seesaws {
            // The beam and pulleys sit half a block above the placement row
            // (`self.y - 1.5` against an entity at `self.y - 1`).
            let beam_y = (s.row as f32 - 0.5) * TILE_SIZE;
            let left_x = s.col as f32 * TILE_SIZE - cam_x;
            let right_x = left_x + s.range * TILE_SIZE;
            let cell = |x: f32, y: f32| [x, y, TILE_SIZE, TILE_SIZE];

            screen.draw_sprite_region(self.tex_seesaw, seesaw_uv(0), cell(left_x, beam_y));
            // `range - 1` middle pieces: the two pulleys already cover the ends.
            for i in 1..s.range as i32 {
                screen.draw_sprite_region(
                    self.tex_seesaw,
                    seesaw_uv(3),
                    cell(left_x + i as f32 * TILE_SIZE, beam_y),
                );
            }
            screen.draw_sprite_region(self.tex_seesaw, seesaw_uv(1), cell(right_x, beam_y));

            for (side, x) in [
                (crate::seesaw::SeesawSide::Left, left_x),
                (crate::seesaw::SeesawSide::Right, right_x),
            ] {
                let dropped = s.drop_of(side);
                let full = s.falloff == Some(side);
                if !full && dropped < 0.0 {
                    continue;
                }
                // The clip starts a block below the beam, where the first rope segment
                // does, and runs as far as the platform has fallen.
                let length = if full {
                    (s.dist1 + s.dist2 - 2.0) * TILE_SIZE
                } else {
                    dropped
                };
                let segments = (length / TILE_SIZE).ceil() as i32;
                let draw_rope = |screen: &mut Screen| {
                    for i in 1..=segments {
                        screen.draw_sprite_region(
                            self.tex_seesaw,
                            seesaw_uv(2),
                            cell(x, beam_y + i as f32 * TILE_SIZE),
                        );
                    }
                };
                if full {
                    draw_rope(screen);
                } else {
                    screen.clipped(x, beam_y + TILE_SIZE, TILE_SIZE, length, draw_rope);
                }
            }

            for p in [&s.left, &s.right] {
                if p.gone {
                    continue;
                }
                // Same art and the same segment-per-block rule as a moving platform,
                // including the extra piece at the right edge of a 1.5-wide one.
                let whole = (p.w / TILE_SIZE).floor() as i32;
                for i in 0..whole {
                    screen.draw_sprite(
                        self.tex_platform,
                        p.x + i as f32 * TILE_SIZE - cam_x,
                        p.y,
                        TILE_SIZE,
                        PLATFORM_HEIGHT,
                    );
                }
                if p.w % TILE_SIZE != 0.0 {
                    screen.draw_sprite(
                        self.tex_platform,
                        p.x + p.w - TILE_SIZE - cam_x,
                        p.y,
                        TILE_SIZE,
                        PLATFORM_HEIGHT,
                    );
                }
            }
        }
    }

    /// Draw every vine: one curled tip with a column of stem stacked below it.
    ///
    /// The scissor is the reason this is its own function. A vine's body starts *inside*
    /// the brick it came from, so `vine:draw` wraps the whole thing in
    /// `setScissor(0, 0, width, (coy - 1.5) * 16)` — half a block above the brick's top
    /// face. Without it the tip sits visibly on top of a brick it has not emerged from,
    /// and the first half block of stem draws over the brick's own art.
    ///
    /// The sprite is offset a quarter block left of the collision box and 5/8 of one up
    /// (`x - 1/16 - (1-width)/2`, `y - 0.5 - 2/16`), which is why the stem you see is
    /// wider than the stem you can hold.
    fn draw_vines(&self, screen: &mut Screen, cam_x: f32) {
        for v in &self.vines {
            let sx = v.x - 0.25 * TILE_SIZE - cam_x;
            let tip_y = v.y - 0.625 * TILE_SIZE;
            let clip_h = v.clip_bottom().max(0.0);
            if clip_h <= 0.0 {
                continue;
            }
            screen.clipped(0.0, 0.0, self.vw, clip_h, |screen| {
                screen.draw_sprite_region(
                    self.tex_vine,
                    vine_uv(self.level.spriteset, false),
                    [sx, tip_y, TILE_SIZE, TILE_SIZE],
                );
                for i in 1..=v.stem_count() {
                    screen.draw_sprite_region(
                        self.tex_vine,
                        vine_uv(self.level.spriteset, true),
                        [sx, tip_y + i as f32 * TILE_SIZE, TILE_SIZE, TILE_SIZE],
                    );
                }
            });
        }
    }

    /// The spinning coin icon beside the counter.
    ///
    /// Its sheet is 5x8 cells, not 16x16 — the HUD is the one place
    /// `coinanimation.png` belongs, and using it for world coins is what drew six
    /// little rings where a single coin should be.
    fn draw_coin_icon(&self, screen: &mut Screen) {
        const ICON_W: f32 = 5.0 * 2.0;
        const ICON_H: f32 = 8.0 * 2.0;
        screen.draw_sprite_region(
            self.tex_coin_anim,
            coin_hud_uv(coin_spin_frame(self.coin_spin)),
            [168.0, 20.0, ICON_W, ICON_H],
        );
    }

    /// Draw one tile, from whichever of the two sheets owns its id.
    ///
    /// The lab tiles are ids 133..220 on a second sheet. Sending those to the SMB
    /// sheet's UV maths samples off the bottom edge, which is what made every lab
    /// level render as garbage.
    pub(crate) fn draw_smb_tile(&self, screen: &mut Screen, tile_id: u32, x: f32, y: f32) {
        let dst = [x, y, TILE_SIZE, TILE_SIZE];
        if tile_id >= FIRST_PORTAL_TILE {
            screen.draw_sprite_region(self.tex_portal_tiles, portal_tile_uv(tile_id), dst);
        } else {
            screen.draw_sprite_region(self.tex_tiles, smb_tile_uv(tile_id), dst);
        }
    }

    /// Draw the whole frame.
    ///
    /// Split in two because a black card needs the HUD without the level behind it: the
    /// "world 1-1" screen would otherwise announce a level you can already see.
    pub(crate) fn draw_world(&self, ctx: &Context, screen: &mut Screen) {
        // The launch intro replaces everything, HUD included.
        if let Some(intro) = self.intro {
            self.draw_intro(screen, &intro);
            return;
        }
        if self.state != GameState::Interlude {
            self.draw_level(ctx, screen);
        }
        self.draw_hud(ctx, screen);
    }

    /// Stabyourself's logo, stabbed.
    ///
    /// The blood is the same image again, revealed through a scissor that grows *upward*
    /// from the logo's bottom edge — a cross-fade would read as the picture changing, and
    /// this reads as blood running up it.
    fn draw_intro(&self, screen: &mut Screen, intro: &crate::interlude::Intro) {
        let alpha = intro.alpha();
        if alpha <= 0.0 {
            return;
        }
        let tint = Color { r: 1.0, g: 1.0, b: 1.0, a: alpha };
        // The original draws with an origin inside the sheet rather than from a corner,
        // and at half scale on a small window; centring on the virtual screen is the same
        // placement expressed in this port's fixed resolution.
        const SHEET: f32 = 512.0;
        let (ox, oy) = INTRO_LOGO_ORIGIN;
        let x = self.vw / 2.0 - ox;
        let y = self.vh / 2.0 - oy;
        screen.draw_sprite_tinted(self.tex_logo, x, y, SHEET, SHEET, tint);
        let wipe = intro.blood_wipe();
        if wipe > 0.0 {
            // Scissor anchored on the logo's bottom edge, opening upward.
            let bottom = y + oy + INTRO_LOGO_ORIGIN.1;
            let top = (bottom - wipe).max(0.0);
            let height = (bottom - top).min(self.vh);
            screen.clipped(0.0, top, self.vw, height, |screen| {
                screen.draw_sprite_tinted(self.tex_logo_blood, x, y, SHEET, SHEET, tint);
            });
        }
    }

    /// The level itself: tiles, actors, effects.
    fn draw_level(&self, ctx: &Context, screen: &mut Screen) {
        let cam_x = self.camera.x;

        // Portal tint colors (convert sRGB → linear for GPU)
        let portal_blue = Color {
            r: srgb_to_linear(0.3),
            g: srgb_to_linear(0.6),
            b: 1.0,
            a: 1.0,
        };
        let portal_orange = Color {
            r: 1.0,
            g: srgb_to_linear(0.5),
            b: srgb_to_linear(0.0),
            a: 1.0,
        };
        let portal_colors = [portal_blue, portal_orange];

        // ── Emerging items (drawn BEHIND tiles so they appear from under blocks) ──
        for item in &self.items {
            if !item.emerging {
                continue;
            }
            let ix = item.x - cam_x;
            let iy = item.y;
            let dst = [ix, iy, TILE_SIZE, TILE_SIZE];
            match item.item_type {
                ItemType::Mushroom => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(1, 0), dst);
                }
                ItemType::OneUp => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(2, 0), dst);
                }
                ItemType::Star => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_star, star_frame_uv(frame), dst);
                }
                ItemType::FireFlower => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_flower, flower_frame_uv(frame), dst);
                }
            }
        }

        // ── Tiles (all non-empty cells from original mari0 data) ──
        let start_col = (cam_x / TILE_SIZE).floor() as i32;
        let end_col = ((cam_x + self.vw) / TILE_SIZE).ceil() as i32 + 1;
        for row in 0..self.level.height as i32 {
            for col in start_col..end_col.min(self.level.width as i32) {
                if col < 0 {
                    continue;
                }
                let tile_id = get_tile(&self.level, col, row);
                if tile_id != SMB_EMPTY && tile_id != SMB_HIDDEN_BLOCK {
                    let x = col as f32 * TILE_SIZE - cam_x;
                    let mut y = row as f32 * TILE_SIZE;
                    // Block bounce offset
                    for bounce in &self.block_bounces {
                        if bounce.col == col && bounce.row == row {
                            let t = bounce.timer / BLOCK_BOUNCE_TIME;
                            // sin curve: up then back down
                            y -= (t * std::f32::consts::PI).sin() * BLOCK_BOUNCE_HEIGHT;
                        }
                    }
                    self.draw_smb_tile(screen, tile_id, x, y);
                    // Gel goes on over the tile it coats.
                    self.draw_gel_paint(screen, (col, row), x, y);
                }
            }
        }

        // ── Springs ──
        // Drawn from the *bottom up*: the body compresses, so its top edge is what
        // moves and its base stays on the cell.
        for s in &self.springs {
            let [x, y, w, h] = s.rect();
            screen.draw_sprite_region(self.tex_spring, spring_uv(s.frame()), [x - cam_x, y, w, h]);
        }

        // ── Bubbles ──
        // Behind the actors, like every other bit of water decoration. `bubble.png` is
        // 4x4, drawn at 2x with its own centre as the origin (`bubble.lua:31`).
        for b in &self.bubbles {
            const S: f32 = 4.0 * 2.0;
            screen.draw_sprite(self.tex_bubble, b.x - cam_x - S / 2.0, b.y - S / 2.0, S, S);
        }

        // ── Vines ──
        // After the tiles and before the actors, which is the original's order too
        // (`game.lua:1061` against the object sweep at `:1228`) — so Mario hangs in
        // front of the stem he is holding.
        self.draw_vines(screen, cam_x);

        // ── Moving platforms ──
        // Before the lab and the actors: they're scenery you stand on, and Mario has
        // to draw in front of one he is riding.
        self.draw_platforms(screen, cam_x);
        self.draw_seesaws(screen, cam_x);

        // ── The lab: buttons, doors, indicators, beams and bridges ──
        // After the tiles, since every one of them is mounted on a wall, and before the
        // actors so Mario walks in front of a beam rather than behind it.
        self.draw_lab(screen);

        // ── The warp-zone text ──
        // Revealed by the camera reaching the right edge. Drawn in world space so it
        // stays over the pipes, and each pipe gets its destination world printed three
        // blocks above it (`game.lua:1067-1073`).
        if self.warp_text
            && let Some(font) = ctx.assets.font("ui")
        {
            let text_x = (self.level.width as f32 - 14.0 - 1.0 / 16.0) * TILE_SIZE - cam_x;
            screen.draw_text(font, "welcome to warp zone!", text_x, 88.0 / 16.0 * TILE_SIZE);
            for ((col, row), world) in &self.level.warp_pipes {
                screen.draw_text(
                    font,
                    &format!("{world}"),
                    (*col as f32 - 9.0 / 16.0) * TILE_SIZE - cam_x,
                    (*row as f32 - 3.0) * TILE_SIZE,
                );
            }
        }

        // ── The flag on the pole ──
        // It sits at the top until the pole is grabbed, then comes down with Mario over
        // the same span in the same time — which is what makes it read as him pulling it.
        if self.level.flag_x > 0.0 {
            let fx = self.level.flag_x - cam_x;
            if fx > -TILE_SIZE && fx < self.vw + TILE_SIZE {
                let fy = match &self.flag {
                    Some(f) => f.flag_y - 0.5 * TILE_SIZE,
                    None => FLAG_IMG_START - 0.5 * TILE_SIZE,
                };
                screen.draw_sprite(self.tex_flag, fx - TILE_SIZE, fy, TILE_SIZE, TILE_SIZE);
            }
        }

        // ── The castle's flag, and the fireworks ──
        // Both only exist during the ending. The castle flag rises the last block and a
        // half into place once the clock has been cashed in (`game.lua:939`).
        if let Some(f) = &self.flag {
            let castle_x = self.level.flag_x + FLAG_CASTLE_DIST - cam_x;
            screen.draw_sprite(
                self.tex_castle_flag,
                castle_x,
                106.0 / 16.0 * TILE_SIZE + f.castle_flag_y,
                TILE_SIZE,
                TILE_SIZE,
            );
        }
        for fw in &self.fireworks_shown {
            // Centred on its own position, and drawn from the fireball sheet's explosion
            // frames — `fireworkboom` has no art of its own.
            screen.draw_sprite_region(
                self.tex_fireball,
                fireball_explode_uv(fw.frame()),
                [
                    fw.x - cam_x - TILE_SIZE / 2.0,
                    fw.y - TILE_SIZE / 2.0,
                    TILE_SIZE,
                    TILE_SIZE,
                ],
            );
        }

        // ── Coins ──
        // One counter for every coin on screen, so they spin in unison as they do in
        // the original. Driven off the clock rather than a frame counter because the
        // clock is what the original's own counter is stepped by.
        let coin_src = coin_uv(coin_spin_frame(self.coin_spin));
        for coin in &self.level.coins {
            if !coin.collected {
                let x = coin.x - cam_x;
                screen.draw_sprite_region(self.tex_coin, coin_src, [x, coin.y, 32.0, 32.0]);
            }
        }

        // ── Portals (animated, matches original mari0 portal.png layout) ──
        for (i, portal_opt) in self.portals.iter().enumerate() {
            if let Some(portal) = portal_opt {
                if !portal.active {
                    continue;
                }
                let (mouth_x, mouth_y) = portal.centre();
                let px = mouth_x - cam_x;
                let py = mouth_y;
                let color = portal_colors[i];
                let scale = portal.open_scale;
                if scale <= 0.0 {
                    continue;
                }

                // Animation frame (0..5 maps to original 1-indexed frames 1..6)
                let frame_y = (self.portal_anim_frame + 1) as f32; // y offset in strip units

                match portal.orientation() {
                    Orientation::Left | Orientation::Right => {
                        // Vertical portal: use portal_v.png (32×64, pre-rotated)
                        // UV: x = (frame+1)*4/32, y = portal_idx*0.5, w = 4/32, h = 0.5
                        let src = [frame_y * (4.0 / 32.0), i as f32 * 0.5, 4.0 / 32.0, 0.5];
                        let h = 64.0 * scale;
                        let dst = [px - 4.0, py - h / 2.0, 8.0, h];
                        screen.draw_sprite_region_tinted(self.tex_portal_v, src, dst, color);
                    }
                    Orientation::Up | Orientation::Down => {
                        // Horizontal portal: use portal.png (64×32)
                        // UV: x = portal_idx*0.5, y = (frame+1)*4/32, w = 0.5, h = 4/32
                        let src = [i as f32 * 0.5, frame_y * (4.0 / 32.0), 0.5, 4.0 / 32.0];
                        let w = 64.0 * scale;
                        let dst = [px - w / 2.0, py - 4.0, w, 8.0];
                        screen.draw_sprite_region_tinted(self.tex_portal, src, dst, color);
                    }
                }
            }
        }

        // ── Enemies ──
        for enemy in &self.enemies {
            // The cannon is tile art (42 over 64), already drawn with the level. The
            // entry in this list is only its firing timer.
            if enemy.enemy_type.harmless() {
                continue;
            }
            let ex = enemy.x - cam_x;
            let ey = enemy.y;
            let eh = enemy_height(enemy.enemy_type, enemy.state);
            let ew = if enemy.enemy_type == EnemyType::Bowser {
                BOWSER_W
            } else {
                PLAYER_SMALL_W
            };
            let dst = [ex, ey, ew, eh];
            match enemy.state {
                EnemyState::Dead => {
                    if enemy.flipped_death {
                        // Star/fireball kill: draw walking sprite upside-down
                        match enemy.enemy_type {
                            EnemyType::Goomba | EnemyType::Plant => {
                                let src = goomba_uv(0, 0);
                                screen.draw_sprite_region_flipped(
                                    self.tex_goomba,
                                    src,
                                    dst,
                                    false,
                                    true,
                                );
                            }
                            // Lakitu evicted from his cloud, which the original draws
                            // by simply negating the vertical scale (`lakito.lua:118`).
                            EnemyType::Lakito => {
                                screen.draw_sprite_region_flipped(
                                    self.tex_lakito,
                                    lakito_uv(0),
                                    lakito_dst(dst),
                                    !enemy.facing_right,
                                    true,
                                );
                            }
                            EnemyType::Spikey | EnemyType::SpikeyFall => {
                                screen.draw_sprite_region_flipped(
                                    self.tex_spikey,
                                    spikey_uv(0),
                                    dst,
                                    false,
                                    true,
                                );
                            }
                            EnemyType::BulletBill => {
                                screen.draw_sprite_region_flipped(
                                    self.tex_bullet_bill,
                                    bullet_bill_uv(),
                                    dst,
                                    enemy.facing_right,
                                    true,
                                );
                            }
                            EnemyType::HammerBro => {
                                screen.draw_sprite_region_flipped(
                                    self.tex_hammer_bro,
                                    hammer_bro_uv(0),
                                    hammer_bro_dst(dst),
                                    enemy.facing_right,
                                    true,
                                );
                            }
                            // **The false Bowser.** In worlds 1-7 the thing you just
                            // killed turns into a different enemy on the way down
                            // (`bowser.lua:196-199`) — the joke being that it was a
                            // painted goomba all along. World 8 is the real one and
                            // keeps his own sprite.
                            EnemyType::Bowser => {
                                let world = self.current.world;
                                if world <= 7 {
                                    screen.draw_sprite_region_flipped(
                                        self.tex_decoys,
                                        decoy_uv(world),
                                        bowser_dst(dst),
                                        false,
                                        true,
                                    );
                                } else {
                                    screen.draw_sprite_region_flipped(
                                        self.tex_bowser,
                                        bowser_uv(0, false),
                                        bowser_dst(dst),
                                        false,
                                        true,
                                    );
                                }
                            }
                            _ => {
                                let src = koopa_uv(0, 0);
                                screen.draw_sprite_region_flipped(
                                    self.tex_koopa,
                                    src,
                                    dst,
                                    false,
                                    true,
                                );
                            }
                        }
                    } else {
                        // Stomp kill: squashed sprite
                        match enemy.enemy_type {
                            EnemyType::Goomba | EnemyType::Plant => {
                                screen.draw_sprite_region(self.tex_goomba, goomba_uv(1, 0), dst);
                            }
                            EnemyType::Spikey | EnemyType::SpikeyFall => {
                                // Unreachable in practice — a spiny can't be stomped —
                                // but a spiny caught by a grill takes this path.
                                screen.draw_sprite_region(self.tex_spikey, spikey_uv(0), dst);
                            }
                            _ => {
                                screen.draw_sprite_region(self.tex_koopa, koopa_uv(4, 0), dst);
                            }
                        }
                    }
                }
                EnemyState::Shell | EnemyState::ShellMoving => {
                    screen.draw_sprite_region(self.tex_koopa, koopa_uv(4, 0), dst);
                }
                EnemyState::Walking => {
                    match enemy.enemy_type {
                        EnemyType::Goomba => {
                            // Only one walking frame (col 0); animation = flip horizontally
                            let src = goomba_uv(0, 0);
                            let flip = ((enemy.anim_timer * 5.0) as u32) % 2 == 1;
                            if flip {
                                screen.draw_sprite_region_flipped(
                                    self.tex_goomba,
                                    src,
                                    dst,
                                    true,
                                    false,
                                );
                            } else {
                                screen.draw_sprite_region(self.tex_goomba, src, dst);
                            }
                        }
                        EnemyType::Lakito => {
                            // Frame 2 is the wind-up: he pulls into the cloud for the
                            // last half-second before an egg leaves, which is the tell
                            // that gives you time to move (`lakito.lua:123-125`).
                            let frame =
                                u32::from(enemy.cycle_timer > LAKITO_THROW_TIME - LAKITO_HIDE_TIME);
                            screen.draw_sprite_region_flipped(
                                self.tex_lakito,
                                lakito_uv(frame),
                                lakito_dst(dst),
                                !enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::BulletBill => {
                            // No animation — it's a bullet. It faces its direction of
                            // travel, and the sheet's frame points left.
                            screen.draw_sprite_region_flipped(
                                self.tex_bullet_bill,
                                bullet_bill_uv(),
                                dst,
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::HammerBro => {
                            // Frames 2/3 are the wind-up: he holds the hammer over his
                            // head for the last `HAMMERBRO_PREPARE_TIME` before it
                            // leaves, which is the half second you get to move
                            // (`hammerbro.lua:159-163`).
                            let step = ((enemy.anim_timer / HAMMERBRO_ANIM_SPEED) as u32) % 2;
                            let raised = enemy.cycle_timer < HAMMERBRO_PREPARE_TIME;
                            let frame = step + if raised { 2 } else { 0 };
                            screen.draw_sprite_region_flipped(
                                self.tex_hammer_bro,
                                hammer_bro_uv(frame),
                                hammer_bro_dst(dst),
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::Bowser => {
                            let walk = (enemy.anim_timer / BOWSER_ANIM_SPEED) as u32 % 2;
                            // Mouth open for the last half second before a breath —
                            // and never while he is backing away, since he can't
                            // breathe then anyway.
                            let breathing = !enemy.backing_off
                                && self.fire_started
                                && self.fire_timer > self.fire_delay - 0.5;
                            screen.draw_sprite_region_flipped(
                                self.tex_bowser,
                                bowser_uv(walk, breathing),
                                bowser_dst(dst),
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::Fire => {
                            let frame = ((enemy.anim_timer / FIRE_ANIM_DELAY) as u32) % 2;
                            screen.draw_sprite_region(
                                self.tex_fire,
                                fire_uv(frame),
                                [ex, ey, FIRE_W, FIRE_H],
                            );
                        }
                        EnemyType::Squid => {
                            // Two frames, but not on a timer: the sprite changes with
                            // the *phase* — arms spread only while it sinks
                            // (`squid.lua:121`, `:130`).
                            let frame = u32::from(enemy.squid_phase == SquidPhase::Sink);
                            screen.draw_sprite_region_flipped(
                                self.tex_squid,
                                squid_uv(frame),
                                dst,
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::FlyingFish => {
                            // The original draws these with the cheep-cheep sheet
                            // (`flyingfish.lua:29`) — a flying fish *is* a cheep-cheep
                            // that got launched.
                            let frame = ((enemy.anim_timer / 0.35) as u32) % 2;
                            screen.draw_sprite_region_flipped(
                                self.tex_cheep_red,
                                cheep_uv(frame),
                                dst,
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::Hammer => {
                            let frame = ((enemy.anim_timer / HAMMER_ANIM_SPEED) as u32) % 4;
                            screen.draw_sprite_region_flipped(
                                self.tex_hammer,
                                hammer_uv(frame),
                                dst,
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::Spikey | EnemyType::SpikeyFall => {
                            // Two frames each, from different halves of the sheet:
                            // 0/1 walking, 2/3 tumbling through the air.
                            let base = if enemy.enemy_type == EnemyType::Spikey {
                                0
                            } else {
                                2
                            };
                            let frame = base + ((enemy.anim_timer / GOOMBA_ANIM_SPEED) as u32) % 2;
                            screen.draw_sprite_region_flipped(
                                self.tex_spikey,
                                spikey_uv(frame),
                                dst,
                                enemy.facing_right,
                                false,
                            );
                        }
                        EnemyType::Plant => {
                            // Two frames alternating on PLANT_ANIM_DELAY — the
                            // snapping mouth. The sprite sheet is 32x128 with
                            // 16x24 cells, two frames per spriteset row.
                            //
                            // Drawn at its own 16x24 rather than squeezed into the
                            // hitbox, offset up by `PLANT_SPRITE_RISE`, and clipped
                            // to the two-block window over the pipe — that scissor is
                            // the only thing hiding a retracted plant, which is
                            // otherwise drawn after the pipe and so on top of it.
                            let frame = ((enemy.anim_timer / PLANT_ANIM_DELAY) as u32) % 2;
                            let sprite =
                                [ex, ey - PLANT_SPRITE_RISE, PLANT_SPRITE_W, PLANT_SPRITE_H];
                            let [cx, cy, cw, ch] = self.plant_clip_rect(enemy, cam_x);
                            screen.clipped(cx, cy, cw, ch, |screen| {
                                screen.draw_sprite_region(self.tex_plant, plant_uv(frame), sprite);
                            });
                        }
                        EnemyType::Firebar | EnemyType::UpFire => {
                            // Both are animated fire. The firebar's own rotation
                            // is expressed by its position around the pivot, but
                            // spinning the sprite too sells the motion — this is
                            // what the engine's new sprite rotation is for.
                            let frame = ((enemy.anim_timer / 0.11) as u32) % 4;
                            let src = fireball_uv(frame);
                            let angle = if enemy.enemy_type == EnemyType::Firebar {
                                enemy.angle_deg.to_radians()
                            } else {
                                0.0
                            };
                            // 8x8 source at 2x = 16x16, which is exactly the
                            // 0.5-block segment spacing. Anything larger makes
                            // neighbouring segments overlap and the bar thicken.
                            const FIRE_W: f32 = 16.0;
                            const FIRE_H: f32 = 16.0;
                            let fire_dst = [
                                dst[0] + (dst[2] - FIRE_W) / 2.0,
                                dst[1] + (dst[3] - FIRE_H) / 2.0,
                                FIRE_W,
                                FIRE_H,
                            ];
                            screen.rotated(angle, |screen| {
                                screen.draw_sprite_region(self.tex_fireball, src, fire_dst);
                            });
                        }
                        EnemyType::CheepRed | EnemyType::CheepWhite => {
                            let frame = ((enemy.anim_timer / 0.35) as u32) % 2;
                            let tex = if enemy.enemy_type == EnemyType::CheepRed {
                                self.tex_cheep_red
                            } else {
                                self.tex_cheep_white
                            };
                            let src = cheep_uv(frame);
                            if enemy.facing_right {
                                screen.draw_sprite_region_flipped(tex, src, dst, true, false);
                            } else {
                                screen.draw_sprite_region(tex, src, dst);
                            }
                        }
                        _ => {
                            // Koopa, red koopa and beetle share the sheet layout;
                            // only the texture differs.
                            let frame = ((enemy.anim_timer * 4.0) as u32) % 2;
                            let src = koopa_uv(frame, 0);
                            let tex = self.koopa_texture(enemy.enemy_type);
                            if enemy.facing_right {
                                screen.draw_sprite_region_flipped(tex, src, dst, true, false);
                            } else {
                                screen.draw_sprite_region(tex, src, dst);
                            }
                        }
                    }
                }
            }
        }

        // ── Items (mushroom, star, 1-up) — only non-emerging (emerging drawn behind tiles) ──
        for item in &self.items {
            if item.emerging {
                continue;
            }
            let ix = item.x - cam_x;
            let iy = item.y;
            let dst = [ix, iy, TILE_SIZE, TILE_SIZE];
            match item.item_type {
                ItemType::Mushroom => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(1, 0), dst);
                }
                ItemType::OneUp => {
                    screen.draw_sprite_region(self.tex_entities, entity_uv(2, 0), dst);
                }
                ItemType::Star => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_star, star_frame_uv(frame), dst);
                }
                ItemType::FireFlower => {
                    let frame = ((item.anim_timer / STAR_ANIM_DELAY) as u32) % 4;
                    screen.draw_sprite_region(self.tex_flower, flower_frame_uv(frame), dst);
                }
            }
        }

        // ── Fireballs ──
        for fb in &self.fireballs {
            let fx = fb.x - cam_x;
            let fy = fb.y;
            if fb.exploding {
                let frame = ((fb.explode_timer / FIREBALL_ANIM_DELAY) as u32).min(2);
                let dst = [
                    fx - FIREBALL_SIZE * 0.5,
                    fy - FIREBALL_SIZE * 0.5,
                    TILE_SIZE,
                    TILE_SIZE,
                ];
                screen.draw_sprite_region(self.tex_fireball, fireball_explode_uv(frame), dst);
            } else {
                let frame = ((fb.anim_timer / FIREBALL_ANIM_DELAY) as u32) % 4;
                let dst = [fx, fy, FIREBALL_SIZE, FIREBALL_SIZE];
                screen.draw_sprite_region(self.tex_fireball, fireball_uv(frame), dst);
            }
        }

        // ── Coin popups ──
        let coin_src = coin_uv(coin_spin_frame(self.coin_spin));
        for popup in &self.coin_popups {
            let cx = popup.x - cam_x + 8.0; // center 16px coin in 32px tile
            let cy = popup.y + 8.0;
            let alpha = 1.0 - (popup.timer / COIN_POPUP_TIME).min(1.0);
            let color = Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: alpha,
            };
            screen.draw_sprite_region_tinted(self.tex_coin, coin_src, [cx, cy, 16.0, 16.0], color);
        }

        // ── Brick debris ──
        for debris in &self.brick_debris {
            let dx = debris.x - cam_x;
            let dy = debris.y;
            // Draw a small piece of brick tile (quarter of the tile)
            let quarter_uv = smb_tile_uv(SMB_BRICK);
            let half_w = TILE_SIZE * 0.5;
            screen.draw_sprite_region(
                self.tex_tiles,
                quarter_uv,
                [dx - half_w * 0.5, dy - half_w * 0.5, half_w, half_w],
            );
        }

        // ── Player ──
        if self.state == GameState::Playing
            || self.state == GameState::Dead
            || self.state == GameState::LevelComplete
        {
            // Once he is through the castle door he is not drawn at all — the original
            // clears `drawable` there (`mario.lua:404`) and the last three beats of the
            // ending happen with him off-stage.
            let in_castle = self.flag.is_some_and(|f| {
                matches!(
                    f.phase,
                    crate::flagpole::FlagPhase::Countdown
                        | crate::flagpole::FlagPhase::CastleFlag
                        | crate::flagpole::FlagPhase::Fireworks
                )
            });
            let visible = !in_castle
                && (self.player.invincible_timer <= 0.0
                    || ((self.player.invincible_timer * 10.0) as u32).is_multiple_of(2));
            // Inside a pipe Mario is clipped to the mouth's outer side, so he
            // disappears into it instead of sliding across the top of it.
            let pipe_clip = self.pipe_clip_rect(cam_x, self.vw, self.vh);
            if visible {
                // Gun-angle sprite row (mari0 getAngleFrame):
                // Row 0 = gun up, 1 = diagonal up, 2 = horizontal, 3 = down
                // Compute angle from vertical ("up"): acos(-sin(crosshair_angle))
                // Use -sin(angle) as the "up-component" and compare to cos(π/8) thresholds
                let up_comp = -self.crosshair_angle.sin();
                let angle_row: u32 = if up_comp > 0.924 {
                    // < π/8 from vertical
                    0
                } else if up_comp > 0.383 {
                    // < 3π/8
                    1
                } else if up_comp > -0.383 {
                    // < 5π/8
                    2
                } else {
                    3
                };

                // Climbing overrides the aim on both axes. The original *assigns*
                // `pointingangle = ±pi/2` when Mario takes hold of a vine
                // (`mario.lua:2316-2320`) — its angles run from straight up, so ±pi/2 is
                // horizontal, hence the gun-level row — and the sign is what decides the
                // flip. He faces the stem he is holding.
                let climbing = self.player.anim_state == PlayerAnim::Climb;
                let angle_row = if climbing { 2 } else { angle_row };
                // Player faces mouse direction (mari0: pointingangle > 0 → face left)
                let face_right = if climbing {
                    self.player.facing_right
                } else {
                    self.crosshair_angle.cos() >= 0.0
                };

                // Columns 7 and 8 of the sheet, the two climbing frames
                // (`marioclimb`, `main.lua:544-546`); 9 and 10 are the swimming pair
                // (`marioswim`, `:548-550`) and 13 is the big-only crouch
                // (`bigmarioduck`, `:600`).
                let climb_col = 6 + self.player.climb_frame.clamp(1, 2);
                let swim_col = 8 + (self.player.swim_phase.floor() as u32).clamp(1, 2);
                let src = if self.player.is_big {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_big_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_big_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_big_uv(5, angle_row),
                        PlayerAnim::Climb => mario_big_uv(climb_col, angle_row),
                        PlayerAnim::Swim => mario_big_uv(swim_col, angle_row),
                        PlayerAnim::Duck => mario_big_uv(13, angle_row),
                    }
                } else {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_uv(5, angle_row),
                        PlayerAnim::Climb => mario_uv(climb_col, angle_row),
                        PlayerAnim::Swim => mario_uv(swim_col, angle_row),
                        // A small Mario has no crouch, so this is unreachable — but
                        // falling back on `idle` beats sampling column 13 of a sheet
                        // that has no such cell.
                        PlayerAnim::Duck => mario_uv(0, angle_row),
                    }
                };
                let px = self.player.x - cam_x;
                let py = self.player.y;
                let (sw, sh) = if self.player.is_big {
                    (MARIO_BIG_SPRITE_W, MARIO_BIG_SPRITE_H)
                } else {
                    (MARIO_SMALL_SPRITE_W, MARIO_SMALL_SPRITE_H)
                };
                let bottom_pad = 2.0 * MARIO_SPRITE_SCALE; // 4px
                let sx = px + (self.player.width - sw) / 2.0;
                let sy = py + self.player.height - sh + bottom_pad;
                let dst = [sx, sy, sw, sh];

                // Mari0 4-layer palette rendering (Player 1 = Red Mario)
                // Draw order: layer1 (primary), layer2 (secondary), layer3 (tertiary), layer0 (outline)
                // Colors are sRGB values from original mari0; convert to linear for GPU tint multiplication
                let mario_colors = mario_palette(self.player.is_fire);
                let layers = if self.player.is_big {
                    &self.tex_mario_big_layers
                } else {
                    &self.tex_mario_layers
                };
                let draw_mario = |screen: &mut Screen| {
                    for (i, color) in mario_colors.iter().enumerate() {
                        let tex = layers[i + 1];
                        if face_right {
                            screen.draw_sprite_region_tinted(tex, src, dst, *color);
                        } else {
                            screen.draw_sprite_region_flipped_tinted(
                                tex, src, dst, true, false, *color,
                            );
                        }
                    }
                    // Layer 0 (outline) drawn last, white tint (as-is)
                    if face_right {
                        screen.draw_sprite_region(layers[0], src, dst);
                    } else {
                        screen.draw_sprite_region_flipped(layers[0], src, dst, true, false);
                    }
                };
                match pipe_clip {
                    Some([cx, cy, cw, ch]) => screen.clipped(cx, cy, cw, ch, draw_mario),
                    None => draw_mario(screen),
                }
            }
        }

        // ── Portal dust ──
        // Behind the projectiles and the player, and tinted the colour of the portal it
        // came out of. `portalparticle.png` is a single pixel, drawn centred.
        for p in &self.portal_particles {
            let base = portal_colors[p.portal];
            let fade = 1.0 - (p.timer / PORTAL_PARTICLE_DURATION).min(1.0);
            const S: f32 = 2.0;
            screen.draw_sprite_tinted(
                self.tex_portal_particle,
                p.x - cam_x - S / 2.0,
                p.y - S / 2.0,
                S,
                S,
                Color { r: base.r, g: base.g, b: base.b, a: fade },
            );
        }

        // ── Projectiles (with particle trail, matches original mari0) ──
        for proj in &self.projectiles {
            let color = portal_colors[proj.portal_index];

            // Trail: 5 fading copies behind the projectile
            let half_color = Color {
                r: color.r * 0.5,
                g: color.g * 0.5,
                b: color.b * 0.5,
                a: color.a,
            };
            for ti in (1..=5).rev() {
                let t = ti as f32 * 0.008; // 8ms apart
                let tx = proj.x - proj.vx * t - cam_x;
                let ty = proj.y - proj.vy * t;
                let alpha = 0.6 - ti as f32 * 0.12;
                if alpha <= 0.0 {
                    continue;
                }
                let tc = Color {
                    r: half_color.r,
                    g: half_color.g,
                    b: half_color.b,
                    a: alpha,
                };
                screen.draw_sprite_tinted(
                    self.tex_portal_projectile,
                    tx - 5.0,
                    ty - 5.0,
                    10.0,
                    10.0,
                    tc,
                );
            }

            // Main projectile orb (8×8 source at 2x scale = 16×16)
            screen.draw_sprite_tinted(
                self.tex_portal_projectile,
                proj.x - cam_x - 8.0,
                proj.y - 8.0,
                16.0,
                16.0,
                color,
            );
        }

        // ── Portal aiming line + crosshair (matches mari0 game.lua:1600-1662) ──
        // mari0 constants: portaldotsdistance=1.2, portaldotstime=0.8,
        //   portaldotsinner=10, portaldotsouter=70, scale=2
        if self.state == GameState::Playing {
            let source_x = self.player.center_x();
            let source_y = self.player.center_y();
            let angle = self.crosshair_angle;
            const SCALE: f32 = 2.0; // our render scale (TILE_SIZE/16)

            let (end_x, end_y, hit_info) =
                trace_aim_line(&self.level, source_x, source_y, angle, cam_x, self.vw);

            // Portal possible? The original asks the *placement* function, not just
            // "is this tile portalable" (`game.lua:1608`), so the crosshair turns red
            // on a wall where the two-tile span won't fit — otherwise it would
            // promise a shot that fails silently.
            let portal_possible = hit_info.is_some_and(|hit| {
                let anchors = [
                    self.portals[0]
                        .as_ref()
                        .filter(|p| p.active)
                        .map(|p| p.anchor),
                    self.portals[1]
                        .as_ref()
                        .filter(|p| p.active)
                        .map(|p| p.anchor),
                ];
                let (hx, hy) = (
                    hit.cell.0 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    hit.cell.1 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                );
                crate::portal_math::portal_position(
                    &self.level,
                    hit.cell,
                    hit.side,
                    crate::portal_math::tendency_for(hx, hy, hit.side),
                    &anchors,
                    // The crosshair isn't replacing a specific portal, so neither
                    // slot is exempt from blocking placement.
                    usize::MAX,
                )
                .is_some()
            });

            // Dot color: green if portal can be placed, red otherwise (original: setColor)
            let dot_rgb = if portal_possible {
                (0.0_f32, 1.0_f32, 0.0_f32)
            } else {
                (1.0, 0.0, 0.0)
            };

            // Distance in pixels from source to endpoint
            let dx_px = end_x - source_x;
            let dy_px = end_y - source_y;
            let dist_px = (dx_px * dx_px + dy_px * dy_px).sqrt();

            // Distance in tile units (original works in tile coords)
            let dist_tiles = dist_px / (16.0 * SCALE);

            // Draw animated dots from source to endpoint (always, like original)
            let dot_count = (dist_tiles / 1.2) as i32 + 1; // portaldotsdistance = 1.2
            let phase = self.aim_dot_timer / 0.8; // portaldotstime = 0.8

            for i in 0..dot_count {
                let t = ((i as f32) + phase) / (dist_tiles / 1.2).max(1.0);
                if t >= 1.0 {
                    continue;
                }

                // Dot position in screen coords
                let dot_screen_x = (source_x - cam_x) + dx_px * t;
                let dot_screen_y = (source_y) + dy_px * t;

                // xplus/yplus = offset from source in screen pixels
                let xplus = dx_px * t;
                let yplus = dy_px * t;

                // Alpha fade near source (original: radius in base pixels)
                let radius = (xplus * xplus + yplus * yplus).sqrt() / SCALE;
                let mut alpha = 1.0_f32;
                if radius < 70.0 {
                    // portaldotsouter
                    // Original: alpha = (radius-inner)*(outer-inner), clamped
                    alpha = ((radius - 10.0) / (70.0 - 10.0)).clamp(0.0, 1.0);
                }

                let dot_color = Color {
                    r: dot_rgb.0,
                    g: dot_rgb.1,
                    b: dot_rgb.2,
                    a: alpha,
                };

                // Dot size = scale×scale = 2×2 pixels, offset -0.25*scale (original)
                let off = 0.25 * SCALE; // 0.5
                screen.draw_sprite_tinted(
                    self.tex_portal_dot,
                    (dot_screen_x - off).floor(),
                    (dot_screen_y - off).floor(),
                    SCALE,
                    SCALE,
                    dot_color,
                );
            }

            // Crosshair only drawn when a wall is hit (original: if cox ~= false)
            if let Some(hit) = hit_info {
                let orient = hit.side;
                let ch_color = Color {
                    r: dot_rgb.0,
                    g: dot_rgb.1,
                    b: dot_rgb.2,
                    a: 1.0,
                };
                let ch_screen_x = end_x - cam_x;
                let ch_screen_y = end_y;

                // Original: portalcrosshairimg 8×8, drawn with origin (4,8), at scale×scale
                // origin (4,8) = center-bottom of the 8px image
                // Rendered size = 8*scale × 8*scale = 16×16
                let ch_w = 8.0 * SCALE; // 16
                let ch_h = 8.0 * SCALE; // 16

                // Position crosshair so its edge touches the wall surface
                // Original rotates based on side; we approximate with position offset
                let (cx, cy) = match orient {
                    Orientation::Up => {
                        // Wall above: crosshair hangs down from hit point
                        (ch_screen_x - ch_w * 0.5, ch_screen_y)
                    }
                    Orientation::Down => {
                        // Wall below: crosshair extends up from hit point
                        (ch_screen_x - ch_w * 0.5, ch_screen_y - ch_h)
                    }
                    Orientation::Left => {
                        // Wall to the left: crosshair extends right
                        (ch_screen_x, ch_screen_y - ch_h * 0.5)
                    }
                    Orientation::Right => {
                        // Wall to the right: crosshair extends left
                        (ch_screen_x - ch_w, ch_screen_y - ch_h * 0.5)
                    }
                };

                screen.draw_sprite_tinted(
                    self.tex_portal_crosshair,
                    cx.floor(),
                    cy.floor(),
                    ch_w,
                    ch_h,
                    ch_color,
                );
            }
        }

        // ── Score popups (floating text) ──
        // Draw before HUD so they appear in game world
        {
            let hud_font = ctx.assets.font("hud");
            if let Some(font) = hud_font {
                for popup in &self.score_popups {
                    let sx = popup.x - cam_x;
                    let sy = popup.y;
                    let alpha = 1.0 - (popup.timer / SCORE_POPUP_TIME).min(1.0);
                    if alpha > 0.0 {
                        match popup.value {
                            Some(value) => screen.draw_text(font, &format!("{value}"), sx, sy),
                            // An extra life floats up on the same track as a score, but as
                            // a graphic rather than a number (`game.lua:1588`).
                            None => screen.draw_sprite(
                                self.tex_oneup_text,
                                sx,
                                sy,
                                ONEUP_TEXT_W,
                                ONEUP_TEXT_H,
                            ),
                        }
                    }
                }
            }
        }

    }

    /// The four-column NES HUD, and whatever screen the current state calls for.
    fn draw_hud(&self, ctx: &Context, screen: &mut Screen) {
        let hud_font = ctx.assets.font("hud");
        let ui_font = ctx.assets.font("ui");
        let title_font = ctx.assets.font("title");

        match self.state {
            GameState::Menu => {
                // NES-style HUD on menu too
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    self.draw_coin_icon(screen);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, &self.current.name(), 320.0, 20.0);
                    screen.draw_text(font, "TIME", 432.0, 8.0);
                }
                if let Some(font) = title_font {
                    screen.draw_text_centered(font, "MARI0", 100.0);
                }
                if let Some(font) = ui_font {
                    screen.draw_text_centered(font, "Mario + Portal", 140.0);
                    screen.draw_text_centered(font, "A tribute to Stabyourself.net", 165.0);

                    // Mappack and level picker. Without it the portal mappack — nine
                    // levels of lab — is unreachable outside the debug protocol.
                    let pack = self.menu.pack_name();
                    let label = if pack == "portal" {
                        "PORTAL (lab)"
                    } else {
                        "SUPER MARIO BROS"
                    };
                    screen.draw_text_centered(font, label, 205.0);
                    screen.draw_text_centered(
                        font,
                        &format!("< {}-{} >", self.menu.world, self.menu.level),
                        230.0,
                    );
                    let (fw, fl) = self.furthest_in(pack);
                    screen.draw_text_centered(font, &format!("furthest {fw}-{fl}"), 252.0);
                    // Which loadout the mouse buttons will carry.
                    let gun = match self.player_type {
                        crate::player::PlayerType::Portal => "PORTAL GUN",
                        crate::player::PlayerType::GelCannon => "GEL CANNON",
                    };
                    screen.draw_text_centered(font, &format!("[F] {gun}"), 274.0);
                    screen.draw_text_centered(
                        font,
                        &format!("high score {:06}", self.high_score),
                        272.0,
                    );

                    screen.draw_text_centered(font, "Left/Right: level   Down: mappack", 310.0);
                    screen.draw_text_centered(
                        font,
                        "Space: start   Mouse: aim, click: portals",
                        330.0,
                    );
                }
            }
            GameState::Playing => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    self.draw_coin_icon(screen);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, &self.current.name(), 320.0, 20.0);
                    screen.draw_text(font, "TIME", 432.0, 8.0);
                    screen.draw_text(
                        font,
                        &format!("{}", self.time_remaining as u32),
                        440.0,
                        20.0,
                    );
                }
                // The original puts a menu here; this just says so, because a frozen
                // frame with no label is indistinguishable from a hang.
                if self.paused && let Some(font) = title_font {
                    screen.draw_text_centered(font, "PAUSED", 200.0);
                }
            }
            GameState::Dead => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    self.draw_coin_icon(screen);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, &self.current.name(), 320.0, 20.0);
                }
                if let Some(font) = title_font {
                    if self.lives > 0 {
                        screen.draw_text_centered(font, "YOU DIED", 150.0);
                    } else {
                        screen.draw_text_centered(font, "GAME OVER", 150.0);
                    }
                }
                if let Some(font) = ui_font {
                    let score_text = format!("Score: {}", self.score);
                    screen.draw_text_centered(font, &score_text, 200.0);
                    screen.draw_text_centered(font, "Press SPACE to continue", 250.0);
                }
            }
            GameState::Interlude => {
                // The card. Everything is drawn over pure black — `draw_world` bails out
                // for this state, so nothing of the level shows through.
                let Some(card) = self.interlude else { return };
                if !card.text_visible() {
                    return;
                }
                // The HUD stays up behind every card (`levelscreen.lua:183-199`), minus
                // the clock reading — "time" is printed with no number after it.
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    self.draw_coin_icon(screen);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, &self.current.name(), 320.0, 20.0);
                    screen.draw_text(font, "TIME", 432.0, 8.0);
                }
                match card.kind {
                    crate::interlude::InterludeKind::Sublevel => {}
                    crate::interlude::InterludeKind::LevelScreen => {
                        if let Some(font) = ui_font {
                            screen.draw_text_centered(
                                font,
                                &format!("world {}", self.current.name()),
                                144.0,
                            );
                        }
                        // The puppet: a static Mario in the same four-layer palette the
                        // sprite sheet uses, with the life count beside it.
                        const PW: f32 = 13.0 * 2.0;
                        const PH: f32 = 16.0 * 2.0;
                        let px = self.vw / 2.0 - 58.0;
                        let py = 194.0;
                        for (i, colour) in mario_palette(false).iter().enumerate() {
                            screen.draw_sprite_tinted(
                                self.tex_puppet[i + 1],
                                px,
                                py,
                                PW,
                                PH,
                                *colour,
                            );
                        }
                        screen.draw_sprite(self.tex_puppet[0], px, py, PW, PH);
                        if let Some(font) = ui_font {
                            screen.draw_text(font, &format!("*  {}", self.lives), px + 40.0, py + 12.0);
                        }
                    }
                    crate::interlude::InterludeKind::GameOver => {
                        if let Some(font) = ui_font {
                            screen.draw_text_centered(font, "game over", 240.0);
                        }
                    }
                    crate::interlude::InterludeKind::MappackFinished => {
                        if let Some(font) = ui_font {
                            screen.draw_text_centered(font, "congratulations!", 240.0);
                            screen.draw_text_centered(
                                font,
                                "you have finished this mappack!",
                                280.0,
                            );
                        }
                    }
                }
            }
            GameState::LevelComplete => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
                    self.draw_coin_icon(screen);
                    screen.draw_text(font, &format!("x{:02}", self.coins), 180.0, 20.0);
                    screen.draw_text(font, "WORLD", 312.0, 8.0);
                    screen.draw_text(font, &self.current.name(), 320.0, 20.0);
                }
                if let Some(font) = title_font {
                    screen.draw_text_centered(font, "LEVEL COMPLETE!", 150.0);
                }
                if let Some(font) = ui_font {
                    let score_text = format!("Score: {}", self.score);
                    screen.draw_text_centered(font, &score_text, 200.0);
                    screen.draw_text_centered(font, "Press SPACE to continue", 250.0);
                }
            }
        }
    }
}
