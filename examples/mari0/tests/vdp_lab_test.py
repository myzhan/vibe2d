#!/usr/bin/env python3
"""
mari0 实验室信号网络验证脚本

规则来自 `door.lua:33-44`（`link()`）、`button.lua:24-38`、`pushbutton.lua`、
`game.lua:52`（六种 output）：

  1. **link 方向是反的**：`"link"` 三元组存在**接收端**上，指向驱动它的发射端
     （编辑器手势是从门拖到按钮）。
  2. 每个 input 只能有一个 driver；一个 output 的 fan-out 无限。
  3. 信号只有 `on`/`off`/`toggle` 三个，无值、无优先级、无防抖、无环检测。
  4. 按钮**只在状态变化时**推送（`button.lua:27`），否则按住会每帧重发。
  5. 门要**完全打开**才不再挡路（`door.lua:64`），半开仍是墙；`doorspeed = 2`
     所以 0→1 要半秒。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_lab_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys
from collections import Counter

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0
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


async def step(ws, frames=1):
    before = (await rpc(ws, "engine.getTime"))["frame_count"]
    await rpc(ws, "engine.step", {"frames": frames})
    for _ in range(900):
        if (await rpc(ws, "engine.getTime"))["frame_count"] >= before + frames:
            return
        await asyncio.sleep(0.005)
    raise RuntimeError(f"engine.step({frames}) never completed")


async def snap(ws):
    return await rpc(ws, "game.inspect")


def check(label, ok, detail=""):
    print(f"    {'OK  ' if ok else 'FAIL'} {label}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(label)


def section(title):
    print(f"\n─── {title} ───")


async def load_lab(ws, world, level):
    await rpc(ws, "game.setLevel", {"pack": "portal", "world": world, "level": level})
    await step(ws)
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 九个实验室关卡的连线全部能解析")
    total_links = 0
    for world, level in [(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (2, 2), (2, 3), (2, 4), (3, 1)]:
        s = await load_lab(ws, world, level)
        lab = s["lab"]
        linked = [e for e in lab if e["driver"] is not None]
        total_links += len(linked)
        check(
            f"{world}-{level}: 有 {len(lab)} 个元件（{len(linked)} 条连线）",
            len(lab) > 0,
            f"元件类型 {dict(Counter(e['kind'] for e in lab))}",
        )
    # 单元测试已经逐关断言过"没有悬空 link"，这里确认运行时确实建起了网络。
    # 2-1 一条线都没有：它只有 5 座没连线的光桥（所以永久开启），这是数据事实。
    check("九关合计接上了几十条连线", total_links > 200, f"{total_links} 条")

    section("2. portal 1-1：一个按钮驱动门 + 一堆指示灯（fan-out 无限）")
    s = await load_lab(ws, 1, 1)
    lab = s["lab"]
    buttons = [i for i, e in enumerate(lab) if e["kind"] == "button"]
    check("有 3 个地板按钮", len(buttons) == 3, str(len(buttons)))
    fan = {}
    for i in buttons:
        fan[i] = [j for j, e in enumerate(lab) if e["driver"] == i]
    biggest = max(fan.values(), key=len)
    check("其中一个按钮驱动了 10 个以上元件", len(biggest) > 10, f"最多 {len(biggest)} 个")
    kinds = Counter(lab[j]["kind"] for j in biggest)
    check("驱动的既有门也有指示灯", kinds["door"] >= 1 and kinds["ground_light"] >= 1, str(dict(kinds)))
    check("每个 input 只有一个 driver", all(isinstance(e["driver"], (int, type(None))) for e in lab))

    section("2b. 地板按钮：站上去才按下，隔几格站着不算")
    # 感应框是从 1 基 tile 坐标推出来的（`button.lua:8-24`：self.x = cox-15/16），
    # 把 cox 当成 0 基就会整块偏一格 —— 连线全对、按钮却在别处，只有真的站上去才测得出。
    await rpc(ws, "game.setState", {"state": "playing"})
    for i in buttons:
        col, row = lab[i]["cell"]
        await rpc(ws, "game.setPlayerPos", {"x": (col + 0.5) * T, "y": row * T})
        await step(ws, 3)
        s2 = await snap(ws)
        driven_doors = [j for j, e in enumerate(s2["lab"]) if e["driver"] == i and e["kind"] == "door"]
        check(f"站在按钮 {i} ({col},{row}) 上按下了它", s2["lab"][i]["on"],
              f"on_ground={s2['player']['on_ground']}")
        check(f"它的门收到了 on", all(s2["lab"][j]["on"] for j in driven_doors), str(driven_doors))
        await rpc(ws, "game.setPlayerPos", {"x": (col - 3) * T, "y": row * T})
        await step(ws, 3)
        s2 = await snap(ws)
        check(f"离开三格后松开", not s2["lab"][i]["on"])

    section("3. 已经没有 inert 元件了：出厂关卡用到的每种元件都真的在动")
    s = await load_lab(ws, 1, 1)
    lab = s["lab"]
    inert = sorted({e["kind"] for e in lab if e["inert"]})
    # 这一栏留着：它是"还没做的东西不假装能用"的诚实开关，只是现在全空了。
    check("1-1 里没有任何 inert 元件", inert == [], str(inert))

    section("4. 门收到 on 之后半秒开满；doorspeed = 2")
    doors = [i for i, e in enumerate(lab) if e["kind"] == "door"]
    d = doors[0]
    check("门初始是关着的", lab[d]["timer"] == 0.0 and not lab[d]["on"], str(lab[d]))
    await rpc(ws, "game.labSignal", {"index": d, "signal": "on"})
    await step(ws, 15)  # 0.25s → 应该开到一半左右
    s = await snap(ws)
    half = s["lab"][d]["timer"]
    check("15 帧（0.25s）后开到约一半", 0.4 < half < 0.75, f"timer={half:.3f}")
    await step(ws, 20)
    s = await snap(ws)
    check("再跑一会儿开满", s["lab"][d]["timer"] == 1.0, f"timer={s['lab'][d]['timer']:.3f}")

    section("5. off 会把门关回去")
    await rpc(ws, "game.labSignal", {"index": d, "signal": "off"})
    await step(ws, 40)
    s = await snap(ws)
    check("门已完全关闭", s["lab"][d]["timer"] == 0.0, f"timer={s['lab'][d]['timer']:.3f}")

    section("6. toggle 是第三个信号，翻转当前状态")
    await rpc(ws, "game.labSignal", {"index": d, "signal": "toggle"})
    s = await snap(ws)
    check("toggle 把关着的门打开", s["lab"][d]["on"], str(s["lab"][d]["on"]))
    await rpc(ws, "game.labSignal", {"index": d, "signal": "toggle"})
    s = await snap(ws)
    check("再 toggle 一次又关上", not s["lab"][d]["on"], str(s["lab"][d]["on"]))

    section("7. 未知信号与越界下标要报错，而不是静默忽略")
    for params, what in [
        ({"index": d, "signal": "wobble"}, "未知信号"),
        ({"index": 9999, "signal": "on"}, "越界下标"),
    ]:
        try:
            await rpc(ws, "game.labSignal", params)
            check(f"{what}应当报错", False, "被静默接受了")
        except RuntimeError:
            check(f"{what}报错", True)

    section("8. 激光真的点亮探测器：光束覆盖自己那格，并**探测终止格**")
    # 每个出厂探测器都装在实心 tile 上（2-3 是 134/135/141/154），光束必然停在它前面
    # 一格。原版靠 `updateoutputs` 多循环一格（laser.lua:240）才能点亮它 —— 不照抄
    # 这一格，全游戏没有一个探测器会响。
    # 1-2 的探测器装在 tile 134 里（实心），光束停在它前面一格 —— 靠探测终止格才点亮。
    s = await load_lab(ws, 1, 2)
    lab = s["lab"]
    laser = next(e for e in lab if e["kind"] == "laser")
    det_i = next(i for i, e in enumerate(lab) if e["kind"] == "laser_detector")
    check("光束第一格就是发射器自己那格", laser["beam"][0]["cells"][0] == laser["cell"], str(laser["beam"][0]["cells"][:2]))
    end = laser["beam"][-1]["end"]
    check("光束终止格正是探测器那格（它嵌在墙里）", end == lab[det_i]["cell"], f"end={end} 探测器={lab[det_i]['cell']}")
    check("探测器被点亮", lab[det_i]["on"])
    driven = [i for i, e in enumerate(lab) if e["driver"] == det_i]
    doors = [i for i in driven if lab[i]["kind"] == "door"]
    check("并且驱动了下游的门", len(doors) > 0, str(driven))
    await step(ws, 40)
    s = await snap(ws)
    check("门被激光开满了", all(s["lab"][i]["timer"] == 1.0 for i in doors),
          str([s["lab"][i]["timer"] for i in doors]))

    section("9. 光束经传送门折射（2-3 的激光谜题，整条链路）")
    s = await load_lab(ws, 2, 3)
    await rpc(ws, "game.setState", {"state": "playing"})
    lab = s["lab"]
    det_i = next(i for i, e in enumerate(lab) if e["cell"] == [10, 11])
    door_i = next(i for i, e in enumerate(lab) if e["kind"] == "door" and e["driver"] == det_i)
    check("(10,11) 的探测器初始是灭的", not lab[det_i]["on"])
    check("它驱动的门初始关着", not lab[door_i]["on"])
    # 入口：(10,3) 那面墙的左面（正对 (7,3) 的激光）；出口：(3,10) 那面墙的右面。
    # 两个都是原版规则下**真的能放**的位置：背板两格实心可开门、前面两格是空的。
    await rpc(ws, "game.setPortal", {"index": 0, "x": 10 * T, "y": 3 * T, "orientation": "left"})
    await rpc(ws, "game.setPortal", {"index": 1, "x": 4 * T, "y": 11 * T, "orientation": "right"})
    await step(ws, 4)
    s = await snap(ws)
    beam = next(e for e in s["lab"] if e["kind"] == "laser" and e["cell"] == [7, 3])["beam"]
    check("光束断成两段（进门 + 出门）", len(beam) == 2, f"{len(beam)} 段")
    check("第一段向右、止于入口门那格", beam[0]["dir"] == "right" and beam[0]["end"] == [10, 3], str(beam[0]["end"]))
    check("第二段从出口门那面射出、方向是出口的朝向", beam[1]["dir"] == "right" and beam[1]["cells"][0] == [4, 11], str(beam[1]["cells"][:1]))
    check("并且打到了 (10,11) 的探测器", beam[1]["end"] == [10, 11], str(beam[1]["end"]))
    check("探测器亮了", s["lab"][det_i]["on"])
    await step(ws, 40)
    s = await snap(ws)
    check("门被这条折射光束打开了", s["lab"][door_i]["timer"] == 1.0, f"timer={s['lab'][door_i]['timer']:.2f}")

    section("10. 光桥是薄板：能站上去，方向决定是地板还是墙")
    await rpc(ws, "game.clearPortals")
    s = await load_lab(ws, 2, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    bridges = [e for e in s["lab"] if e["kind"] == "light_bridge"]
    check("2-1 的五座光桥都是常亮的（没连线 → 永久开启）", len(bridges) == 5 and all(b["on"] for b in bridges), str(len(bridges)))
    cells = sum(len(seg["cells"]) for b in bridges for seg in b["beam"])
    # solid_rects 里还有喷嘴/管子那些 2×2 的方块，光桥板是其中"薄"的那一类。
    slabs = [r for r in s["solid_rects"] if min(r[2], r[3]) < T / 4]
    check("每覆盖一格铺一块薄板", len(slabs) == cells, f"{len(slabs)} 板 / {cells} 格")
    check("薄板只有 1/8 格厚", all(min(r[2], r[3]) <= T / 8 for r in slabs))
    # 横向光桥（(1,7) 往右 9 格）铺出来的板顶在 (7+7/16)*32
    top = (7 + 7 / 16) * T
    await rpc(ws, "game.setPlayerPos", {"x": 5 * T, "y": top - 52})
    await step(ws, 20)
    s = await snap(ws)
    check("玩家落在光桥上而不是穿过去", s["player"]["on_ground"] and abs(s["player"]["y"] + s["player"]["height"] - top) < 0.01,
          f"y={s['player']['y']:.1f} 板顶={top:.1f} on_ground={s['player']['on_ground']}")

    section("11. 关掉光桥，板子就消失（连了线的光桥就是这么用的）")
    # 关掉的必须是他脚下这一座 —— 别的桥关了他当然不会掉。
    idx = next(
        i
        for i, e in enumerate(s["lab"])
        if e["kind"] == "light_bridge"
        and any([5, 7] in seg["cells"] for seg in e["beam"])
    )
    before = len([r for r in s["solid_rects"] if min(r[2], r[3]) < T / 4])
    await rpc(ws, "game.labSignal", {"index": idx, "signal": "off"})
    await step(ws, 2)
    s = await snap(ws)
    now = len([r for r in s["solid_rects"] if min(r[2], r[3]) < T / 4])
    check("薄板数量减少", now < before, f"{before} → {now}")
    check("关掉的那座桥没有光束", not s["lab"][idx]["beam"])
    await step(ws, 20)
    s = await snap(ws)
    check("站在被关掉的桥上的话会掉下去", not s["player"]["on_ground"] or s["player"]["y"] > top,
          f"y={s['player']['y']:.1f}")

    section("12. 激光会杀人，并且被身体挡住")
    s = await load_lab(ws, 2, 4)
    await rpc(ws, "game.setState", {"state": "playing"})
    laser = next(e for e in s["lab"] if e["kind"] == "laser")
    row = laser["cell"][1]
    await rpc(ws, "game.setPlayerPos", {"x": (laser["cell"][0] + 1) * T, "y": row * T})
    await step(ws, 2)
    s = await snap(ws)
    check("站进光束里会死", s["state"] == "dead", s["state"])
    beam = next(e for e in s["lab"] if e["kind"] == "laser")["beam"]
    check("光束被身体截断，止于他之前", len(beam[0]["cells"]) == 1, str(beam[0]["cells"]))
    check("截断之后不再有终止格可探测", beam[0]["end"] is None, str(beam[0]["end"]))

    section("13. 计时器：松开按钮后门还会开着，是它在数秒")
    # 3-1 的形状：按钮 → 计时器(walltimer) → 门。计时器是唯一既是 input 又是 output 的元件：
    # 收到 on 就停表保持，收到 off 才开始数，数完才向下游发 off（`walltimer.lua:65-77`）。
    s = await load_lab(ws, 3, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setScore", {"lives": 9})
    lab = s["lab"]
    # 选 1 秒那个（3-1 另有一个 4 秒的，但它的按钮正好泡在一条常亮激光里 ——
    # 那关的解法显然是把方块推上去，而方块还没做）。
    ti = next(i for i, e in enumerate(lab) if e["kind"] == "timer" and e["duration"] == 1.0)
    btn = lab[ti]["driver"]
    doors = [i for i, e in enumerate(lab) if e["driver"] == ti and e["kind"] == "door"]
    check("计时器不再是 inert", not lab[ti]["inert"])
    check("它由一个按钮驱动、并驱动着门", lab[btn]["kind"] == "button" and len(doors) == 1,
          f"driver={lab[btn]['kind']} doors={doors}")
    check("装载时是停表状态（否则首帧就会发一次 off）", lab[ti]["timer"] == lab[ti]["duration"])

    col, row = lab[btn]["cell"]
    await rpc(ws, "game.setPlayerPos", {"x": (col + 0.5) * T, "y": (row - 1) * T})
    await step(ws, 40)
    s = await snap(ws)
    check("站上按钮：门开满，计时器停在满值", s["lab"][doors[0]]["timer"] == 1.0 and s["lab"][ti]["timer"] == 1.0,
          f"door={s['lab'][doors[0]]['timer']:.2f} timer={s['lab'][ti]['timer']:.2f}")

    await rpc(ws, "game.setPlayerPos", {"x": (col - 4) * T, "y": (row - 1) * T})
    await step(ws, 30)
    s = await snap(ws)
    check("离开按钮 0.5 秒后门仍然是开的", s["lab"][doors[0]]["timer"] == 1.0,
          f"timer 走到 {s['lab'][ti]['timer']:.2f}")
    check("计时器正在数", 0.0 < s["lab"][ti]["timer"] < 1.0, f"{s['lab'][ti]['timer']:.2f}")
    await step(ws, 40)
    s = await snap(ws)
    check("数满之后它向下游发 off，门开始关", not s["lab"][doors[0]]["on"],
          f"timer={s['lab'][ti]['timer']:.2f} door_on={s['lab'][doors[0]]['on']}")
    await step(ws, 40)
    s = await snap(ws)
    check("门最终完全关上", s["lab"][doors[0]]["timer"] == 0.0, f"{s['lab'][doors[0]]['timer']:.2f}")

    section("14. 方块：推、举、放，压住按钮，掉下去由分配器补一个")
    s = await load_lab(ws, 1, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setScore", {"lives": 9})
    lab = s["lab"]
    slots = [i for i, e in enumerate(lab) if e["kind"] == "box"]
    check("1-1 的两个 box 实体各生成了一个方块", len(s["cubes"]) == len(slots) == 2, str(len(s["cubes"])))
    check("每个方块都接着自己的 slot 与分配器",
          all(c["slot"] is not None and c["dispenser"] is not None for c in s["cubes"]),
          str([(c["slot"], c["dispenser"]) for c in s["cubes"]]))
    check("装载后它们都已落地", all(not c["falling"] for c in s["cubes"]), str([c["falling"] for c in s["cubes"]]))

    # 1-1 里那个方块停在 (29,6)：左边 (28,6) 是空的、脚下 (28..30,7) 是墙，可以走过去推。
    cube = next(c for c in s["cubes"] if c["x"] > 900)
    await rpc(ws, "game.setPlayerPos", {"x": 28 * T, "y": 6 * T})
    await step(ws, 3)
    s = await snap(ws)
    await rpc(ws, "engine.simulateInput",
              {"device": "mouse", "action": "move", "x": s["player"]["x"] - s["camera_x"] + 200, "y": 200.0})
    x0 = next(c for c in s["cubes"] if c["x"] > 900)["x"]
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": "Right"})
    await step(ws, 30)
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": "Right"})
    s = await snap(ws)
    pushed = next(c for c in s["cubes"] if c["x"] > 900)
    check("走进方块会把它推着走（没有力、没有质量，就是贴着他的边）", pushed["x"] > x0 + 16,
          f"{x0:.0f} → {pushed['x']:.0f}")

    # 举起来：use 的探测框在**瞄准方向**上一格，不是身体周围
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": "E"})
    await step(ws, 2)
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": "E"})
    await step(ws, 2)
    s = await snap(ws)
    held = [c for c in s["cubes"] if c["held"]]
    check("按 use 举起了瞄着的那个方块", len(held) == 1, str([c["held"] for c in s["cubes"]]))
    # 举着的方块跟着准星转，这就是它能当盾牌的原因
    await rpc(ws, "engine.simulateInput",
              {"device": "mouse", "action": "move", "x": s["player"]["x"] - s["camera_x"] - 200, "y": 200.0})
    await step(ws, 3)
    s2 = await snap(ws)
    left_side = next(c for c in s2["cubes"] if c["held"])
    check("准星转到左边，方块也跟到左边", left_side["x"] < held[0]["x"], f"{held[0]['x']:.0f} → {left_side['x']:.0f}")

    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": "E"})
    await step(ws, 2)
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": "E"})
    await step(ws, 25)
    s = await snap(ws)
    check("再按一次放下，并且落回地面", not any(c["held"] for c in s["cubes"]) and not any(c["falling"] for c in s["cubes"]),
          str([(round(c["x"]), round(c["y"]), c["falling"]) for c in s["cubes"]]))

    section("15. 分配器：方块没了就补一个，管口会打开让它掉出来")
    s = await load_lab(ws, 1, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    tube = next(i for i, e in enumerate(s["lab"]) if e["kind"] == "box_tube")
    slot = s["lab"][tube]["driver"]
    solid_before = len(s["solid_rects"])
    doomed = next(c for c in s["cubes"] if c["dispenser"] == tube)
    # 方块掉出地图时干的正是这件事：从自己的 slot 推一个 toggle 上去。
    await rpc(ws, "game.labSignal", {"index": slot, "signal": "toggle"})
    await step(ws, 3)
    s = await snap(ws)
    check("旧方块被回收", not any(c["dispenser"] == tube for c in s["cubes"]), str(len(s["cubes"])))
    await step(ws, 12)
    s = await snap(ws)
    check("周期中段管口不再是实体（方块要从这里掉出来）", len(s["solid_rects"]) < solid_before,
          f"{solid_before} → {len(s['solid_rects'])}")
    await step(ws, 30)
    s = await snap(ws)
    fresh = [c for c in s["cubes"] if c["dispenser"] == tube]
    check("0.6 秒后新方块出现在管口", len(fresh) == 1, str(len(fresh)))
    check("而且是从上面掉下来的", fresh and fresh[0]["falling"], str(fresh))
    await step(ws, 90)
    s = await snap(ws)
    fresh = [c for c in s["cubes"] if c["dispenser"] == tube]
    check("它落到了原来那个方块的位置", fresh and abs(fresh[0]["y"] - doomed["y"]) < 1.0,
          f"{fresh[0]['y']:.0f} vs {doomed['y']:.0f}" if fresh else "没有方块")
    check("并且只补了一个", len(fresh) == 1, str(len(fresh)))

    section("16. 凝胶：喷出来、涂在面上、蓝胶把人弹起来")
    s = await load_lab(ws, 1, 3)
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setScore", {"lives": 9})
    disp = [e for e in s["lab"] if e["kind"] == "gel_dispenser"]
    check("1-3 有一台凝胶喷嘴", len(disp) == 1, str([e["cell"] for e in disp]))
    check("装载时地图上还没有涂层", s["gels"] == [], str(s["gels"]))
    # 站远一点，让喷嘴喷两秒
    await rpc(ws, "game.setPlayerPos", {"x": 12 * T, "y": 12 * T})
    await step(ws, 120)
    s = await snap(ws)
    check("空中有飞行的凝胶球", len(s["gel_blobs"]) > 0, str(len(s["gel_blobs"])))
    painted = [g for g in s["gels"] if g["top"] == "blue"]
    check("落地的凝胶把地面顶面涂成了蓝色", len(painted) > 0, str(s["gels"]))

    col = painted[0]["cell"][0]
    floor = painted[0]["cell"][1]
    # 从三格高摔到蓝胶上：会被弹起来；摔到干净地面上不会
    for target, label in [(col, "蓝胶地面"), (col + 5, "干净地面")]:
        await rpc(ws, "game.setPlayerPos", {"x": target * T, "y": (floor - 4) * T, "vx": 0.0, "vy": 0.0})
        rebound = 0.0
        for _ in range(40):
            await step(ws, 2)
            rebound = min(rebound, (await snap(ws))["player"]["vy"])
        if label == "蓝胶地面":
            check("落在蓝胶上会被弹起来（速度反向）", rebound < -200, f"vy={rebound:.0f}")
        else:
            check("落在干净地面上不会弹", rebound > -1, f"vy={rebound:.0f}")
    # 按住下键取消弹跳 —— 这就是在蓝胶上停下来的方法
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": "Down"})
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": (floor - 4) * T, "vx": 0.0, "vy": 0.0})
    rebound = 0.0
    for _ in range(40):
        await step(ws, 2)
        rebound = min(rebound, (await snap(ws))["player"]["vy"])
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": "Down"})
    check("按住下键就不弹了", rebound > -1, f"vy={rebound:.0f}")

    section("17. 信仰之跃：绝对赋值，不是冲量")
    s = await load_lab(ws, 3, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    plate = next(e for e in s["lab"] if e["kind"] == "faith_plate")
    col, row = plate["cell"]
    # 从两格高落到踏板上，无论怎么来都以固定速度离开（右向踏板 = 30/-30 格每秒）
    await rpc(ws, "game.setPlayerPos", {"x": (col + 1) * T, "y": (row - 2) * T, "vx": 0.0, "vy": 0.0})
    launch = (0.0, 0.0)
    for _ in range(10):
        await step(ws, 3)
        p = (await snap(ws))["player"]
        if p["vy"] < launch[1]:
            launch = (p["vx"], p["vy"])
    check("向右的踏板给出 +30 / -30 格每秒", abs(launch[0] - 30 * T) < 1 and abs(launch[1] + 30 * T) < 1,
          f"vx={launch[0]:.0f} vy={launch[1]:.0f}")
    # 关键：上限不是硬夹，而是被 superfriction 慢慢磨掉 —— 否则斜向踏板一出手就被削到走速
    check("水平速度远超走速上限（说明没有被硬夹）", launch[0] > 3 * 205, f"{launch[0]:.0f}")

    section("18. Emancipation Grill：穿过去传送门就散了")
    s = await load_lab(ws, 1, 1)
    await rpc(ws, "game.setState", {"state": "playing"})
    check("1-1 的三道栅栏都解析出了跨度", len(s["grills"]) == 3, str(s["grills"]))
    grill = next(g for g in s["grills"] if not g["horizontal"])
    check("竖直栅栏的跨度是一段连续的空格", grill["end"] >= grill["start"], str(grill))
    # 放一对传送门，然后从栅栏一侧走到另一侧
    await rpc(ws, "game.setPortal", {"index": 0, "x": 10 * T, "y": 3 * T, "orientation": "left"})
    await rpc(ws, "game.setPortal", {"index": 1, "x": 4 * T, "y": 11 * T, "orientation": "right"})
    await step(ws, 2)
    s = await snap(ws)
    check("两个门都在", s["portals"]["blue"] and s["portals"]["orange"])
    gx, row = grill["cell"][0], grill["start"] + 1
    # 直接跨到栅栏另一侧：上一帧的中心在左、这一帧在右 → 扫掠判定命中
    await rpc(ws, "game.setPlayerPos", {"x": (gx - 2) * T, "y": row * T, "vx": 0.0, "vy": 0.0})
    await step(ws, 2)
    await rpc(ws, "game.setPlayerPos", {"x": (gx + 1) * T, "y": row * T, "vx": 0.0, "vy": 0.0})
    await step(ws, 2)
    s = await snap(ws)
    check("穿过栅栏后两个门都消失了", not s["portals"]["blue"] and not s["portals"]["orange"],
          str(s["portals"]))

    section("19. 把镜头横扫过全部九关：clip 出屏不能把画面搞崩")
    # 门是用 scissor 裁到自己那两格的，门滚出屏幕右侧时裁剪框会伸到画面外 ——
    # wgpu 要求 x+w <= 宽度，越界会直接 panic 整帧。这一条扫描就是那次崩溃的复现。
    swept = 0
    for world, level in [(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (2, 2), (2, 3), (2, 4), (3, 1)]:
        await rpc(ws, "game.setLevel", {"pack": "portal", "world": world, "level": level})
        await step(ws, 2)
        await rpc(ws, "game.setState", {"state": "playing"})
        s = await snap(ws)
        for col in range(0, s["level"]["width"], 3):
            await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": T})
            await step(ws)
        swept += 1
    # 活着走到这里就是结论：崩了的话 rpc 会直接抛连接错误。
    check("九关全部扫完，进程还活着", swept == 9, f"{swept}/9")

    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
    await rpc(ws, "game.setScore", {"lives": 3})
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 实验室信号网络验证")
    print("=" * 60)
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except (OSError, websockets.exceptions.WebSocketException) as e:
        print(f"错误: 无法连接到游戏 ({e})。请先启动:")
        print("  cargo run -p mari0 --features vdp")
        return 1

    print()
    if FAILURES:
        print(f"失败 {len(FAILURES)} 项: {FAILURES}")
        return 1
    print("实验室信号网络规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
