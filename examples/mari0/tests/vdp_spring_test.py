#!/usr/bin/env python3
"""
mari0 弹簧验证脚本

实体 93，在 2-1 / 3-1 / 5-2 / 6-3 / 7-1 / 8-2 六关。它是**唯一可以蓄力的发射器**。

规则（`spring.lua` + `mario.lua:2254-2282`）：
  - 本体一格宽、**31/16 格高**，从格子里立起来（`y = cell - 31/16`），
    所以格子标的是底座，身体往上顶出快两格。
  - 落上去会**夺走控制权** `springtime = 0.2` 秒：`hitspring` 清零 speedy 和重力、
    钉住 x，把马里奥摆在弹簧面上，而他的高度直接由 `springytable[frame]` = {0, 0.5, 1}
    驱动 —— 也就是说他是**跟着动画下沉**，不是动画去追他。
  - 帧序是 2、3 然后往回（`frame = 6 - frame`），所以是**压下去再弹回来**，不是啪一下张开。
  - **那 0.2 秒里按住跳跃就是蓄力**：`springhighforce = 41` 对 `springforce = 24`，
    将近两倍，也是马里奥全场靠自己能到的最高速度的两倍多。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_spring_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0

# variables.lua:335-339
SPRING_TIME = 0.2
SPRING_FORCE = 24.0
SPRING_HIGH_FORCE = 41.0
SPRING_H = 31.0 / 16.0

HOLD_JUMP = [{"device": "keyboard", "action": "press", "key": "Space"}]
RELEASE_JUMP = [{"device": "keyboard", "action": "release", "key": "Space"}]

req_id = 0
FAILURES = []


async def rpc(ws, method, params=None):
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    await ws.send(json.dumps(msg))
    while True:
        reply = json.loads(await ws.recv())
        if reply.get("id") == req_id:
            if "error" in reply:
                raise RuntimeError(f"{method}: {reply['error']}")
            return reply.get("result")


async def si(ws, frames=1, inputs=None):
    """Step and inspect in one call — the autopilot's trick, and much faster."""
    params = {"frames": frames}
    if inputs:
        params["inputs"] = inputs
    return await rpc(ws, "engine.stepAndInspect", params)


def check(label, ok, detail=""):
    print(f"    {'OK  ' if ok else 'FAIL'} {label}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(label)


def section(title):
    print(f"\n─── {title} ───")


async def land_on(ws, spring):
    """Drop the player onto a spring from just above it."""
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": spring["x"], "y": spring["y"] - 40.0, "vy": 200.0},
    )


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 2-1 第 189 列有一个弹簧，一格宽、快两格高，底座落在格子上")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 2, "level": 1})
    await si(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 186 * TILE_SIZE, "y": 8 * TILE_SIZE})
    s = await si(ws, 20)
    check("弹簧在场", len(s["springs"]) == 1, f"{len(s['springs'])} 个")
    if not s["springs"]:
        return
    spring = s["springs"][0]
    check("一格宽", abs(spring["w"] - TILE_SIZE) < 0.01, f"{spring['w']}px")
    check(
        f"{SPRING_H:.4f} 格高（比一格高）",
        abs(spring["h"] / TILE_SIZE - SPRING_H) < 0.01,
        f"{spring['h'] / TILE_SIZE:.4f} 格",
    )
    check(
        "底座落在第 13 行的顶边（格子标的是底座，身体往上顶）",
        abs(spring["y"] + spring["h"] - 13 * TILE_SIZE) < 0.01,
        f"底边 y={spring['y'] + spring['h']}",
    )

    section("2. 落上去被接住：控制权交出去，人跟着动画下沉再弹回")
    await land_on(ws, spring)
    frames = []
    for _ in range(16):
        s = await si(ws, 2)
        frames.append((s["player"]["y"], s["spring_ride"], s["springs"][0]["frame"]))
        if s["spring_ride"] is None and s["player"]["vy"] < -1.0:
            break
    rides = [r for _, r, _ in frames if r is not None]
    check("进入了骑乘状态", bool(rides), f"{len(rides)} 帧")
    seq = [f for _, r, f in frames if r is not None]
    check(
        "帧序是压下去再弹回来（0→2→0），不是啪一下张开",
        seq and max(seq) >= 2 and seq[0] == 0 and seq[-1] < max(seq),
        f"帧序 {seq}",
    )
    ys = [y for y, r, _ in frames if r is not None]
    check(
        "人跟着一起下沉再抬起",
        len(ys) > 3 and max(ys) > ys[0] and ys[-1] < max(ys),
        f"y {[round(v) for v in ys]}",
    )
    check(
        f"骑乘时长约 {SPRING_TIME} 秒",
        rides and abs(max(r["timer"] for r in rides) - SPRING_TIME) < 0.05,
        f"最长 {max(r['timer'] for r in rides):.3f} 秒",
    )

    section("3. 不蓄力：以 24 格/秒弹出")
    await land_on(ws, spring)
    plain = None
    for _ in range(20):
        s = await si(ws, 2)
        if s["spring_ride"] is None and s["player"]["vy"] < -1.0:
            plain = -s["player"]["vy"] / TILE_SIZE
            break
    check(
        f"弹射速度约 {SPRING_FORCE} 格/秒",
        plain is not None and abs(plain - SPRING_FORCE) < 2.0,
        f"实测 {plain:.1f} 格/秒" if plain else "没弹出来",
    )
    await si(ws, 60)

    section("4. 那 0.2 秒里按住跳跃 → 41 格/秒，将近两倍")
    await land_on(ws, spring)
    charged = None
    for _ in range(20):
        s = await si(ws, 2, HOLD_JUMP)
        if s["spring_ride"] is not None and s["spring_ride"]["charged"]:
            pass
        if s["spring_ride"] is None and s["player"]["vy"] < -1.0:
            charged = -s["player"]["vy"] / TILE_SIZE
            break
    await si(ws, 1, RELEASE_JUMP)
    check(
        f"弹射速度约 {SPRING_HIGH_FORCE} 格/秒",
        charged is not None and abs(charged - SPRING_HIGH_FORCE) < 2.0,
        f"实测 {charged:.1f} 格/秒" if charged else "没弹出来",
    )
    if plain and charged:
        check(
            "蓄力值将近两倍",
            charged > 1.5 * plain,
            f"{plain:.1f} → {charged:.1f} 格/秒",
        )

    await rpc(ws, "engine.resume")


async def main():
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except (OSError, websockets.exceptions.WebSocketException) as exc:
        print(f"\n无法连接 {WS_URL}: {exc}")
        print("先启动: cargo run -p mari0 --features vdp")
        sys.exit(2)

    print()
    if FAILURES:
        print(f"✗ {len(FAILURES)} 项失败:")
        for f in FAILURES:
            print(f"    - {f}")
        sys.exit(1)
    print("✓ 全部通过")


if __name__ == "__main__":
    asyncio.run(main())
