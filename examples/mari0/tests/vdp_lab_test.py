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
    for world, level in [(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (2, 2), (2, 3), (2, 4), (3, 1)]:
        s = await load_lab(ws, world, level)
        lab = s["lab"]
        linked = [e for e in lab if e["driver"] is not None]
        # 有 link 的元件在 inspect 里就是 driver 不为 null 的；解析失败会是 null。
        # 单元测试已经逐关断言过"没有悬空 link"，这里确认运行时确实建起了网络。
        check(
            f"{world}-{level}: {len(lab)} 个元件，{len(linked)} 条连线已接上",
            len(lab) == 0 or len(linked) > 0,
            f"元件类型 {dict(Counter(e['kind'] for e in lab))}" if not lab else "",
        )

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

    section("3. 尚未实现行为的元件被标成 inert，不假装能用")
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

    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
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
