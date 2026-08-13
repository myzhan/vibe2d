#!/usr/bin/env python3
"""
mari0 水管 / 子关卡 / warp zone 验证脚本

以前 `LevelId` 只会 `W-L` 递进，`W-L_N` 子关和水管完全没接。这次要验的规则都来自原版：

  1. 站在水管口按下键进入（`mario.lua:930-938`），侧面走进去也算（`:1935-1946`）。
  2. 进管**不是瞬移**：滑入 0.7s + 藏在管里 1s，然后才换关（`:298-313`）；
     出口那侧先静止 1s 再滑出 0.7s。
  3. 进入**子关卡不重置时钟**（`game.lua:2111` 只在非子关分支重置）。
  4. 回到主关时按 `prevsublevel` 匹配 `pipespawn` 落点 —— 这就是 1-1 那根
     "抄近路" 水管：从 (58,9) 下去，从 (165,12) 上来。
  5. warp pipe 直接跳世界，且**重置时钟**（它是离开当前关，不是子关往返）。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_pipe_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
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
    for _ in range(600):
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


async def load(ws, world, level, sublevel=0, pack="smb"):
    await rpc(
        ws,
        "game.setLevel",
        {"pack": pack, "world": world, "level": level, "sublevel": sublevel},
    )
    await step(ws)
    return await snap(ws)


async def press_key(ws, key):
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": key})


async def release_key(ws, key):
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": key})


async def hold(ws, key, frames):
    """Hold one key down for N frames, stepping as we go, then release."""
    await press_key(ws, key)
    await step(ws, frames)
    await release_key(ws, key)


async def stand_on_pipe_and_duck(ws, col, row, frames=40):
    """Drop the player onto the pipe mouth at (col, row), then hold crouch."""
    # 落点：脚站在 row 这一格的上表面。玩家高度 32，所以 y = row*32 - 32。
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": (col - 0.5) * TILE_SIZE, "y": row * TILE_SIZE - TILE_SIZE},
    )
    await step(ws, 3)
    await hold(ws, "Down", frames)
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 1-1 的水管在 (58,9)：站上去按下键就进管")
    s = await load(ws, 1, 1)
    check("起始在主关 1-1", s["level"]["name"] == "1-1", s["level"]["name"])
    check("不在管中", s["pipe"] is None, str(s["pipe"]))

    s = await stand_on_pipe_and_duck(ws, 58, 9, frames=6)
    check("按下键后进入了管道（方向 down）", s["pipe"] == "down", str(s["pipe"]))
    check("进管瞬间还在 1-1", s["level"]["name"] == "1-1", s["level"]["name"])

    section("2. 进管不是瞬移：0.7s 滑入 + 1s 停留才换关")
    # 0.7 + 1.0 = 1.7s ≈ 102 帧。先跑 60 帧，应该还没换关。
    await step(ws, 60)
    s = await snap(ws)
    check(
        "1 秒后仍未换关（还在滑入/停留）",
        s["level"]["name"] == "1-1",
        f"level={s['level']['name']}, pipe={s['pipe']}",
    )
    clock_before = s["time_remaining"]

    await step(ws, 60)
    s = await snap(ws)
    check("1.7 秒后到了子关 1-1_1", s["level"]["name"] == "1-1_1", s["level"]["name"])
    check("sublevel 字段是 1", s["level"]["sublevel"] == 1, str(s["level"]["sublevel"]))

    section("3. 进子关不重置时钟")
    check(
        "时钟延续，没有跳回 400",
        s["time_remaining"] < 400.0 and abs(s["time_remaining"] - clock_before) < 6.0,
        f"进管前 {clock_before:.1f} → 现在 {s['time_remaining']:.1f}",
    )
    check("子关也是有限时的地下关", s["level"]["music"] == 3, f"music={s['level']['music']}")

    section("4. 子关的水管 (13,12) 通回主关，从 pipespawn (164,11) 出来")
    # 1-1_1 宽 17，出口水管 1-based (14,13) → 0-based (13,12)。
    s = await snap(ws)
    check("子关宽度 17", s["level"]["width"] == 17, str(s["level"]["width"]))
    # 侧面走进去：把玩家放在管左边，一路按右。
    await rpc(ws, "game.setPlayerPos", {"x": 11 * TILE_SIZE, "y": 12 * TILE_SIZE - TILE_SIZE})
    await step(ws, 3)
    await press_key(ws, "Right")
    for _ in range(90):
        await step(ws)
        s = await snap(ws)
        if s["pipe"] is not None or s["level"]["name"] != "1-1_1":
            break
    await release_key(ws, "Right")
    check("侧面走进管子触发了 right 方向", s["pipe"] == "right" or s["level"]["name"] == "1-1", str(s["pipe"]))

    for _ in range(200):
        await step(ws, 3)
        s = await snap(ws)
        if s["level"]["name"] == "1-1":
            break
    check("回到了主关 1-1", s["level"]["name"] == "1-1", s["level"]["name"])
    check("sublevel 归 0", s["level"]["sublevel"] == 0, str(s["level"]["sublevel"]))

    # pipespawn 在 1-based (165,12) → 0-based (164,11)。出口应该在那附近，
    # 而不是关卡开头 —— 这正是 "抄近路" 的意义。
    px = s["player"]["x"] / TILE_SIZE
    check(
        "从关卡后段的 pipespawn 出来（不是回到开头）",
        px > 150.0,
        f"玩家在第 {px:.1f} 列",
    )
    check("正在从管里升起（方向 up）或已升完", s["pipe"] in ("up", None), str(s["pipe"]))

    section("5. warp pipe 跳世界：1-2_1 的 (179,10) → 世界 4")
    # 1-2_1 有三根 warppipe，1-based (180,11)/(184,11)/(188,11) → 世界 4/3/2。
    s = await load(ws, 1, 2, sublevel=1)
    check("载入 1-2_1", s["level"]["name"] == "1-2_1", s["level"]["name"])
    # 先把时钟压到 150：这样 "warp 后回到 400" 就是个真断言，而不是
    # "两个都恰好是 400" 的巧合。
    await rpc(ws, "game.setTime", {"time": 150.0})
    s = await stand_on_pipe_and_duck(ws, 179, 10, frames=6)
    check("站上 warp pipe 按下键进管", s["pipe"] == "down", str(s["pipe"]))
    for _ in range(200):
        await step(ws, 3)
        s = await snap(ws)
        if s["level"]["world"] != 1:
            break
    check("跳到了世界 4", s["level"]["world"] == 4, f"world={s['level']['world']}")
    check("落在该世界第 1 关", s["level"]["level"] == 1, f"level={s['level']['level']}")
    # 容差而非精确相等：换关后又跑了几帧，时钟已经往下走了一点。
    check(
        "warp 重置了时钟：150 → 回到限时（离开当前关，不是子关往返）",
        s["level"]["time_limit"] - 5.0 < s["time_remaining"] <= s["level"]["time_limit"],
        f"time={s['time_remaining']:.1f}, limit={s['level']['time_limit']:.1f}",
    )

    section("6. 回归：从水管顶上走过去不该掉进去")
    # 这是实现过程中真实踩到的 bug：侧面进管的判定原本只看"有没有按右"，
    # 而 1-1 的 pipe 实体就在管口右上格 (58,9)，站在管顶的玩家中线正好齐平 ——
    # 于是走过管顶就被吞进去了。autopilot 帧数从 2259 掉到 2055 才暴露出来。
    # 原版把这条判定放在**水平碰撞解算内部**，所以只有真的撞上去才算。
    s = await load(ws, 1, 1)
    # 必须**站在管顶上**：管口只有 57、58 两列宽，放在更左边会直接掉到地面，
    # 那样撞到的是管子侧面（第 12 行）而不是管口实体（第 9 行），复现不了。
    await rpc(ws, "game.setPlayerPos", {"x": 57 * TILE_SIZE, "y": 9 * TILE_SIZE - TILE_SIZE})
    await step(ws, 3)
    await press_key(ws, "Right")
    entered = None
    for _ in range(120):
        await step(ws)
        s = await snap(ws)
        if s["pipe"] is not None:
            entered = s["pipe"]
            break
        if s["player"]["x"] / TILE_SIZE > 60.0:
            break
    await release_key(ws, "Right")
    check(
        "走过管顶没有被吞进去",
        entered is None,
        f"在第 {s['player']['x'] / TILE_SIZE:.1f} 列被吸进了 {entered} 方向",
    )
    check("仍在主关", s["level"]["name"] == "1-1", s["level"]["name"])

    await load(ws, 1, 1)
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 水管 / 子关卡 / warp zone 验证")
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
    print("水管与子关卡规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
