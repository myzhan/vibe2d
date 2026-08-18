#!/usr/bin/env python3
"""
mari0 乌贼 + 飞鱼验证脚本

**乌贼**（squid，实体 94，在 2-2_1 / 5-2_1 / 6-2_2 / 7-2_1 / 8-4_1 / M-1 六个水下关）：
三拍一循环（`squid.lua:76-132`），而且它**不追你、它拦你**：
  1. `idle`：以 0.9 格/秒**往下飘**，等你和它齐平；
  2. `lunge`：以 10 格/秒² 同时向上和向侧加速，两个方向都封顶 3 格/秒 ——
     结束条件是**横向走了 2 格**（不是时间也不是高度）；
  3. `sink`：以 0.9 格/秒沉 1 格，然后回到 idle。
判定"齐平"用的是**大马里奥的头**在哪（`:80` 拿 24/16 减他的实际身高），
所以小马里奥会被从更高处扑。**乌贼踩不死**（`mario.lua:1778` 把 squid 列在 KILL 里）。
转向那一步原版包在 `if true then` 里、`math.random(2)` 被注释掉了（`:87`），
所以它**每次都转**向玩家。

**飞鱼**（flyingfish，标记 95/96，在 2-3 / 7-3 / 8-4_3）：
和 Bullet Bill 的区间生成器同一个形状（玩家 x 的双向闩 + while 循环 + 每轮新随机延迟），
但鱼是**从下面来**的：在**屏幕可见范围内**随机一列、从世界底下起跳（`flyingfish.lua:5`），
不是从画面外飞进来。横向速度是**玩家自己的速度**加上 -4..5 格/秒的随机扰动（`:11`），
所以**设计上就跑不掉**。重力 20（比世界的 80 轻），不吃地形，可以踩死。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_underwater_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:253-257, :267-268
SQUID_FALL_SPEED = 0.9
SQUID_X_SPEED = 3.0
SQUID_UP_SPEED = 3.0
SQUID_LUNGE_DIST = 2.0
SQUID_DOWN_DIST = 1.0
FLYING_FISH_FORCE = 23.0
FLYING_FISH_GRAVITY = 20.0
FLYING_FISH_MIN = 0.6
FLYING_FISH_MAX = 2.0

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


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 乌贼的三拍循环：飘下来 → 斜着扑 2 格 → 沉 1 格 → 再飘")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 2, "level": 2, "sublevel": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 10 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await step(ws, 20)
    await rpc(ws, "game.setStar", {"seconds": 999})
    s = await snap(ws)
    check("2-2_1 里有乌贼", bool(of_type(s, "squid")), f"{len(of_type(s, 'squid'))} 只")

    frames = []
    for _ in range(70):
        await step(ws, 6)
        frames.append(await snap(ws))
    phases = [e["squid_phase"] for f in frames for e in of_type(f, "squid")]
    check(
        "三个阶段都出现过",
        set(phases) >= {"idle", "lunge", "sink"},
        f"观测到 {sorted(set(phases))}",
    )

    # 每个阶段的速度都是定值，直接核对。
    idle_vy = {
        round(e["vy"] / TILE_SIZE, 2)
        for f in frames
        for e in of_type(f, "squid")
        if e["squid_phase"] in ("idle", "sink")
    }
    check(
        f"idle / sink 都是 {SQUID_FALL_SPEED} 格/秒往下飘",
        idle_vy and all(abs(v - SQUID_FALL_SPEED) < 0.05 for v in idle_vy),
        f"{sorted(idle_vy)}",
    )
    lunge = [e for f in frames for e in of_type(f, "squid") if e["squid_phase"] == "lunge"]
    check("扑的时候同时往上和往侧走", bool(lunge) and any(e["vy"] < 0 for e in lunge))
    if lunge:
        check(
            f"上升封顶 {SQUID_UP_SPEED} 格/秒",
            all(e["vy"] / TILE_SIZE >= -SQUID_UP_SPEED - 0.05 for e in lunge),
            f"最快 {min(e['vy'] / TILE_SIZE for e in lunge):.2f}",
        )
        check(
            f"横向封顶 {SQUID_X_SPEED} 格/秒",
            all(abs(e["vx"]) / TILE_SIZE <= SQUID_X_SPEED + 0.05 for e in lunge),
            f"最快 {max(abs(e['vx']) / TILE_SIZE for e in lunge):.2f}",
        )

    section("2. 扑的结束条件是横向走了 2 格，不是时间也不是高度")
    # 逐帧跟一次完整的 lunge：记下进入和离开时的 x。
    spans = []
    run_x = None
    seen_non_lunge = False
    for f in frames:
        sq = of_type(f, "squid")
        if not sq:
            continue
        e = sq[0]
        if e["squid_phase"] == "lunge":
            # Only start timing a lunge we saw begin. Sampling can open mid-lunge,
            # which records a short span and looks like the rule is wrong.
            if run_x is None and seen_non_lunge:
                run_x = e["x"]
        else:
            seen_non_lunge = True
            if run_x is not None:
                spans.append(abs(e["x"] - run_x) / TILE_SIZE)
                run_x = None
    check("跟到了完整的扑击", bool(spans), f"{len(spans)} 次")
    if spans:
        check(
            f"每次横向都是约 {SQUID_LUNGE_DIST} 格",
            all(abs(v - SQUID_LUNGE_DIST) < 0.35 for v in spans),
            f"{[round(v, 2) for v in spans]}",
        )

    section("3. 乌贼踩不死：从上面落下去会掉一级而不是弹起")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 6 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await step(ws, 20)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await rpc(ws, "game.setStar", {"seconds": 0})
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "squid", "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False},
    )
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE - 72, "vy": 300.0},
    )
    bounced = hurt = False
    for _ in range(12):
        await step(ws, 1)
        s = await snap(ws)
        if s["player"]["vy"] < -1.0:
            bounced = True
            break
        if not s["player"]["is_big"]:
            hurt = True
            break
    check("踩乌贼不弹、反而掉一级", hurt and not bounced, f"bounce={bounced} hurt={hurt}")

    section("4. 飞鱼区间：从屏幕内某一列、世界底下往上蹿")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 2, "level": 3})
    await step(ws, 3)
    s = await snap(ws)
    # 2-3 的 start 在第 8 列、end 在第 205 列 —— 开局（第 3 列）还在区间外。
    check("开局在区间外", s["flying_fish_zone"] is False)
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 25)
    s = await snap(ws)
    check("走过第 8 列之后区间打开", s["flying_fish_zone"] is True)
    await rpc(ws, "game.setStar", {"seconds": 999})
    seen = []
    for _ in range(60):
        await step(ws, 10)
        s = await snap(ws)
        cam = s["camera_x"]
        for e in of_type(s, "flying_fish"):
            # Fresh means near the bottom **and still rising**. Near-the-bottom alone
            # isn't enough: the arc is over two seconds long, so a fish re-enters that
            # band on the way *down* having drifted most of a screen sideways.
            if (
                e["state"] == "walking"
                and e["vy"] < 0
                and e["y"] > (s["level"]["height"] - 1.5) * TILE_SIZE
            ):
                seen.append((e["x"] - cam, e["y"], e["vy"]))
    check("持续有鱼蹿出来", len(seen) >= 5, f"累计 {len(seen)} 次观测")
    if seen:
        # 生成点是 `cam + [0..16] 格`，也就是 0..512。采样不是瞬时的：最快的鱼
        # 11 格/秒，十帧就能漂 59px，镜头本身也在动 —— 所以留两格容差。要钉的是
        # "在画面里出生"这件事本身（和 Bullet Bill 从画面外飞进来正相反）。
        check(
            "出生点在屏幕范围内（不是像子弹那样从画面外进来）",
            all(-2 * TILE_SIZE <= sx <= 512 + 2 * TILE_SIZE for sx, _, _ in seen),
            f"屏幕内 x 范围 {min(s for s, _, _ in seen):.0f}..{max(s for s, _, _ in seen):.0f}",
        )
        check("有鱼在上升段（是往上蹿的）", any(vy < 0 for _, _, vy in seen))
    # 重力得在一条**确定**的鱼上量。区间每秒都在放新鱼，快照里 `[0]` 不是同一条，
    # 前后两帧相减会得到毫无意义的数（第一次这么写量出了 -460）。所以换个干净的关卡
    # 自己放一条。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 20)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "flying_fish", "x": 24 * TILE_SIZE, "y": 6 * TILE_SIZE, "facing_right": True},
    )
    await step(ws, 2)
    live = [e for e in of_type(await snap(ws), "flying_fish") if e["state"] == "walking"]
    check("放出了一条可跟踪的鱼", bool(live))
    if live:
        vy0 = live[0]["vy"]
        await step(ws, 6)
        live = [e for e in of_type(await snap(ws), "flying_fish") if e["state"] == "walking"]
        if live:
            g = (live[0]["vy"] - vy0) / TILE_SIZE / (6 / FPS)
            check(
                f"重力是 {FLYING_FISH_GRAVITY} 格/秒²（比世界的 80 轻）",
                abs(g - FLYING_FISH_GRAVITY) < 2.0,
                f"实测 {g:.1f}",
            )

    section("5. 飞鱼的横向扰动范围是 -4..5 格/秒，而且从不为 0")
    # 给玩家一个明确的速度，新出的鱼的 vx 应该围绕它分布。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 2, "level": 3})
    await step(ws, 3)
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    await rpc(ws, "game.clearEnemies")
    # 玩家站着不动（速度 0），此时鱼的横向速度**就是**那个随机扰动，可以直接核对范围。
    # 想反过来验"跟着玩家的速度"是量不到的：从外面戳一个速度进去之后，摩擦力在鱼真正
    # 生成的那一帧之前就已经把它磨掉一部分了。
    fresh = []
    for _ in range(50):
        await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 10 * TILE_SIZE})
        await step(ws, 8)
        s = await snap(ws)
        for e in of_type(s, "flying_fish"):
            if e["state"] == "walking" and e["y"] > (s["level"]["height"] - 1.5) * TILE_SIZE:
                fresh.append(e["vx"] / TILE_SIZE)
    check("采到了刚出生的鱼", bool(fresh), f"{len(fresh)} 条")
    if fresh:
        check(
            "扰动落在 -4..5 格/秒",
            all(-4.05 <= v <= 5.05 for v in fresh),
            f"范围 {min(fresh):.1f}..{max(fresh):.1f}",
        )
        check(
            "从不为 0（原版把 0 顶成 1，否则站着不动时鱼会垂直上下）",
            all(abs(v) > 1e-6 for v in fresh),
        )
        check("两个方向都出现过", any(v > 0 for v in fresh) and any(v < 0 for v in fresh))

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
