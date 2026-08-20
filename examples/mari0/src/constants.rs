//! Gameplay constants, converted from Mari0's units.
//!
//! The original works in blocks and blocks/second with `1 block = 16px`
//! (`variables.lua:1-2`). This port renders at `TILE_SIZE = 32` because the
//! 512x480 virtual resolution is built around a 16x15 tile screen, so every
//! original constant appears here multiplied by 32 — the comment on each line
//! keeps the original value visible so it can be checked against the Lua source.

// ── Physics Constants (mari0-inspired, 1 block = 32px) ─────────────
pub(crate) const TILE_SIZE: f32 = 32.0;
pub(crate) const GRAVITY: f32 = 2560.0; // 80 blocks/s^2
/// Friction applied to horizontal speed **above** the walk/run limit
/// (`superfriction = 100`, `variables.lua:20`).
///
/// The limit is not a clamp: anything that throws you faster than you could accelerate
/// — a faith plate, orange gel, a portal — keeps its speed and bleeds it off at this
/// rate. Hard-clamping instead is what made a diagonal faith plate barely move you.
pub(crate) const SUPER_FRICTION: f32 = 100.0 * TILE_SIZE;
pub(crate) const GRAVITY_JUMPING: f32 = 960.0; // reduced while holding jump
pub(crate) const JUMP_VELOCITY: f32 = -512.0; // initial upward (walking)
pub(crate) const JUMP_VELOCITY_RUN: f32 = -608.0; // higher jump when sprinting (like original SMB)
pub(crate) const MAX_WALK_SPEED: f32 = 204.8; // 6.4 blocks/s
pub(crate) const MAX_RUN_SPEED: f32 = 358.4; // 11.2 blocks/s (sprint with fire/shift)
pub(crate) const WALK_ACCEL: f32 = 256.0; // 8 blocks/s^2
pub(crate) const RUN_ACCEL: f32 = 512.0; // 16 blocks/s^2 (sprint, fast acceleration)
pub(crate) const FRICTION: f32 = 448.0; // 14 blocks/s^2
pub(crate) const MAX_Y_SPEED: f32 = 3200.0; // terminal velocity
pub(crate) const STOMP_BOUNCE: f32 = -300.0; // bounce velocity after stomp

/// `oneuptext.png` is 16x8 — the "1UP" graphic that floats up instead of a number.
pub(crate) const ONEUP_TEXT_W: f32 = 16.0 * 2.0;
pub(crate) const ONEUP_TEXT_H: f32 = 8.0 * 2.0;

// ── Portal particles (variables.lua:197-200) ──
/// Dust drifting out of an open portal's mouth. One every `PORTAL_PARTICLE_TIME`, moving
/// outward along the portal's own facing, fading over its half-second life.
pub(crate) const PORTAL_PARTICLE_SPEED: f32 = 1.0 * TILE_SIZE;
pub(crate) const PORTAL_PARTICLE_TIME: f32 = 0.05;
pub(crate) const PORTAL_PARTICLE_DURATION: f32 = 0.5;
/// The wander added each frame, `math.random(-10, 10)/70` blocks/s.
pub(crate) const PORTAL_PARTICLE_WANDER: f32 = 10.0 / 70.0 * TILE_SIZE;

// ── The intro (intro.lua) ──
// Stabyourself's logo, shown once at launch: it fades in, is stabbed — the blood wipes
// upward out of a scissor — and fades out. Any key skips it.
/// How long the logo is on screen, and the black beat after it before the title.
pub(crate) const INTRO_DURATION: f32 = 2.5;
pub(crate) const INTRO_BLACK_AFTER: f32 = 0.3;
/// Fade in and out, at each end of `INTRO_DURATION`.
pub(crate) const INTRO_FADE_TIME: f32 = 0.5;
/// The timer starts *negative*, so the screen is black for a moment before the fade.
pub(crate) const INTRO_START: f32 = -0.2;
/// When the stab lands.
pub(crate) const INTRO_STAB_TIME: f32 = 0.5;
/// The logo is drawn from this origin inside its 512x512 sheet (`intro.lua:52`).
pub(crate) const INTRO_LOGO_ORIGIN: (f32, f32) = (142.0, 150.0);
/// How far the blood wipe travels, in the original's own units.
pub(crate) const INTRO_BLOOD_SPAN: f32 = 206.0 * 5.0;

// ── Interludes and death (variables.lua:166-169, :316-319) ──
/// The "world 1-1" card. Stretched 50% for the first level of a world.
pub(crate) const LEVELSCREEN_TIME: f32 = 2.4;
/// The blink between a level and one of its sublevels. Exactly two lead-ins long, which
/// is why it never gets as far as drawing anything.
pub(crate) const SUBLEVELSCREEN_TIME: f32 = 0.2;
/// "game over" and "congratulations!" both hold this long before the title screen.
pub(crate) const GAMEOVER_TIME: f32 = 7.0;
/// Black at each end of every card, with nothing drawn over it.
pub(crate) const BLACKTIME_SUB: f32 = 0.1;
/// The death throw: still for `DEATH_JUMP_TIME`, then thrown up at `DEATH_JUMP_FORCE` and
/// falling under `DEATH_GRAVITY` — half the world's, so the arc is slow enough to read.
pub(crate) const DEATH_TOTAL_TIME: f32 = 4.0;
pub(crate) const DEATH_JUMP_TIME: f32 = 0.3;
pub(crate) const DEATH_JUMP_FORCE: f32 = 17.0 * TILE_SIZE;
pub(crate) const DEATH_GRAVITY: f32 = 40.0 * TILE_SIZE;

// ── The gel cannon (variables.lua:392-393) ──
/// One of the three `playertypelist` loadouts. Picked in the menu, and it replaces the
/// portal gun outright: left click sprays blue, right click orange.
pub(crate) const GEL_CANNON_DELAY: f32 = 0.05;
pub(crate) const GEL_CANNON_SPEED: f32 = 30.0 * TILE_SIZE;

// ── The flagpole ending (variables.lua:322-333) ──
/// How long the slide down the pole takes, and how far it covers. The flag comes down
/// over exactly the same span, so the two move together.
pub(crate) const FLAG_DESCEND_TIME: f32 = 0.9;
pub(crate) const FLAG_Y_DISTANCE: f32 = (7.0 + 10.0 / 16.0) * TILE_SIZE;
/// Where Mario ends up: `68/16 + flagydistance`, less his own height
/// (`mario.lua:367`).
pub(crate) const FLAG_BOTTOM: f32 = 68.0 / 16.0 * TILE_SIZE + FLAG_Y_DISTANCE;
/// The flag sprite's own starting height, `49/16` (`game.lua:2470`).
pub(crate) const FLAG_IMG_START: f32 = 49.0 / 16.0 * TILE_SIZE;
/// Frame flip while sliding — twice as fast as the vine's climb.
pub(crate) const FLAG_CLIMB_FRAME_DELAY: f32 = 0.07;
/// He hangs at the bottom this long before letting go.
pub(crate) const FLAG_ANIM_DELAY: f32 = 0.6;
/// Then runs off at a fixed 4.27 blocks/s — not his own top speed.
pub(crate) const FLAG_RUN_SPEED: f32 = 4.27 * TILE_SIZE;
/// He disappears into the castle 6 blocks past the pole.
pub(crate) const FLAG_CASTLE_DIST: f32 = 6.0 * TILE_SIZE;
/// The clock is cashed in one unit per frame, at 50 points each.
pub(crate) const SCORE_SUBTRACT_SPEED: f32 = 1.0 / 60.0;
/// The castle's flag rises from here to 0 at this speed…
pub(crate) const CASTLE_FLAG_START_Y: f32 = 1.5 * TILE_SIZE;
pub(crate) const CASTLE_FLAG_SPEED: f32 = 3.0 * TILE_SIZE;
/// …but not before the sequence has been running this long. Without the floor the flag
/// would go up while the clock is still being counted down, and the two are meant to be
/// consecutive beats (`mario.lua:433-435`).
pub(crate) const CASTLE_MIN_TIME: f32 = 7.0;
/// Fireworks go off this far apart, and the level ends `FLAG_END_TIME` after the last.
pub(crate) const FIREWORK_DELAY: f32 = 0.55;
pub(crate) const FLAG_END_TIME: f32 = 2.0;
/// A firework's bang lands partway through its own life.
pub(crate) const FIREWORK_SOUND_TIME: f32 = 0.1;

// ── Underwater (variables.lua:51-73) ──
// Six levels are flagged `underwater` — 2-2_1, 5-2_1, 6-2_2, 7-2_1, 8-4_1 and M-1 —
// and in them the player runs an entirely separate movement function
// (`mario:underwatermovement`). Land physics in a water level is not merely wrong, it is
// unplayable: 2-2_1's terrain expects you to *float* over obstacles a land jump cannot
// clear, and a measured run gets 10 columns into 192 before it sticks.
/// Gravity while sinking, 9 blocks/s² against the land's 80. This is the float.
pub(crate) const UW_GRAVITY: f32 = 9.0 * TILE_SIZE;
/// …and while rising, 12. **Higher** than the sinking figure, which is the reverse of
/// the land pair (30 rising against 80 falling): a stroke shoots you up and is checked
/// quickly, then you drift back down slowly.
pub(crate) const UW_GRAVITY_JUMPING: f32 = 12.0 * TILE_SIZE;
/// One stroke, 5.9 blocks/s. `uwjumpforceadd` is 0, so unlike a land jump your speed
/// makes no difference to it.
pub(crate) const UW_JUMP_FORCE: f32 = 5.9 * TILE_SIZE;
/// Walking on the sea floor, 3.6 blocks/s — barely over half the land walk.
pub(crate) const UW_MAX_WALK_SPEED: f32 = 3.6 * TILE_SIZE;
/// Swimming, 5 blocks/s. **Faster than walking**, which is why the floor is not the
/// quick way anywhere and is the reason this cannot share the land limits.
pub(crate) const UW_MAX_AIR_SPEED: f32 = 5.0 * TILE_SIZE;
pub(crate) const UW_WALK_ACCEL: f32 = 8.0 * TILE_SIZE;
pub(crate) const UW_WALK_ACCEL_AIR: f32 = 8.0 * TILE_SIZE;
/// The surface. Rise so your feet are above this and you are pushed back down: you can
/// swim up to the waterline but never out of it (`mario.lua:1499-1501`).
pub(crate) const UW_MAX_HEIGHT: f32 = 2.5 * TILE_SIZE;
pub(crate) const UW_PUSH_DOWN_SPEED: f32 = 3.0 * TILE_SIZE;
/// Stroke animation: two frames stepped at the run animation's own rate.
pub(crate) const UW_SWIM_ANIM_SPEED: f32 = 10.0;

// ── Bubbles (variables.lua:70-73) ──
/// Mario breathes out every 1.2 or 1.6 seconds, alternating.
pub(crate) const BUBBLE_TIMES: [f32; 2] = [1.2, 1.6];
/// They rise at 2.3 blocks/s with a wandering ±0.5, and pop at the waterline.
pub(crate) const BUBBLE_SPEED: f32 = 2.3 * TILE_SIZE;
pub(crate) const BUBBLE_MARGIN: f32 = 0.5 * TILE_SIZE;
pub(crate) const BUBBLE_MAX_Y: f32 = 2.5 * TILE_SIZE;

// ── Ducking (mario.lua:2865-2878) ──
/// Crouching halves a big Mario's box and drops its top by the same amount, so his feet
/// stay put. Only a big Mario standing still on the ground can do it, and growing,
/// shrinking or taking a stroke underwater all cancel it.
pub(crate) const DUCK_HEIGHT: f32 = PLAYER_BIG_H / 2.0;

// Portal
pub(crate) const PORTAL_GUN_DELAY: f32 = 0.2;
pub(crate) const PROJECTILE_SPEED: f32 = 800.0;
pub(crate) const PORTAL_TELEPORT_COOLDOWN: f32 = 0.15;

// Portal animation (matches original mari0: 6 frames at 0.08s per frame)
pub(crate) const PORTAL_ANIM_FRAMES: u32 = 6;
pub(crate) const PORTAL_ANIM_DELAY: f32 = 0.08;

// Enemy
pub(crate) const ENEMY_SPEED: f32 = 64.0; // 2 blocks/s

// ── Piranha plant (variables.lua:280-285) ──
// The most widely used enemy in SMB: 22 of the 32 main levels place one.
/// Seconds spent extended before retracting.
pub(crate) const PLANT_OUT_TIME: f32 = 2.0;
/// Seconds spent retracted before rising again.
pub(crate) const PLANT_IN_TIME: f32 = 1.8;
/// Frame flip interval for the snapping mouth.
pub(crate) const PLANT_ANIM_DELAY: f32 = 0.15;
/// Travel distance, 23/16 blocks.
pub(crate) const PLANT_MOVE_DIST: f32 = 23.0 / 16.0 * TILE_SIZE;
/// Rise/fall speed, 2.3 blocks/s.
pub(crate) const PLANT_MOVE_SPEED: f32 = 2.3 * TILE_SIZE;
/// Collision height, 14/16 blocks (`plant.lua:13`) — shorter than everything else.
pub(crate) const PLANT_HEIGHT: f32 = 14.0 / 16.0 * TILE_SIZE;
/// How far **below** its cell a fully retracted plant's hitbox sits.
///
/// `plant:new` is handed the cell as the original's *1-based* row index and then
/// adds `9/16` (`plant.lua:11`), so the drop is one block of index shift plus the
/// plant tucking itself down inside the pipe. Every other enemy is lifted *up* by
/// its own height instead, which is why a plant cannot share that code path.
///
/// Two independent checks that this is the right number, both landing exactly:
/// at full extension (`PLANT_MOVE_DIST` above here) the hitbox's **bottom** sits on
/// the pipe's rim, and the sprite's **top** sits on the rim when retracted.
pub(crate) const PLANT_REST_DROP: f32 = (1.0 + 9.0 / 16.0) * TILE_SIZE;
/// How far above the hitbox the sprite's top edge sits.
///
/// `offsetY = 17` with the quad's origin at its own top-left (`plant.lua:29-33`),
/// less the 8px by which the original's whole world is drawn higher than its
/// coordinates read (tiles go to `(row-1)*16 - 8`, `game.lua:1005`). Net 9/16 of a
/// block. Skip the 8 and the plant rides half a tile high — visible as a gap
/// between the plant and the pipe it is supposed to be growing out of.
pub(crate) const PLANT_SPRITE_RISE: f32 = (17.0 - 8.0) / 16.0 * TILE_SIZE;
/// Rendered sprite size at this port's 2x scale.
///
/// The cell is **23** px tall, not 24 (`main.lua:464` steps rows by 23) — the sheet
/// has no padding, so a 24 here shears the next spriteset's head onto this one.
pub(crate) const PLANT_SPRITE_W: f32 = 16.0 * 2.0;
pub(crate) const PLANT_SPRITE_H: f32 = 23.0 * 2.0;
/// A retracted plant will not emerge while the player is within ±3 blocks
/// horizontally — standing on the pipe is what makes it safe to wait.
pub(crate) const PLANT_PLAYER_NEAR: f32 = 3.0 * TILE_SIZE;

// ── Flying koopa (variables.lua:105, 140-141) ──
/// Vertical travel of a hovering koopa, 7.5 blocks.
pub(crate) const KOOPA_FLYING_DISTANCE: f32 = 7.5 * TILE_SIZE;
/// Seconds for one full up-down cycle.
pub(crate) const KOOPA_FLYING_TIME: f32 = 7.0;
/// Reduced gravity used by a flying koopa once it loses its wings.
pub(crate) const KOOPA_FLYING_GRAVITY: f32 = 30.0 * TILE_SIZE;

// ── Firebar (variables.lua:276-278) ──
/// Degrees advanced per tick.
pub(crate) const FIREBAR_ANGLE_STEP: f32 = 11.25;
/// Seconds per tick — 3.4s for a full revolution.
pub(crate) const FIREBAR_DELAY: f32 = 3.4 / (360.0 / FIREBAR_ANGLE_STEP);
/// Spacing between fireballs along the bar, in blocks.
pub(crate) const FIREBAR_SEGMENT_SPACING: f32 = 0.5 * TILE_SIZE;

// ── Up-fire, the lava geyser (variables.lua:263-265) ──
pub(crate) const UPFIRE_FORCE: f32 = 19.0 * TILE_SIZE;
pub(crate) const UPFIRE_GRAVITY: f32 = 20.0 * TILE_SIZE;

// ── Lakitu and his spinies (variables.lua:143-149) ──
/// Seconds between thrown spiny eggs.
pub(crate) const LAKITO_THROW_TIME: f32 = 4.0;
/// How long before the throw he ducks into his cloud (the wind-up frame).
pub(crate) const LAKITO_HIDE_TIME: f32 = 0.5;
/// Seconds a shot lakitu stays gone before returning at the screen edge.
pub(crate) const LAKITO_RESPAWN: f32 = 16.0;
/// How far past the player he may drift before turning around, in blocks.
pub(crate) const LAKITO_SPACE: f32 = 4.0;
/// He aims at where the player *will be*, this many seconds ahead. Chasing the
/// current position would let you outrun him by simply holding a direction.
pub(crate) const LAKITO_DISTANCE_TIME: f32 = 1.5;
/// Speed of the one-way drift he settles into past `lakitoend`, in blocks/s.
pub(crate) const LAKITO_PASSIVE_SPEED: f32 = 3.0 * TILE_SIZE;
/// He stops throwing while this many spinies are already out (`lakito.lua:70`).
pub(crate) const LAKITO_MAX_SPINIES: usize = 3;
/// Upward toss given to a spiny egg, 10 blocks/s (`goomba.lua:57`).
pub(crate) const SPIKEY_TOSS_SPEED: f32 = 10.0 * TILE_SIZE;
/// A falling egg is lighter than everything else in the game: 30 blocks/s² rather
/// than the usual 80 (`goomba.lua:56`), which is what makes the lob readable.
pub(crate) const SPIKEY_FALL_GRAVITY: f32 = 30.0 * TILE_SIZE;
/// How far an egg must fall past its release point before it can hit the lakitu who
/// threw it (`goomba.lua:54`, `:132`), in blocks.
pub(crate) const SPIKEY_IGNORES_LAKITO_WITHIN: f32 = 2.0 * TILE_SIZE;
/// Frame flip interval shared by goombas and spinies (`goombaanimationspeed`).
pub(crate) const GOOMBA_ANIM_SPEED: f32 = 0.2;
/// Points for downing lakitu (`firepoints["lakito"]`, `variables.lua:36`).
pub(crate) const LAKITO_SCORE: u32 = 200;

// ── Springs (variables.lua:335-339) ──
/// How long the compression lasts. Also the window in which holding jump charges it.
pub(crate) const SPRING_TIME: f32 = 0.2;
/// Launch speed with the jump button held during the compression — nearly double.
pub(crate) const SPRING_HIGH_FORCE: f32 = 41.0 * TILE_SIZE;
/// …and without it.
pub(crate) const SPRING_FORCE: f32 = 24.0 * TILE_SIZE;
/// How far down the spring's surface has sunk at each of its three frames
/// (`springytable`), in blocks. Mario's height is driven straight off this, so he rides
/// the animation rather than being animated to match it.
pub(crate) const SPRING_Y_TABLE: [f32; 3] = [0.0, 0.5, 1.0];
/// A spring stands 31/16 blocks tall in a one-block column.
pub(crate) const SPRING_H: f32 = 31.0 / 16.0 * TILE_SIZE;
pub(crate) const SPRING_W: f32 = TILE_SIZE;

// ── Seesaws (variables.lua:136-138) ──
/// How hard one rider pulls, in blocks/s² **per unit of imbalance**. The speed
/// accumulates for as long as the sides are unequal, so a seesaw never reaches a
/// terminal velocity — it just runs out of rope.
pub(crate) const SEESAW_SPEED: f32 = 4.0 * TILE_SIZE;
/// What the rig falls at once the rope gives, and it is over seven times `SEESAW_SPEED`
/// — the collapse is meant to be unsurvivable if you are still standing on it.
pub(crate) const SEESAW_GRAVITY: f32 = 30.0 * TILE_SIZE;
/// Bleeds speed off, but only while the imbalance does *not* support the current
/// direction. Equal to `SEESAW_SPEED`, so stepping off exactly cancels your own pull.
pub(crate) const SEESAW_FRICTION: f32 = 4.0 * TILE_SIZE;
/// How close to the surface counts as riding, ±0.1 of a block
/// (`seesawplatform.lua:46`). The same figure the moving platforms use.
pub(crate) const SEESAW_RIDE_TOLERANCE: f32 = 0.1 * TILE_SIZE;
/// A seesaw platform hangs 17/16 of a block *above* the point it is slung from — 2/16
/// higher than a moving platform's 15/16 (`seesawplatform.lua:10` against
/// `platform.lua:12`).
///
/// Negative, and measured from the beam rather than from a cell corner: unlike
/// [`PLATFORM_CELL_DROP`] this is not folded together with a `+1` for the cell, because
/// the rope lengths are added to the beam's own height. Writing it in that other form is
/// a two-block error, and it shows up as the pair's total drop no longer matching the
/// rope.
pub(crate) const SEESAW_PLATFORM_DROP: f32 = -17.0 / 16.0 * TILE_SIZE;
/// Slack in the rope: with one platform hauled up to the beam the other hangs
/// `dist1 + dist2` minus this (`seesaw.lua:73`), which is what keeps the pair's total
/// drop constant.
pub(crate) const SEESAW_ROPE_SLACK: f32 = (2.0 + 2.0 / 16.0) * TILE_SIZE;

// ── Vines (variables.lua:287-297) ──
/// How fast a vine grows out of its block, 2.13 blocks/s.
pub(crate) const VINE_SPEED: f32 = 2.13 * TILE_SIZE;
/// How fast Mario climbs *up* one.
pub(crate) const VINE_MOVE_SPEED: f32 = 3.21 * TILE_SIZE;
/// Sliding down is exactly twice as fast (`vinemovespeed*2`).
pub(crate) const VINE_MOVE_DOWN_SPEED: f32 = 2.0 * VINE_MOVE_SPEED;
/// Seconds per climbing frame going up, and going down — the descent flickers twice
/// as fast because the delay is halved, not because there are more frames.
pub(crate) const VINE_FRAME_DELAY: f32 = 0.15;
pub(crate) const VINE_FRAME_DELAY_DOWN: f32 = VINE_FRAME_DELAY / 2.0;
/// Climb until Mario's head is this far from the top of the level and he leaves it
/// entirely — the trip into the bonus room starts here, not at the vine's own tip.
pub(crate) const VINE_ANIM_START: f32 = 4.0 * TILE_SIZE;
/// The bonus-stage intro's numbers. The vine grows this far before Mario starts
/// after it, which at `VINE_SPEED` is `VINE_ANIM_MARIO_START` seconds of him just
/// standing there — by which point the vine has already stopped, so what you see is
/// a fully grown vine being climbed.
pub(crate) const VINE_ANIM_GROW_HEIGHT: f32 = 6.0 * TILE_SIZE;
pub(crate) const VINE_ANIM_MARIO_START: f32 = 6.0 / 2.13;
/// He stops this far short of where the vine grew to, and lets go half a second later.
pub(crate) const VINE_ANIM_STOP: f32 = 1.75 * TILE_SIZE;
pub(crate) const VINE_ANIM_DROP_DELAY: f32 = 0.5;
/// The grabbable box is 10/16 of a block wide, narrower than the sprite.
pub(crate) const VINE_W: f32 = 10.0 / 16.0 * TILE_SIZE;
/// The box's bottom sits this far *above* the bottom edge of the block the vine came
/// out of, so the last stretch of sprite by the block is decoration you cannot hold.
pub(crate) const VINE_FOOT: f32 = 1.7 * TILE_SIZE;
/// The bonus-stage intro's vine and Mario both start on this row — one below a
/// 15-row level, which is why he is out of sight when the room fades in.
pub(crate) const VINE_INTRO_START_Y: f32 = 15.0 * TILE_SIZE;
/// …and in this column, the one every bonus room leaves a hole in its floor for.
pub(crate) const VINE_INTRO_COL: i32 = 4;
/// How far up that vine grows, `limit = 9+1/16` (`vine.lua:10`).
pub(crate) const VINE_INTRO_LIMIT: f32 = (9.0 + 1.0 / 16.0) * TILE_SIZE;

// ── The castle ending (variables.lua:346-353) ──
/// How long the chain hangs there after the axe is taken, before the bridge starts to go.
pub(crate) const CASTLE_CHAIN_DISAPPEAR: f32 = 0.38;
/// Gap between bridge tiles vanishing, and between the chain and the first tile.
pub(crate) const CASTLE_BRIDGE_DELAY: f32 = 0.06;
/// When Mario is released, **measured from Bowser's fall** rather than from the axe —
/// the timer is reset once at the fall (`mario.lua:517`).
pub(crate) const CASTLE_MARIO_MOVE: f32 = 1.07;
/// Fixed speed he runs at once released; there is no input during the sequence.
pub(crate) const CASTLE_MARIO_SPEED: f32 = 4.27 * TILE_SIZE;
/// He stops this many blocks short of the level's right edge, where the toad stands.
pub(crate) const CASTLE_STOP_FROM_END: f32 = 8.0;
/// When the next level begins (`castleanimationnextlevel`).
pub(crate) const CASTLE_NEXT_LEVEL: f32 = 9.47;
/// Bowser's death drop is **heavier than his own gravity** — 27.5 against 10.9
/// (`mario.lua:512`), so he plummets rather than floating down as he does when hopping.
pub(crate) const CASTLE_BOWSER_FALL_GRAVITY: f32 = 27.5 * TILE_SIZE;
/// The two tile ids the sequence names: the bridge deck and the chain that holds it.
pub(crate) const CASTLE_BRIDGE_TILE: u32 = 11;
pub(crate) const CASTLE_CHAIN_TILE: u32 = 10;

// ── Bowser (variables.lua:107-118) ──
/// Pacing speed while advancing on the player.
pub(crate) const BOWSER_SPEED_FORWARDS: f32 = 0.875 * TILE_SIZE;
/// …and while retreating, which is **more than twice as fast**. Walk past him and he
/// scrambles backwards quicker than he ever comes at you.
pub(crate) const BOWSER_SPEED_BACKWARDS: f32 = 1.875 * TILE_SIZE;
pub(crate) const BOWSER_JUMP_FORCE: f32 = 7.0 * TILE_SIZE;
pub(crate) const BOWSER_JUMP_DELAY: f32 = 1.0;
/// A tenth of the world's gravity — his hops float, and it's why he lands so slowly.
pub(crate) const BOWSER_GRAVITY: f32 = 10.9 * TILE_SIZE;
pub(crate) const BOWSER_FALL_SPEED: f32 = 8.25 * TILE_SIZE;
pub(crate) const BOWSER_ANIM_SPEED: f32 = 0.5;
/// Five fireballs, and only fireballs.
pub(crate) const BOWSER_HEALTH: u32 = 5;
pub(crate) const BOWSER_SCORE: u32 = 5000;
/// He throws hammers only from world 6 on (`bowser.lua:49`).
pub(crate) const BOWSER_HAMMER_WORLD: u32 = 6;
/// Gaps between his hammers — ten tenths and four long ones, drawn uniformly
/// (`bowserhammertable`), so hammers come in bursts with pauses between.
pub(crate) const BOWSER_HAMMER_TABLE: [f32; 14] = [
    0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.5, 1.0, 2.0, 1.0,
];
/// His patrol turns around at `startx - 1 - rand(2)` and `startx - 7 - rand(2)`, so
/// the beat is roughly six blocks wide and never quite the same twice.
pub(crate) const BOWSER_TURN_NEAR: f32 = 1.0 * TILE_SIZE;
pub(crate) const BOWSER_TURN_FAR: f32 = 7.0 * TILE_SIZE;
/// His body, 30x28 px — the only thing in the game bigger than big Mario.
pub(crate) const BOWSER_W: f32 = 30.0 / 16.0 * TILE_SIZE;
pub(crate) const BOWSER_H: f32 = 28.0 / 16.0 * TILE_SIZE;

// ── Bowser's fire breath (variables.lua:259-261) ──
pub(crate) const FIRE_SPEED: f32 = 4.69 * TILE_SIZE;
/// It drifts vertically towards the height it was aimed at, rather than travelling
/// straight — which is what makes ducking under one unreliable.
pub(crate) const FIRE_VER_SPEED: f32 = 2.0 * TILE_SIZE;
pub(crate) const FIRE_ANIM_DELAY: f32 = 0.05;
/// A breath is 24x8 px, so a block and a half wide and half a block tall.
pub(crate) const FIRE_W: f32 = 24.0 / 16.0 * TILE_SIZE;
pub(crate) const FIRE_H: f32 = 8.0 / 16.0 * TILE_SIZE;

// ── Squid, the bloober (variables.lua:253-257) ──
/// Sink rate, both while idling and while settling after a lunge. Slow — it *drifts*.
pub(crate) const SQUID_FALL_SPEED: f32 = 0.9 * TILE_SIZE;
/// Top speed of a lunge, sideways and upward.
pub(crate) const SQUID_X_SPEED: f32 = 3.0 * TILE_SIZE;
pub(crate) const SQUID_UP_SPEED: f32 = 3.0 * TILE_SIZE;
/// How hard it accelerates into a lunge.
pub(crate) const SQUID_ACCELERATION: f32 = 10.0 * TILE_SIZE;
/// Sideways distance that ends a lunge, in blocks.
pub(crate) const SQUID_LUNGE_DIST: f32 = 2.0 * TILE_SIZE;
/// How far it sinks after a lunge before idling again.
pub(crate) const SQUID_DOWN_DIST: f32 = 1.0 * TILE_SIZE;
/// Slack in the "am I level with the player yet" test that starts a lunge
/// (`squid.lua:80`), in blocks.
pub(crate) const SQUID_TRIGGER_SLACK: f32 = 0.0625 * TILE_SIZE;
/// Height of a big Mario, which that same test measures the player's head from.
pub(crate) const SQUID_TRIGGER_HEAD: f32 = 24.0 / 16.0 * TILE_SIZE;

// ── Flying fish (variables.lua:267-268) ──
/// The leap out of the water.
pub(crate) const FLYING_FISH_FORCE: f32 = 23.0 * TILE_SIZE;
/// Lighter than the world, so the arc hangs.
pub(crate) const FLYING_FISH_GRAVITY: f32 = 20.0 * TILE_SIZE;
/// Random gap between leaps (`math.random(6, 20)/10`).
pub(crate) const FLYING_FISH_MIN: f32 = 0.6;
pub(crate) const FLYING_FISH_MAX: f32 = 2.0;
/// Sideways speed is *the player's own* plus a random nudge in this range, in blocks/s
/// (`flyingfish.lua:11` — `math.random(10) - 5`, so -4..=5).
pub(crate) const FLYING_FISH_DRIFT: (i32, i32) = (-4, 5);

// ── Moving platforms (variables.lua:126-134) ──
/// Half a block thick — you stand on one, you don't stand *in* it.
pub(crate) const PLATFORM_HEIGHT: f32 = 8.0 / 16.0 * TILE_SIZE;
/// How far below its cell's top edge a platform hangs (`y - 15/16`).
pub(crate) const PLATFORM_CELL_DROP: f32 = (1.0 - 15.0 / 16.0) * TILE_SIZE;
/// Sideways travel and its period.
pub(crate) const PLATFORM_HOR_DIST: f32 = 3.3125 * TILE_SIZE;
pub(crate) const PLATFORM_HOR_TIME: f32 = 4.0;
/// Vertical travel and its period — over eight blocks, the tallest ride in the game.
pub(crate) const PLATFORM_VER_DIST: f32 = 8.625 * TILE_SIZE;
pub(crate) const PLATFORM_VER_TIME: f32 = 6.4;
/// Constant speed of the one-way shaft lifts.
pub(crate) const PLATFORM_JUST_SPEED: f32 = 3.5 * TILE_SIZE;
/// Seconds between a spawner's releases.
pub(crate) const PLATFORM_SPAWN_DELAY: f32 = 2.18;
/// Speed the bonus-stage platform slides at once headbutted.
pub(crate) const PLATFORM_BONUS_SPEED: f32 = 3.75 * TILE_SIZE;
/// Descent rate of a platform with someone standing on it — flat, never accelerating.
pub(crate) const PLATFORM_FALL_SPEED: f32 = 4.0 * TILE_SIZE;
/// How close to the surface counts as riding a *vertically* moving platform. The
/// horizontal case is exact instead; see `carry_rider`.
pub(crate) const PLATFORM_RIDE_TOLERANCE: f32 = 0.1 * TILE_SIZE;

// ── Hammer bro (variables.lua:240-251) ──
/// Patrol speed, 1.5 blocks/s. He shuffles rather than walks.
pub(crate) const HAMMERBRO_SPEED: f32 = 1.5 * TILE_SIZE;
/// He stays within one block of where he spawned — `startx - 1` to `startx`.
pub(crate) const HAMMERBRO_PATROL: f32 = 1.0 * TILE_SIZE;
/// The two gaps between throws he picks between (`hammerbrotime`).
pub(crate) const HAMMERBRO_TIME: [f32; 2] = [0.6, 1.6];
/// He raises the hammer this long before it leaves his hand — the tell.
pub(crate) const HAMMERBRO_PREPARE_TIME: f32 = 0.5;
pub(crate) const HAMMERBRO_ANIM_SPEED: f32 = 0.15;
/// Seconds between hops between floors.
pub(crate) const HAMMERBRO_JUMP_TIME: f32 = 3.0;
/// Upward kick for a hop to the floor above, 19 blocks/s.
pub(crate) const HAMMERBRO_JUMP_FORCE: f32 = 19.0 * TILE_SIZE;
/// …and for a hop *down*, which still starts upward, just gently.
pub(crate) const HAMMERBRO_JUMP_FORCE_DOWN: f32 = 6.0 * TILE_SIZE;
/// A downward hop keeps passing through floors until it is this far below where it
/// started, which is what picks out the next floor down.
pub(crate) const HAMMERBRO_DROP_THROUGH: f32 = 2.0 * TILE_SIZE;
/// Above this row he is forced to hop up, below it forced down — in blocks from the
/// top, as the original compares raw `self.y` against 12 and 6.
pub(crate) const HAMMERBRO_LOW_ROW: f32 = 12.0 * TILE_SIZE;
pub(crate) const HAMMERBRO_HIGH_ROW: f32 = 6.0 * TILE_SIZE;
/// He falls at half the world's rate, which is what makes his hops float.
pub(crate) const HAMMERBRO_GRAVITY: f32 = 40.0 * TILE_SIZE;
/// A hammer leaves his hand at 4 blocks/s sideways and 8 up, and falls at 25.
pub(crate) const HAMMER_SPEED: f32 = 4.0 * TILE_SIZE;
pub(crate) const HAMMER_TOSS_SPEED: f32 = 8.0 * TILE_SIZE;
pub(crate) const HAMMER_GRAVITY: f32 = 25.0 * TILE_SIZE;
pub(crate) const HAMMER_ANIM_SPEED: f32 = 0.05;

// ── Bullet bills and the cannons that fire them (variables.lua:235-238, :403) ──
/// Flight speed, 8 blocks/s — faster than the player can run away from.
pub(crate) const BULLET_BILL_SPEED: f32 = 8.0 * TILE_SIZE;
/// A bill removes itself after this long (`bulletbilllifetime`), since nothing else
/// will: it ignores terrain, so it would otherwise fly forever.
pub(crate) const BULLET_BILL_LIFETIME: f32 = 20.0;
/// Random gap between shots from one cannon, in seconds.
pub(crate) const BULLET_BILL_TIME_MIN: f32 = 1.0;
pub(crate) const BULLET_BILL_TIME_MAX: f32 = 4.5;
/// A cannon holds its fire while the player is within this many blocks either side,
/// so standing on top of one is safe.
pub(crate) const BULLET_BILL_RANGE: f32 = 3.0 * TILE_SIZE;
/// Cap on live bills from cannons. The `bulletbillstart` zone spawner ignores it.
pub(crate) const MAX_BULLET_BILLS: usize = 5;
/// A fresh cannon fires half a second after it appears (`bulletbill.lua:8`).
pub(crate) const BULLET_BILL_FIRST_SHOT: f32 = 0.5;
/// Random gap between the `bulletbillstart` zone's bills (`game.lua:830`).
pub(crate) const BULLET_BILL_ZONE_MIN: f32 = 0.5;
pub(crate) const BULLET_BILL_ZONE_MAX: f32 = 4.0;
/// Rows the zone spawner picks from, 0-based (`math.random(4, 12)` over 1-based rows).
pub(crate) const BULLET_BILL_ZONE_ROWS: (i32, i32) = (3, 11);

// ── Enemies killed by fire, a star or a shell (variables.lua:162-164) ──
/// Constant horizontal speed of a shot enemy, 4 blocks/s.
pub(crate) const SHOT_SPEED_X: f32 = 4.0 * TILE_SIZE;
/// Initial upward kick, 8 blocks/s.
pub(crate) const SHOT_JUMP_FORCE: f32 = 8.0 * TILE_SIZE;
/// Shot enemies fall at 60 blocks/s², not the world's 80.
pub(crate) const SHOT_GRAVITY: f32 = 60.0 * TILE_SIZE;
/// How long a shot enemy is kept around — long enough to fall clear of the screen.
pub(crate) const SHOT_DEATH_TIME: f32 = 3.0;

// ── Cheep-cheep (variables.lua:120-124) ──
pub(crate) const CHEEP_RED_SPEED: f32 = 1.8 * TILE_SIZE;
pub(crate) const CHEEP_WHITE_SPEED: f32 = 1.0 * TILE_SIZE;
/// Vertical bob speed.
pub(crate) const CHEEP_Y_SPEED: f32 = 0.3 * TILE_SIZE;
/// Bob amplitude, 1 block.
pub(crate) const CHEEP_HEIGHT: f32 = 1.0 * TILE_SIZE;
pub(crate) const SHELL_SPEED: f32 = 384.0; // 12 blocks/s (mari0)
pub(crate) const ENEMY_DEATH_TIME: f32 = 0.5;

// Block interaction
pub(crate) const BLOCK_BOUNCE_TIME: f32 = 0.2;
pub(crate) const BLOCK_BOUNCE_HEIGHT: f32 = 0.4 * TILE_SIZE; // 12.8px
pub(crate) const COIN_POPUP_TIME: f32 = 0.4;
pub(crate) const COIN_POPUP_SPEED: f32 = -320.0; // initial upward velocity
pub(crate) const SCORE_POPUP_TIME: f32 = 0.8;
pub(crate) const SCORE_POPUP_HEIGHT: f32 = 2.5 * TILE_SIZE; // 80px
pub(crate) const MULTI_COIN_TIMEOUT: f32 = 4.0;
pub(crate) const BRICK_BREAK_SCORE: u32 = 50;
pub(crate) const DEBRIS_GRAVITY: f32 = 1920.0; // 60*32

// Items (mushroom, star, 1-up)
pub(crate) const ITEM_POP_TIME: f32 = 0.7; // time to emerge from block
pub(crate) const ITEM_SPEED: f32 = 115.2; // 3.6 blocks/s horizontal
pub(crate) const ITEM_SCORE: u32 = 1000;
pub(crate) const STAR_JUMP_FORCE: f32 = -416.0; // 13 blocks/s upward
pub(crate) const STAR_ANIM_DELAY: f32 = 0.04;
pub(crate) const STAR_DURATION: f32 = 12.0; // seconds of invincibility

// Fireball (fire flower power-up)
pub(crate) const FIREBALL_SPEED: f32 = 480.0; // 15 blocks/s horizontal
pub(crate) const FIREBALL_BOUNCE: f32 = -320.0; // 10 blocks/s upward bounce
pub(crate) const FIREBALL_SIZE: f32 = 16.0; // 8px * 2 scale
pub(crate) const FIREBALL_EXPLODE_TIME: f32 = 0.12;
pub(crate) const FIREBALL_ANIM_DELAY: f32 = 0.04;
pub(crate) const MAX_FIREBALLS: usize = 2;

// Scoring
pub(crate) const COMBO_SCORES: [u32; 10] = [100, 200, 400, 500, 800, 1000, 2000, 4000, 5000, 8000];
pub(crate) const COIN_SCORE: u32 = 200;

// Player sizes (in pixels) — match tile size like original Mario
pub(crate) const PLAYER_SMALL_W: f32 = 32.0;
pub(crate) const PLAYER_SMALL_H: f32 = 32.0;
pub(crate) const PLAYER_BIG_W: f32 = 32.0;
pub(crate) const PLAYER_BIG_H: f32 = 64.0;

// Sprite render sizes (original cell × 2 scale, separate from collision box)
pub(crate) const MARIO_SPRITE_SCALE: f32 = 2.0;
pub(crate) const MARIO_SMALL_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_SMALL_SPRITE_H: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_BIG_SPRITE_W: f32 = 20.0 * MARIO_SPRITE_SCALE; // 40
pub(crate) const MARIO_BIG_SPRITE_H: f32 = 36.0 * MARIO_SPRITE_SCALE; // 72

// ── SMB Tileset IDs (smbtiles.png: 374×102, 22×6 grid, 17×17 cells) ──
// Tile 1 = empty sky. All other IDs map directly to smbtiles.png cells.
//
// Only the handful the game loop names live here; the ids that exist purely so
// `game.setTile` can accept a friendly name are in `vdp.rs`, which is where
// they're used and where they compile away with the feature.
pub(crate) const SMB_EMPTY: u32 = 1;
pub(crate) const SMB_BRICK: u32 = 7;
pub(crate) const SMB_QUESTION: u32 = 8;
pub(crate) const SMB_HIDDEN_BLOCK: u32 = 115;
