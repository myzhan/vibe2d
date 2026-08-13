#!/usr/bin/env python3
"""
mari0 传送门验证脚本

规则来自原版 `game.lua:3076`（`getportalposition`）与 `physics.lua:452-940`
（`inportal` / `checkportalHOR` / `checkportalVER` / `portalcoords`）：

  1. 门是 **1 格宽 × 2 格长**、锚定在 tile 上；锚点按面归一化（up 取低列、down 取高列、
     right 取低行、left 取高行）。
  2. **单个门不是洞**：`modifyportaltiles` 要两个门都存在才删碰撞 tile，
     而且只删碰撞 —— 墙照常渲染、`getTile` 照常报实心。
  3. 入口判定是**扫掠**的：判断物体中心这一步有没有跨过门的平面，
     所以高速穿过也不会漏检（纯 AABB 重叠判定会漏）。
  4. 出口被堵时**不传送而是反弹**：地板门 `-vy*0.95`（有下限），墙门 `-vx`（无衰减）。
  5. 朝上的出口有最小速度 `sqrt(2·g·height)`，保证一定钻得出地面。
  6. 相对的两个面**完全不改速度** —— 无限下落加速就是这么来的。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_portal_test.py

依赖: pip install websockets
"""
import asyncio
import json
import math
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0
# GRAVITY = 2560 px/s^2 = 80 blocks/s^2; 最小出口速度 = sqrt(2*80*1) blocks/s。
MIN_UP_SPEED = math.sqrt(2 * (2560.0 / T) * 1.0) * T

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


async def fresh(ws):
    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
    await rpc(ws, "game.reset")
    await step(ws)


async def set_portal(ws, index, col, row, facing):
    """Place a portal by its mouth centre, which is what `game.setPortal` takes."""
    await rpc(
        ws,
        "game.setPortal",
        {"index": index, "x": col * T, "y": row * T, "orientation": facing, "active": True},
    )


async def park(ws):
    """Somewhere harmless, long enough for the 0.15s teleport cooldown to lapse.

    Without this the next case is refused and looks like a missed teleport — which is
    exactly how the first draft of this script fooled me.
    """
    await rpc(ws, "game.setPlayerPos", {"x": 3 * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    await step(ws, 20)


async def drop_into(ws, col, row, vy, frames=30, landed_col=15.0):
    """Drop the player down column `col` from `row` and report the teleport, if any."""
    await park(ws)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": row * T, "vx": 0.0, "vy": vy})
    for i in range(frames):
        await step(ws)
        s = await snap(ws)
        if s["player"]["x"] / T > landed_col:
            return i, s
    return None, await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 门是 1×2 的，锚点按面归一化")
    await fresh(ws)
    await set_portal(ws, 0, 10, 13, "up")
    s = await snap(ws)
    b = s["portals"]["blue"]
    check("up 面锚在低列", b["anchor"] == [9, 13], str(b["anchor"]))
    check("覆盖同一行的两列", b["cells"] == [[9, 13], [10, 13]], str(b["cells"]))
    await set_portal(ws, 0, 10, 13, "left")
    b = (await snap(ws))["portals"]["blue"]
    check("left 面锚在高行", b["anchor"] == [10, 13], str(b["anchor"]))
    check("覆盖同一列的两行", b["cells"] == [[10, 12], [10, 13]], str(b["cells"]))

    section("2. 单个门不是洞：踩在上面不会掉下去")
    await fresh(ws)
    await set_portal(ws, 0, 10, 13, "up")
    await rpc(ws, "game.setPlayerPos", {"x": 9 * T, "y": 11 * T, "vx": 0.0, "vy": 200.0})
    await step(ws, 20)
    s = await snap(ws)
    check(
        "只有一个门时地面仍是实心的",
        s["player"]["on_ground"] and s["player"]["y"] / T < 13.0,
        f"row={s['player']['y'] / T:.2f} on_ground={s['player']['on_ground']}",
    )

    section("3. 两个门齐备 → 那四格变成洞，掉进去会传送")
    await fresh(ws)
    await set_portal(ws, 0, 10, 13, "up")
    await set_portal(ws, 1, 20, 13, "up")
    frame, s = await drop_into(ws, 9, 10, 300.0)
    check("掉进蓝门后出现在橙门处", frame is not None, f"第 {frame} 帧" if frame is not None else "没有传送")
    if frame is not None:
        check("落点在第 19 列附近", 18.0 < s["player"]["x"] / T < 21.0, f"col={s['player']['x'] / T:.2f}")
        check("出口朝上（vy 为负）", s["player"]["vy"] < 0.0, f"vy={s['player']['vy']:.1f}")

    section("4. 扫掠判定：高速穿过也不漏检")
    # 3000 px/s 时一帧要走 50px，远超门口的厚度；纯重叠判定会整帧跨过去而看不见。
    for vy in (300.0, 1500.0):
        frame, s = await drop_into(ws, 9, 10, vy)
        check(f"vy={vy:.0f} 时被接住", frame is not None, f"第 {frame} 帧" if frame else "漏了")

    section("5. 入得太深 → 出口会撞地，于是反弹而不是传送")
    # 出口深度由入口深度决定；一帧走 50px 时中心已远离平面，出口就会伸进洞下方的
    # 实心地面。原版这时不传送，改成把速度反弹回去（`physics.lua:592-604`）。
    await park(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 9 * T, "y": 10 * T, "vx": 0.0, "vy": 3000.0})
    bounced = None
    for _ in range(6):
        await step(ws)
        s = await snap(ws)
        if s["player"]["vy"] < 0.0:
            bounced = s["player"]["vy"]
            break
    check("被反弹回上方", bounced is not None, f"vy={bounced:.1f}" if bounced else "没有反弹")
    if bounced is not None:
        check("仍留在原地（没有传送）", s["player"]["x"] / T < 11.0, f"col={s['player']['x'] / T:.2f}")
        check(
            "地板门的反弹带 0.95 衰减",
            abs(bounced) < 3200.0,
            f"|vy|={abs(bounced):.1f} 应小于入射速度",
        )

    section("6. 朝上的出口有最小速度：慢慢滑进去也一定钻得出来")
    await fresh(ws)
    await set_portal(ws, 0, 10, 13, "up")
    await set_portal(ws, 1, 20, 13, "up")
    frame, s = await drop_into(ws, 9, 12, 20.0, frames=40)
    check("低速也能传送", frame is not None, f"第 {frame} 帧" if frame is not None else "没有传送")
    if frame is not None:
        check(
            f"出口速度被抬到最小值 {MIN_UP_SPEED:.1f}",
            abs(s["player"]["vy"] + MIN_UP_SPEED) < 30.0,
            f"vy={s['player']['vy']:.1f}",
        )

    section("7. 相对的两面完全不改速度（无限下落加速的来源）")
    # 地板上的 up 门 + 天花板上的 down 门：掉进去应当保持速度继续下落。
    await fresh(ws)
    await set_portal(ws, 0, 10, 13, "up")
    await set_portal(ws, 1, 20, 4, "down")
    await park(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 9 * T, "y": 11 * T, "vx": 0.0, "vy": 400.0})
    before = None
    after = None
    for _ in range(20):
        s = await snap(ws)
        prev = s["player"]
        await step(ws)
        s = await snap(ws)
        if s["player"]["x"] / T > 15.0:
            before, after = prev["vy"], s["player"]["vy"]
            break
    check("发生了传送", after is not None, "" if after is not None else "没有传送")
    if after is not None:
        check("速度方向不变（仍向下）", after > 0.0, f"vy {before:.1f} → {after:.1f}")
        check(
            "速度大小保持（up→down 不改速度）",
            abs(abs(after) - abs(before)) < 60.0,
            f"|vy| {abs(before):.1f} → {abs(after):.1f}",
        )

    section("8. 清掉门之后洞也消失")
    await rpc(ws, "game.clearPortals")
    await rpc(ws, "game.setPlayerPos", {"x": 9 * T, "y": 11 * T, "vx": 0.0, "vy": 200.0})
    await step(ws, 25)
    s = await snap(ws)
    check(
        "地面恢复实心",
        s["player"]["on_ground"] and s["player"]["y"] / T < 13.0,
        f"row={s['player']['y'] / T:.2f} on_ground={s['player']['on_ground']}",
    )

    section("9. 敌人也走传送门（原版是每个 mover 自己调这三段判定）")
    for kind in ("goomba", "koopa"):
        await fresh(ws)
        await set_portal(ws, 0, 10, 13, "up")
        await set_portal(ws, 1, 20, 13, "up")
        await rpc(ws, "game.clearEnemies")
        # 生成在洞口正上方：它一边下落一边走，偏出洞口就会落在洞沿上 ——
        # 探针第一版就是从第 9 列放的，飘到第 8 列踩住了洞沿，看着像"不会传送"。
        # 另外**不要**把玩家挪到关卡中段：镜头一跟过去，惰性生成就会在他旁边
        # 放出一只栗子怪，玩家立刻死亡，`update_playing` 停摆，整个场景冻住。
        await rpc(ws, "game.spawnEnemy", {"type": kind, "x": 10 * T, "y": 11 * T})
        moved = None
        for i in range(60):
            await step(ws)
            s = await snap(ws)
            if s["state"] != "playing" or not s["enemies"]:
                break
            if s["enemies"][0]["x"] / T > 15.0:
                moved = (i, s["enemies"][0]["x"] / T)
                break
        check(
            f"{kind} 掉进洞里被传送到另一个门",
            moved is not None,
            f"第 {moved[0]} 帧 → 第 {moved[1]:.1f} 列" if moved else "没有传送",
        )

    await fresh(ws)
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 传送门验证")
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
    print("传送门规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
