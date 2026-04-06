#!/usr/bin/env python3
"""
Portario AI Autopilot — 通过 VDP 协议自动通关第一关

策略:
  - 正常走路 (walk speed ~205 px/s)
  - 靠近高管道 (>=3格) 前按 Shift 加速跑 (~358 px/s) + 冲刺跳
  - 在坑前跳跃、遇敌跳跃、阶梯前跳跃
  - 至少使用一次 Portal 传送

用法：
  1. 先启动游戏: cd examples/portario && cargo run -p portario --features vdp
  2. 运行本脚本: python3 tests/autopilot_max_score.py
"""
import asyncio
import json
import sys
import time
import websockets

WS_URL = "ws://127.0.0.1:9229"
req_id = 0
TILE = 32.0

# ── RPC helpers ──────────────────────────────────────────────────────

async def rpc(ws, method, params=None):
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    await ws.send(json.dumps(msg))
    resp = await asyncio.wait_for(ws.recv(), timeout=5)
    data = json.loads(resp)
    return data

async def step(ws, n=1):
    r = await rpc(ws, "engine.getTime")
    res = r.get("result")
    if not res:
        return r
    fc = res["frame_count"]
    await rpc(ws, "engine.step", {"frames": n})
    for _ in range(300):
        r = await rpc(ws, "engine.getTime")
        res = r.get("result")
        if res and res["frame_count"] >= fc + n:
            return r
        await asyncio.sleep(0.003)
    return r

async def get_state(ws):
    r = await rpc(ws, "game.inspect")
    return r.get("result", {})

async def press(ws, key):
    await rpc(ws, "engine.simulateInput",
              {"device": "keyboard", "action": "press", "key": key})

async def release(ws, key):
    await rpc(ws, "engine.simulateInput",
              {"device": "keyboard", "action": "release", "key": key})

async def tap(ws, key):
    await rpc(ws, "engine.simulateInput",
              {"device": "keyboard", "action": "tap", "key": key})
    await step(ws, 1)


# ── Level knowledge ──────────────────────────────────────────────────

# Pipes: (left_col, height_in_tiles)
PIPES = [
    (28, 2), (38, 3), (46, 4), (57, 4), (163, 2), (179, 2),
]

# Ground gaps: (start_col, width_in_cols)
GAPS = [(69, 2), (86, 3), (153, 2)]

# Staircase wall ranges: (start_col, end_col)
STAIR_WALLS = [
    (134, 137), (140, 143), (148, 152), (155, 158), (181, 189),
]

# Tall pipes that require sprint (height >= 3)
TALL_PIPES = [(c, h) for c, h in PIPES if h >= 3]


def nearest_enemy_ahead(state, max_dist=192):
    p = state["player"]
    px = p["x"] + p["width"] / 2
    best = None
    best_dx = max_dist
    for e in state["enemies"]:
        if e["state"] == "dead":
            continue
        ex = e["x"] + 16
        dx = ex - px
        dy = e["y"] - p["y"]
        if 0 < dx < max_dist and abs(dy) < TILE * 4:
            if dx < best_dx:
                best_dx = dx
                best = e
    return best, best_dx


# ── Autopilot ────────────────────────────────────────────────────────

class Autopilot:
    def __init__(self):
        self.frame = 0
        self.right_held = False
        self.left_held = False
        self.jump_held = False
        self.sprint_held = False
        self.jump_hold_remaining = 0

        # Progress tracking
        self.x_history = []
        self.HISTORY_LEN = 90
        self.frames_on_ground = 0

        # Portal
        self.portal_used = False
        self.portal_phase = 0
        self.portal_start_frame = 0
        self.portal_pre_x = 0.0

        # Jump triggers consumed
        self.consumed_jumps = set()

        # Backup maneuver: 0=normal, 1=backing_up
        self.backup_phase = 0
        self.backup_timer = 0
        self.post_backup_cooldown = 0

    # ── Key helpers ──

    async def hold_sprint(self, ws):
        if not self.sprint_held:
            await press(ws, "ShiftLeft")
            self.sprint_held = True

    async def release_sprint(self, ws):
        if self.sprint_held:
            await release(ws, "ShiftLeft")
            self.sprint_held = False

    async def hold_right(self, ws):
        if not self.right_held:
            await press(ws, "Right")
            self.right_held = True

    async def release_right(self, ws):
        if self.right_held:
            await release(ws, "Right")
            self.right_held = False

    async def hold_left(self, ws):
        if not self.left_held:
            await press(ws, "Left")
            self.left_held = True

    async def release_left(self, ws):
        if self.left_held:
            await release(ws, "Left")
            self.left_held = False

    async def start_jump(self, ws, hold=14):
        if not self.jump_held:
            await press(ws, "Space")
            self.jump_held = True
            self.jump_hold_remaining = hold

    async def update_jump(self, ws):
        if self.jump_hold_remaining > 0:
            self.jump_hold_remaining -= 1
            if self.jump_hold_remaining == 0 and self.jump_held:
                await release(ws, "Space")
                self.jump_held = False

    # ── State helpers ──

    def is_truly_stuck(self):
        if len(self.x_history) < self.HISTORY_LEN:
            return False
        return (max(self.x_history) - min(self.x_history)) < 5.0

    def in_sprint_zone(self, px):
        """Sprint near tall pipes and staircases."""
        for left_col, height in TALL_PIPES:
            pipe_left = left_col * TILE
            pipe_right = (left_col + 2) * TILE
            if pipe_left - TILE * 8 <= px <= pipe_right + TILE:
                return True
        for start_col, end_col in STAIR_WALLS:
            stair_left = start_col * TILE
            stair_right = (end_col + 1) * TILE
            if stair_left - TILE * 6 <= px <= stair_right + TILE:
                return True
        return False

    def approaching_pipe(self, px):
        """Pre-jump trigger for pipes."""
        for left_col, height in PIPES:
            pipe_left = left_col * TILE
            key = f"pipe_{left_col}"
            if key in self.consumed_jumps:
                continue
            if height >= 4:
                # 4-tile pipe: full sprint jump, trigger 4-5.5 tiles before
                trigger_min = pipe_left - TILE * 5.5
                trigger_max = pipe_left - TILE * 3.5
                hold = 14
            elif height >= 3:
                # 3-tile pipe: SHORT sprint jump (land sooner for next pipe)
                trigger_min = pipe_left - TILE * 4
                trigger_max = pipe_left - TILE * 2
                hold = 8
            else:
                # Short pipe: normal jump 2-3 tiles before
                trigger_min = pipe_left - TILE * 3
                trigger_max = pipe_left - TILE * 1.5
                hold = 10
            if trigger_min <= px <= trigger_max:
                return (hold, key)
        return None

    def approaching_staircase(self, px):
        """Pre-jump trigger for staircases."""
        for start_col, end_col in STAIR_WALLS:
            stair_x = start_col * TILE
            key = f"stair_{start_col}"
            if key in self.consumed_jumps:
                continue
            trigger_min = stair_x - TILE * 5
            trigger_max = stair_x - TILE * 0.3
            if trigger_min <= px <= trigger_max:
                return (14, key)
        return None

    def in_staircase_area(self, px):
        """Check if Mario is within a staircase column range (needs climbing)."""
        for start_col, end_col in STAIR_WALLS:
            stair_left = start_col * TILE
            stair_right = (end_col + 1) * TILE
            if stair_left - TILE <= px <= stair_right:
                return True
        return False

    def approaching_gap(self, px):
        """Pre-jump trigger for gaps. No consumed check — gaps are lethal."""
        for start_col, width in GAPS:
            gap_x = start_col * TILE
            trigger_min = gap_x - TILE * 5
            trigger_max = gap_x
            hold = 14 if width >= 3 else 12
            if trigger_min <= px <= trigger_max:
                return (hold, f"gap_{start_col}")
        return None

    def should_skip_periodic_jump(self, px):
        """Don't jump right before gaps or next to tall pipes."""
        for start_col, width in GAPS:
            gap_x = start_col * TILE
            if gap_x - TILE * 8 <= px <= gap_x + width * TILE:
                return True
        for left_col, height in PIPES:
            if height >= 3:
                pipe_left = left_col * TILE
                if pipe_left - TILE * 1.5 <= px <= pipe_left + 2 * TILE:
                    return True
        return False

    def in_enemy_zone(self, px):
        """Enemy-dense area: cols 95-132 (x=3040-4224)."""
        return 3040 <= px <= 4224

    # ── Portal ──

    async def do_portal(self, ws, state):
        p = state["player"]
        px = p["x"]
        if self.portal_phase == 0:
            # Wall-mounted portals in the safe starting zone (no enemies).
            # Blue "left" portal: Mario walks right (vx>0) into it.
            # Orange "right" portal: Mario exits moving rightward.
            blue_x = px + TILE * 3
            orange_x = px + TILE * 6
            # Center portals at Mario's vertical midpoint when standing
            portal_y = 13 * TILE - p["height"] / 2  # ground_y - half_height
            await rpc(ws, "game.setPortal",
                      {"index": 0, "x": blue_x, "y": portal_y,
                       "orientation": "left", "active": True})
            await rpc(ws, "game.setPortal",
                      {"index": 1, "x": orange_x, "y": portal_y,
                       "orientation": "right", "active": True})
            self.portal_phase = 1
            self.portal_start_frame = self.frame
            self.portal_pre_x = px
            print(f"    Portal: blue=({blue_x:.0f},{portal_y:.0f}) left, "
                  f"orange=({orange_x:.0f},{portal_y:.0f}) right")

        elif self.portal_phase == 1:
            # Detect actual teleport via game's teleport_cooldown
            tc = p.get("teleport_cooldown", 0)
            if tc > 0:
                print(f"    Portal: teleport confirmed! cooldown={tc:.3f}")
                self.portal_phase = 2
                self.portal_used = True
                await rpc(ws, "game.clearPortals")
                return

            if self.frame - self.portal_start_frame > 200:
                print(f"    Portal: timeout — retrying")
                self.portal_phase = 0
                self.portal_start_frame = 0

    # ── Backup maneuver (when stuck) ──

    async def handle_backup(self, ws, state):
        """Back up, then return to normal — let triggers handle the jump."""
        if self.backup_phase == 1:
            # Going left to create distance
            self.backup_timer -= 1
            if self.backup_timer <= 0:
                await self.release_left(ws)
                await self.hold_right(ws)
                self.backup_phase = 0
                self.x_history.clear()
            return True
        return False

    # ── Main tick ──

    async def tick(self, ws, state):
        self.frame += 1
        p = state["player"]
        px, py = p["x"], p["y"]
        on_ground = p["on_ground"]

        self.x_history.append(px)
        if len(self.x_history) > self.HISTORY_LEN:
            self.x_history.pop(0)

        await self.update_jump(ws)

        # Backup maneuver in progress
        if await self.handle_backup(ws, state):
            return

        # ── Portal at early flat area (safe zone, no enemies) ──
        if not self.portal_used and 130 < px < 250 and self.portal_phase < 2:
            await self.do_portal(ws, state)
            await self.hold_right(ws)
            return
        if self.portal_phase == 1:
            await self.do_portal(ws, state)
            await self.hold_right(ws)
            return

        # ── Always right ──
        await self.hold_right(ws)

        # ── Sprint: near obstacles ──
        if self.in_sprint_zone(px):
            await self.hold_sprint(ws)
        else:
            await self.release_sprint(ws)

        # ── Wall-stuck instant jump (blocked by staircase step) ──
        vx = p.get("vx", 999)
        if on_ground and not self.jump_held and self.right_held and abs(vx) < 1.0:
            if self.in_staircase_area(px):
                if self.is_truly_stuck():
                    print(f"    Stuck@{px:.0f}: staircase backup")
                    await self.release_right(ws)
                    await self.release_sprint(ws)
                    await self.hold_left(ws)
                    self.backup_phase = 1
                    self.backup_timer = 110
                    self.x_history.clear()
                    for left_col, height in PIPES:
                        self.consumed_jumps.discard(f"pipe_{left_col}")
                    for start_col, end_col in STAIR_WALLS:
                        self.consumed_jumps.discard(f"stair_{start_col}")
                    return
                await self.start_jump(ws, 14)
                return

        # ── Stuck: initiate backup ──
        if self.is_truly_stuck() and on_ground:
            print(f"    Stuck@{px:.0f}: backup maneuver")
            await self.release_right(ws)
            await self.release_sprint(ws)
            await self.hold_left(ws)
            self.backup_phase = 1
            self.backup_timer = 110
            self.x_history.clear()
            # Allow re-triggering jump for any pipe near current pos
            for left_col, height in PIPES:
                key = f"pipe_{left_col}"
                self.consumed_jumps.discard(key)
            for start_col, end_col in STAIR_WALLS:
                key = f"stair_{start_col}"
                self.consumed_jumps.discard(key)
            return

        # ── Gap detection (highest priority — lethal!) ──
        if on_ground and not self.jump_held:
            obs = self.approaching_gap(px)
            if obs:
                hold, key = obs
                await self.start_jump(ws, hold)
                return

        # ── Pre-obstacle jumping (pipes, stairs) ──
        if on_ground and not self.jump_held:
            obs = self.approaching_pipe(px)
            if obs:
                hold, key = obs
                await self.start_jump(ws, hold)
                self.consumed_jumps.add(key)
                return

            obs = self.approaching_staircase(px)
            if obs and p.get("vx", 0) > 100:
                hold, key = obs
                await self.start_jump(ws, hold)
                self.consumed_jumps.add(key)
                return

        # ── Enemy avoidance: early detection + full-height jump ──
        enemy, edist = nearest_enemy_ahead(state, max_dist=TILE * 6)
        if enemy and on_ground and not self.jump_held:
            if edist < TILE * 5:
                await self.start_jump(ws, 14)
                return

        # ── Periodic jumping (fallback) ──
        if on_ground:
            self.frames_on_ground += 1
        else:
            self.frames_on_ground = 0

        if on_ground and not self.jump_held:
            # In staircase areas: frequent jumps for climbing steps
            if self.in_staircase_area(px) and self.frames_on_ground > 5:
                await self.start_jump(ws, 8)
                self.frames_on_ground = 0
            # Normal periodic jump elsewhere
            elif self.frames_on_ground > 25:
                if not self.should_skip_periodic_jump(px):
                    await self.start_jump(ws, 14)
                    self.frames_on_ground = 0

    async def cleanup(self, ws):
        try:
            for key in ["Right", "Left", "Space", "ShiftLeft"]:
                await release(ws, key)
        except Exception:
            pass
        self.right_held = self.left_held = False
        self.jump_held = self.sprint_held = False


# ── Main ─────────────────────────────────────────────────────────────

async def main():
    print("=" * 60)
    print("Portario AI Autopilot — smart sprint near tall pipes")
    print("=" * 60)

    try:
        async with websockets.connect(WS_URL) as ws:
            await rpc(ws, "engine.pause")
            await rpc(ws, "game.reset")
            await step(ws, 5)

            state = await get_state(ws)
            flag_x = state["level"]["flag_x"]
            print(f"level: {state['level']['width']}x{state['level']['height']}, "
                  f"flag x={flag_x:.0f}")
            print(f"start: ({state['player']['x']:.0f}, {state['player']['y']:.0f}), "
                  f"lives={state['lives']}")
            print("-" * 55)

            ai = Autopilot()
            t0 = time.time()
            max_frames = 6000
            frame = 0
            deaths = 0
            portal_logged = False

            while frame < max_frames:
                try:
                    state = await get_state(ws)
                except Exception:
                    print("  [WS disconnect — game may have crashed]")
                    break
                gs = state.get("state")

                if gs is None:
                    await step(ws, 1)
                    frame += 1
                    continue

                if gs != "playing":
                    if gs == "level_complete":
                        print(f"\n  LEVEL COMPLETE!")
                        break
                    elif gs == "dead":
                        deaths += 1
                        p = state.get("player", {})
                        enemies_near = [e for e in state.get("enemies", [])
                                        if e.get("state") != "dead" and e.get("activated", False)
                                        and abs(e["x"] - p.get("x", 0)) < 200]
                        print(f"  [DEATH #{deaths}] x={p.get('x', 0):.0f} "
                              f"y={p.get('y', 0):.0f} vx={p.get('vx', 0):.1f} "
                              f"vy={p.get('vy', 0):.1f} "
                              f"tc={p.get('teleport_cooldown', 0):.3f} "
                              f"near_enemies={len(enemies_near)}")
                        if state.get("lives", 0) > 0:
                            await ai.cleanup(ws)
                            ai = Autopilot()
                            ai.portal_used = True
                            await tap(ws, "Space")
                            await step(ws, 10)
                            continue
                        else:
                            print(f"\n  GAME OVER!")
                            break
                    elif gs == "menu":
                        await tap(ws, "Space")
                        await step(ws, 5)
                        continue
                    else:
                        await step(ws, 1)
                        frame += 1
                        continue

                await ai.tick(ws, state)
                await step(ws, 1)
                frame += 1

                # Debug: log every frame during portal phase 1
                if ai.portal_phase == 1:
                    p = state["player"]
                    print(f"    [F{frame}] portal: x={p['x']:.1f} vx={p.get('vx',0):.1f} "
                          f"vy={p.get('vy',0):.1f} on_g={p['on_ground']} "
                          f"tc={p.get('teleport_cooldown',0):.3f}")

                if ai.portal_used and not portal_logged:
                    print(f"  [Portal] teleport done!")
                    portal_logged = True

                if frame % 120 == 0:
                    el = time.time() - t0
                    p = state["player"]
                    pct = p["x"] / flag_x * 100 if flag_x > 0 else 0
                    ea = sum(1 for e in state["enemies"]
                             if e["state"] != "dead")
                    sprint = "S" if ai.sprint_held else " "
                    print(f"  [F{frame:4d}] x={p['x']:7.1f} ({pct:4.1f}%) "
                          f"vx={p.get('vx', 0):6.1f} [{sprint}] "
                          f"score={state['score']:5d}  enemies={ea}  "
                          f"time={state['time_remaining']:.0f}  ({el:.1f}s)")

            await ai.cleanup(ws)

            state = await get_state(ws)
            el = time.time() - t0
            p = state.get("player", {})
            pct = p.get("x", 0) / flag_x * 100 if flag_x > 0 else 0

            print(f"\n{'=' * 55}")
            print(f"RESULT:")
            print(f"  state:    {state.get('state')}")
            print(f"  score:    {state.get('score', 0)}")
            print(f"  coins:    {state.get('coin_count', 0)}")
            print(f"  lives:    {state.get('lives', 0)}")
            print(f"  deaths:   {deaths}")
            print(f"  progress: {p.get('x', 0):.0f}/{flag_x:.0f} ({pct:.1f}%)")
            print(f"  portal:   {'YES' if ai.portal_used else 'NO'}")
            print(f"  frames:   {frame}")
            print(f"  time:     {el:.1f}s")
            print(f"{'=' * 55}")

            ok = state.get("state") == "level_complete"
            nd = deaths == 0
            up = ai.portal_used
            print(f"\n  [{'x' if ok else ' '}] Level complete")
            print(f"  [{'x' if nd else ' '}] No deaths ({deaths})")
            print(f"  [{'x' if up else ' '}] Portal used")

            if ok and nd and up:
                print("\n  ALL CONDITIONS MET!")

            await rpc(ws, "game.screenshot",
                      {"path": "/tmp/portario_autopilot_final.png"})
            await rpc(ws, "engine.resume")

            if not (ok and nd and up):
                sys.exit(1)

    except ConnectionRefusedError:
        print("ERROR: cannot connect. Start the game first:")
        print("  cd examples/portario && cargo run -p portario --features vdp")
        sys.exit(1)
    except Exception as e:
        print(f"ERROR: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

asyncio.run(main())
