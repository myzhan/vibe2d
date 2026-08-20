#!/usr/bin/env python3
"""
mari0 藤蔓验证脚本

实体 14，全游戏只有 **5 处**：2-1 (83,5)、3-1 (131,5)、4-2_1 (64,5)、5-2 (85,5)、
6-2 (81,5)。五处**全都在砖块里**，而且 4-2_1 那块是 tile 49 —— 地下配色的砖。
砸开它，藤蔓从砖里长出来，这是全游戏唯一可以攀爬的东西。

规则（`vine.lua` + `mario.lua:809-857, 2284-2321`）：
  - 长速 `vinespeed = 2.13` 格/秒，一直长到 `limit`（砖上的藤是 -1，也就是长出屏幕顶）。
  - **能抓的盒子只有 10/16 格宽**，而且底边钉死在 `coy-1.7`（砖底往上 1.7 格，
    也就是砖顶上方 0.7 格）—— 砖边那一小截茎是画出来的，抓不着。
  - 画的时候整根藤被 `setScissor(0, 0, width, (coy-1.5)*16)` 裁掉：
    卷芽是从砖**里面**长出来的，不裁就会看到芽尖凭空坐在还没长开的砖头上。
  - 上 3.21 格/秒，下 **正好两倍** 6.42 格/秒；两帧动画，停手时固定第 2 帧。
  - 左右键第一下是**绕到茎的另一边**（±8/16 格），第二下才松手（±7/16 格）。
    所以判定必须吃按键沿，不能吃按住。
  - 头顶爬到第 4 格（`vineanimationstart`）就**离开这一关**：一路升到 y < -4 后
    切进目标 sublevel。爬升期间 `controlsenabled = false`，而原版的计时器是跟着
    controlsenabled 走的（`game.lua:189-196`），所以这段**不掉秒**。
    五处藤蔓砖**全在第 5 行**，而判定线在第 4 行，站在砖上抓住时头顶正好在第 5 行
    —— 也就是说砖上的藤**只有一格可爬**，按住上 0.31 秒就走了。
  - 目标是 `bonusstage` 关：开场不是站着，而是 `vinestart` —— 人在地板**底下**，
    等藤长 6 格（2.82 秒，比藤长到头还慢一点，所以你看到的是爬一根长好的藤），
    爬到 y=10.75 停住、绕到另一边，再过 0.5 秒才交还控制权。
  - 金币房**没有出口管道**：踩着 bonus 平台走到头掉下去，掉坑在 bonusstage 里
    不是死亡而是**唯一的出路**（`mario.lua:2603-2607`），回到来时那关的 pipespawn。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_vine_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
DT = 1.0 / 60.0

# variables.lua:287-297
VINE_SPEED = 2.13
VINE_MOVE_SPEED = 3.21
VINE_MOVE_DOWN_SPEED = VINE_MOVE_SPEED * 2
VINE_ANIM_START = 4.0
VINE_ANIM_GROW_HEIGHT = 6.0
VINE_ANIM_MARIO_START = VINE_ANIM_GROW_HEIGHT / VINE_SPEED
VINE_ANIM_STOP = 1.75
VINE_ANIM_DROP_DELAY = 0.5
VINE_W = 10.0 / 16.0

# 2-1 的藤蔓砖
VINE_COL, VINE_ROW = 83, 5
# 藤本体（能抓的盒子）左边和中线
VINE_X = VINE_COL * TILE_SIZE + (TILE_SIZE - VINE_W * TILE_SIZE) / 2.0
VINE_CENTRE = VINE_X + VINE_W * TILE_SIZE / 2.0
# 站在被砸开的砖上的落脚点
STAND_X = VINE_COL * TILE_SIZE
STAND_Y = VINE_ROW * TILE_SIZE - TILE_SIZE

HOLD_UP = [{"device": "keyboard", "action": "press", "key": "Up"}]
HOLD_DOWN = [{"device": "keyboard", "action": "press", "key": "Down"}]
TAP_RIGHT = [{"device": "keyboard", "action": "tap", "key": "Right"}]
TAP_LEFT = [{"device": "keyboard", "action": "tap", "key": "Left"}]
# 按下的键会一直按着，跨小节漏出去就会污染下一段测量 —— 每段结束都全放开
FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Up", "Down", "Left", "Right")
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


async def setup(ws, world, level, sublevel=0):
    """装载关卡并清场。

    敌人要清掉，`state` 要按回 playing：藤长满要 5 秒，这 5 秒里一只库巴龟就够让
    `update_playing` 整个停下来 —— 那样藤会**停在半空**，看起来像长速算错了。
    """
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    return await si(ws)


async def sprout(ws, col=VINE_COL):
    """从下面顶开藤蔓砖，返回长出来的藤。

    人贴着砖底放、给一点上速度：这样解算器当帧就把他顶回砖底、`vy` 归零，
    也就触发顶砖。跳一段过去反而不行 —— 4-2_1 的藤蔓砖下面第 7 行是隐藏砖，
    起跳点会直接卡在砖里。
    """
    await rpc(
        ws,
        "game.setPlayerPos",
        {
            "x": col * TILE_SIZE,
            "y": (VINE_ROW + 1) * TILE_SIZE,
            "vx": 0.0,
            "vy": -300.0,
        },
    )
    for _ in range(20):
        s = await si(ws)
        if s["vines"]:
            # 挪到藤旁边的砖上等它长 —— 掉回地面会被敌人吃掉，人一死藤就不长了
            await rpc(
                ws,
                "game.setPlayerPos",
                {"x": (col - 2) * TILE_SIZE, "y": STAND_Y, "vx": 0.0, "vy": 0.0},
            )
            await rpc(ws, "game.clearEnemies")
            return await si(ws)
    return s


async def grab(ws):
    """回到干净的“刚抓住”状态，返回抓住那一帧。

    先松开所有键、再挪到藤外面待一帧把旧的抓握清掉 —— 抓握是**只在第一次接触时**
    建立的（原版抓着的时候直接把藤的碰撞屏蔽了），所以带着旧抓握直接传送过去，
    人不会被重新吸到茎上，`side` 也还是上一段留下的那个。
    """
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": STAND_X - 3 * TILE_SIZE, "y": STAND_Y})
    await si(ws)
    await rpc(
        ws, "game.setPlayerPos", {"x": STAND_X, "y": STAND_Y, "vx": 0.0, "vy": 0.0}
    )
    return await si(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 五处藤蔓都在砖里，2-1 的那块在 (83, 5)")
    s = await setup(ws, 2, 1)
    vine_blocks = [b for b in s["block_contents"] if b["content"] == "vine"]
    check(
        "2-1 只有一块藤蔓砖",
        len(vine_blocks) == 1,
        f"{len(vine_blocks)} 块",
    )
    if vine_blocks:
        b = vine_blocks[0]
        check(
            "在 (83, 5)",
            (b["col"], b["row"]) == (VINE_COL, VINE_ROW),
            f"({b['col']}, {b['row']})",
        )
    check("开局场上没有藤", not s["vines"], f"{len(s['vines'])} 根")
    check("2-1 本身不是金币房", not s["bonusstage"])

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 从下面顶开砖：藤长出来，砖里的东西没了")
    s = await sprout(ws)
    check("藤长出来了", len(s["vines"]) == 1, f"{len(s['vines'])} 根")
    if not s["vines"]:
        return
    v = s["vines"][0]
    check(
        "目标是 sublevel 1（2-1_1）",
        v["dest"] == 1,
        f"dest={v['dest']}",
    )
    check(
        f"盒子 {VINE_W:.4f} 格宽（比砖窄）",
        abs(v["w"] / TILE_SIZE - VINE_W) < 0.01,
        f"{v['w'] / TILE_SIZE:.4f} 格",
    )
    check(
        "盒子在砖的中线上",
        abs(v["x"] - VINE_X) < 0.01,
        f"x={v['x']} 期望 {VINE_X}",
    )
    check(
        "刚长出来时高度是 0（底边钉在砖顶上方 0.7 格，一开始还在 y 之上）",
        v["h"] < 1.0,
        f"h={v['h']}",
    )
    check(
        "裁切线在砖顶上方半格（coy-1.5）",
        abs(v["clip_bottom"] - (VINE_ROW - 0.5) * TILE_SIZE) < 0.01,
        f"clip_bottom={v['clip_bottom']} 期望 {(VINE_ROW - 0.5) * TILE_SIZE}",
    )
    check(
        "砖里的藤已经消耗掉了",
        not [b for b in s["block_contents"] if b["content"] == "vine"],
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. 以 {VINE_SPEED} 格/秒往上长，长到 limit = -1 格就停")
    y0 = s["vines"][0]["y"]
    s = await si(ws, 30)
    v = s["vines"][0]
    grew = (y0 - v["y"]) / TILE_SIZE / (30 * DT)
    check(
        f"长速约 {VINE_SPEED} 格/秒",
        abs(grew - VINE_SPEED) < 0.1,
        f"实测 {grew:.3f} 格/秒",
    )
    check("还在长", not v["grown"])
    s = await si(ws, 300)
    v = s["vines"][0]
    check("长到头了", v["grown"], f"y={v['y']}")
    check(
        "顶端停在屏幕上方一格（limit = -1 格）",
        abs(v["y"] + TILE_SIZE) < 0.01,
        f"y={v['y']}",
    )
    check(
        "整根都能抓（茎数够铺满盒子）",
        v["stems"] >= v["h"] / TILE_SIZE - 1,
        f"{v['stems']} 节 / {v['h'] / TILE_SIZE:.2f} 格",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 抓住：重力关掉，人被吸到茎上，朝着茎看")
    s = await grab(ws)
    check("抓住了", s["vine"] is not None and s["vine"]["phase"] == "grip",
          str(s["vine"] and s["vine"]["phase"]))
    if not s["vine"]:
        return
    check(
        "从左边靠过去 → 挂在左侧",
        s["vine"]["side"] == "left",
        s["vine"]["side"],
    )
    check(
        "右边缘越过中线 2/16 格",
        abs(s["player"]["x"] + s["player"]["width"] - (VINE_CENTRE + 2.0 / 16.0 * TILE_SIZE)) < 0.01,
        f"右边缘 {s['player']['x'] + s['player']['width']} 期望 {VINE_CENTRE + 2.0 / 16.0 * TILE_SIZE}",
    )
    check("挂在左侧就朝右看（面朝茎）", s["player"]["facing_right"])
    check("速度清零", abs(s["player"]["vx"]) < 0.01 and abs(s["player"]["vy"]) < 0.01)
    check("停手时是第 2 帧", s["vine"]["climb_frame"] == 2)
    check("攀爬姿势", s["player"]["anim_state"] == "climb")
    check("还有控制权（所以计时器照走）", s["vine"]["has_control"])

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. 上 {VINE_MOVE_SPEED} 格/秒，下正好两倍 {VINE_MOVE_DOWN_SPEED} 格/秒")
    # 注意可爬的余量只有**一格**：藤蔓砖在第 5 行，而离场判定是头顶到第 4 行，
    # 站在砖上抓住时头顶正好在第 5 行。1 格 / 3.21 格每秒 = 0.31 秒 = 18 帧。
    # 这一段每项都要重新抓一次，多爬十几帧就会直接被送进 2-1_1，
    # 后面所有测量就都在金币房里跑了。
    y0 = s["player"]["y"]
    s = await si(ws, 6, HOLD_UP)
    up_rate = (y0 - s["player"]["y"]) / TILE_SIZE / (6 * DT)
    check(
        f"上升约 {VINE_MOVE_SPEED} 格/秒",
        abs(up_rate - VINE_MOVE_SPEED) < 0.1,
        f"实测 {up_rate:.3f} 格/秒",
    )
    frames = set()
    for _ in range(10):
        s = await si(ws, 1, HOLD_UP)
        if s["vine"] and s["vine"]["phase"] == "grip":
            frames.add(s["vine"]["climb_frame"])
    check("爬的时候两帧交替", frames == {1, 2}, f"看到帧 {sorted(frames)}")
    check(
        "16 帧还没爬到顶（可爬余量就是一格）",
        s["vine"] is not None and s["vine"]["phase"] == "grip",
        str(s["vine"] and s["vine"]["phase"]),
    )

    # 先爬 6 帧腾出空间再往下滑：抓住的那一刻他正踩在被砸开的砖上，
    # 立刻下滑会当帧撞回砖顶，量不到速度。基准值要在按下之前当帧读，
    # 不能拿上一段的快照 —— 差一帧就是差一个 1.712px。
    s = await grab(ws)
    await si(ws, 6, HOLD_UP)
    y0 = (await si(ws, 1, FREE_ALL))["player"]["y"]
    s = await si(ws, 1, HOLD_DOWN)
    down_rate = (s["player"]["y"] - y0) / TILE_SIZE / DT
    check(
        f"下滑约 {VINE_MOVE_DOWN_SPEED} 格/秒",
        abs(down_rate - VINE_MOVE_DOWN_SPEED) < 0.2,
        f"实测 {down_rate:.3f} 格/秒",
    )
    await si(ws, 1, FREE_ALL)

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 左右键：第一下绕到另一边，第二下才松手")
    s = await grab(ws)
    x0 = s["player"]["x"]
    s = await si(ws, 1, TAP_RIGHT)
    check(
        "还挂着，只是换到了右侧",
        s["vine"] is not None and s["vine"]["side"] == "right",
        str(s["vine"] and s["vine"]["side"]),
    )
    check(
        "往右挪了 8/16 格",
        abs(s["player"]["x"] - (x0 + 8.0 / 16.0 * TILE_SIZE)) < 0.01,
        f"挪了 {s['player']['x'] - x0}px",
    )
    check("换到右侧就朝左看", not s["player"]["facing_right"])
    x1 = s["player"]["x"]
    s = await si(ws, 1, TAP_RIGHT)
    check("第二下松手", s["vine"] is None)
    check(
        "松手时往右让开 7/16 格",
        abs(s["player"]["x"] - (x1 + 7.0 / 16.0 * TILE_SIZE)) < 0.01,
        f"让开 {s['player']['x'] - x1}px",
    )
    check("变成下落姿势", s["player"]["anim_state"] == "fall")

    s = await grab(ws)
    s = await si(ws, 1, TAP_LEFT)
    check(
        "挂在左侧时按左 → 直接松手（不是绕过去）",
        s["vine"] is None,
        str(s["vine"] and s["vine"]["side"]),
    )
    check(
        "往左让开 7/16 格",
        s["player"]["x"] < STAND_X,
        f"x={s['player']['x']}",
    )

    # ── 7 ───────────────────────────────────────────────────────────
    section(f"7. 头顶爬到第 {VINE_ANIM_START:.0f} 格 → 离开这一关（这段不掉秒）")
    s = await grab(ws)
    clock0 = s["time_remaining"]
    leaving = None
    for _ in range(40):
        s = await si(ws, 1, HOLD_UP)
        if s["vine"] and s["vine"]["phase"] == "leaving":
            leaving = s
            break
    check("进入了离场爬升", leaving is not None)
    if leaving:
        head = leaving["player"]["y"]
        check(
            f"是头顶（y+h）越过第 {VINE_ANIM_START:.0f} 格时触发的",
            head + leaving["player"]["height"] <= VINE_ANIM_START * TILE_SIZE + 4.0,
            f"头顶 {(head + leaving['player']['height']) / TILE_SIZE:.3f} 格",
        )
        check("交出了控制权", not leaving["vine"]["has_control"])
        check("目标还是 sublevel 1", leaving["vine"]["dest"] == 1)
    check(
        "爬到这里计时器一直在走（还有控制权时会掉秒）",
        s["time_remaining"] < clock0,
        f"{clock0:.2f} → {s['time_remaining']:.2f}",
    )

    clock1 = s["time_remaining"]
    arrived = None
    for _ in range(200):
        s = await si(ws, 2, HOLD_UP)
        if s["level"]["sublevel"] == 1:
            arrived = s
            break
    await si(ws, 1, FREE_ALL)
    check("切到了 2-1_1", arrived is not None,
          arrived["level"]["name"] if arrived else s["level"]["name"])
    if not arrived:
        return
    check("2-1_1 是金币房", arrived["bonusstage"])
    check(
        "离场爬升期间没掉秒（controlsenabled 关着）",
        abs(arrived["time_remaining"] - clock1) < 0.2,
        f"{clock1:.2f} → {arrived['time_remaining']:.2f}",
    )

    # ── 8 ───────────────────────────────────────────────────────────
    section("8. 金币房开场是 vinestart：人在地板底下等藤长好，再爬进来")
    check(
        "开场就是 intro",
        arrived["vine"] is not None and arrived["vine"]["phase"] == "intro",
        str(arrived["vine"] and arrived["vine"]["phase"]),
    )
    check("场上有那根开场藤", len(arrived["vines"]) == 1, f"{len(arrived['vines'])} 根")
    check(
        "人起手在第 15 格 —— 15 行的关卡里，也就是地板底下",
        abs(arrived["player"]["y"] - 15.0 * TILE_SIZE) < 1.0,
        f"y={arrived['player']['y'] / TILE_SIZE:.3f} 格",
    )
    check("没有控制权", not arrived["vine"]["has_control"])
    if arrived["vines"]:
        v = arrived["vines"][0]
        check(
            "开场藤的 limit 是 9+1/16 格（停在屏幕里，不像砖上的藤长出去）",
            abs(v["limit"] / TILE_SIZE - (9.0 + 1.0 / 16.0)) < 0.01,
            f"limit={v['limit'] / TILE_SIZE:.4f} 格",
        )

    # 等到他开始爬
    y_wait = arrived["player"]["y"]
    clock2 = arrived["time_remaining"]
    started = None
    elapsed = 0.0
    for _ in range(400):
        s = await si(ws, 1)
        elapsed += DT
        if s["vine"] and s["vine"]["intro_climbing"]:
            started = elapsed
            break
        if s["vine"] is None:
            break
    check(
        f"等了约 {VINE_ANIM_MARIO_START:.3f} 秒才起步（藤先长 {VINE_ANIM_GROW_HEIGHT:.0f} 格）",
        started is not None and abs(started - VINE_ANIM_MARIO_START) < 0.1,
        f"实测 {started:.3f} 秒" if started else "没起步",
    )
    check(
        "等的时候人一动不动",
        abs(s["player"]["y"] - y_wait) < 2.0,
        f"y={s['player']['y'] / TILE_SIZE:.3f} 格",
    )
    check(
        "藤已经先长到头了 —— 看到的是爬一根长好的藤",
        s["vines"] and s["vines"][0]["grown"],
    )
    check(
        "这段也不掉秒",
        abs(s["time_remaining"] - clock2) < 0.2,
        f"{clock2:.2f} → {s['time_remaining']:.2f}",
    )

    # 爬到 stop，再绕过去，再交还控制权
    stop_y = (15.0 - VINE_ANIM_GROW_HEIGHT + VINE_ANIM_STOP) * TILE_SIZE
    dropped = None
    for _ in range(200):
        s = await si(ws, 1)
        if s["vine"] and s["vine"]["intro_dropping_off"]:
            dropped = s
            break
        if s["vine"] is None:
            break
    check("爬到位后绕到茎的另一边", dropped is not None)
    if dropped:
        check(
            f"停在 y = 15 - {VINE_ANIM_GROW_HEIGHT:.0f} + {VINE_ANIM_STOP} = "
            f"{stop_y / TILE_SIZE:.2f} 格",
            abs(dropped["player"]["y"] - stop_y) < 0.01,
            f"y={dropped['player']['y'] / TILE_SIZE:.4f} 格",
        )
        check("绕过去以后朝左看", not dropped["player"]["facing_right"])

    released = None
    waited = 0.0
    for _ in range(120):
        s = await si(ws, 1)
        waited += DT
        if s["vine"] is None:
            released = s
            break
    check(
        f"再过约 {VINE_ANIM_DROP_DELAY} 秒交还控制权",
        released is not None and abs(waited - VINE_ANIM_DROP_DELAY) < 0.1,
        f"实测 {waited:.3f} 秒" if released else "没交还",
    )
    if released:
        # 4-3/16 + 9/16 + 7/16 = 4.8125 格
        expect_x = (4.0 - 3.0 / 16.0 + 9.0 / 16.0 + 7.0 / 16.0) * TILE_SIZE
        check(
            "落点在 4.8125 格 —— 金币房地板留的那个洞的右边",
            abs(released["player"]["x"] - expect_x) < 0.01,
            f"x={released['player']['x'] / TILE_SIZE:.4f} 格",
        )
        s = await si(ws, 60)
        check(
            "交还控制权后重力回来了，人落到地板上",
            s["player"]["on_ground"],
            f"y={s['player']['y'] / TILE_SIZE:.2f} 格",
        )
        check("这时候计时器才开始走", s["time_remaining"] < released["time_remaining"])

    # ── 9 ───────────────────────────────────────────────────────────
    section("9. 金币房没有出口管道：掉坑就是出路，回到 2-1 的 pipespawn")
    # 要从**右头**掉下去，不是从地板上那个洞。那个洞就是开场藤长上来的地方，
    # 而开场藤爬完并不会消失（原版也一样留在场上），所以往洞里放人只会被藤接住 ——
    # 真正的出路是踩 bonus 平台走到第 62 列以后，那边地板就没了。
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 70.0 * TILE_SIZE, "y": 11.0 * TILE_SIZE, "vx": 0.0, "vy": 200.0},
    )
    back = None
    deepest = 0.0
    for _ in range(240):
        s = await si(ws, 2)
        deepest = max(deepest, s["player"]["y"])
        if s["level"]["sublevel"] == 0:
            back = s
            break
    check("回到了 2-1（不是死亡画面）", back is not None,
          f"state={s['state']} level={s['level']['name']} "
          f"最深 y={deepest / TILE_SIZE:.2f} 格")
    if back:
        check("没扣命", back["lives"] == arrived["lives"],
              f"{arrived['lives']} → {back['lives']}")
        check(
            "从 2-1 第 162 列的 pipespawn 冒出来（和原版一样，不是回关卡开头）",
            abs(back["player"]["x"] / TILE_SIZE - 162.0) < 1.5,
            f"x={back['player']['x'] / TILE_SIZE:.2f} 格",
        )
        check("走的是管道升出动画", back["pipe"] == "up", str(back["pipe"]))

    # ── 10 ──────────────────────────────────────────────────────────
    section("10. 另外四处：4-2_1 那块是 tile 49 的地下砖，一样要能砸开")
    for world, level, sublevel, col, dest in [
        (3, 1, 0, 131, 1),
        (4, 2, 1, 64, 2),
        (5, 2, 0, 85, 2),
        (6, 2, 0, 81, 3),
    ]:
        name = f"{world}-{level}" + (f"_{sublevel}" if sublevel else "")
        s = await setup(ws, world, level, sublevel)
        blocks = [b for b in s["block_contents"] if b["content"] == "vine"]
        ok = len(blocks) == 1 and blocks[0]["col"] == col
        check(f"{name} 的藤蔓砖在第 {col} 列", ok,
              f"{[(b['col'], b['row']) for b in blocks]}")
        if not ok:
            continue
        s = await sprout(ws, col)
        check(f"{name} 砸开后长出藤，目标 sublevel {dest}",
              bool(s["vines"]) and s["vines"][0]["dest"] == dest,
              f"dest={s['vines'][0]['dest']}" if s["vines"] else "没长出来")

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
