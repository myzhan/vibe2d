#!/usr/bin/env python3
"""
v18 autopilot 无限制跑分测试
不设得分上限，看最高能拿多少分。
"""
import asyncio
import json
import websockets

WS_URL = "ws://127.0.0.1:9229"
req_id = 0

# 游戏常量
BIRD_H = 27.0
PIPE_W = 50.0
PIPE_GAP = 70.0
GAP_HALF = PIPE_GAP / 2.0
GRAVITY = 500.0
JUMP_VY = -200.0
PIPE_SPEED = 200.0
DT = 1.0 / 60.0
BIRD_X = 128.0
BIRD_W = 36.0
BIRD_LEFT = BIRD_X - BIRD_W / 2.0
BIRD_RIGHT = BIRD_X + BIRD_W / 2.0
DEFAULT_GROUND_TOP = 258.0
FLAP_TARGET_OFFSET = 4.0
LOOKAHEAD = 120


async def rpc(ws, method, params=None):
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params:
        msg["params"] = params
    await ws.send(json.dumps(msg))
    resp = await asyncio.wait_for(ws.recv(), timeout=5)
    return json.loads(resp)


async def step_and_wait(ws, frames=1):
    r = await rpc(ws, "engine.getTime")
    fc_before = r["result"]["frame_count"]
    await rpc(ws, "engine.step", {"frames": frames})
    for _ in range(200):
        r = await rpc(ws, "engine.getTime")
        if r["result"]["frame_count"] >= fc_before + frames:
            return r
        await asyncio.sleep(0.005)
    return r


def sim_future_with_rule(y, vy, target_y, pipe_x, gap_y, n_frames, ground_top):
    cy, cvy = y, vy
    pipe_spd = PIPE_SPEED * DT
    for i in range(n_frames):
        flap = False
        if cy < 5.0 and cvy <= 0:
            flap = False
        elif cy > ground_top - BIRD_H - 15 and cvy >= 0:
            flap = True
        elif cy >= target_y and cvy > 0:
            flap = True
        if flap:
            cvy = JUMP_VY
        cvy += GRAVITY * DT
        cy += cvy * DT
        if cy < 0:
            cy, cvy = 0.0, 0.0
        cpx = pipe_x - pipe_spd * (i + 1)
        if cy + BIRD_H > ground_top:
            return i + 1
        if cpx < BIRD_RIGHT and cpx + PIPE_W > BIRD_LEFT:
            if cy < gap_y - GAP_HALF or cy + BIRD_H > gap_y + GAP_HALF:
                return i + 1
    return n_frames + 1


def get_relevant_pipes(pipes):
    return sorted(
        [p for p in pipes if p["x"] + PIPE_W > BIRD_LEFT],
        key=lambda p: p["x"]
    )


async def main():
    print("=" * 50)
    print("v18 Autopilot 无限制跑分测试")
    print("=" * 50)

    async with websockets.connect(WS_URL) as ws:
        # 初始化游戏
        await rpc(ws, "engine.pause")
        await rpc(ws, "game.setState", {"state": "idle"})
        await rpc(ws, "game.setState", {"state": "countdown"})
        await rpc(ws, "game.setState", {"state": "playing"})
        await rpc(ws, "game.setBirdY", {"y": 130.0, "vy": 0.0})

        last_score = 0
        frame = 0
        ground_top = DEFAULT_GROUND_TOP

        print("开始自动飞行...")

        while True:
            r = await rpc(ws, "game.inspect")
            res = r.get("result", {})
            state = res.get("state", "")
            score = res.get("score", 0)
            bird = res.get("bird", {})
            pipes = res.get("pipes", [])

            if state != "playing":
                print(f"\n[f{frame}] 游戏结束! state={state}, 最终得分: {score}")
                await rpc(ws, "engine.resume")
                break

            by = bird.get("y", 0.0)
            bvy = bird.get("vy", 0.0)

            if score != last_score:
                print(f"  [f{frame}] ★ 得分: {score} (y={by:.1f} vy={bvy:.1f})")
                last_score = score

            # 找最近的管道
            relevant_pipes = get_relevant_pipes(pipes)
            nearest = relevant_pipes[0] if relevant_pipes else None

            want_flap = False
            if nearest:
                pipe_x = nearest["x"]
                gap_y = nearest["gap_y"]
                target_y = gap_y + FLAP_TARGET_OFFSET

                survive_coast = sim_future_with_rule(
                    by, bvy, target_y, pipe_x, gap_y, LOOKAHEAD, ground_top)
                survive_flap = sim_future_with_rule(
                    by, JUMP_VY, target_y, pipe_x, gap_y, LOOKAHEAD, ground_top)

                if survive_coast > LOOKAHEAD and survive_flap > LOOKAHEAD:
                    if by < 5.0 and bvy <= 0:
                        want_flap = False
                    elif by > ground_top - BIRD_H - 15 and bvy >= 0:
                        want_flap = True
                    elif by >= target_y and bvy > 0:
                        want_flap = True
                elif survive_flap > survive_coast:
                    want_flap = True
            else:
                target = ground_top / 2.0
                if by >= target and bvy > 0:
                    want_flap = True

            if want_flap:
                await rpc(ws, "engine.simulateInput",
                          {"device": "keyboard", "action": "tap", "key": "Space"})

            # 每 120 帧打印状态
            if frame % 120 == 0:
                p_info = ""
                if nearest:
                    p_info = f" pipe=({nearest['x']:.0f},gap={nearest['gap_y']:.0f})"
                print(f"  [f{frame}] y={by:.1f} vy={bvy:.1f} sc={score}{p_info}")

            await step_and_wait(ws, 1)
            frame += 1

            if frame > 30000:
                print(f"\n[f{frame}] 帧数上限! 最终得分: {score}")
                await rpc(ws, "engine.resume")
                break

        print(f"\n{'=' * 50}")
        print(f"最终得分: {last_score}")
        print(f"总帧数: {frame}")
        print(f"{'=' * 50}")


asyncio.run(main())
