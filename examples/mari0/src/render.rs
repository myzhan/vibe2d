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

impl Mari0Game {
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

    /// Draw the whole frame: level, actors, effects, then HUD.
    pub(crate) fn draw_world(&self, ctx: &Context, screen: &mut Screen) {
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

        // ── The lab: buttons, doors, indicators, beams and bridges ──
        // After the tiles, since every one of them is mounted on a wall, and before the
        // actors so Mario walks in front of a beam rather than behind it.
        self.draw_lab(screen);

        // ── Flag sprite (drawn beside the flagpole pole) ──
        if self.level.flag_x > 0.0 {
            let fx = self.level.flag_x - cam_x;
            if fx > -TILE_SIZE && fx < self.vw + TILE_SIZE {
                screen.draw_sprite(
                    self.tex_flag,
                    fx - TILE_SIZE,
                    3.0 * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
            }
        }

        // ── Coins ──
        let coin_frame = ((self.time_remaining * 4.0) as u32) % 2;
        let coin_src = coin_frame_uv(coin_frame);
        for coin in &self.level.coins {
            if !coin.collected {
                let x = coin.x - cam_x;
                screen.draw_sprite_region(self.tex_coin_anim, coin_src, [x, coin.y, 16.0, 16.0]);
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
            let ex = enemy.x - cam_x;
            let ey = enemy.y;
            let eh = match enemy.enemy_type {
                EnemyType::Koopa if enemy.state == EnemyState::Walking => 48.0,
                _ => PLAYER_SMALL_H,
            };
            let dst = [ex, ey, PLAYER_SMALL_W, eh];
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
                            let frame = ((enemy.anim_timer / PLANT_ANIM_DELAY) as u32) % 2;
                            screen.draw_sprite_region(self.tex_plant, plant_uv(frame), dst);
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
        let coin_frame = ((self.time_remaining * 8.0) as u32) % 2;
        let coin_src = coin_frame_uv(coin_frame);
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
            screen.draw_sprite_region_tinted(
                self.tex_coin_anim,
                coin_src,
                [cx, cy, 16.0, 16.0],
                color,
            );
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
            let visible = self.player.invincible_timer <= 0.0
                || ((self.player.invincible_timer * 10.0) as u32).is_multiple_of(2);
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

                // Player faces mouse direction (mari0: pointingangle > 0 → face left)
                let face_right = self.crosshair_angle.cos() >= 0.0;

                let src = if self.player.is_big {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_big_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_big_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_big_uv(5, angle_row),
                    }
                } else {
                    match self.player.anim_state {
                        PlayerAnim::Idle => mario_uv(0, angle_row),
                        PlayerAnim::Run => {
                            let frame = (self.player.run_frame as u32) % 3;
                            mario_uv(1 + frame, angle_row)
                        }
                        PlayerAnim::Jump | PlayerAnim::Fall => mario_uv(5, angle_row),
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
                let mario_colors = if self.player.is_fire {
                    [
                        Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }, // layer1: white shirt (fire)
                        Color {
                            r: srgb_to_linear(224.0 / 255.0),
                            g: srgb_to_linear(32.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer2: red overalls
                        Color {
                            r: srgb_to_linear(252.0 / 255.0),
                            g: srgb_to_linear(152.0 / 255.0),
                            b: srgb_to_linear(56.0 / 255.0),
                            a: 1.0,
                        }, // layer3: skin
                    ]
                } else {
                    [
                        Color {
                            r: srgb_to_linear(224.0 / 255.0),
                            g: srgb_to_linear(32.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer1: red shirt
                        Color {
                            r: srgb_to_linear(136.0 / 255.0),
                            g: srgb_to_linear(112.0 / 255.0),
                            b: srgb_to_linear(0.0 / 255.0),
                            a: 1.0,
                        }, // layer2: brown
                        Color {
                            r: srgb_to_linear(252.0 / 255.0),
                            g: srgb_to_linear(152.0 / 255.0),
                            b: srgb_to_linear(56.0 / 255.0),
                            a: 1.0,
                        }, // layer3: skin
                    ]
                };
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
                        // Draw score text (simple white text with fade)
                        let text = format!("{}", popup.value);
                        screen.draw_text(font, &text, sx, sy);
                    }
                }
            }
        }

        // ── HUD (NES-style four-column layout) ──
        let hud_font = ctx.assets.font("hud");
        let ui_font = ctx.assets.font("ui");
        let title_font = ctx.assets.font("title");

        match self.state {
            GameState::Menu => {
                // NES-style HUD on menu too
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
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
            }
            GameState::Dead => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
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
            GameState::LevelComplete => {
                if let Some(font) = hud_font {
                    screen.draw_text(font, "MARIO", 24.0, 8.0);
                    screen.draw_text(font, &format!("{:06}", self.score), 24.0, 20.0);
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
