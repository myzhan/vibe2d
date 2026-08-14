#!/usr/bin/env python3
"""
mari0 铁锤兵（Hammer Bro）验证脚本

在 3-1 / 5-2 / 7-1 / 8-3 / 8-4_4 五关。规则全部照抄 `hammerbro.lua`：

  1. **不是走，是原地挪**：巡逻范围只有一格，从 `startx - 1` 到 `startx`，
     速度 1.5 格/秒（别人都是 2）。
  2. **朝向和移动方向无关**：每帧朝着玩家（`:144`），所以他会一边往后挪一边朝你扔。
  3. **每 0.6 或 1.6 秒扔一把锤子**（`hammerbrotime` 两个值里随机取一个）。
     出手前 `hammerbropreparetime = 0.5` 秒举锤 —— 那半秒就是留给你躲的。
  4. **穿楼层是靠临时关掉自己的碰撞**：每 3 秒跳一次，往上 `speedy = -19`、
     往下 `-6`，两种都会把 `mask[2]` 打开。而 mari0 的 mask 是**排除**表
     （`physics.lua:113` 判据是 `mask[cat] ~= true` 才碰撞），所以"打开"= 不再撞 tile。
     他不是跳到楼上去，而是**穿过天花板**再把碰撞打开，落在它上面。
     往上跳在开始下落时结束；往下跳要掉到起点下方 2 格才结束（这就是选中下一层地板）。
     y > 12 格强制往上、y < 6 格强制往下，中间随机。
  5. **重力只有 40**（世界是 80），所以他的跳看着是飘的。
  6. 锤子：出手速度 4 格/秒横向 + 8 格/秒向上、重力 25、**不吃地形**、
     **踩不到也打不掉**（火球在它上面炸开但拦不住它）。
  7. 踩死他给 1000 分（`firepoints["hammerbro"]`，全场第二高，只低于 Bowser）。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_hammerbro_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:240-251
HAMMERBRO_SPEED = 1.5
HAMMERBRO_PATROL = 1.0
HAMMERBRO_TIME = (0.6, 1.6)
HAMMERBRO_PREPARE_TIME = 0.5
HAMMERBRO_JUMP_TIME = 3.0
HAMMER_SPEED = 4.0
HAMMER_TOSS_SPEED = 8.0
HAMMER_GRAVITY = 25.0

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


async def watch_31(ws, seconds, chunk=15):
    """Stand next to 3-1's pair of hammer bros and sample.

    A star, not `clearEnemies`: the bros themselves are the subject, and hammers
    fly through walls, so there is no vantage point that is out of range. Without
    it the player dies within a second or two and the frozen scene reads as "the
    hammer bro does nothing".
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 3, "level": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 105 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    frames = []
    for _ in range(int(seconds * FPS / chunk)):
        await step(ws, chunk)
        frames.append(await snap(ws))
    return frames


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 3-1 有两个铁锤兵，一个在第 9 行、一个在第 13 行（1 基）")
    frames = await watch_31(ws, 1.0)
    bros = of_type(frames[0], "hammer_bro")
    check("两个都生成了", len(bros) == 2, f"{len(bros)} 个")
    rows = sorted(round(b["y"] / TILE_SIZE) for b in bros)
    check("分处两层（第 8 行和第 12 行，0 基）", rows == [8, 12], f"{rows}")

    section("2. 巡逻只有一格宽，速度 1.5 格/秒")
    frames = await watch_31(ws, 4.0, chunk=6)
    # 两个的锚点是第 113 和 116 列，巡逻区间 112..113 和 115..116 —— 只隔两格，
    # 分组窗口开太宽就会把对方也框进来（一开始就是这么错的，跨度量成了 3.95）。
    for idx, (lo, hi) in enumerate(((111.5, 113.5), (114.5, 116.5))):
        xs = [
            b["x"] / TILE_SIZE
            for f in frames
            for b in of_type(f, "hammer_bro")
            if lo <= b["x"] / TILE_SIZE <= hi
        ]
        if not xs:
            check(f"第 {idx + 1} 个铁锤兵有采样", False)
            continue
        span = max(xs) - min(xs)
        check(
            f"第 {idx + 1} 个的活动范围约一格",
            0.5 < span < 1.3,
            f"跨度 {span:.2f} 格（{min(xs):.2f}..{max(xs):.2f}）",
        )
    speeds = {
        round(abs(b["vx"]) / TILE_SIZE, 2)
        for f in frames
        for b in of_type(f, "hammer_bro")
    }
    check(
        f"横向速度就是 {HAMMERBRO_SPEED} 格/秒",
        speeds and all(abs(v - HAMMERBRO_SPEED) < 0.05 for v in speeds),
        f"观测到 {sorted(speeds)}",
    )

    section("3. 朝向跟着玩家，和挪动方向无关")
    # 玩家在第 105 列，两个铁锤兵在 113/116 → 都该朝左。
    facings = {b["facing_right"] for f in frames for b in of_type(f, "hammer_bro")}
    check("玩家在左侧时全都朝左", facings == {False}, f"{facings}")
    # 有些采样里他正往右挪（vx > 0）却仍然朝左 —— 这正是要钉住的那一条。
    mismatch = any(
        b["vx"] > 0 and not b["facing_right"]
        for f in frames
        for b in of_type(f, "hammer_bro")
    )
    check("确实出现过「往右挪、朝左看」", mismatch)

    section("4. 锤子：斜上抛、穿地形、越飞越快往下掉")
    hammers = [h for f in frames for h in of_type(f, "hammer")]
    check("扔出了锤子", bool(hammers), f"{len(hammers)} 次观测")
    if hammers:
        speeds = {round(abs(h["vx"]) / TILE_SIZE, 2) for h in hammers}
        check(
            f"横向速度是 {HAMMER_SPEED} 格/秒",
            all(abs(v - HAMMER_SPEED) < 0.05 for v in speeds),
            f"{sorted(speeds)}",
        )
        check("有锤子在上升段（初速向上）", any(h["vy"] < 0 for h in hammers))
        # 穿地形：3-1 的地面在第 13 行，锤子能掉到它下面去。
        deepest = max(h["y"] / TILE_SIZE for h in hammers)
        check(
            "锤子能穿过地面继续往下掉（不吃地形）",
            deepest > 13.5,
            f"最深到第 {deepest:.1f} 行",
        )

    section("5. 举锤是出手前 0.5 秒 —— 投掷计时器的最后半秒")
    # `cycle_timer` 就是投掷倒计时，只需要确认它落在 0..1.6 之间并且会重置。
    timers = [b["cycle_timer"] for f in frames for b in of_type(f, "hammer_bro")]
    check(
        f"投掷倒计时始终在 0..{HAMMERBRO_TIME[1]} 之间",
        timers and all(-0.01 <= t <= HAMMERBRO_TIME[1] + 0.01 for t in timers),
        f"范围 {min(timers):.2f}..{max(timers):.2f}",
    )
    check(
        "两个投掷间隔都出现过（0.6 和 1.6 各自重置）",
        any(t > 1.0 for t in timers) and any(t < 0.7 for t in timers),
    )

    section("6. 每 3 秒换一层楼：穿过天花板/地板，不是跳上去")
    frames = await watch_31(ws, 8.0, chunk=6)
    # 记录每个铁锤兵待过的行；换层意味着行集合不止一个值。
    rows_seen = {}
    swapped = False
    for f in frames:
        got = sorted(round(b["y"] / TILE_SIZE) for b in of_type(f, "hammer_bro"))
        for r in got:
            rows_seen[r] = rows_seen.get(r, 0) + 1
        if got == [8, 8] or got == [12, 12]:
            swapped = True
    check(
        "八秒里出现过多于两种落脚高度（说明有人换了层）",
        len(rows_seen) > 2,
        f"待过的行: {sorted(rows_seen)}",
    )
    check("并且出现过两人同层（一上一下换过来了）", swapped)
    # 中途一定有过腾空（vy != 0），而且是先向上
    ups = [b for f in frames for b in of_type(f, "hammer_bro") if b["vy"] < -1.0]
    check("换层是先往上蹬一下（连往下那次也是）", bool(ups), f"{len(ups)} 次上升采样")

    section("7. 踩死他给 1000 分，锤子踩不到")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 6 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await step(ws, 20)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await rpc(ws, "game.setScore", {"score": 0})
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "hammer_bro", "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False},
    )
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE - 72, "vy": 300.0},
    )
    bounced = False
    for _ in range(12):
        await step(ws, 1)
        s = await snap(ws)
        if s["player"]["vy"] < -1.0:
            bounced = True
            break
    check("从上面落下弹起来了（可以踩）", bounced)
    if bounced:
        s = await snap(ws)
        dead = [b for b in of_type(s, "hammer_bro") if b["state"] == "dead"]
        check("他倒了", bool(dead))

    # 锤子不可踩：同样的下落，结果应该是掉一级而不是弹起。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 6 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await step(ws, 20)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "hammer", "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False},
    )
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE - 72, "vy": 300.0},
    )
    hurt = False
    bounced = False
    for _ in range(12):
        await step(ws, 1)
        s = await snap(ws)
        if s["player"]["vy"] < -1.0:
            bounced = True
            break
        if not s["player"]["is_big"]:
            hurt = True
            break
    check("踩锤子不弹、反而掉一级", hurt and not bounced, f"bounce={bounced} hurt={hurt}")

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
