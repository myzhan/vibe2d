#!/usr/bin/env python3
"""
mari0 1-2 金币房往返的回归测试：从食人花管道进金币房，再出来必须从
原管道（pipespawn）口升出，而不是落回关卡开头。

原版规则（game.lua:2306）：pipespawn 的参数匹配 `prevsublevel`——这一趟
出发的子关卡（主关卡为 0），而不是要进入的子关卡。smb/1-2_1 的
pipespawn 参数是 2（"从金币房回来"），出金币房（子关卡 2 → 1）时应当
从 (116,11) 的管道口升出。修复前端口按目标子关卡（1）查找，找不到就落
在默认出生点——正是用户报告的"再出来的时候场景不对"。

用法：
  1. 启动游戏: cargo run -p mari0 --features vdp
  2. 运行: python3 -u examples/mari0/tests/vdp_pipe_coin_room_roundtrip_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

# smb/1-2_1：食人花管道 (104,10) → 金币房(子关卡 2)；金币房 (13,12) 的
# 横管道 → 回子关卡 1；pipespawn 参数 2 在 (116,11)。
PLANT_PIPE = (104, 10)
COIN_ROOM_EXIT_PIPE = (13, 12)
EXPECTED_EXIT = (116, 11)

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


def release_all():
    return [{"device": "keyboard", "action": "release", "key": k}
            for k in ("Left", "Right", "Space", "F", "Down")]


async def run(ws):
    await rpc(ws, "engine.pause")

    print("\n─── 进金币房（食人花管道，向下进入） ───")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 2, "sublevel": 1})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setPlayerSize", {"size": "small"})
    await si(ws, 1, release_all())

    s = await si(ws)
    h = s["player"]["height"]
    # 站上管道口顶面（row 10 的上表面）
    px = PLANT_PIPE[0] * T
    py = PLANT_PIPE[1] * T - h
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerPos", {"x": px, "y": py, "vx": 0.0, "vy": 0.0})
    for _ in range(10):
        s = await si(ws, 2)
    await rpc(ws, "game.clearEnemies")
    check("玩家站在食人花管道口上", s["player"]["on_ground"],
          f"on_ground={s['player']['on_ground']} y={s['player']['y']}")

    # 按下进入管道，等待 0.7s 滑入 + 1.0s 停留后换关
    await si(ws, 1, [{"device": "keyboard", "action": "press", "key": "Down"}])
    await si(ws, 1, release_all())
    arrived = False
    for _ in range(200):
        s = await si(ws, 2)
        if s["player"]["x"] < 500:
            arrived = True
            break
    check("进入金币房（子关卡 2）", arrived, f"x={s['player']['x']} y={s['player']['y']}")

    print("\n─── 出金币房（横管道，向右进入） ───")
    await rpc(ws, "game.clearEnemies")
    # 紧贴横管道口 (13,12) 左侧站好，中线和管口同排
    sx = COIN_ROOM_EXIT_PIPE[0] * T - s["player"]["width"]
    sy = (COIN_ROOM_EXIT_PIPE[1] + 1) * T - h
    await rpc(ws, "game.setPlayerPos", {"x": sx, "y": sy, "vx": 0.0, "vy": 0.0})
    for _ in range(6):
        s = await si(ws, 2)
    # 向右走一步：被管口挡住即触发进入
    await si(ws, 3, [{"device": "keyboard", "action": "press", "key": "Right"}])
    await si(ws, 1, release_all())

    # 0.7s 滑入 + 1.0s 停留 → 换关；随后 1.0s 停留 + 0.7s 升出管道
    rose = False
    for _ in range(300):
        s = await si(ws, 2)
        if s["player"]["x"] > 3000:
            await rpc(ws, "game.clearEnemies")
            rose = True
            break
    for _ in range(140):
        s = await si(ws, 2)
        if s["player"]["on_ground"]:
            break

    ex, ey = EXPECTED_EXIT[0] * T, (EXPECTED_EXIT[1]) * T - h
    check("从原管道口 (116,11) 升出（而不是落回关卡开头）",
          rose and abs(s["player"]["x"] - ex) < 8.0,
          f"x={s['player']['x']} 期望 {ex}")
    check("升出后站回管口顶面", s["player"]["on_ground"] and abs(s["player"]["y"] - ey) < 8.0,
          f"y={s['player']['y']} 期望 {ey} on_ground={s['player']['on_ground']}")

    await si(ws, 1, release_all())
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
