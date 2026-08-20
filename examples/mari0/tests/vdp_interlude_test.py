#!/usr/bin/env python3
"""
mari0 过场验证脚本：死亡动画 + 四种黑屏卡片

`levelscreen.lua` 一个函数管着游戏里所有的关卡间空隙。原版有 9 个 gamestate，
之前端口只有 4 个 —— 缺的正好是这一层「包装」：死了直接切文字屏、换关直接进游戏。

四种卡片只差两件事：持续多久、中间印什么。
  - `level_screen`   `levelscreentime = 2.4` 秒，印 "world 1-1" + 马里奥木偶 + 命数。
                     **世界的第一关拉长 1.5 倍**（`levelscreen.lua:60-62`），
                     所以 2-1 比 2-2 多一拍。
  - `sublevel`       `sublevelscreentime = 0.2` 秒，管道/藤蔓切子关用。
  - `game_over`      `gameovertime = 7` 秒，结束后回标题而不是回关卡。
  - `mappack_finished` 同样 7 秒，而且是**全游戏唯一**放 princessmusic 的地方。

两个细节是「像原版」而不是「像延迟」的关键：
  - 文字只在黑屏的中段画：头尾各留 `blacktimesub = 0.1` 秒纯黑（`levelscreen.lua:106`），
    所以每张卡都是从黑里淡入再淡回黑里。
  - 而 0.2 秒的子关卡黑屏**整个时长正好是两个 blacktimesub**，所以它**永远画不出字**。
    这就是为什么钻管道是「闪一下」，而通关是「一张卡」—— 同一套机制，两种体感。

死亡（`mario.lua:591-612`）：
  - 定住 `deathanimationjumptime = 0.3` 秒 → 以 `deathanimationjumpforce = 17` 弹起
    → 按 `deathgravity = 40`（只有世界重力的一半，所以看得清）落出画面，共 4 秒。
  - **掉坑死不弹起**：他已经在往下掉了，再弹一下会把他从洞里弹回来。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_interlude_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
DT = 1.0 / 60.0

# variables.lua:166-169, :316-319
LEVELSCREEN_TIME = 2.4
SUBLEVELSCREEN_TIME = 0.2
GAMEOVER_TIME = 7.0
BLACKTIME_SUB = 0.1
DEATH_TOTAL_TIME = 4.0
DEATH_JUMP_TIME = 0.3

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


async def si(ws, frames=1, inputs=None):
    params = {"frames": frames}
    if inputs:
        params["inputs"] = inputs
    return await rpc(ws, "engine.stepAndInspect", params)


def check(label, ok, detail=""):
    print(f"    {'OK  ' if ok else 'FAIL'} {label}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(label)


def section(title):
    print(f"\n─── {title} ───")


async def setup(ws, world, level, sublevel=0, lives=True):
    """装载关卡，默认**把命数恢复成 3**。

    这一整套测试要反复弄死玩家，而 `setLevel` 不碰命数 —— 上一小节把命耗光以后，
    下一小节每次死都会变成 game_over，把 level_screen / sublevel 的断言全带跑。
    `game.reset` 是唯一会把 lives 归 3 的入口。
    """
    if lives:
        await rpc(ws, "game.reset")
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    return await si(ws, 8)


async def drop_in_pit(ws):
    """把人丢到世界底下，触发掉坑死。"""
    await rpc(ws, "game.setPlayerPos", {"x": 10 * TILE_SIZE, "y": 700.0, "vx": 0.0, "vy": 300.0})
    return await si(ws, 2)


async def wait_for(ws, pred, limit=600, frames=2, inputs=None):
    """步进到条件满足，返回那一帧（或最后一帧）。"""
    s = None
    for _ in range(limit):
        s = await si(ws, frames, inputs)
        if pred(s):
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section(f"1. 死亡动画：{DEATH_TOTAL_TIME} 秒，定住 → 弹起 → 落出画面")
    await setup(ws, 1, 1)
    s = await si(ws)
    lives0 = s["lives"]
    # 用敌人杀（不是掉坑），这样才会有弹起
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "goomba", "x": s["player"]["x"] + 40.0, "y": s["player"]["y"]},
    )
    track = []
    s = await wait_for(
        ws,
        lambda s: s["state"] == "interlude",
        limit=400,
        frames=2,
    )
    check("死了以后进了黑屏卡片（不再是等按键的文字屏）", s["state"] == "interlude",
          f"state={s['state']}")
    check("扣了一条命", s["lives"] == lives0 - 1, f"{lives0} → {s['lives']}")

    # 重来一次，这次记录轨迹
    await setup(ws, 1, 1)
    s = await si(ws)
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "goomba", "x": s["player"]["x"] + 40.0, "y": s["player"]["y"]},
    )
    for _ in range(400):
        s = await si(ws, 2)
        if s["death_timer"] is not None:
            track.append((s["death_timer"], s["player"]["y"]))
        if s["state"] == "interlude":
            break
    check("拿到了死亡轨迹", len(track) > 10, f"{len(track)} 个采样点")
    if len(track) > 10:
        # 定住阶段：0.3 秒内 y 不动
        still = [y for t, y in track if t <= DEATH_JUMP_TIME]
        check(
            f"前 {DEATH_JUMP_TIME} 秒定住不动",
            len(still) > 1 and max(still) - min(still) < 1.0,
            f"y 变化 {max(still) - min(still):.3f}px",
        )
        top = min(y for _, y in track)
        start = track[0][1]
        check("然后弹起（有一段比出发点更高）", top < start - 10.0,
              f"出发 {start / TILE_SIZE:.2f} 格 → 最高 {top / TILE_SIZE:.2f} 格")
        check("再落出画面", track[-1][1] > start + 100.0,
              f"最后 {track[-1][1] / TILE_SIZE:.1f} 格")
        check(
            f"整段约 {DEATH_TOTAL_TIME} 秒",
            abs(track[-1][0] - DEATH_TOTAL_TIME) < 0.2,
            f"实测 {track[-1][0]:.2f} 秒",
        )

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 掉坑死不弹起 —— 他已经在往下掉了")
    await setup(ws, 1, 1)
    await drop_in_pit(ws)
    pit = []
    for _ in range(400):
        s = await si(ws, 2)
        if s["death_timer"] is not None:
            pit.append(s["player"]["y"])
        if s["state"] == "interlude":
            break
    check("拿到了掉坑轨迹", len(pit) > 5, f"{len(pit)} 个采样点")
    if len(pit) > 5:
        check(
            "y 全程只增不减（没有弹起那一下）",
            all(b >= a - 0.01 for a, b in zip(pit, pit[1:])),
            f"{pit[0] / TILE_SIZE:.1f} → {pit[-1] / TILE_SIZE:.1f} 格",
        )

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. level_screen：{LEVELSCREEN_TIME} 秒，世界第一关拉长 1.5 倍")
    # 2-1（level == 1）应为 3.6
    await setup(ws, 2, 1)
    await drop_in_pit(ws)
    s = await wait_for(ws, lambda s: s["state"] == "interlude", limit=400)
    check("2-1 复活的卡片是 level_screen", s["interlude"]["kind"] == "level_screen",
          str(s["interlude"] and s["interlude"]["kind"]))
    check(
        f"2-1 拉长到 {LEVELSCREEN_TIME * 1.5} 秒",
        abs(s["interlude"]["total"] - LEVELSCREEN_TIME * 1.5) < 0.01,
        f"{s['interlude']['total']:.2f} 秒",
    )
    # 2-2（level != 1，world != 1）应为 2.4
    await setup(ws, 2, 2)
    await drop_in_pit(ws)
    s = await wait_for(ws, lambda s: s["state"] == "interlude", limit=400)
    check(
        f"2-2 是标准的 {LEVELSCREEN_TIME} 秒",
        abs(s["interlude"]["total"] - LEVELSCREEN_TIME) < 0.01,
        f"{s['interlude']['total']:.2f} 秒",
    )
    # 淡入淡出：文字不是全程可见
    seen = set()
    s = await wait_for(
        ws,
        lambda s: s["state"] == "playing",
        limit=400,
        frames=1,
    )
    check("卡片结束后回到关卡", s["state"] == "playing", s["state"])

    await setup(ws, 3, 2)
    await drop_in_pit(ws)
    await wait_for(ws, lambda s: s["state"] == "interlude", limit=400)
    for _ in range(400):
        s = await si(ws, 1)
        if s["interlude"]:
            seen.add(s["interlude"]["text_visible"])
        if s["state"] == "playing":
            break
    check(
        "文字是淡入淡出的（头尾纯黑，中段才印字）",
        seen == {True, False},
        f"看到 text_visible={sorted(seen)}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section(f"4. sublevel 闪屏：{SUBLEVELSCREEN_TIME} 秒，而且**永远印不出字**")
    await setup(ws, 1, 1)
    # 1-1 的管道实体在第 58 列
    await rpc(ws, "game.setPlayerPos", {"x": 58 * TILE_SIZE, "y": 8 * TILE_SIZE, "vx": 0.0, "vy": 0.0})
    await rpc(ws, "game.clearEnemies")
    kinds, visible, frames = set(), set(), 0
    down = [{"device": "keyboard", "action": "press", "key": "Down"}]
    for _ in range(400):
        s = await si(ws, 1, down)
        if s["interlude"]:
            kinds.add(s["interlude"]["kind"])
            visible.add(s["interlude"]["text_visible"])
            frames += 1
        if s["level"]["sublevel"] == 1 and s["state"] == "playing":
            break
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": "Down"}])
    check("钻管道触发的是 sublevel 闪屏", kinds == {"sublevel"}, f"{kinds}")
    check(
        f"持续约 {SUBLEVELSCREEN_TIME} 秒（{int(SUBLEVELSCREEN_TIME * 60)} 帧）",
        abs(frames - SUBLEVELSCREEN_TIME * 60) <= 2,
        f"{frames} 帧",
    )
    check(
        "全程一个字都没印 —— 时长正好是两个 blacktimesub",
        visible == {False},
        f"text_visible={sorted(visible)}；{SUBLEVELSCREEN_TIME} == 2 × {BLACKTIME_SUB}",
    )
    check("到了 1-1_1", s["level"]["name"] == "1-1_1", s["level"]["name"])

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. game_over：{GAMEOVER_TIME} 秒，然后回标题")
    await setup(ws, 1, 1)  # 从 3 条命开始，下面一条条耗掉
    # 把命耗到 0
    for _ in range(6):
        s = await si(ws)
        if s["lives"] == 0:
            break
        await drop_in_pit(ws)
        s = await wait_for(ws, lambda s: s["state"] in ("playing", "interlude"), limit=400)
        if s["state"] == "interlude" and s["interlude"]["kind"] == "game_over":
            break
        await wait_for(ws, lambda s: s["state"] == "playing", limit=400)
    s = await wait_for(
        ws,
        lambda s: s["state"] == "interlude" and s["interlude"]["kind"] == "game_over",
        limit=400,
    )
    check("命耗尽后是 game_over 卡片",
          s["interlude"] is not None and s["interlude"]["kind"] == "game_over",
          str(s["interlude"] and s["interlude"]["kind"]))
    if s["interlude"]:
        check(f"持续 {GAMEOVER_TIME} 秒",
              abs(s["interlude"]["total"] - GAMEOVER_TIME) < 0.01,
              f"{s['interlude']['total']:.1f} 秒")
    s = await wait_for(ws, lambda s: s["state"] == "menu", limit=400, frames=4)
    check("结束后回标题（不是回关卡）", s["state"] == "menu", s["state"])

    # ── 6 ───────────────────────────────────────────────────────────
    section(f"6. mappack_finished：打完 8-4 之后，同样 {GAMEOVER_TIME} 秒再回标题")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 8, "level": 4})
    await rpc(ws, "game.setState", {"state": "playing"})
    await si(ws, 5)
    await rpc(ws, "game.nextLevel")
    s = await si(ws)
    check(
        "8-4 之后是 mappack_finished（不是直接跳回菜单）",
        s["state"] == "interlude" and s["interlude"]["kind"] == "mappack_finished",
        f"state={s['state']} kind={s['interlude'] and s['interlude']['kind']}",
    )
    if s["interlude"]:
        check(f"持续 {GAMEOVER_TIME} 秒",
              abs(s["interlude"]["total"] - GAMEOVER_TIME) < 0.01,
              f"{s['interlude']['total']:.1f} 秒")
    s = await wait_for(ws, lambda s: s["state"] == "menu", limit=400, frames=4)
    check("结束后回标题", s["state"] == "menu", s["state"])

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
