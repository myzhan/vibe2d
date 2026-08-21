#!/usr/bin/env python3
"""
mari0 帽子（hats）验证脚本

帽子是全游戏唯一**纯装饰**的东西：33 顶，画在马里奥描边之上，不碰任何碰撞盒。
数据来自 `hatconfigs.lua`（小）与 `bighatconfigs.lua`（大），绘制算术在
`game.lua:1306-1352`。

规则：
  - 每顶帽子在两种体型下有各自的 `x/y`（贴在精灵格上的位置）和 `height`。
    `height` **不是**图片高度，而是叠帽子时上面那顶被抬高的量 —— 故意小于图片，
    这样帽檐会压住下面那顶。
  - 位置还要叠一层**按姿态**的偏移 `hatoffsets[animationstate]`。跑、爬、游
    三种姿态再按子帧索引一次。
  - **原版 quirk**：`falling` 读的是 *running* 表，用起跳时停在的那一帧
    （`game.lua:1318`、`:1328`）。看着像笔误，表现也像 —— 同一次下落里帽子的
    位置取决于你是怎么起跳的。但大小两套分支都这么写，所以原版的
    `hatoffsets["falling"]` 是死数据。本脚本第 4 节专门验证这一点。
  - `hatoffsets["dead"]` 是 `false`，配合 `and hatoffsets[...]` 守卫，意思是
    「死的时候整顶不画」而不是「偏移为 0」。这条只影响渲染，快照看不出来。
  - 1 号 standard 是唯一被染成**衬衫色**的帽子（`game.lua:1339-1342`）——
    所以火焰马里奥的帽子不用第二张图就是白的。
  - 音爆彩虹会把整叠帽子**换成** 33 号 bestpony（`mario.lua:3133`）：
    不是加上去，是替换掉。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_hat_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

HAT_COUNT = 33
HAT_STANDARD = 1
HAT_BEST_PONY = 33

# hatconfigs.lua:1-10 / bighatconfigs.lua:1-11
SMALL = {"idle": [0, 0], "jump": [0, -1], "swim": [1, -1], "climb": [[2, 0], [2, -1]]}
SMALL_RUN = [[0, 0], [0, 0], [-1, -1]]
BIG = {"idle": [-4, -2], "jump": [-4, -4], "duck": [-5, -12], "swim": [-5, -4]}
BIG_RUN = [[-5, -4], [-4, -3], [-3, -2]]
# 大马里奥那条从来没被读到的 falling 值，恰好和 idle 相同 —— 第 4 节靠它区分
BIG_FALLING_DEAD_DATA = [-4, -2]

# variables.lua — 音爆彩虹的门槛
RAINBOOM_SPEED = 45.0 * T

PRESS = lambda k: [{"device": "keyboard", "action": "press", "key": k}]
FREE = lambda k: [{"device": "keyboard", "action": "release", "key": k}]
TAP = lambda k: [{"device": "keyboard", "action": "tap", "key": k}]
FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Space", "Up", "Down", "Left", "Right", "F", "H", "P")
]

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


async def setup(ws, world=1, level=1, sublevel=0, big=False):
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setStar", {"seconds": 99999})
    await rpc(ws, "game.setPlayerSize", {"size": "big" if big else "small"})
    await si(ws, 1, FREE_ALL)
    return await si(ws)


async def ground(ws, col=20, row=9):
    """从空中放下去，落到地面站定。

    不能直接把人摆在某一行就断言 idle：大小马里奥盒子高度不同，同一个 y 一个踩到地
    另一个还在半空 —— 这正是本脚本第一版把 idle 测成 fall 的原因。
    """
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": row * T, "vx": 0.0, "vy": 0.0})
    for _ in range(30):
        s = await si(ws, 4)
        if s["player"]["on_ground"] and s["player"]["anim_state"] == "idle":
            return s
    return s


async def afloat(ws, col=20, row=5):
    """水下离地悬着 —— 游泳精灵只在离地时用，沉底走路还是跑步帧。"""
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": row * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 2)
        if s["player"]["anim_state"] == "swim":
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 默认戴自己那顶帽子，33 顶都能戴")
    await rpc(ws, "game.setHats", {"hats": [HAT_STANDARD]})
    s = await setup(ws)
    check(
        "默认就是 1 号 standard（main.lua:1130-1132 每个玩家都从标准帽开始）",
        s["player"]["hats"] == [HAT_STANDARD]
        and s["player"]["hat_selection"] == [HAT_STANDARD],
        f"hats={s['player']['hats']} selection={s['player']['hat_selection']}",
    )
    bad = []
    for i in range(1, HAT_COUNT + 1):
        await rpc(ws, "game.setHats", {"hats": [i]})
        s = await si(ws)
        if s["player"]["hats"] != [i]:
            bad.append(i)
    check(f"{HAT_COUNT} 顶帽子逐一戴上都能生效", not bad, f"异常: {bad}" if bad else "1..33")

    # 越界必须被拒绝，而不是静默画错一顶
    for wrong in (0, HAT_COUNT + 1, 200):
        try:
            await rpc(ws, "game.setHats", {"hats": [wrong]})
            check(f"{wrong} 号越界应被拒绝", False, "居然接受了")
        except RuntimeError:
            check(f"{wrong} 号越界被拒绝", True)

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 可以叠，也可以不戴")
    await rpc(ws, "game.setHats", {"hats": [14, 2, 6]})
    s = await si(ws)
    check(
        "整叠帽子按自下而上的顺序保留（原版皮肤编辑器能堆的那种）",
        s["player"]["hats"] == [14, 2, 6],
        f"hats={s['player']['hats']}",
    )
    await rpc(ws, "game.setHats", {"hats": []})
    s = await si(ws)
    check("空列表就是光头", s["player"]["hats"] == [], f"hats={s['player']['hats']}")

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 偏移按姿态走，而且大小两套表不一样")
    await rpc(ws, "game.setHats", {"hats": [HAT_STANDARD]})

    s = await setup(ws, big=False)
    s = await ground(ws)
    check(
        f"小马里奥站着 {SMALL['idle']}",
        s["player"]["anim_state"] == "idle" and s["player"]["hat_offset"] == SMALL["idle"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )
    s = await si(ws, 3, TAP("Space"))
    check(
        f"小马里奥起跳 {SMALL['jump']} —— 帽子比站着低一像素",
        s["player"]["anim_state"] == "jump" and s["player"]["hat_offset"] == SMALL["jump"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )

    s = await setup(ws, big=True)
    s = await ground(ws)
    check(
        f"大马里奥站着 {BIG['idle']}，和小马里奥完全不同的表",
        s["player"]["anim_state"] == "idle" and s["player"]["hat_offset"] == BIG["idle"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )
    s = await si(ws, 3, TAP("Space"))
    check(
        f"大马里奥起跳 {BIG['jump']}",
        s["player"]["anim_state"] == "jump" and s["player"]["hat_offset"] == BIG["jump"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )
    await ground(ws)
    s = await si(ws, 4, PRESS("Down"))
    check(
        f"大马里奥蹲下 {BIG['duck']} —— 12 像素，帽子跟着头压下去",
        s["player"]["anim_state"] == "duck" and s["player"]["hat_offset"] == BIG["duck"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )
    await si(ws, 1, FREE_ALL)

    # 跑：三帧三个偏移，至少要能观察到不止一个值
    s = await setup(ws, big=True)
    await ground(ws)
    seen = set()
    for _ in range(40):
        s = await si(ws, 3, PRESS("Right"))
        if s["player"]["anim_state"] == "run":
            seen.add(tuple(s["player"]["hat_offset"]))
    await si(ws, 1, FREE_ALL)
    expected = {tuple(v) for v in BIG_RUN}
    check(
        "大马里奥跑动时帽子在三个偏移间循环（跑步表按子帧索引）",
        seen and seen <= expected and len(seen) > 1,
        f"见到 {sorted(seen)} / 表里 {sorted(expected)}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 原版 quirk：下落读的是**跑步**表，不是 falling 那一行")
    s = await setup(ws, big=True)
    await ground(ws)
    # 先跑起来把 run_frame 推上去，再原地腾空 —— 这就是原版会串味的情形
    await si(ws, 20, PRESS("Right"))
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": 20 * T, "y": 4 * T, "vx": 0.0, "vy": 200.0})
    s = await si(ws, 2)
    off = s["player"]["hat_offset"]
    check(
        "下落中的偏移落在跑步表里",
        s["player"]["anim_state"] == "fall" and off in BIG_RUN,
        f"{s['player']['anim_state']} {off} / 跑步表 {BIG_RUN}",
    )
    check(
        f"而且不是原版那条死数据 hatoffsets['falling'] = {BIG_FALLING_DEAD_DATA}",
        off != BIG_FALLING_DEAD_DATA,
        f"{off}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. 水下：游泳姿态有自己的偏移")
    await setup(ws, 2, 2, 1, big=True)
    s = await afloat(ws)
    check(
        f"大马里奥游泳 {BIG['swim']}",
        s["player"]["anim_state"] == "swim" and s["player"]["hat_offset"] == BIG["swim"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )
    await setup(ws, 2, 2, 1, big=False)
    s = await afloat(ws)
    check(
        f"小马里奥游泳 {SMALL['swim']}",
        s["player"]["anim_state"] == "swim" and s["player"]["hat_offset"] == SMALL["swim"],
        f"{s['player']['anim_state']} {s['player']['hat_offset']}",
    )

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 音爆彩虹把帽子换成 33 号 bestpony")
    await setup(ws, 1, 1, 0, big=False)
    await rpc(ws, "game.setRainboom", {"on": True})
    await rpc(ws, "game.setHats", {"hats": [HAT_STANDARD]})
    # 一对传送门：踩进去，从朝右那扇以超过 45 格/秒的速度出来
    await rpc(ws, "game.clearPortals")
    await rpc(
        ws,
        "game.setPortal",
        {"index": 0, "x": 24 * T, "y": 11 * T, "orientation": "up", "active": True},
    )
    await rpc(
        ws,
        "game.setPortal",
        {"index": 1, "x": 40 * T, "y": 11 * T, "orientation": "right", "active": True},
    )
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 24 * T, "y": 9 * T, "vx": 0.0, "vy": RAINBOOM_SPEED * 1.5},
    )
    s = await si(ws, 6)
    check(
        "破了音障就换上 bestpony，这条命戴的那顶被顶掉",
        s["player"]["hats"] == [HAT_BEST_PONY],
        f"hats={s['player']['hats']} 彩虹数={s.get('rainbooms')}",
    )
    # 但奖来的帽子只算**这一条命**：原版 `mario:new()` 每次重生都从 `mariohats`
    # 重新取一份（`mario.lua:97`），而彩虹只改了那份副本。
    check(
        "菜单里挑的那顶没被动过",
        s["player"]["hat_selection"] == [HAT_STANDARD],
        f"selection={s['player']['hat_selection']}",
    )
    await rpc(ws, "game.setState", {"state": "dead"})
    for _ in range(120):
        s = await si(ws, 8)
        if s["state"] == "playing":
            break
    check(
        "重生之后又戴回自己那顶 —— 彩虹帽不跨命",
        s["player"]["hats"] == [HAT_STANDARD],
        f"state={s['state']} hats={s['player']['hats']}",
    )
    await rpc(ws, "game.setRainboom", {"on": False})

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
