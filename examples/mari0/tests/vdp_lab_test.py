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

    section("3. 尚未实现行为的元件被标成 inert，不假装能用")
    s = await load_lab(ws, 1, 1)
    lab = s["lab"]
    inert = sorted({e["kind"] for e in lab if e["inert"]})
    check("box / box_tube 标为 inert", inert == ["box", "box_tube"], str(inert))
    check("门和按钮不是 inert", not any(e["inert"] for e in lab if e["kind"] in ("door", "button")))

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
    check("每覆盖一格铺一块薄板", len(s["solid_rects"]) == cells, f"{len(s['solid_rects'])} 板 / {cells} 格")
    thin = [r for r in s["solid_rects"] if min(r[2], r[3]) < T / 4]
    check("每块都是薄的（1/8 格左右）", len(thin) == len(s["solid_rects"]))
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
    before = len(s["solid_rects"])
    await rpc(ws, "game.labSignal", {"index": idx, "signal": "off"})
    await step(ws, 2)
    s = await snap(ws)
    check("薄板数量减少", len(s["solid_rects"]) < before, f"{before} → {len(s['solid_rects'])}")
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
