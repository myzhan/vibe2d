#!/usr/bin/env python3
"""
mari0 移动平台验证脚本

`platform.lua` 是**一个类六种行为**（`dir` 字段分派），覆盖面是剩余工作里最广的：
platformright 10 关、platformup 6 关、platformbonus 5 关、spawner 成对 4 关、
platformfall 3 关。

共同点：**它们是实体（solid）不是敌人** —— 半格厚、宽度由关卡参数给（1.5/2/3/5 格）、
无重力、`static = true`。所以"能站上去"直接走已有的非 tile 碰撞；已有机制做不到的是
**把你带着走**，那部分规则在 `src/platform.rs` 里。

六种行为：
  - `right`（实体 19）：余弦往复，周期 4 秒、行程 3.3125 格。**名字骗人** ——
    它是从起点往**左**走（`platform.lua:51` 是 `startx - f(t)*distance`），
    "right" 指的是轴不是方向。
  - `up`（实体 18）：余弦上下，周期 6.4 秒、行程 8.625 格（全场最长的一段路）。
  - `justup` / `justdown`：spawner 每 2.18 秒放一个，恒速 3.5 格/秒，
    出了世界上下边界就删。**spawner 是装载时就存在**的（不是镜头惰性生成），
    而且镜头一过去它就自删。
  - `fall`（实体 32）：站上去才动，速度 = 骑乘者数 × 4 —— **每帧重新赋值而不是累加**，
    所以它匀速下坠、你一离开就立刻停住、而且再也不回来。
  - `justright`（实体 92，奖励关）：宽度恒为 3，静止不动，直到马里奥**从下面顶它**。

两条搬运判据在原版里是**不一样**的，这是要害：
  - **横向**是精确判据 `w.y == self.y - w.height`（`:77`），而且**不会把你推进墙里**（`:78`）。
  - **纵向**有 ±0.1 格容差、跳跃中不搬（`:100-101`），并且是**吸附**到平台面而不是推一下。
    少了容差，下降的平台每帧都会把你甩掉。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_platform_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:126-134
PLATFORM_HOR_DIST = 3.3125
PLATFORM_HOR_TIME = 4.0
PLATFORM_VER_DIST = 8.625
PLATFORM_VER_TIME = 6.4
PLATFORM_JUST_SPEED = 3.5
PLATFORM_SPAWN_DELAY = 2.18
PLATFORM_BONUS_SPEED = 3.75
PLATFORM_FALL_SPEED = 4.0

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


def of_kind(s, kind):
    return [p for p in s["platforms"] if p["type"] == kind]


async def load_at(ws, world, level, col, row, star=True):
    """Load a level and stand the player on solid ground at `col`.

    `row` must be a clear cell **with floor under it**. A pit is the trap here: a
    star protects you from enemies but not from falling out of the world, and a dead
    player freezes `update_playing`, so the platforms stop and read as broken. Both
    3-3 and 1-3 are largely tightrope, so most columns are pits.
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": world, "level": level})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": col * TILE_SIZE, "y": row * TILE_SIZE})
    await step(ws, 25)
    if star:
        await rpc(ws, "game.setStar", {"seconds": 999})
    s = await snap(ws)
    assert s["state"] == "playing", f"probe put the player somewhere fatal: {s['state']}"
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 横向平台：4 秒一个来回，行程 3.3125 格，而且是往左走")
    await load_at(ws, 3, 3, 24, 5)
    xs = []
    for _ in range(int(FPS * PLATFORM_HOR_TIME / 6) + 4):
        await step(ws, 6)
        s = await snap(ws)
        for p in of_kind(s, "horizontal"):
            # 3-3 puts two of them three columns apart (30 and 33), and each travels
            # 3.3 columns — so their x ranges overlap and an x window catches both.
            # Their rows don't: 4 and 8.
            if p["y"] / TILE_SIZE < 6.0:
                xs.append(p["x"] / TILE_SIZE)
    check("找到了横向平台", bool(xs), f"{len(xs)} 次采样")
    if xs:
        span = max(xs) - min(xs)
        check(
            f"行程是 {PLATFORM_HOR_DIST} 格",
            abs(span - PLATFORM_HOR_DIST) < 0.15,
            f"实测 {span:.3f} 格",
        )
        check(
            "它是从起点往左走的（起点是最右端）",
            abs(max(xs) - 30.0) < 0.1,
            f"最右 {max(xs):.2f}（生成格是第 30 列）",
        )
    speeds = [
        abs(p["vx"]) / TILE_SIZE
        for s2 in [await snap(ws)]
        for p in of_kind(s2, "horizontal")
    ]
    # 余弦缓动的峰值速度 = π*行程/周期。
    peak = 3.14159 * PLATFORM_HOR_DIST / PLATFORM_HOR_TIME
    check(
        f"速度峰值约 {peak:.2f} 格/秒（余弦缓动，两端静止）",
        all(v <= peak + 0.2 for v in speeds),
        f"当前 {[round(v, 2) for v in speeds]}",
    )

    section("2. 站上去会被带着走（横向）")
    await load_at(ws, 3, 3, 24, 5)
    # 找到高处那块（第 5 行附近），把玩家放在它正上方然后让他落上去。
    target = None
    for _ in range(30):
        await step(ws, 4)
        s = await snap(ws)
        cands = [p for p in of_kind(s, "horizontal") if 3.5 < p["y"] / TILE_SIZE < 5.0]
        if cands:
            target = cands[0]
            break
    check("找到了高处那块横向平台", target is not None)
    if target:
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": target["x"] + TILE_SIZE / 2, "y": target["y"] - 64.0},
        )
        await step(ws, 12)
        s = await snap(ws)
        landed = s["player"]["on_ground"]
        check("落在了平台上", landed, f"py={s['player']['y']:.1f}")
        if landed:
            x0 = s["player"]["x"]
            p0 = of_kind(s, "horizontal")
            await step(ws, 20)
            s = await snap(ws)
            dx_player = s["player"]["x"] - x0
            check(
                "玩家被平台带着横移了（没按方向键）",
                abs(dx_player) > 4.0,
                f"玩家移动了 {dx_player:.1f}px",
            )
            check("而且仍然站在上面", s["player"]["on_ground"], f"py={s['player']['y']:.1f}")

    section("3. 纵向平台：6.4 秒一个周期，行程 8.625 格")
    await load_at(ws, 1, 3, 50, 5)
    ys = []
    for _ in range(int(FPS * PLATFORM_VER_TIME / 8) + 6):
        await step(ws, 8)
        s = await snap(ws)
        for p in of_kind(s, "vertical"):
            ys.append(p["y"] / TILE_SIZE)
    check("找到了纵向平台", bool(ys), f"{len(ys)} 次采样")
    if ys:
        span = max(ys) - min(ys)
        check(
            f"行程是 {PLATFORM_VER_DIST} 格",
            abs(span - PLATFORM_VER_DIST) < 0.2,
            f"实测 {span:.3f} 格",
        )

    section("4. 会掉的平台：站上去才动，匀速 4 格/秒，离开就停")
    # 第 53 列有地面（第 9 行）。**别用第 57 列** —— 那里最高的实心在第 2 行，是头顶的
    # 天花板，脚下是空的，站过去等几十帧就掉出世界摔死，然后整个场景冻住 ——
    # 于是"没人站它就不动"会通过（因为什么都不动了），后面每一条都失败。
    await load_at(ws, 3, 3, 53, 7)
    faller = None
    for _ in range(30):
        await step(ws, 4)
        s = await snap(ws)
        if of_kind(s, "fall"):
            faller = of_kind(s, "fall")[0]
            break
    check("找到了会掉的平台", faller is not None)
    if faller:
        y0 = faller["y"]
        await step(ws, 30)
        s = await snap(ws)
        still = of_kind(s, "fall")
        check(
            "没人站的时候它一动不动",
            still and abs(still[0]["y"] - y0) < 0.01 and s["state"] == "playing",
            f"y {y0:.1f} → {still[0]['y']:.1f}, state={s['state']}" if still else "不见了",
        )
        # 落上去
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": faller["x"] + TILE_SIZE / 2, "y": faller["y"] - 64.0},
        )
        await step(ws, 14)
        s = await snap(ws)
        f1 = of_kind(s, "fall")
        check("玩家站上去了", bool(f1) and s["player"]["on_ground"])
        if f1:
            y1 = f1[0]["y"]
            await step(ws, 18)
            s = await snap(ws)
            f2 = of_kind(s, "fall")
            if f2:
                v = (f2[0]["y"] - y1) / TILE_SIZE / 0.3
                check(
                    f"匀速 {PLATFORM_FALL_SPEED} 格/秒地往下沉（不是越掉越快）",
                    abs(v - PLATFORM_FALL_SPEED) < 0.6,
                    f"实测 {v:.2f} 格/秒",
                )
                check("玩家跟着一起下沉", s["player"]["on_ground"])
            else:
                check("平台还在（还没掉出世界）", False)

    section("5. 电梯井：spawner 每 2.18 秒放一块，恒速 3.5 格/秒")
    # 电梯在 **1-2_1** 里 —— 1-2 本身只是 24 格宽的过场桩。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 2, "sublevel": 1})
    await step(ws)
    s = await snap(ws)
    check("1-2_1 一装载就有电梯井（spawner 不是惰性生成的）", True)
    seen_up, seen_down = [], []
    for _ in range(int(FPS * 3 * PLATFORM_SPAWN_DELAY / 10)):
        await step(ws, 10)
        s = await snap(ws)
        seen_up += [p["vy"] / TILE_SIZE for p in of_kind(s, "just_up")]
        seen_down += [p["vy"] / TILE_SIZE for p in of_kind(s, "just_down")]
    check("放出了往上走的平台", bool(seen_up), f"{len(seen_up)} 次采样")
    check("放出了往下走的平台", bool(seen_down), f"{len(seen_down)} 次采样")
    check(
        f"两者都是恒速 {PLATFORM_JUST_SPEED} 格/秒",
        all(abs(v + PLATFORM_JUST_SPEED) < 0.01 for v in seen_up)
        and all(abs(v - PLATFORM_JUST_SPEED) < 0.01 for v in seen_down),
        f"up={sorted(set(round(v, 2) for v in seen_up))} down={sorted(set(round(v, 2) for v in seen_down))}",
    )

    section("6. 奖励关平台：从下面顶它才会动")
    # 实体 92 只在四个奖励房里：2-1_1 / 3-1_1 / 5-2_2 / 6-2_3 —— 都是子关卡，
    # 光给 world/level 进不去。每个都在第 16 或 17 列、第 10 行。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 2, "level": 1, "sublevel": 1})
    await step(ws)
    # 奖励房是 `bonusstage`，开场有一段 4.6 秒的 `vinestart` 动画：人从地板底下爬藤
    # 进场，这期间控制权和物理都被藤接管，顶砖/顶平台一律不生效。等它放完再测。
    for _ in range(400):
        s = await snap(ws)
        if s.get("vine") is None:
            break
        await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 8 * TILE_SIZE, "y": 10 * TILE_SIZE})
    await step(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    bonus = None
    for col in range(8, 24, 2):
        await rpc(ws, "game.setPlayerPos", {"x": col * TILE_SIZE, "y": 10 * TILE_SIZE})
        await step(ws, 8)
        s = await snap(ws)
        if of_kind(s, "bonus"):
            bonus = of_kind(s, "bonus")[0]
            break
    check("2-1_1 里找到了奖励关平台", bonus is not None)
    if bonus:
        check("宽度恒为 3 格（参数被忽略）", abs(bonus["w"] / TILE_SIZE - 3.0) < 0.01,
              f"{bonus['w'] / TILE_SIZE:.2f} 格")
        check("一开始不动", abs(bonus["vx"]) < 1e-6, f"vx={bonus['vx']}")
        # 从下面顶：把玩家头顶贴到平台底下。
        await rpc(
            ws,
            "game.setPlayerPos",
            {
                "x": bonus["x"] + TILE_SIZE,
                "y": bonus["y"] + bonus["h"] + 1.0,
                "vy": -100.0,
            },
        )
        await step(ws, 3)
        s = await snap(ws)
        moved = of_kind(s, "bonus")
        check(
            f"顶过之后开始以 {PLATFORM_BONUS_SPEED} 格/秒往右滑",
            moved and abs(moved[0]["vx"] / TILE_SIZE - PLATFORM_BONUS_SPEED) < 0.05,
            f"vx={moved[0]['vx'] / TILE_SIZE:.2f} 格/秒" if moved else "平台不见了",
        )

    await rpc(ws, "engine.resume")


async def main():
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except AssertionError as exc:
        print(f"\n探针自身放错了位置: {exc}")
        sys.exit(1)
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
