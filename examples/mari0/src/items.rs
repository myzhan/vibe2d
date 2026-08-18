//! Block contents, the items they release, and Mario's fireballs.

use vibe2d::prelude::*;

use crate::constants::*;
use crate::effects::*;
use crate::enemies::{EnemyState, EnemyType, enemy_height};
use crate::game::Mari0Game;
use crate::physics::*;
use crate::portal::{PortalBody, portal_carry};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BlockContent {
    Coin,
    MultiCoin(u32),
    Mushroom,
    Star,
    OneUp,
    // Note: there is intentionally no `FireFlower` variant. In SMB the
    // FireFlower is *produced* dynamically when a big Mario hits a
    // `Mushroom` block (see the `BlockContent::Mushroom` arm in the block
    // hit logic) — level data only ever stores `Mushroom`. If a future
    // level needs to force a FireFlower regardless of Mario's state, add
    // the variant back together with a tilemap producer.
}

#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(feature = "vdp", derive(serde::Serialize))]
#[cfg_attr(feature = "vdp", serde(rename_all = "snake_case"))]
pub(crate) enum ItemType {
    Mushroom,
    Star,
    #[cfg_attr(feature = "vdp", serde(rename = "1up"))]
    OneUp,
    FireFlower,
}

pub(crate) struct Fireball {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) anim_timer: f32,
    pub(crate) exploding: bool,
    pub(crate) explode_timer: f32,
}

pub(crate) struct Item {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) item_type: ItemType,
    pub(crate) emerging: bool, // still popping out of block
    pub(crate) emerge_y: f32,  // target y after emerging
    pub(crate) emerge_timer: f32,
    pub(crate) anim_timer: f32,
}

impl Mari0Game {
    pub(crate) fn hit_block(&mut self, ctx: &Context, row: usize, col: usize, tile: u32) {
        let key = (row, col);
        let bx = col as f32 * TILE_SIZE;
        let by = row as f32 * TILE_SIZE;

        match tile {
            SMB_QUESTION => {
                let content = self
                    .level
                    .block_contents
                    .get(&key)
                    .copied()
                    .unwrap_or(BlockContent::Coin);
                match content {
                    BlockContent::Coin => {
                        // Turn question block into used block
                        self.level.tiles[row][col] = self.used_block_tile();
                        self.level.block_contents.remove(&key);
                        self.score += COIN_SCORE;
                        self.coins += 1;
                        if self.coins >= 100 {
                            self.coins -= 100;
                            self.lives += 1;
                        }
                        self.coin_popups.push(CoinPopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            vy: COIN_POPUP_SPEED,
                            timer: 0.0,
                        });
                        self.score_popups.push(ScorePopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            value: COIN_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("coin");
                    }
                    BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                        self.level.tiles[row][col] = self.used_block_tile();
                        self.level.block_contents.remove(&key);
                        let item_type = match content {
                            BlockContent::Mushroom => {
                                if self.player.is_big {
                                    ItemType::FireFlower
                                } else {
                                    ItemType::Mushroom
                                }
                            }
                            BlockContent::Star => ItemType::Star,
                            _ => ItemType::OneUp,
                        };
                        self.items.push(Item {
                            x: bx,
                            y: by,
                            vx: 0.0,
                            vy: 0.0,
                            item_type,
                            emerging: true,
                            emerge_y: by - TILE_SIZE,
                            emerge_timer: 0.0,
                            anim_timer: 0.0,
                        });
                        ctx.audio.play("mushroomappear");
                    }
                    BlockContent::MultiCoin(remaining) => {
                        // Start timer on first hit
                        self.level
                            .multi_coin_timers
                            .entry(key)
                            .or_insert(MULTI_COIN_TIMEOUT);
                        if remaining > 1 {
                            self.level
                                .block_contents
                                .insert(key, BlockContent::MultiCoin(remaining - 1));
                        } else {
                            self.level.tiles[row][col] = self.used_block_tile();
                            self.level.block_contents.remove(&key);
                            self.level.multi_coin_timers.remove(&key);
                        }
                        self.score += COIN_SCORE;
                        self.coins += 1;
                        if self.coins >= 100 {
                            self.coins -= 100;
                            self.lives += 1;
                        }
                        self.coin_popups.push(CoinPopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            vy: COIN_POPUP_SPEED,
                            timer: 0.0,
                        });
                        self.score_popups.push(ScorePopup {
                            x: bx,
                            y: by - TILE_SIZE,
                            value: COIN_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("coin");
                    }
                }
                self.block_bounces.push(BlockBounce {
                    col: col as i32,
                    row: row as i32,
                    timer: 0.0,
                });
                ctx.audio.play("blockhit");
            }
            SMB_BRICK => {
                if let Some(content) = self.level.block_contents.get(&key).copied() {
                    // Brick with content
                    match content {
                        BlockContent::MultiCoin(remaining) => {
                            self.level
                                .multi_coin_timers
                                .entry(key)
                                .or_insert(MULTI_COIN_TIMEOUT);
                            if remaining > 1 {
                                self.level
                                    .block_contents
                                    .insert(key, BlockContent::MultiCoin(remaining - 1));
                            } else {
                                self.level.tiles[row][col] = self.used_block_tile();
                                self.level.block_contents.remove(&key);
                                self.level.multi_coin_timers.remove(&key);
                            }
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            if self.coins >= 100 {
                                self.coins -= 100;
                                self.lives += 1;
                            }
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                        BlockContent::Coin => {
                            self.level.tiles[row][col] = self.used_block_tile();
                            self.level.block_contents.remove(&key);
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            if self.coins >= 100 {
                                self.coins -= 100;
                                self.lives += 1;
                            }
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                        BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                            self.level.tiles[row][col] = self.used_block_tile();
                            self.level.block_contents.remove(&key);
                            let item_type = match content {
                                BlockContent::Mushroom => {
                                    if self.player.is_big {
                                        ItemType::FireFlower
                                    } else {
                                        ItemType::Mushroom
                                    }
                                }
                                BlockContent::Star => ItemType::Star,
                                _ => ItemType::OneUp,
                            };
                            self.items.push(Item {
                                x: bx,
                                y: by,
                                vx: 0.0,
                                vy: 0.0,
                                item_type,
                                emerging: true,
                                emerge_y: by - TILE_SIZE,
                                emerge_timer: 0.0,
                                anim_timer: 0.0,
                            });
                            ctx.audio.play("mushroomappear");
                        }
                    }
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                } else if self.player.is_big {
                    // Big Mario breaks empty brick
                    self.level.tiles[row][col] = SMB_EMPTY;
                    self.score += BRICK_BREAK_SCORE;
                    self.score_popups.push(ScorePopup {
                        x: bx,
                        y: by - TILE_SIZE,
                        value: BRICK_BREAK_SCORE,
                        timer: 0.0,
                    });
                    // 4 debris particles
                    let cx = bx + TILE_SIZE * 0.5;
                    let cy = by + TILE_SIZE * 0.5;
                    for &(dvx, dvy) in &[
                        (-112.0f32, -736.0f32),
                        (112.0, -736.0),
                        (-112.0, -448.0),
                        (112.0, -448.0),
                    ] {
                        self.brick_debris.push(BrickDebris {
                            x: cx,
                            y: cy,
                            vx: dvx,
                            vy: dvy,
                            timer: 0.0,
                        });
                    }
                    ctx.audio.play("blockbreak");
                } else {
                    // Small Mario just bounces the brick
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                }
            }
            SMB_HIDDEN_BLOCK => {
                if let Some(content) = self.level.block_contents.get(&key).copied() {
                    self.level.tiles[row][col] = self.used_block_tile();
                    self.level.block_contents.remove(&key);
                    match content {
                        BlockContent::Mushroom | BlockContent::Star | BlockContent::OneUp => {
                            let item_type = match content {
                                BlockContent::Mushroom => {
                                    if self.player.is_big {
                                        ItemType::FireFlower
                                    } else {
                                        ItemType::Mushroom
                                    }
                                }
                                BlockContent::Star => ItemType::Star,
                                _ => ItemType::OneUp,
                            };
                            self.items.push(Item {
                                x: bx,
                                y: by,
                                vx: 0.0,
                                vy: 0.0,
                                item_type,
                                emerging: true,
                                emerge_y: by - TILE_SIZE,
                                emerge_timer: 0.0,
                                anim_timer: 0.0,
                            });
                            ctx.audio.play("mushroomappear");
                        }
                        _ => {
                            self.score += COIN_SCORE;
                            self.coins += 1;
                            self.coin_popups.push(CoinPopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                vy: COIN_POPUP_SPEED,
                                timer: 0.0,
                            });
                            self.score_popups.push(ScorePopup {
                                x: bx,
                                y: by - TILE_SIZE,
                                value: COIN_SCORE,
                                timer: 0.0,
                            });
                            ctx.audio.play("coin");
                        }
                    }
                    self.block_bounces.push(BlockBounce {
                        col: col as i32,
                        row: row as i32,
                        timer: 0.0,
                    });
                    ctx.audio.play("blockhit");
                }
            }
            _ => {}
        }
    }

    pub(crate) fn update_items(&mut self, ctx: &Context, dt: f32) {
        // Cloned up front: the loop holds `&mut self.items`.
        let portals = self.portal_pair();
        // Update item physics
        let level = &self.level;
        for item in &mut self.items {
            if item.emerging {
                item.emerge_timer += dt;
                let progress = (item.emerge_timer / ITEM_POP_TIME).min(1.0);
                item.y = item.emerge_y + TILE_SIZE * (1.0 - progress);
                if progress >= 1.0 {
                    item.emerging = false;
                    item.y = item.emerge_y;
                    // Flower stays in place; mushroom/star/1-up move horizontally
                    if item.item_type != ItemType::FireFlower {
                        item.vx = ITEM_SPEED;
                    }
                    if item.item_type == ItemType::Star {
                        item.vy = STAR_JUMP_FORCE;
                    }
                }
                continue;
            }

            item.anim_timer += dt;

            // Gravity
            item.vy += GRAVITY * dt;
            if item.vy > MAX_Y_SPEED {
                item.vy = MAX_Y_SPEED;
            }

            let iw = TILE_SIZE;
            let ih = TILE_SIZE;

            // A mushroom/1-up/star is `static = true` while it is still rising out of
            // the block and becomes a mover once clear (`mushroom.lua:60`), so
            // `emerging` is exactly the original's gate. The fire flower stays static
            // for good and never travels.
            if !item.emerging
                && item.item_type != ItemType::FireFlower
                && let Some((nx, ny, nvx, nvy)) = portal_carry(
                    &self.level,
                    portals.as_ref(),
                    PortalBody {
                        x: item.x,
                        y: item.y,
                        w: iw,
                        h: ih,
                        vx: item.vx,
                        vy: item.vy,
                    },
                    dt,
                    true,
                )
            {
                item.x = nx;
                item.y = ny;
                item.vx = nvx;
                item.vy = nvy;
                continue;
            }

            // Horizontal movement + wall collision
            item.x += item.vx * dt;
            let left_col = (item.x / TILE_SIZE).floor() as i32;
            let right_col = ((item.x + iw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (item.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((item.y + ih - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap([item.x, item.y, iw, ih], [tx, ty, tw, th]) {
                            if item.vx > 0.0 {
                                item.x = tx - iw;
                            } else if item.vx < 0.0 {
                                item.x = tx + tw;
                            }
                            item.vx = -item.vx;
                        }
                    }
                }
            }

            // Vertical movement + ground/ceiling collision
            item.y += item.vy * dt;
            let left_col = (item.x / TILE_SIZE).floor() as i32;
            let right_col = ((item.x + iw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (item.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((item.y + ih - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap([item.x, item.y, iw, ih], [tx, ty, tw, th]) {
                            if item.vy > 0.0 {
                                item.y = ty - ih;
                                if item.item_type == ItemType::Star {
                                    item.vy = STAR_JUMP_FORCE; // star bounces
                                } else {
                                    item.vy = 0.0;
                                }
                            } else if item.vy < 0.0 {
                                item.y = ty + th;
                                item.vy = 0.0;
                            }
                        }
                    }
                }
            }
        }

        // Player-item collision
        let px = self.player.x;
        let py = self.player.y;
        let pw = self.player.width;
        let ph = self.player.height;

        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].emerging {
                i += 1;
                continue;
            }
            if aabb_overlap(
                [px, py, pw, ph],
                [self.items[i].x, self.items[i].y, TILE_SIZE, TILE_SIZE],
            ) {
                let item = self.items.remove(i);
                match item.item_type {
                    ItemType::Mushroom => {
                        if !self.player.is_big {
                            self.player.set_size(true);
                        }
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                    ItemType::Star => {
                        self.star_timer = STAR_DURATION;
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                    ItemType::OneUp => {
                        self.lives += 1;
                        ctx.audio.play("oneup");
                    }
                    ItemType::FireFlower => {
                        if !self.player.is_big {
                            self.player.set_size(true);
                        }
                        self.player.is_fire = true;
                        self.score += ITEM_SCORE;
                        self.score_popups.push(ScorePopup {
                            x: item.x,
                            y: item.y - TILE_SIZE,
                            value: ITEM_SCORE,
                            timer: 0.0,
                        });
                        ctx.audio.play("mushroomeat");
                    }
                }
            } else {
                i += 1;
            }
        }

        // Remove items that fell off the map
        let map_bottom = self.level.height as f32 * TILE_SIZE + 100.0;
        self.items.retain(|item| item.y < map_bottom);
    }

    pub(crate) fn update_fireballs(&mut self, _ctx: &Context, dt: f32) {
        let portals = self.portal_pair();
        // Physics update
        let level = &self.level;
        for fb in &mut self.fireballs {
            if fb.exploding {
                fb.explode_timer += dt;
                fb.anim_timer += dt;
                continue;
            }
            fb.anim_timer += dt;

            // Gravity
            fb.vy += GRAVITY * dt;
            if fb.vy > MAX_Y_SPEED {
                fb.vy = MAX_Y_SPEED;
            }

            let fw = FIREBALL_SIZE;
            let fh = FIREBALL_SIZE;

            // Fireballs go through portals, but `mask[2] = true` (`fireball.lua:20`)
            // exempts them from the `inportal` fallback — hence `false` here. They
            // must cross a mouth's plane to be taken, not merely be found inside one.
            if let Some((nx, ny, nvx, nvy)) = portal_carry(
                &self.level,
                portals.as_ref(),
                PortalBody {
                    x: fb.x,
                    y: fb.y,
                    w: fw,
                    h: fh,
                    vx: fb.vx,
                    vy: fb.vy,
                },
                dt,
                false,
            ) {
                fb.x = nx;
                fb.y = ny;
                fb.vx = nvx;
                fb.vy = nvy;
                continue;
            }

            // Horizontal movement + wall collision → explode
            fb.x += fb.vx * dt;
            let left_col = (fb.x / TILE_SIZE).floor() as i32;
            let right_col = ((fb.x + fw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (fb.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((fb.y + fh - 0.01) / TILE_SIZE).floor() as i32;
            'h_check: for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap([fb.x, fb.y, fw, fh], [tx, ty, tw, th]) {
                            fb.exploding = true;
                            fb.explode_timer = 0.0;
                            break 'h_check;
                        }
                    }
                }
            }
            if fb.exploding {
                continue;
            }

            // Vertical movement + ground bounce / ceiling
            fb.y += fb.vy * dt;
            let left_col = (fb.x / TILE_SIZE).floor() as i32;
            let right_col = ((fb.x + fw - 0.01) / TILE_SIZE).floor() as i32;
            let top_row = (fb.y / TILE_SIZE).floor() as i32;
            let bottom_row = ((fb.y + fh - 0.01) / TILE_SIZE).floor() as i32;
            for row in top_row..=bottom_row {
                for col in left_col..=right_col {
                    if is_solid(get_tile(level, col, row)) {
                        let (tx, ty, tw, th) = tile_rect(col, row);
                        if aabb_overlap([fb.x, fb.y, fw, fh], [tx, ty, tw, th]) {
                            if fb.vy > 0.0 {
                                fb.y = ty - fh;
                                fb.vy = FIREBALL_BOUNCE; // bounce off floor
                            } else if fb.vy < 0.0 {
                                fb.y = ty + th;
                                fb.vy = 0.0;
                            }
                        }
                    }
                }
            }
        }

        // Fireball-enemy collision (separate pass to avoid borrow issues)
        let mut fb_explode = Vec::new();
        let mut enemy_kills = Vec::new();
        for (fi, fb) in self.fireballs.iter().enumerate() {
            if fb.exploding {
                continue;
            }
            for (ei, enemy) in self.enemies.iter().enumerate() {
                // A bullet-bill cannon has no hitbox at all, so a fireball flies
                // through the spot it occupies rather than bursting on it.
                if enemy.state == EnemyState::Dead || enemy.enemy_type.harmless() {
                    continue;
                }
                let eh = enemy_height(enemy.enemy_type, enemy.state);
                if aabb_overlap(
                    [fb.x, fb.y, FIREBALL_SIZE, FIREBALL_SIZE],
                    [enemy.x, enemy.y, PLAYER_SMALL_W, eh],
                ) {
                    fb_explode.push(fi);
                    // A buzzy beetle is immune: the fireball bursts against its
                    // shell and it keeps walking. That immunity is the entire
                    // reason the enemy exists. Firebars and geysers are hazards
                    // rather than creatures and can't be removed at all.
                    if !enemy.enemy_type.fireball_immune() && !enemy.enemy_type.indestructible() {
                        enemy_kills.push(ei);
                    }
                    break;
                }
            }
        }
        for &fi in &fb_explode {
            self.fireballs[fi].exploding = true;
            self.fireballs[fi].explode_timer = 0.0;
        }
        for &ei in &enemy_kills {
            // Bowser is the one thing a single fireball doesn't finish: five hits, and
            // the earlier four do nothing visible at all (`bowser.lua:176-181`). No
            // flash, no knockback — which is why he reads as invulnerable until the
            // fifth one drops him.
            if self.enemies[ei].enemy_type == EnemyType::Bowser {
                self.enemies[ei].hp = self.enemies[ei].hp.saturating_sub(1);
                if self.enemies[ei].hp > 0 {
                    continue;
                }
            }
            // Fire kills pay a flat per-kind rate, not the stomp combo ladder.
            self.score += self.enemies[ei].enemy_type.fire_points();
            self.enemies[ei].shotted();
        }

        // Remove expired fireballs
        let map_bottom = self.level.height as f32 * TILE_SIZE + 100.0;
        self.fireballs.retain(|fb| {
            if fb.exploding && fb.explode_timer >= FIREBALL_EXPLODE_TIME {
                return false;
            }
            if fb.y > map_bottom {
                return false;
            }
            true
        });
    }
}
