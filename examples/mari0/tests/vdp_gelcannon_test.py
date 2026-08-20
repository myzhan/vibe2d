#!/usr/bin/env python3
"""
mari0 凝胶枪验证脚本

`playertypelist` 有三种随身装备（`variables.lua:7`）：portal（传送门枪）、
gelcannon（凝胶枪）、minecraft（挖方块模式，另一套玩法，没移植）。
凝胶枪不是「多一把枪」，而是**把传送门枪整个换掉**：选了它就没有传送门了，
左键喷蓝胶、右键喷橙胶，无限量。

规则（`game.lua:341-355` + `mario:shootgel`）：
  - 吃**按住**而不是按下，`gelcannondelay = 0.05` 秒一发 —— 所以是喷雾不是单发。
  - 出膛速度 `gelcannonspeed = 30` 格/秒，是喷嘴往下推的六倍，
    这才够把胶喷到对面墙上而不是脚底下。
  - 左右键同时按时蓝色优先（原版是 if/elseif）。
  - 喷出去的 blob 走的还是原来那套：撞到面就涂在**对面那一面**上，
    落到同色地板上会顺着流 —— 这些逻辑本来就有，凝胶枪只是多了个发射口。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_gelcannon_test.py

依赖: pip install websockets
"""
import asyncio
import json
import math
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0

GEL_CANNON_SPEED = 30.0
GEL_CANNON_DELAY = 0.05
GEL_GRAVITY = 50.0

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


def press(button):
    return [{"device": "mouse", "action": "press", "button": button}]


RELEASE = [
    {"device": "mouse", "action": "release", "button": b} for b in ("Left", "Right")
]


async def setup(ws, loadout):
    """装载一关实验室、清干净、选好装备。"""
    await rpc(ws, "game.setLevel", {"pack": "portal", "world": 1, "level": 1})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setPlayerType", {"player_type": loadout})
    await rpc(ws, "game.clearPortals")
    await si(ws, 1, RELEASE)
    await si(ws, 10)
    # 瞄向右上，喷出去的胶才会飞出去而不是砸脚下
    await rpc(ws, "engine.simulateInput", {"device": "mouse", "action": "move", "x": 460, "y": 180})
    return await si(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 默认是传送门枪，左键开门、不喷胶")
    s = await setup(ws, "portal")
    check("默认 loadout 是 portal", s["player_type"] == "portal", s["player_type"])
    blobs0 = len(s["gel_blobs"])
    s = await si(ws, 3, press("Left"))
    check("左键射出了传送门投射物", len(s["projectiles"]) > 0, f"{len(s['projectiles'])} 个")
    check(
        "而且没喷出凝胶",
        len(s["gel_blobs"]) <= blobs0,
        f"{blobs0} → {len(s['gel_blobs'])}（多出来的是关卡里喷嘴自己喷的）",
    )
    await si(ws, 3, RELEASE)

    # ── 2 ───────────────────────────────────────────────────────────
    section(f"2. 换成凝胶枪：传送门枪没了，左键喷蓝，出膛 {GEL_CANNON_SPEED} 格/秒")
    s = await setup(ws, "gel_cannon")
    check("loadout 切过来了", s["player_type"] == "gel_cannon", s["player_type"])
    proj0 = len(s["projectiles"])
    # 只走一帧，读到的就是刚出膛的那一颗
    s = await si(ws, 1, press("Left"))
    fresh = s["gel_blobs"]
    check("喷出来了", len(fresh) >= 1, f"{len(fresh)} 颗")
    check(
        "而且**没有**传送门投射物 —— 凝胶枪是替换不是叠加",
        len(s["projectiles"]) <= proj0,
        f"{proj0} → {len(s['projectiles'])}",
    )
    if fresh:
        blue = [b for b in fresh if b["gel"] == "blue"]
        check("左键喷的是蓝色", len(blue) >= 1, f"{[b['gel'] for b in fresh]}")
        if blue:
            speed = math.hypot(blue[0]["vx"], blue[0]["vy"]) / TILE_SIZE
            # 生成那一帧读到的就是净出膛速度：重力要到下一帧才加上去。
            check(
                f"出膛速度 {GEL_CANNON_SPEED} 格/秒",
                abs(speed - GEL_CANNON_SPEED) < 0.5,
                f"实测 {speed:.2f}",
            )
    await si(ws, 1, RELEASE)
    await si(ws, 150)

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 右键喷橙色")
    s = await si(ws, 1, press("Right"))
    fresh = s["gel_blobs"]
    orange = [b for b in fresh if b["gel"] == "orange"]
    check("右键喷的是橙色", len(orange) >= 1, f"{[b['gel'] for b in fresh]}")
    await si(ws, 1, RELEASE)
    await si(ws, 150)

    # ── 4 ───────────────────────────────────────────────────────────
    section(f"4. 吃按住不吃按下：{GEL_CANNON_DELAY} 秒一发，所以是喷雾")
    # 不能数「同时在场几颗」：胶一撞面就涂上去然后消失，出膛速度下每颗只活三四帧，
    # 所以任何一帧看过去都只有一两颗。真正能看出连喷的是**涂到了多少格**。
    await rpc(ws, "engine.simulateInput", {"device": "mouse", "action": "move", "x": 380, "y": 300})
    await setup(ws, "gel_cannon")
    await rpc(ws, "engine.simulateInput", {"device": "mouse", "action": "move", "x": 380, "y": 300})
    s = await si(ws)
    base = len(s["gels"])
    # 点一下就松手 = 一发
    await si(ws, 1, press("Left"))
    await si(ws, 1, RELEASE)
    s = await si(ws, 150)
    one_shot = len(s["gels"]) - base
    check("单发也能涂上", one_shot >= 1, f"涂了 {one_shot} 格")
    # 换个干净场地，按住半秒
    await setup(ws, "gel_cannon")
    await rpc(ws, "engine.simulateInput", {"device": "mouse", "action": "move", "x": 380, "y": 300})
    s = await si(ws)
    base = len(s["gels"])
    await si(ws, 30, press("Left"))
    await si(ws, 1, RELEASE)
    s = await si(ws, 150)
    held = len(s["gels"]) - base
    check(
        f"按住半秒涂得比单发多（{GEL_CANNON_DELAY} 秒一发 ≈ 10 发）",
        held > one_shot,
        f"单发 {one_shot} 格 → 按住 {held} 格",
    )
    before = len(s["gel_blobs"])
    s = await si(ws, 10)
    check(
        "松手就不再喷",
        len(s["gel_blobs"]) <= before,
        f"{before} → {len(s['gel_blobs'])}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. 左右键同按时蓝色优先（原版是 if/elseif）")
    await setup(ws, "gel_cannon")
    s = await si(ws, 1, press("Left") + press("Right"))
    both = s["gel_blobs"]
    check(
        "同按只喷蓝色",
        bool(both) and all(b["gel"] == "blue" for b in both),
        f"{[b['gel'] for b in both]}",
    )
    await si(ws, 1, RELEASE)

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 喷出去的胶照样涂到墙面上（沿用原有的落点逻辑）")
    await setup(ws, "gel_cannon")
    s = await si(ws)
    painted0 = len(s["gels"])
    # 往脚下斜前方喷一串，等它们落地涂完
    await rpc(ws, "engine.simulateInput", {"device": "mouse", "action": "move", "x": 380, "y": 300})
    await si(ws, 40, press("Left"))
    await si(ws, 1, RELEASE)
    s = await si(ws, 180)
    check(
        "涂到的格子变多了",
        len(s["gels"]) > painted0,
        f"{painted0} → {len(s['gels'])} 格",
    )
    check(
        "涂上去的确实是蓝色",
        any(
            g.get("top") == "blue" or g.get("left") == "blue" or g.get("right") == "blue"
            or g.get("bottom") == "blue"
            for g in s["gels"]
        ),
        f"{[{k: v for k, v in g.items() if v and k != 'cell'} for g in s['gels'][:4]]}",
    )

    # 复位，别把状态留给下一个脚本
    await rpc(ws, "game.setPlayerType", {"player_type": "portal"})
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
