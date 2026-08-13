#!/usr/bin/env python3
"""
mari0 复活点（checkpoint）验证脚本

原版规则（`mario.lua:998-1005`、`game.lua:2144-2164`、`levelscreen.lua:11/34/43/49`）：

  1. 只看**下一个**未通过的复活点，`x > 该列` 就算通过 —— 所以往回走不会取消。
  2. 复活点**只在死亡时生效**：`checkcheckpoint` 每次换关都先清成 false，
     只有死亡分支才置 true。进新关一律从关卡自己的出发点开始。
  3. 死亡重生回 `respawnsublevel`。它只在**从过场桩（intermission）进管**时被设置，
     所以在 1-2_1 里死掉不会被丢回 24 格宽的 1-2。
  4. 进下一关 / game over 都会清掉复活点。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_progress_test.py

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


async def tap(ws, key):
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "tap", "key": key})
    await step(ws)


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


async def die_and_respawn(ws):
    """Force a death, then press jump to continue."""
    await rpc(ws, "game.setState", {"state": "dead"})
    await step(ws, 2)
    await tap(ws, "Space")
    await step(ws, 3)
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 1-1 的复活点在第 82 列（0 基），一开始没通过")
    s = await load(ws, 1, 1)
    cps = s["level"]["checkpoints"]
    check("1-1 有一个复活点", len(cps) == 1, str(cps))
    # 82 而不是 83：关卡文件里的坐标是 1 基的，解析器统一转成 0 基网格索引，
    # inspect 里出来的就是 0 基。第一版断言写了 83，混了两套坐标。
    check("它在第 82 列（0 基）", cps and cps[0][0] == 82, str(cps))
    check("开局尚未通过任何复活点", s["checkpoint"] is None, str(s["checkpoint"]))

    section("2. 走过第 82 列就记下复活点")
    await rpc(ws, "game.setPlayerPos", {"x": 90 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 5)
    s = await snap(ws)
    check("通过后记下了复活点", s["checkpoint"] == cps[0], str(s["checkpoint"]))

    section("3. 往回走不会取消（只看下一个未通过的）")
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 5)
    s = await snap(ws)
    check("退回开头，复活点仍在", s["checkpoint"] == cps[0], str(s["checkpoint"]))

    section("4. 死亡后从复活点重生，而不是关卡开头")
    s = await die_and_respawn(ws)
    px = s["player"]["x"] / TILE_SIZE
    # 精确到那一列：容差写宽会让 82/83 的差别蒙过去。
    check("重生在复活点那一列", abs(px - 82.0) < 0.5, f"玩家在第 {px:.1f} 列")
    check("镜头也跟到了那里", s["camera_x"] > 60 * TILE_SIZE, f"camera_x={s['camera_x']:.0f}")
    check("复活点还留着（可以再死一次）", s["checkpoint"] == cps[0], str(s["checkpoint"]))
    check("重生后仍在 1-1", s["level"]["name"] == "1-1", s["level"]["name"])

    section("5. 复活点不影响进新关：进下一关从头开始")
    await rpc(ws, "game.nextLevel")
    await step(ws, 3)
    s = await snap(ws)
    check("到了下一关", s["level"]["name"] != "1-1", s["level"]["name"])
    check("复活点被清掉", s["checkpoint"] is None, str(s["checkpoint"]))
    px = s["player"]["x"] / TILE_SIZE
    check("从关卡自己的出发点开始", px < 10.0, f"玩家在第 {px:.1f} 列")

    section("6. 没有复活点的关卡：死亡从头开始")
    s = await load(ws, 1, 4)  # 城堡关，无复活点
    check("1-4 没有复活点", s["level"]["checkpoints"] == [], str(s["level"]["checkpoints"]))
    await rpc(ws, "game.setPlayerPos", {"x": 60 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 5)
    s = await die_and_respawn(ws)
    px = s["player"]["x"] / TILE_SIZE
    check("重生回关卡开头", px < 10.0, f"玩家在第 {px:.1f} 列")

    section("7. 过场桩：从 1-2 进管后，死亡回 1-2_1 而不是 1-2")
    s = await load(ws, 1, 2)
    check("1-2 是过场桩", s["level"]["intermission"], str(s["level"]["intermission"]))
    check("初始 respawn_sublevel 为 0", s["respawn_sublevel"] == 0, str(s["respawn_sublevel"]))
    # 1-2 的管子在 0-based (10,12)，是侧面管口：走右边撞进去。
    await rpc(ws, "game.setPlayerPos", {"x": 7 * TILE_SIZE, "y": 12 * TILE_SIZE - TILE_SIZE})
    await step(ws, 3)
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": "press", "key": "Right"})
    for _ in range(120):
        await step(ws)
        s = await snap(ws)
        if s["pipe"] is not None:
            break
    await rpc(
        ws, "engine.simulateInput", {"device": "keyboard", "action": "release", "key": "Right"}
    )
    check("撞进了侧面管口", s["pipe"] == "right", str(s["pipe"]))
    check("respawn_sublevel 记成了 1", s["respawn_sublevel"] == 1, str(s["respawn_sublevel"]))

    for _ in range(200):
        await step(ws, 3)
        s = await snap(ws)
        if s["level"]["name"] == "1-2_1":
            break
    check("到了真正的地下关 1-2_1", s["level"]["name"] == "1-2_1", s["level"]["name"])

    s = await die_and_respawn(ws)
    check(
        "在 1-2_1 死掉后重生回 1-2_1（不是 24 格的 1-2）",
        s["level"]["name"] == "1-2_1",
        s["level"]["name"],
    )

    await load(ws, 1, 1)
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 复活点验证")
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
    print("复活点规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
