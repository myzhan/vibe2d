#!/usr/bin/env python3
"""
mari0 敌人惰性生成验证脚本

原版不在装载时生成敌人，而是**随镜头按列惰性生成**（`game.lua:681-686`，
`spawnenemy` 在 `:3687`）。这不只是省内存：8-1 有 **400 tile 宽**，从第一帧就存在的
敌人早在玩家赶到之前就走下平台了。所以这是手感，不是优化。

生成前沿在**镜头左沿 + 一屏宽 + 1 列**处，即屏幕右缘外一格。1-1 的第一个敌人在
第 23 列、8-1 在第 19 列，而开局前沿是第 17 列 —— 所以**开局一个敌人都不该存在**。
这正是本脚本第 1 节要钉住的边界：以前所有 17 只栗子怪从第一帧就活着。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_spawn_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
# 虚拟分辨率 512 宽 = 16 tile，前沿 = 镜头列 + 16 + 1。
SCREEN_COLS = 16
FRONTIER_AHEAD = SCREEN_COLS + 1

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
    for _ in range(400):
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


async def load(ws, world, level, pack="smb"):
    await rpc(ws, "game.setLevel", {"pack": pack, "world": world, "level": level})
    await step(ws)
    return await snap(ws)


async def camera_to(ws, col):
    """Drag the camera up to `col` by teleporting the player, then settle."""
    await rpc(ws, "game.setPlayerPos", {"x": col * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 10)
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 开局前沿在第 17 列，1-1 的第一只栗子怪在第 23 列 → 开局无敌人")
    s = await load(ws, 1, 1)
    check(
        "1-1 开局一个敌人都没有（以前是 17 只全活着）",
        len(s["enemies"]) == 0,
        f"{len(s['enemies'])} 个",
    )
    check("镜头在原点", s["camera_x"] == 0.0, f"camera_x={s['camera_x']}")

    section("2. 镜头推到第 23 列，第一只栗子怪出现")
    # 前沿要盖住第 23 列，镜头列需要 ≥ 23 - 17 = 6。取 8 留点余量。
    s = await camera_to(ws, 8 + SCREEN_COLS // 3)
    first = [e for e in s["enemies"] if e["type"] == "goomba"]
    check("第一只栗子怪已生成", len(first) >= 1, f"{len(first)} 只")
    if first:
        col = min(e["x"] for e in first) / TILE_SIZE
        check("它就在第 23 列附近", 21.0 <= col <= 24.0, f"col={col:.1f}")

    section("3. 前沿之外始终没有敌人")
    for target_col in (30, 60, 100):
        s = await camera_to(ws, target_col)
        cam_col = s["camera_x"] / TILE_SIZE
        frontier_px = (cam_col + FRONTIER_AHEAD + 1) * TILE_SIZE
        ahead = [e for e in s["enemies"] if e["x"] > frontier_px]
        check(
            f"镜头列 {cam_col:.0f}：前沿 {frontier_px / TILE_SIZE:.0f} 列之外无敌人",
            not ahead,
            f"越界 {len(ahead)} 个，最远 {max((e['x'] / TILE_SIZE for e in ahead), default=0):.0f} 列",
        )

    section("4. 生成过的格子永不复活")
    s = await load(ws, 1, 1)
    s = await camera_to(ws, 30)
    revealed = len(s["enemies"])
    check("已揭开一批敌人", revealed > 0, f"{revealed} 个")
    await rpc(ws, "game.clearEnemies")
    await step(ws, 2)
    check("清空成功", len((await snap(ws))["enemies"]) == 0)
    # 前沿只会前进，但即便原地空转很久，扫过的列也不该再吐敌人。
    await step(ws, 180)
    s = await snap(ws)
    check(
        "原地空转 180 帧后仍是 0（被清掉的不会回来）",
        len(s["enemies"]) == 0,
        f"{len(s['enemies'])} 个又出现了",
    )

    section("5. 重载关卡重新武装生成器")
    s = await load(ws, 1, 1)
    check("重载后回到开局的 0 个", len(s["enemies"]) == 0, f"{len(s['enemies'])} 个")
    s = await camera_to(ws, 30)
    check(
        "再推镜头，敌人又能生成",
        len(s["enemies"]) == revealed,
        f"{len(s['enemies'])} vs 之前 {revealed}",
    )

    section("6. 8-1（400 tile 宽）整关敌人远多于任一时刻在场的数量")
    s = await load(ws, 8, 1)
    check("8-1 宽 400", s["level"]["width"] == 400, f"width={s['level']['width']}")
    check("8-1 开局也是 0 个（第一只在第 19 列）", len(s["enemies"]) == 0, f"{len(s['enemies'])} 个")

    seen, peak = set(), 0
    for col in range(0, 400, 10):
        s = await camera_to(ws, col)
        peak = max(peak, len(s["enemies"]))
        for e in s["enemies"]:
            seen.add((round(e["x"]), round(e["y"]), e["type"]))
    check("整关确实生成了大批敌人", len(seen) > 40, f"整关见过 {len(seen)} 个")
    check(
        "任一时刻在场的远少于整关总数（这就是惰性生成的意义）",
        peak < len(seen) / 2,
        f"峰值同屏 {peak} 个 vs 整关 {len(seen)} 个",
    )

    await load(ws, 1, 1)
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 敌人惰性生成验证")
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
    print("惰性生成规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
