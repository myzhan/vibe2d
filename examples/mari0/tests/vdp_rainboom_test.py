#!/usr/bin/env python3
"""
mari0 音爆彩虹（sonic rainboom）验证脚本

一个默认关掉的彩蛋（`sonicrainboom`），原版作者自己在注释里都不敢相信自己写了这个。
从传送门里以超过 45 格/秒的速度出来就会炸出一道彩虹，并且**全场敌人当场毙命**。

规则（`mario:checkrainboom`，`mario.lua:3094-3135`）：
  - 只有 **上、左、右** 三个出口算：向下出来不炸，重力免费送你到那个速度。
  - 门槛是 `rainboomspeed = 45` 格/秒，按出口方向取对应的速度分量。
  - **一次落地只能炸一次**：`rainboomallowed` 被花掉后只有踩到地面才恢复。
  - 清场（`mario.lua:3115-3131`）对整关生效，**没有距离判定** —— 屏幕外的也一起死。
    能清哪些种类是照 `enemies`（`game.lua:55`）那张硬编码名单来的，有两处反直觉：
      · **buzzy beetle 会死**，尽管它免疫火球 —— 彩虹直接调 `shotted`，
        免疫根本没机会参与。spiny 同理（它也踩不死）。
      · **bullet bill 活下来**，尽管火球打得死它 —— 它压根不在那张名单上。
  - 分数按火球那套**固定费率**（`firepoints`）结算，不是踩敌人的连击阶梯。
  - Bowser 是原版专门写成嵌套循环的例外：先跟大家一样挨一下，再补六下
    （7 击对 5 点血，所以必死）。原版清场循环**不给他加分**，因为钱在
    `bowser:firedeath` 里付（`bowser.lua:193`）；本移植没有 `firedeath`，
    所以那 5000 在清场处一并付掉 —— 总额一样，和火球路径同一个写法。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_rainboom_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

# variables.lua:88 + mario.lua:3120-3124 / bowser.lua:176-181
RAINBOOM_SPEED = 45.0 * T
BOWSER_HEALTH = 5
RAINBOOM_BOWSER_HITS = 7

# variables.lua:28-37 —— 固定费率
FIRE_POINTS = {"goomba": 100, "koopa": 200, "hammer_bro": 1000, "spikey": 100, "beetle": 200}

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


async def arm(ws, on=True):
    """回到 1-1，清空场面，把彩蛋开关拨到指定状态。"""
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.clearPortals")
    await rpc(ws, "game.setRainboom", {"on": on})
    # 故意**不给**星星：无敌状态下玩家一碰就杀，而那是按连击阶梯记分的，
    # 会混进第 6 节要验的固定费率里。出门那一帧的死伤要全部来自彩虹。
    await rpc(ws, "game.setScore", {"score": 0})
    # 不给星星就有可能被撞死，而一路死下去会耗光命、掉进 game over 回标题，
    # 后面每一节都跟着废掉。每节开头补满，让每节都从同一个状态起跑。
    await rpc(ws, "game.setLives", {"lives": 5})
    return await si(ws, 2)


async def fire_through(ws, exit_facing="right", speed=RAINBOOM_SPEED * 1.5, frames=8):
    """从一扇朝上的门掉进去，再从 `exit_facing` 那扇冲出来，停在彩虹炸开的那一帧。

    返回 `(炸前一帧, 炸开那帧)`；没炸出来时第二项是 `None`。

    **必须逐帧走**：出门后镜头一步跟到第 37 格，身后的敌人当帧就被剔出快照。
    本脚本第一版直接跑 6 帧再数人头，于是「被彩虹清掉」和「被镜头剔掉」分不出来
    —— bullet bill 和炮台看着都像是死了，其实炸开那一帧它们还好好地在走。
    """
    await rpc(
        ws,
        "game.setPortal",
        {"index": 0, "x": 24 * T, "y": 11 * T, "orientation": "up", "active": True},
    )
    await rpc(
        ws,
        "game.setPortal",
        {"index": 1, "x": 40 * T, "y": 11 * T, "orientation": exit_facing, "active": True},
    )
    await rpc(ws, "game.setPlayerPos", {"x": 24 * T, "y": 9 * T, "vx": 0.0, "vy": speed})
    prev = await si(ws)
    for _ in range(frames):
        s = await si(ws)
        if s["rainbooms"] > prev["rainbooms"]:
            return prev, s
        prev = s
    return prev, None


async def spawn(ws, etype, col, row=11):
    await rpc(ws, "game.spawnEnemy", {"type": etype, "x": col * T, "y": row * T})


def at(snap, etype, col, tol=1.5):
    """快照里这个位置上的这只敌人，找不到就是 `None`。

    按坐标认人，而不是按种类数个数：1-1 自己会在第 22/40/51 格惰性生成 goomba，
    它们也一样被彩虹扫掉，纯数数会把它们算进来。
    """
    for e in snap["enemies"]:
        if e["type"] == etype and abs(e["x"] / T - col) <= tol:
            return e
    return None


def swept(before, after):
    """这一帧里从活到死的那些敌人，按 (种类, 大致位置) 配对。"""
    out = []
    for e in before["enemies"]:
        if e["state"] == "dead":
            continue
        now = at(after, e["type"], e["x"] / T, tol=1.0)
        if now is not None and now["state"] == "dead":
            out.append(e["type"])
    return out


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 出口方向与速度门槛")
    await arm(ws)
    _, s = await fire_through(ws, "right")
    check("朝右超速出门 → 炸", s is not None, "炸开了" if s else "一直没炸")

    await arm(ws)
    _, s = await fire_through(ws, "down")
    check("朝下出门 → 不炸（重力免费送你到这个速度）", s is None, "竟然炸了" if s else "没炸")

    await arm(ws)
    _, s = await fire_through(ws, "right", speed=RAINBOOM_SPEED * 0.5)
    check("没到 45 格/秒 → 不炸", s is None, "竟然炸了" if s else "没炸")

    await arm(ws, on=False)
    _, s = await fire_through(ws, "right")
    check("开关关掉 → 怎么冲都不炸（默认就是关的）", s is None, "竟然炸了" if s else "没炸")

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 一次落地只炸一次")
    await arm(ws)
    _, s = await fire_through(ws, "right")
    # 不落地就再来一次：rainboomallowed 已经花掉了，只有踩到地面才恢复
    _, again = await fire_through(ws, "right")
    check(
        "没沾地之前第二次冲不出彩虹",
        s is not None and again is None,
        f"第一次={'炸' if s else '没炸'} 第二次={'炸' if again else '没炸'}",
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 清场：整关一起死，没有距离判定")
    await arm(ws)
    # 一只在脚边，一只远到镜头根本看不见
    await spawn(ws, "goomba", 26)
    await spawn(ws, "goomba", 150)
    await spawn(ws, "koopa", 28)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    check("确实炸了", after is not None)
    if after:
        for label, etype, col in (
            ("脚边第 26 格的 goomba", "goomba", 26),
            ("第 28 格的 koopa", "koopa", 28),
            ("镜头外第 150 格的 goomba —— 清场没有距离判定", "goomba", 150),
        ):
            was, now = at(before, etype, col), at(after, etype, col)
            check(
                f"{label} 被扫掉",
                was is not None
                and was["state"] != "dead"
                and now is not None
                and now["state"] == "dead",
                f"炸前={was and was['state']} 炸后={now and now['state']}",
            )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 名单上的两处反直觉")
    await arm(ws)
    await spawn(ws, "beetle", 26)
    await spawn(ws, "spikey", 28)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    if after:
        b, sp = at(after, "beetle", 26), at(after, "spikey", 28)
        check(
            "buzzy beetle 照样死 —— 火球免疫拦不住彩虹（彩虹直接调 shotted）",
            b is not None and b["state"] == "dead",
            f"beetle={b and b['state']}",
        )
        check(
            "spiny 也死 —— 踩不死不代表炸不死",
            sp is not None and sp["state"] == "dead",
            f"spikey={sp and sp['state']}",
        )

    await arm(ws)
    await spawn(ws, "bullet_bill", 26)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    if after:
        bb = at(after, "bullet_bill", 26, tol=3.0)
        check(
            "bullet bill 活下来 —— 火球打得死它，但它不在 game.lua:55 那张名单上",
            bb is not None and bb["state"] != "dead",
            f"bullet_bill={bb and bb['state']}",
        )

    await arm(ws)
    for i, t in enumerate(("fire", "hammer", "bullet_bill_cannon")):
        await spawn(ws, t, 26 + 2 * i)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    if after:
        still = {
            t: at(after, t, 26 + 2 * i, tol=3.0)
            for i, t in enumerate(("fire", "hammer", "bullet_bill_cannon"))
        }
        check(
            "火焰 / 锤子 / 炮台全都活着（本来就杀不死的东西）",
            all(e is not None and e["state"] != "dead" for e in still.values()),
            ", ".join(f"{k}={v and v['state']}" for k, v in still.items()),
        )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. Bowser：一发彩虹就够（7 击对 5 点血）")
    await arm(ws)
    await spawn(ws, "bowser", 26)
    s = await si(ws, 2)
    b = at(s, "bowser", 26)
    check(f"起手 {BOWSER_HEALTH} 点血", b and b["hp"] == BOWSER_HEALTH, f"hp={b and b['hp']}")
    check(
        f"彩虹打 {RAINBOOM_BOWSER_HITS} 下，比血厚 —— 所以必死",
        RAINBOOM_BOWSER_HITS > BOWSER_HEALTH,
        f"{RAINBOOM_BOWSER_HITS} > {BOWSER_HEALTH}",
    )
    before, after = await fire_through(ws, "right")
    if after:
        b = at(after, "bowser", 26, tol=3.0)
        check(
            "一发就倒（火球得打满五发）",
            b is not None and b["state"] == "dead",
            f"bowser={b and b['state']} hp={b and b['hp']}",
        )

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 分数按固定费率结算，不是连击阶梯")
    await arm(ws)
    await spawn(ws, "goomba", 26)
    await spawn(ws, "koopa", 28)
    await spawn(ws, "hammer_bro", 30)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    if after:
        # 期望值从**实际被扫掉的那批**算出来，而不是假设场上只有我放的三只：
        # 1-1 自己会惰性生成 goomba，它们也一样被扫，一样付钱。
        killed = swept(before, after)
        want = sum(FIRE_POINTS[t] for t in killed if t in FIRE_POINTS)
        got = after["score"] - before["score"]
        check(
            f"这一帧扫掉 {len(killed)} 只，按固定费率共 {want} 分",
            bool(killed) and got == want,
            f"扫掉 {sorted(killed)} 得分 {got}，期望 {want}",
        )

    # 三只同种：连击阶梯会给 100+200+400=700，固定费率只给 100×3
    await arm(ws)
    for col in (26, 28, 30):
        await spawn(ws, "goomba", col)
    await si(ws, 2)
    before, after = await fire_through(ws, "right")
    if after:
        killed = swept(before, after)
        n = killed.count("goomba")
        got = after["score"] - before["score"]
        check(
            "同种多只是简单相加，没有连击加成（阶梯会是 100+200+400）",
            n >= 3 and got == n * FIRE_POINTS["goomba"],
            f"扫掉 {n} 只 goomba 得分 {got}，期望 {n * FIRE_POINTS['goomba']}",
        )

    await rpc(ws, "game.setRainboom", {"on": False})
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
