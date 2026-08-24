#!/usr/bin/env python3
"""
mari0 变身动画、连踩顶格与星星验证脚本

三件之前没做的事：

  1. **变身会冻结整个世界**。原版不是「播个动画」，而是设 `noupdate` 之后从 update
     顶部返回，只调玩家自己的动画（`game.lua:229-234`）—— 敌人、时钟、物理、金币
     旋转全停 0.9 秒。体型和碰撞盒在动画**之前**就已经改完了
     （`mario.lua:1641-1644`），所以这 0.9 秒纯粹是戏。
     三个翻帧循环「小 → 变身格 → 大」（`mario.lua:740-760`）；吃花那一档
     （`grow2`）不换精灵，改闪**星星配色**。缩小结束的那一刻才开始无敌。
  2. **连踩阶梯顶格给命**。守卫是严格 `combo < #mariocombo`
     （`mario.lua:1851`），所以 `mariocombo[10] = 8000` **靠踩敌人永远拿不到**：
     前九次 100→5000，第十次起给命。踩子弹**不推进**连击（`:1853`）。
  3. **星星有退场提示**。最后 `mariostarrunout` = 1 秒里，闪烁从 0.08 放慢到
     0.16，并且**把关卡音乐提前一秒交还**（`mario.lua:256-284`）。那一秒你仍然
     无敌 —— 音乐就是唯一的警告。

顺带钉住一个顺手修掉的 bug：星星撞死敌人给的是 `firepoints` 固定费率、**不碰**
连击链（`mario:starcollide`，`mario.lua:2240-2247`），而不是走连踩阶梯。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_powerup_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0
DT = 1.0 / 60.0

# variables.lua:24, :91-94, :307-309
COMBO = [100, 200, 400, 500, 800, 1000, 2000, 4000, 5000, 8000]
GROW_TIME = 0.9
GROW_FRAME_DELAY = 0.08
STAR_DURATION = 12.0
STAR_RUNOUT = 1.0
STAR_BLINK = 0.08
STAR_BLINK_SLOW = 0.16
INVINCIBLE_TIME = 3.2
GOOMBA_FIRE_POINTS = 100

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


def near(a, b, tol):
    return abs(a - b) <= tol


def section(title):
    print(f"\n─── {title} ───")


FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Left", "Right", "Space", "F", "Down")
]


async def stand(ws, size="small", col=6):
    """站在 1-1 平地上。y 用 10 格 —— 11 格会撞进管道实体，当场就死。"""
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setScore", {"score": 0})
    await rpc(ws, "game.setStar", {"seconds": 0.0})
    await rpc(ws, "game.setPlayerSize", {"size": size})
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 4)
        if s["player"]["on_ground"]:
            break
    return s


async def take_a_hit(ws):
    """放一只 goomba 从侧面撞上来，返回变身刚开始那一帧。"""
    p = (await si(ws))["player"]
    await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": p["x"] + 1.2 * T, "y": p["y"]})
    for _ in range(90):
        s = await si(ws, 1)
        if s.get("transform") or s["state"] != "playing":
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section(f"1. 缩小：冻结 {GROW_TIME}s，世界完全静止")
    await stand(ws, "big")
    s = await take_a_hit(ws)
    t = s.get("transform")
    check(
        "受伤立刻进入 shrink 变身",
        t is not None and t["kind"] == "shrink",
        f"transform={t}",
    )
    check(
        "体型已经变小了 —— 动画之前就改完，不是动画结束才改",
        not s["player"]["is_big"],
        f"big={s['player']['is_big']}",
    )
    check(
        "此刻还没有无敌 —— 它要等冻结结束",
        s["player"]["invincible_timer"] == 0.0,
        f"invincible={s['player']['invincible_timer']}",
    )
    # 逐帧走完冻结，同时盯住时钟和敌人
    clock0 = s["time_remaining"]
    enemies0 = len(s["enemies"])
    frames_seen, n = set(), 0
    while s.get("transform") and n < 120:
        s = await si(ws, 1)
        n += 1
        if s.get("transform"):
            frames_seen.add(s["transform"]["frame"])
    check(
        f"冻结约 {GROW_TIME}s",
        near(n * DT, GROW_TIME, 0.05),
        f"{n} 帧 = {n * DT:.3f}s",
    )
    check(
        "冻结期间时钟一动不动（原版 noupdate 连时钟一起停）",
        s["time_remaining"] == clock0,
        f"{clock0} → {s['time_remaining']}",
    )
    check(
        "冻结期间敌人数量没变",
        len(s["enemies"]) == enemies0,
        f"{enemies0} → {len(s['enemies'])}",
    )
    check(
        "三个翻帧都出现过（小 / 变身格 / 大）",
        frames_seen == {1, 2, 3},
        f"见到 {sorted(frames_seen)}",
    )
    check(
        f"冻结结束才开始无敌，{INVINCIBLE_TIME}s",
        near(s["player"]["invincible_timer"], INVINCIBLE_TIME, 0.05),
        f"{s['player']['invincible_timer']:.3f}s",
    )

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 变大与吃花是两种不同的变身")
    blocks = [
        b
        for b in (await si(ws)).get("block_contents", [])
        if b.get("content") == "mushroom" and b.get("col", 0) < 40
    ]
    if not blocks:
        check("找到 1-1 的蘑菇砖", False, "block_contents 里没有")
    else:
        col, row = blocks[0]["col"], blocks[0]["row"]
        for size, want_item, want_kind in (
            ("small", "mushroom", "grow"),
            ("big", "fire_flower", "fire"),
        ):
            await stand(ws, size)
            await rpc(
                ws,
                "game.setPlayerPos",
                {"x": col * T, "y": (row + 1) * T, "vx": 0.0, "vy": 0.0},
            )
            for _ in range(20):
                s = await si(ws, 4)
                if s["player"]["on_ground"]:
                    break
            # 顶砖之前把体型和场面都重置一遍：砖里出什么**取决于顶的那一刻**的体型，
            # 而 1-1 会一路惰性生成 goomba —— 被撞一下变小，砖就改吐蘑菇了。
            # 本脚本第一版就是这么把 big → fire 测成 grow 的。
            await rpc(ws, "game.clearEnemies")
            await rpc(ws, "game.setPlayerSize", {"size": size})
            s = await si(ws, 2)
            check(
                f"顶砖前确实是 {size}",
                s["player"]["is_big"] == (size == "big"),
                f"big={s['player']['is_big']}",
            )
            item = None
            for _ in range(12):
                await si(ws, 1, [{"device": "keyboard", "action": "release", "key": "Space"}])
                s = await si(ws, 16, [{"device": "keyboard", "action": "press", "key": "Space"}])
                if s.get("items"):
                    item = s["items"][0]["type"]
                    break
            check(
                f"{size} 顶出来的是 {want_item}",
                item == want_item,
                f"实际 {item}",
            )
            # 直接按道具坐标对位，而不是走过去：**火花是不动的**，长在砖块顶上，
            # 大马里奥光在下面来回走一辈子也碰不到，得跳上去。逐帧对位省掉这个变数。
            await rpc(ws, "game.clearEnemies")
            got = None
            for _ in range(120):
                s = await si(ws, 1)
                if s.get("transform"):
                    got = s["transform"]["kind"]
                    break
                if not s.get("items"):
                    break
                it = s["items"][0]
                await rpc(
                    ws,
                    "game.setPlayerPos",
                    {"x": it["x"], "y": it["y"], "vx": 0.0, "vy": 0.0},
                )
            check(
                f"{size} 吃下去 → {want_kind} 变身",
                got == want_kind,
                f"kind={got}",
            )
            await si(ws, 1, FREE_ALL)

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. 连踩阶梯：前九次 {COMBO[0]}…{COMBO[8]}，第十次起给命")
    await stand(ws)
    gains = []
    for _ in range(11):
        await rpc(ws, "game.setPlayerPos", {"x": 8 * T, "y": 9 * T, "vx": 0.0, "vy": 0.0})
        await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": 8 * T, "y": 11 * T})
        await si(ws, 2)
        s0 = await si(ws)
        before, lives_before = s0["score"], s0["lives"]
        await rpc(ws, "game.setPlayerPos", {"x": 8 * T, "y": 9 * T, "vx": 0.0, "vy": 250.0})
        for _ in range(40):
            s = await si(ws, 1)
            if s["score"] != before or s["lives"] != lives_before:
                break
        gains.append((s["score"] - before, s["lives"] - lives_before))
    scores = [g for g, _ in gains]
    check(
        f"前九次正好是 {COMBO[:9]}",
        scores[:9] == COMBO[:9],
        f"实测 {scores[:9]}",
    )
    check(
        f"第十次起给命、不给分 —— {COMBO[9]} 这一级踩敌人永远拿不到",
        all(g == 0 and l == 1 for g, l in gains[9:]),
        f"第 10、11 次 = {gains[9:]}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 星星撞死敌人用固定费率，不碰连击链")
    await stand(ws)
    await rpc(ws, "game.setStar", {"seconds": 99.0})
    total, combo_after = 0, None
    for i in range(3):
        p = (await si(ws))["player"]
        await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": p["x"] + 1.2 * T, "y": p["y"]})
        before = (await si(ws))["score"]
        for _ in range(60):
            s = await si(ws, 1)
            if s["score"] != before:
                break
        total += s["score"] - before
        combo_after = s["combo_index"]
    check(
        f"三只 goomba 各 {GOOMBA_FIRE_POINTS} 分，共 {3 * GOOMBA_FIRE_POINTS}（连击阶梯会是 700）",
        total == 3 * GOOMBA_FIRE_POINTS,
        f"共 {total} 分",
    )
    check(
        "连击链没被推进",
        combo_after == 0,
        f"combo_index={combo_after}",
    )
    await rpc(ws, "game.setStar", {"seconds": 0.0})

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. 星星退场：最后 {STAR_RUNOUT}s 闪烁从 {STAR_BLINK} 放慢到 {STAR_BLINK_SLOW}")
    await stand(ws)
    await rpc(ws, "game.setStar", {"seconds": STAR_DURATION})
    gaps = {"normal": [], "runout": []}
    prev, last_at, elapsed = None, 0.0, 0.0
    for _ in range(900):
        s = await si(ws, 1)
        elapsed += DT
        idx, left = s["star_color_index"], s["star_timer"]
        if prev is not None and idx != prev:
            bucket = "runout" if left <= STAR_RUNOUT else "normal"
            gaps[bucket].append(elapsed - last_at)
            last_at = elapsed
        prev = idx
        if left <= 0.0:
            break
    for bucket, want in (("normal", STAR_BLINK), ("runout", STAR_BLINK_SLOW)):
        vals = gaps[bucket]
        avg = sum(vals) / len(vals) if vals else 0.0
        check(
            f"{'常速' if bucket == 'normal' else '最后一秒'}换色间隔 ≈ {want * 1000:.0f} ms",
            bool(vals) and near(avg, want, 0.012),
            f"平均 {avg * 1000:.1f} ms，{len(vals)} 次",
        )
    check(
        "退场期间仍然无敌（音乐是唯一的警告）",
        s["star_timer"] <= 0.0,
        "星星走完了",
    )

    await si(ws, 1, FREE_ALL)
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
