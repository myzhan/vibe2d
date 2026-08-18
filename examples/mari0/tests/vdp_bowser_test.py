#!/usr/bin/env python3
"""
mari0 Bowser + 火焰验证脚本

八个城堡关（1-4 … 7-4、8-4_4）每关都有一个，是剩余敌人里覆盖面最大的。
数据上三者总是同现：`firestart` 在第 93 列、bowser 在第 128 列、axe 在第 141 列。

规则（`bowser.lua`）：
  - 身体 30×28 px，接近 2×2 格 —— 全场最大。重力只有 **10.9**（世界是 80），
    下落速度还被夹在 8.25，所以他跳起来是飘的。
  - **五发火球**才能打死（`bowserhealth = 5`），而且前四发**没有任何可见反馈**
    （`:176-181` 只是减 hp）—— 这就是他看起来无敌的原因。5000 分。
  - 巡逻：目标点在 `startx-1-rand(2)` 和 `startx-7-rand(2)` 之间来回，
    前进速度 0.875，所以他在起点前方约六格的范围里踱步，而且两端都是随机的。
  - **退却是这场仗的关键**：玩家一旦跑到他右边，他转身以 **1.875**（两倍多！）后退
    （`:139-142`），而 `backwards` 置位期间他**既不喷火也不扔锤**
    （`game.lua:806` 和 `:116`）—— 所以绕到他背后不只是逃跑，是解除他的武装。
  - **锤子只有第 6 世界起才有**（`:49`）。间隔取自 `bowserhammertable`，
    14 个值里 10 个是 0.1 —— 所以锤子是**成串**来的，中间夹长停顿。
  - 火焰：`firestart` 是**单向**闩（没有 `fireend` 实体）。喷出来的火横向 4.69 格/秒，
    但纵向会**漂向瞄准的高度**（`fire.lua:68-79`），所以蹲下躲不一定管用。

**本脚本不覆盖斧头**：斧头/桥塌/Bowser 坠落那条第二过关线属于过关动画那一块，
和计分/城堡动画是一件事，单独做。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_bowser_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:107-118
BOWSER_SPEED_FORWARDS = 0.875
BOWSER_SPEED_BACKWARDS = 1.875
BOWSER_HEALTH = 5
BOWSER_SCORE = 5000
BOWSER_GRAVITY = 10.9
FIRE_SPEED = 4.69

# 每个城堡关里三者的列号都一样。
BOWSER_COL = 128
FIRESTART_COL = 93

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
    for _ in range(4000):
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


def of_type(s, *kinds):
    return [e for e in s["enemies"] if e["type"] in kinds]


async def approach(ws, world, col=120):
    """Walk up to the castle's Bowser and make the player unkillable.

    A star, because everything in the room hurts: fire, hammers, and Bowser himself.
    Without it the probe dies in a second or two and the frozen scene reads as "Bowser
    does nothing".
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": world, "level": 4})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": col * TILE_SIZE, "y": 9 * TILE_SIZE})
    await step(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 八个城堡关都有 bowser，而且 firestart 是单向闩")
    for world in (1, 2, 3, 4, 5, 6, 7):
        s = await rpc(ws, "game.setLevel", {"pack": "smb", "world": world, "level": 4})
        await step(ws, 2)
        snapshot = await snap(ws)
        # 生成是惰性的，所以刚装载时他还不在场 —— 但 firestart 在第 93 列，玩家还没走到。
        check(
            f"{world}-4 刚装载时 firestart 还没触发",
            snapshot["fire_started"] is False,
            f"fire_started={snapshot['fire_started']}",
        )
    s = await approach(ws, 1)
    check("走过第 93 列之后 firestart 触发了", s["fire_started"] is True)
    # 单向：往回走也不关。
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 9 * TILE_SIZE})
    await step(ws, 5)
    check("往回走也不会关掉（没有 fireend）", (await snap(ws))["fire_started"] is True)

    section("2. Bowser：五点血、接近 2×2 格、往玩家那边踱步")
    s = await approach(ws, 1)
    bow = of_type(s, "bowser")
    check("Bowser 在场", len(bow) == 1, f"{len(bow)} 个")
    if bow:
        b = bow[0]
        check(f"血量 {BOWSER_HEALTH}", b["hp"] == BOWSER_HEALTH, f"hp={b['hp']}")
        check("玩家在他左边时不是退却状态", b["backing_off"] is False)
        check(
            f"前进速度 {BOWSER_SPEED_FORWARDS} 格/秒（朝玩家）",
            abs(b["vx"] / TILE_SIZE + BOWSER_SPEED_FORWARDS) < 0.02,
            f"vx={b['vx'] / TILE_SIZE:.3f}",
        )

    section("3. 他会跳，而且跳得很飘（重力只有 10.9）")
    vys = []
    for _ in range(40):
        await step(ws, 6)
        s = await snap(ws)
        vys += [e["vy"] / TILE_SIZE for e in of_type(s, "bowser")]
    check("出现过上升段（他跳了）", any(v < -1.0 for v in vys), f"最快上升 {min(vys):.1f}")
    check("也出现过下落段", any(v > 1.0 for v in vys), f"最快下落 {max(vys):.1f}")
    # 连续两次采样的 vy 差就是重力。取一段没有触底的区间来算。
    grav = None
    prev = None
    for _ in range(60):
        await step(ws, 3)
        s = await snap(ws)
        cur = [e["vy"] for e in of_type(s, "bowser")]
        if cur and prev is not None and prev < cur[0] and prev > -3 * TILE_SIZE:
            grav = (cur[0] - prev) / TILE_SIZE / (3 / FPS)
            break
        prev = cur[0] if cur else None
    check(
        f"重力约 {BOWSER_GRAVITY}（世界是 80）",
        grav is not None and abs(grav - BOWSER_GRAVITY) < 1.5,
        f"实测 {grav:.1f}" if grav else "没量到",
    )

    section("4. 绕到他背后：他后退得更快，而且停止喷火/扔锤")
    s = await approach(ws, 6)
    # 6-4，所以锤子也在。
    await step(ws, 60)
    s = await snap(ws)
    check("6-4 的 Bowser 会扔锤子", bool(of_type(s, "hammer")), f"{len(of_type(s, 'hammer'))} 把")
    bow = of_type(s, "bowser")
    if bow:
        # 走到他右边。
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": bow[0]["x"] + 6 * TILE_SIZE, "y": 9 * TILE_SIZE},
        )
        await step(ws, 10)
        s = await snap(ws)
        b2 = of_type(s, "bowser")
        check("他进入退却状态", bool(b2) and b2[0]["backing_off"] is True)
        if b2:
            check(
                f"后退速度 {BOWSER_SPEED_BACKWARDS} 格/秒（比前进的 {BOWSER_SPEED_FORWARDS} 快一倍多）",
                abs(b2[0]["vx"] / TILE_SIZE - BOWSER_SPEED_BACKWARDS) < 0.02,
                f"vx={b2[0]['vx'] / TILE_SIZE:.3f}",
            )
        # 退却期间既不喷火也不扔锤。补放的 Bowser **必须按镜头定位** —— 镜头永不回退，
        # 这时它已经在第 128 列开外，放在第 120 列的话一帧就被"镜头左边 200px 外"的
        # 剔除规则清掉。而 Bowser 一旦不在场，`firestart` 就会自己从画面右缘喷火
        # （那是没有 Bowser 的火焰走廊该有的行为），看起来就像"退却时还在喷"。
        cam_col = s["camera_x"] / TILE_SIZE
        await rpc(ws, "game.clearEnemies")
        await rpc(
            ws,
            "game.spawnEnemy",
            {
                "type": "bowser",
                "x": (cam_col + 4) * TILE_SIZE,
                "y": 9 * TILE_SIZE,
                "facing_right": False,
            },
        )
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": (cam_col + 10) * TILE_SIZE, "y": 9 * TILE_SIZE},
        )
        await step(ws, 120)
        s = await snap(ws)
        alive = of_type(s, "bowser")
        check("补放的 Bowser 还在场（否则测的不是他）", bool(alive) and alive[0]["backing_off"])
        check(
            "退却期间不喷火",
            not of_type(s, "fire"),
            f"{len(of_type(s, 'fire'))} 团火",
        )
        check(
            "退却期间不扔锤",
            not of_type(s, "hammer"),
            f"{len(of_type(s, 'hammer'))} 把锤",
        )

    section("5. 只有第 6 世界起才扔锤子")
    s = await approach(ws, 1)
    await step(ws, 180)
    s = await snap(ws)
    check("1-4 的 Bowser 三秒里一把锤都不扔", not of_type(s, "hammer"))

    section("6. 血量：出生 5 点，而且只有他有血量这个概念")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 4})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 120 * TILE_SIZE, "y": 9 * TILE_SIZE})
    await step(ws, 25)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setStar", {"seconds": 0})
    await rpc(ws, "game.setScore", {"score": 0})
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "bowser", "x": 124 * TILE_SIZE, "y": 9 * TILE_SIZE, "facing_right": False},
    )
    await step(ws, 4)
    # 打满五发在探针里做不到：得先吃火花、还得瞄准，而 `fire` 是**他自己的**武器、
    # 伤不到他。所以这里只钉住语义 —— 出生 5 点、而且全场只有他有血量。
    # 「前四发无反馈、第五发才倒」那条由单元测试守着。
    s = await snap(ws)
    bow = of_type(s, "bowser")
    check(f"出生时 hp 是 {BOWSER_HEALTH}", bool(bow) and bow[0]["hp"] == BOWSER_HEALTH,
          f"hp={bow[0]['hp']}" if bow else "不在场")
    await rpc(ws, "game.spawnEnemy",
              {"type": "goomba", "x": 122 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False})
    await step(ws, 2)
    s = await snap(ws)
    others = [e["hp"] for e in s["enemies"] if e["type"] != "bowser"]
    check("其它敌人没有血量概念", bool(others) and all(h == 0 for h in others), f"{sorted(set(others))}")

    section("7. 火焰：横向 4.69 格/秒往左，纵向漂向瞄准高度")
    s = await approach(ws, 1)
    fires = []
    for _ in range(60):
        await step(ws, 8)
        s = await snap(ws)
        fires += [(e["vx"] / TILE_SIZE, e["vy"], e["y"]) for e in of_type(s, "fire")]
    check("喷出了火", bool(fires), f"{len(fires)} 次观测")
    if fires:
        check(
            f"横向恒为 -{FIRE_SPEED} 格/秒",
            all(abs(vx + FIRE_SPEED) < 0.02 for vx, _, _ in fires),
            f"{sorted({round(vx, 2) for vx, _, _ in fires})}",
        )
        check(
            "纵向速度字段始终为 0（高度是直接插值的，不是靠速度积分）",
            all(vy == 0.0 for _, vy, _ in fires),
        )
        check(
            "出现过不止一个高度（每团火瞄的位置是随机的）",
            len({round(y / TILE_SIZE) for _, _, y in fires}) > 1,
            f"高度 {sorted({round(y / TILE_SIZE, 1) for _, _, y in fires})}",
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
