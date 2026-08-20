#!/usr/bin/env python3
"""
mari0 水下游泳 + 蹲下验证脚本

六关标了 `underwater`：2-2_1、5-2_1、6-2_2、7-2_1、8-4_1、M-1。这六关里玩家跑的是
**另一套移动系统**（`mario:underwatermovement`），不是陆地物理换个重力那么简单。
用陆地物理跑水下关不是「手感不对」而是**过不去**：改之前在 2-2_1 按右+跳 30 秒
只能走到第 10 格（共 192 格）。

规则（`mario:underwatermovement` + `mario:jump` 的 else 分支）：
  - 重力关系**和陆地相反**：陆地上升 30、下落 80（按住跳跃更飘）；
    水下上升 12、下落 9 —— 划一下很快被刹住，然后慢慢沉。这就是浮力感。
  - 划水（`uwjumpforce = 5.9`）**没有任何落地判定**：任何时候按跳跃都能划，
    这才叫游泳。而且力度是定值（`uwjumpforceadd = 0`），跟陆地不同，速度不影响跳多高。
  - 速度上限**离地比贴地高**：游泳 5 格/秒，沉底走路只有 3.6 —— 所以海底永远不是近路。
    水下没有「跑」：`underwatermovement` 从不读跑键，按住冲刺完全无效。
  - 水面是硬天花板（`uwmaxheight = 2.5`）：脚一旦高过水线就被 `uwpushdownspeed`
    压回去，能游到水面但游不出去。
  - 游泳精灵只在**离地**时用（`mario.lua:1516` 用 jumping/falling 判定），
    沉底走路还是普通跑步帧。

蹲下（`mario:duck`，和水下耦合在同一个移动函数里）：
  - 只有**大**马里奥、站在地上、没跳跃时能蹲；盒子高度减半而脚底不动。
  - 蹲着**不能走**（原版地面移动分支全都带 `ducking == false` 守卫）。
  - 变身和水下划水都会取消蹲下。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_swim_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
DT = 1.0 / 60.0

# variables.lua:51-68
UW_JUMP_FORCE = 5.9
UW_GRAVITY = 9.0
UW_GRAVITY_JUMPING = 12.0
UW_MAX_WALK_SPEED = 3.6
UW_MAX_AIR_SPEED = 5.0
UW_MAX_HEIGHT = 2.5
# 陆地对照 (variables.lua:39-43)
GRAVITY = 80.0
GRAVITY_JUMPING = 30.0

UW_LEVELS = [(2, 2, 1), (5, 2, 1), (6, 2, 2), (7, 2, 1), (8, 4, 1)]

PRESS = lambda k: [{"device": "keyboard", "action": "press", "key": k}]
FREE = lambda k: [{"device": "keyboard", "action": "release", "key": k}]
TAP = lambda k: [{"device": "keyboard", "action": "tap", "key": k}]
FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Space", "Up", "Down", "Left", "Right", "F")
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


async def setup(ws, world, level, sublevel=0):
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    # 免疫敌人：这一套测的是移动，不是战斗；水下关有乌贼和鱼到处游
    await rpc(ws, "game.setStar", {"seconds": 99999})
    await si(ws, 1, FREE_ALL)
    return await si(ws)


async def open_water(ws, col=20, row=7):
    """把人放到一片开阔水域，速度归零。"""
    await si(ws, 1, FREE_ALL)
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": col * TILE_SIZE, "y": row * TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    return await si(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 六关标了 underwater（M-1 是 mappack 自带的第六关）")
    for world, level, sub in UW_LEVELS:
        s = await setup(ws, world, level, sub)
        name = f"{world}-{level}_{sub}"
        check(f"{name} 是水下关", s["level"]["underwater"], f"{s['level']['name']}")
        check(f"{name} 用水下配色与音乐", s["level"]["spriteset"] == 4 and s["level"]["music"] == 5,
              f"spriteset={s['level']['spriteset']} music={s['level']['music']}")

    # ── 2 ───────────────────────────────────────────────────────────
    section(f"2. 划水 {UW_JUMP_FORCE} 格/秒，而且**没有落地判定** —— 半空也能划")
    await setup(ws, 2, 2, 1)
    await open_water(ws)
    s = await si(ws, 1, TAP("Space"))
    got = -s["player"]["vy"] / TILE_SIZE
    # 量到的是划水力度减掉一帧上升重力
    expect = UW_JUMP_FORCE - UW_GRAVITY_JUMPING * DT
    check(
        f"一次划水给 {UW_JUMP_FORCE} 格/秒（实测值 = 力度 − 一帧上升重力）",
        abs(got - expect) < 0.05,
        f"实测 {got:.3f}，期望 {expect:.3f}",
    )
    check("划水后离地", not s["player"]["on_ground"])
    # 点一下就松手的划水，整段上升用的是**下沉**重力 9 而不是上升重力 12：
    # `jumping` 一松键就置 false（原版 `mario:stopjump` 同样如此），而 12 只在
    # jumping 为真时生效。所以 -5.7 要 38 帧才被 9 格/秒² 刹到 0，不是十几帧。
    s = await si(ws, 50)
    check(
        "点一下松手，靠下沉重力慢慢刹住再往下沉",
        s["player"]["vy"] > 0,
        f"50 帧后 vy={s['player']['vy'] / TILE_SIZE:.2f} 格/秒",
    )
    s = await si(ws, 1, TAP("Space"))
    check(
        "半空中再按一次还能划（陆地这时候按是没反应的）",
        s["player"]["vy"] < 0,
        f"vy={s['player']['vy'] / TILE_SIZE:.2f} 格/秒",
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. 重力和陆地反过来：上升 {UW_GRAVITY_JUMPING} / 下沉 {UW_GRAVITY}")
    await open_water(ws)
    s = await si(ws, 1, PRESS("Space"))
    v0 = s["player"]["vy"]
    s = await si(ws, 10, PRESS("Space"))
    rise = (s["player"]["vy"] - v0) / TILE_SIZE / (10 * DT)
    await si(ws, 1, FREE("Space"))
    check(
        f"上升中 {UW_GRAVITY_JUMPING} 格/秒²",
        abs(rise - UW_GRAVITY_JUMPING) < 0.2,
        f"实测 {rise:.2f}",
    )
    await open_water(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 7 * TILE_SIZE, "vy": 10.0})
    s = await si(ws)
    v0 = s["player"]["vy"]
    s = await si(ws, 10)
    sink = (s["player"]["vy"] - v0) / TILE_SIZE / (10 * DT)
    check(f"下沉中 {UW_GRAVITY} 格/秒²", abs(sink - UW_GRAVITY) < 0.2, f"实测 {sink:.2f}")
    check(
        "上升重力**大于**下沉重力（陆地是反的：30 对 80）",
        rise > sink and GRAVITY_JUMPING < GRAVITY,
        f"水下 {rise:.0f} > {sink:.0f}；陆地 {GRAVITY_JUMPING:.0f} < {GRAVITY:.0f}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section(f"4. 游泳 {UW_MAX_AIR_SPEED} 比沉底走路 {UW_MAX_WALK_SPEED} 快 —— 海底不是近路")
    await open_water(ws, 20, 12)
    s = await si(ws, 200, PRESS("Right"))
    walk = s["player"]["vx"] / TILE_SIZE
    check("沉在海底走路", s["player"]["on_ground"])
    check(f"走路上限 {UW_MAX_WALK_SPEED} 格/秒", abs(walk - UW_MAX_WALK_SPEED) < 0.05,
          f"实测 {walk:.2f}")
    await si(ws, 1, FREE("Right"))
    await open_water(ws, 20, 7)
    peak = 0.0
    for _ in range(50):
        s = await si(ws, 3, PRESS("Right") + TAP("Space"))
        if not s["player"]["on_ground"]:
            peak = max(peak, s["player"]["vx"] / TILE_SIZE)
    await si(ws, 1, FREE("Right"))
    check(f"游泳上限 {UW_MAX_AIR_SPEED} 格/秒", abs(peak - UW_MAX_AIR_SPEED) < 0.05,
          f"实测 {peak:.2f}")
    check("游得比走得快", peak > walk, f"{peak:.2f} > {walk:.2f}")

    # 冲刺键无效
    await open_water(ws, 20, 12)
    s = await si(ws, 200, PRESS("Right") + PRESS("F"))
    sprint = s["player"]["vx"] / TILE_SIZE
    await si(ws, 1, FREE_ALL)
    check(
        "按住冲刺键也还是 3.6（水下没有跑）",
        abs(sprint - UW_MAX_WALK_SPEED) < 0.05,
        f"实测 {sprint:.2f}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. 水面是硬天花板：脚过不了第 {UW_MAX_HEIGHT} 格")
    await open_water(ws, 20, 4)
    highest = 99.0
    for _ in range(60):
        s = await si(ws, 2, TAP("Space"))
        highest = min(highest, (s["player"]["y"] + s["player"]["height"]) / TILE_SIZE)
    await si(ws, 1, FREE_ALL)
    check(
        f"一直划水，脚最高只到第 {UW_MAX_HEIGHT} 格附近",
        UW_MAX_HEIGHT - 0.2 <= highest <= UW_MAX_HEIGHT + 0.2,
        f"最高 {highest:.3f} 格",
    )

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 游泳精灵只在离地时用，沉底走路还是跑步帧")
    await open_water(ws, 20, 7)
    frames = set()
    state_air = None
    for _ in range(20):
        s = await si(ws, 3, TAP("Space"))
        if not s["player"]["on_ground"]:
            state_air = s["player"]["anim_state"]
    check("离地时是 swim", state_air == "swim", f"{state_air}")
    await open_water(ws, 20, 12)
    s = await si(ws, 60, PRESS("Right"))
    check(
        "沉底走路是 run，不是 swim",
        s["player"]["anim_state"] == "run",
        f"{s['player']['anim_state']} og={s['player']['on_ground']}",
    )
    await si(ws, 1, FREE_ALL)

    # ── 7 ───────────────────────────────────────────────────────────
    section("7. 陆地关卡没被带跑：1-1 的重力和跳跃还是陆地那套")
    s = await setup(ws, 1, 1)
    check("1-1 underwater = false", not s["level"]["underwater"])
    await rpc(ws, "game.setPlayerPos", {"x": 10 * TILE_SIZE, "y": 11 * TILE_SIZE, "vx": 0.0, "vy": 0.0})
    await si(ws, 30)
    s = await si(ws, 1, TAP("Space"))
    land_jump = -s["player"]["vy"] / TILE_SIZE
    check(
        "陆地跳跃还是 16 格/秒那一档（远高于水下的 5.9）",
        land_jump > 10.0,
        f"实测 {land_jump:.2f} 格/秒",
    )
    # 陆地半空按跳跃必须没反应 —— 这正是水下唯一被去掉的那个守卫，
    # 要是漏改成全局的，陆地就变成能无限二段跳了。
    s = await si(ws, 15)
    v_air = s["player"]["vy"]
    s = await si(ws, 1, TAP("Space"))
    check(
        "陆地半空按跳跃没反应（落地判定只在水下才去掉）",
        s["player"]["vy"] >= v_air,
        f"半空 vy {v_air / TILE_SIZE:.2f} → 按跳跃后 {s['player']['vy'] / TILE_SIZE:.2f}",
    )
    await si(ws, 1, FREE_ALL)

    # ── 8 ───────────────────────────────────────────────────────────
    section("8. 蹲下：只有大马里奥、盒子减半、脚不动、蹲着不能走")
    await setup(ws, 1, 1)
    await rpc(ws, "game.setPlayerPos", {"x": 10 * TILE_SIZE, "y": 11 * TILE_SIZE, "vx": 0.0, "vy": 0.0})
    await si(ws, 30)
    s = await si(ws, 4, PRESS("Down"))
    check("小马里奥蹲不下去", not s["player"]["ducking"] and s["player"]["height"] == 32.0,
          f"ducking={s['player']['ducking']} h={s['player']['height']:.0f}")
    await si(ws, 1, FREE("Down"))

    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await si(ws, 30)
    s = await si(ws)
    y0, h0 = s["player"]["y"], s["player"]["height"]
    check("大马里奥站立高 64px", h0 == 64.0, f"h={h0:.0f}")
    s = await si(ws, 3, PRESS("Down"))
    y1, h1 = s["player"]["y"], s["player"]["height"]
    check("蹲下了", s["player"]["ducking"] and s["player"]["anim_state"] == "duck",
          f"ducking={s['player']['ducking']} anim={s['player']['anim_state']}")
    check("盒子高度减半", abs(h1 - h0 / 2) < 0.01, f"{h0:.0f} → {h1:.0f}")
    check("脚底一点没动", abs((y1 + h1) - (y0 + h0)) < 0.01,
          f"{y0 + h0:.1f} → {y1 + h1:.1f}")
    x0 = s["player"]["x"]
    s = await si(ws, 40, PRESS("Down") + PRESS("Right"))
    check("蹲着按右一步都走不动", abs(s["player"]["x"] - x0) < 0.01,
          f"位移 {(s['player']['x'] - x0) / TILE_SIZE:.4f} 格")
    s = await si(ws, 3, FREE("Down") + FREE("Right"))
    check("松开就站起来，脚底还是原处",
          not s["player"]["ducking"] and s["player"]["height"] == 64.0
          and abs((s["player"]["y"] + s["player"]["height"]) - (y0 + h0)) < 0.01,
          f"h={s['player']['height']:.0f} 脚底={s['player']['y'] + s['player']['height']:.1f}")

    # ── 9 ───────────────────────────────────────────────────────────
    section("9. 水下不能蹲，而且划水会取消蹲下")
    await setup(ws, 2, 2, 1)
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await open_water(ws, 20, 12)
    s = await si(ws, 60, PRESS("Down"))
    check(
        "水下按下键不会蹲（下键在水里没有蹲的语义）",
        not s["player"]["ducking"],
        f"ducking={s['player']['ducking']}",
    )
    await si(ws, 1, FREE_ALL)

    # ── 10 ──────────────────────────────────────────────────────────
    section("10. 回归：2-2_1 现在游得动了（改之前按右+跳只能到第 10 格）")
    await setup(ws, 2, 2, 1)
    best = 0.0
    for i in range(120):
        s = await si(ws, 12, PRESS("Right") + (TAP("Space") if i % 2 == 0 else []))
        best = max(best, s["player"]["x"] / TILE_SIZE)
        if s["state"] != "playing" or s["level"]["name"] != "2-2_1":
            break
    await si(ws, 1, FREE_ALL)
    check(
        "光按右+定期划水就能推进到 30 格以上（陆地物理下卡在第 10 格）",
        best > 30.0,
        f"最远第 {best:.1f} 格 / 共 {s['level']['width']} 格",
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
