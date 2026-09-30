#!/usr/bin/env python3
"""
mari0 高速下落踩龟不死验证脚本

问题：马里奥从高处落到乌龟上时，如果一帧内穿透太深，旧的
`player_feet < enemy_mid + 8.0` 判断会把合法踩踏误判成侧面撞击，
导致马里奥在乌龟变壳之前直接死掉。

本脚本以多种下落速度（包括接近终端速度的 3200 px/s）让马里奥
垂直砸向一只绿龟，断言：
  1. 马里奥始终存活（state == "playing"，命数不变）；
  2. 绿龟变成静止的壳（state == "shell"，|vx| < 1.0）。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_koopa_stomp_death_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0
MAX_Y_SPEED = 3200.0

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


FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Left", "Right", "Space", "F", "Down")
]


async def setup(ws):
    """1-1，清场，站在第 6 列平地上，命数钉死便于观察。"""
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setPlayerSize", {"size": "small"})
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": 6 * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 4)
        if s["player"]["on_ground"]:
            break
    return s


def koopa(snap):
    return next((e for e in snap["enemies"] if e["type"] == "koopa"), None)


async def drop_and_stomp(ws, vy):
    """把马里奥从 enemy 正上方以指定 vy 砸下，返回踩完那一帧。"""
    await rpc(ws, "game.spawnEnemy", {"type": "koopa", "x": 12 * T, "y": 11 * T})
    await si(ws, 4)
    s = await si(ws)
    k = koopa(s)
    if k is None:
        return None
    # 让马里奥的脚刚好贴着龟顶开始下落，vy 越大、一帧内穿透越深。
    player_h = s["player"]["height"]
    start_y = k["y"] - player_h + 1.0
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": k["x"], "y": start_y, "vx": 0.0, "vy": vy},
    )
    for _ in range(30):
        s = await si(ws, 2)
        k = koopa(s)
        if k is not None and k["state"] == "shell":
            return s
        # 马里奥死亡会立刻切状态，提前退出。
        if s["state"] != "playing":
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    for vy in (200.0, 800.0, 1600.0, MAX_Y_SPEED):
        section(f"vy={vy:.0f} px/s 砸向绿龟")
        s = await setup(ws)
        lives_before = s["lives"]
        s = await drop_and_stomp(ws, vy)
        k = koopa(s) if s else None

        check(
            "马里奥没死",
            s is not None and s["lives"] == lives_before and s["state"] == "playing",
            f"lives={s and s['lives']} state={s and s['state']}",
        )
        check(
            "绿龟被踩成静止的壳",
            k is not None and k["state"] == "shell" and abs(k["vx"]) < 1.0,
            f"state={k and k['state']} vx={k and round(k['vx'] / T, 2)}",
        )

    await si(ws, 1, FREE_ALL)
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
