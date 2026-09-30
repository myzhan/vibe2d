#!/usr/bin/env python3
"""
mari0 小马里奥从下方顶砖块时，砖块顶上的怪物会被顶飞的回归测试。

原版中，砖块/问号块被从下方击中时，站在上面的敌人会朝远离方块中心
的方向翻飞出去并 +100 分（`mario.lua:2487-2503`）。本测试钉死这一行为。

用法：
  1. 启动游戏: cargo run -p mari0 --features vdp
  2. 运行: python3 -u examples/mari0/tests/vdp_block_enemy_fling_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

BLOCK_HIT_ENEMY_SCORE = 100

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


async def setup(ws):
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setPlayerSize", {"size": "small"})
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
    return await si(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("小马里奥顶砖块，怪物被顶飞")
    s = await setup(ws)
    score_before = s["score"]

    # 在 1-1 的空旷处搭一列：砖块在 (15,10)，地面在 (15,12)，敌人站在砖块上。
    brick_col = 15
    brick_row = 10
    bx = brick_col * T
    by = brick_row * T
    await rpc(ws, "game.setTile", {"col": brick_col, "row": brick_row, "type": "brick"})
    await rpc(ws, "game.setTile", {"col": brick_col, "row": brick_row + 2, "type": "ground"})
    await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": bx, "y": by - T})

    player_h = s["player"]["height"]
    await rpc(ws, "game.setPlayerPos", {"x": bx, "y": by + 2 * T - player_h, "vx": 0.0, "vy": 0.0})

    # 让怪物和马里奥安定下来
    for _ in range(10):
        s = await si(ws, 2)

    # 起跳
    await si(ws, 1, [{"device": "keyboard", "action": "press", "key": "Space"}])
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": "Space"}])

    # 等待顶到砖块或超时
    for _ in range(60):
        s = await si(ws, 2)
        if s["score"] >= score_before + BLOCK_HIT_ENEMY_SCORE:
            break

    enemy = next((e for e in s["enemies"] if e["type"] == "goomba"), None)
    check("敌人被顶飞后进入死亡状态",
          enemy is not None and enemy["state"] == "dead",
          f"state={enemy['state'] if enemy else None}")
    check("得分增加 100",
          s["score"] == score_before + BLOCK_HIT_ENEMY_SCORE,
          f"{score_before} -> {s['score']}")

    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
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
