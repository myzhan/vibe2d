#!/usr/bin/env python3
"""
mari0 Bullet Bill 验证脚本

**两套完全不同的来源**，别当成一件事：

1. **炮台**（实体 60 `bulletbill`，在 5-1 / 5-2 / 7-1 / 8-2 / 8-3）——
   `rocketlauncher`（`bulletbill.lua:1-47`）。炮身本体是 **tile 42 + 64**，实体只是
   一个计时器；所以它没有碰撞盒、不能被踩也不能被打，站在炮台上是安全的。
   每 `random(1.0, 4.5)` 秒尝试开火，条件三条：**在镜头内**、全场存活 < 5 发、
   **玩家水平距离超过 3 格**（这最后一条才是"站在炮口旁边不会被打"的原因）。

2. **区间生成器**（实体 33/34 `bulletbillstart`/`bulletbillend`，在 5-3 / 6-3）——
   `game.lua:826-831`。**没有炮台、不看距离、也不受 5 发上限约束**：子弹直接在
   镜头右缘外两格、第 4..12 行（1 基）随机高度出现往左飞。5-3 两个标记都有，
   所以是一段围起来的区域；6-3 只有 start，开了就再也不关。
   闩的判据是**玩家**的 x（`mario.lua:985-991`），不是镜头。

子弹本身（`bulletbill.lua`）：
  - 8 格/秒、**无重力**、20 秒后自行消失
  - **完全不吃地形**：它所有碰撞回调都 `return false`，而在原版物理里
    返回 false 就是"别解算这次接触"（`physics.lua:288-296`）—— 所以它穿墙穿地
  - 可以踩、可以火球打
  - **穿过传送门之后会变成武器**：`portaled()` 置 `killstuff`，此后它撞到的
    栗子怪/乌龟会被 `shotted` 打飞（`bulletbill.lua:181-194`）；没穿门之前
    它和敌人互相穿过

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_bulletbill_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:235-238, :403
BULLET_BILL_SPEED = 8.0
BULLET_BILL_LIFETIME = 20.0
BULLET_BILL_RANGE = 3.0
MAX_BULLET_BILLS = 5
BULLET_BILL_FIRST_SHOT = 0.5

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


async def stand_at(ws, col, row=10):
    """Put the player somewhere clear and let him settle onto the ground.

    Row 10 rather than a row with floor in it: dropping him *inside* a solid tile
    leaves him wedged and the whole scene stops advancing, which reads as "none of
    the new code runs" rather than "the probe put him in a wall".
    """
    await rpc(ws, "game.setPlayerPos", {"x": col * TILE_SIZE, "y": row * TILE_SIZE})
    await step(ws, 20)
    return await snap(ws)


async def survive(ws):
    """Keep the player alive through whatever is being measured.

    Two hazards, both of which end a probe silently. Enemies: teleporting reveals new
    columns, so the lazy spawner drops fresh goombas next to wherever the player just
    landed — `clearEnemies` before *settling* doesn't help, it has to be after. Damage:
    one hit while small is death, and a dead player freezes `update_playing`, so every
    later reading looks like the feature under test doing nothing.

    Big rather than a star, because a star would *destroy* the bullet bills being
    counted.
    """
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 炮台是布景：没有碰撞盒、踩不到、也打不掉")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 5, "level": 1})
    await step(ws)
    s = await stand_at(ws, 105)
    cannons = of_type(s, "bullet_bill_cannon")
    check("5-1 第 111 列的炮台已生成", len(cannons) == 1, f"{len(cannons)} 个")
    if cannons:
        # 炮台的 y 就是它自己那一格（不是像别的敌人那样抬高一格），
        # 否则子弹会从炮口上方一格飞出来。
        check(
            "炮台停在自己那一格（第 11 行 → y=352）",
            abs(cannons[0]["y"] - 11 * TILE_SIZE) < 0.01,
            f"y={cannons[0]['y']}",
        )

    section("2. 站在炮台上不会被打：3 格之内它不开火")
    s = await stand_at(ws, 111)
    await survive(ws)
    before = len(of_type(s, "bullet_bill"))
    await step(ws, int(FPS * 6))
    s = await snap(ws)
    check(
        "正踩在炮台上待 6 秒，一发都没有",
        len(of_type(s, "bullet_bill")) == before,
        f"{before} → {len(of_type(s, 'bullet_bill'))}",
    )
    # 往旁边走 5 格（> 3）就该开火了。`survive` 会把炮台一起清掉（它就在敌人
    # 表里），所以清完得自己补一门回去 —— 位置也就完全可控了。
    await stand_at(ws, 106)
    await survive(ws)
    await rpc(
        ws,
        "game.spawnEnemy",
        {
            "type": "bullet_bill_cannon",
            "x": 111 * TILE_SIZE,
            "y": 11 * TILE_SIZE,
            "facing_right": False,
        },
    )
    fired = None
    for _ in range(40):
        await step(ws, 6)
        bills = of_type(await snap(ws), "bullet_bill")
        if bills:
            fired = bills[0]
            break
    check("退到 5 格外它就开火了", fired is not None)
    if fired:
        check(
            "朝玩家那一侧打（玩家在左 → 子弹往左飞）",
            fired["vx"] < 0,
            f"vx={fired['vx'] / TILE_SIZE:.1f} 格/秒",
        )
        check(
            f"弹速是 {BULLET_BILL_SPEED:.0f} 格/秒",
            abs(abs(fired["vx"]) / TILE_SIZE - BULLET_BILL_SPEED) < 0.1,
            f"{abs(fired['vx']) / TILE_SIZE:.2f}",
        )
        check("没有重力（水平直飞）", abs(fired["vy"]) < 1e-6, f"vy={fired['vy']}")

    section("3. 子弹不吃地形：穿墙穿地，只有 20 秒寿命能拦住它")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await stand_at(ws, 20)
    await survive(ws)
    # 直接把一发子弹放在地面里，往右飞：如果吃地形，它会立刻停住。
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "bullet_bill", "x": 22 * TILE_SIZE, "y": 13 * TILE_SIZE, "facing_right": True},
    )
    await step(ws, 2)
    bills = of_type(await snap(ws), "bullet_bill")
    if not bills:
        check("子弹还在", False)
    else:
        x0, y0 = bills[0]["x"], bills[0]["y"]
        await step(ws, 30)
        s = await snap(ws)
        bills = of_type(s, "bullet_bill")
        check("埋在地里的子弹照样往前飞", bool(bills), f"state={s['state']}")
        if bills:
            moved = (bills[0]["x"] - x0) / TILE_SIZE
            check(
                "半秒走了约 4 格（8 格/秒，没被地形拦住）",
                3.5 < moved < 4.5,
                f"走了 {moved:.2f} 格",
            )
            check("高度没变（重力为 0）", abs(bills[0]["y"] - y0) < 0.01)

    section("4. 可以踩死")
    # 先重载关卡把镜头拉回原点。镜头**永不回退**（和原版一样），上一节把它推到了
    # 第 15 列开外；不重载就直接瞬移回第 6 列的话，新放的子弹落在"镜头左边 200px
    # 之外"，一帧就被剔除规则清掉，看起来像踩不动。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await stand_at(ws, 6, row=8)
    await survive(ws)
    # 得把他**扔**到子弹上，不能让他自由落体：子弹以 8 格/秒横穿，等他从 2 格高
    # 落下来的那 14 帧里，子弹已经飞出去两格了。给个初速度，两帧内就接触。
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "bullet_bill", "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False},
    )
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE - 72, "vy": 300.0},
    )
    bounced = False
    for _ in range(10):
        await step(ws, 1)
        s = await snap(ws)
        if s["player"]["vy"] < -1.0:
            bounced = True
            break
    check("从上面落下会弹起来（可以踩）", bounced)

    section("5. 20 秒寿命：没有地形能停下它，所以它自己到期")
    await stand_at(ws, 20)
    await survive(ws)
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "bullet_bill", "x": 24 * TILE_SIZE, "y": 8 * TILE_SIZE, "facing_right": True},
    )
    await step(ws, 2)
    alive_at = None
    for k in range(30):
        await step(ws, 45)
        await rpc(ws, "game.setPlayerSize", {"size": "big"})
        if not of_type(await snap(ws), "bullet_bill"):
            alive_at = 0.75 * (k + 1)
            break
    check(
        f"约 {BULLET_BILL_LIFETIME:.0f} 秒后自行消失",
        alive_at is not None and abs(alive_at - BULLET_BILL_LIFETIME) < 2.0,
        f"实测 {alive_at} 秒" if alive_at else "始终没消失",
    )

    section("6. 区间生成器：5-3 的 start 就在第 0 列，所以它从第一帧就在下雨")
    # 这一条一开始写反了，值得留个记录：想当然以为"刚装载 → 区间是关的"，
    # 但 5-3 的 `bulletbillstart` 在**第 0 列**、`end` 在第 125 列 —— 整关几乎
    # 全程在区间里，最后 50 列才停。6-3 只有 start，开了就不关。
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 5, "level": 3})
    await step(ws, 3)
    s = await snap(ws)
    check("5-3 一开局就在区间内（start 在第 0 列）", s["bullet_bill_zone"] is True)

    section("7. 区间里的子弹：从镜头右缘外进来、朝左飞、高度随机")
    # 5-3 第 30 列脚下是空的（最近的实心在第 5 行，头顶上），站过去就掉坑死了。
    # 关卡开头 0..15 列才有地面。
    await stand_at(ws, 10)
    await survive(ws)
    # 子弹以 8 格/秒往左飞，飞过玩家之后很快被"镜头左边 200px 外"的规则剔除，
    # 所以**同一瞬间**场上只有一两发。要数产量就得逐帧累计，不能只看一张快照。
    seen = []
    for _ in range(60):
        await step(ws, 12)
        await rpc(ws, "game.setPlayerSize", {"size": "big"})
        s = await snap(ws)
        for b in of_type(s, "bullet_bill"):
            if b["state"] == "walking":
                seen.append((round(b["y"] / TILE_SIZE), b["vx"]))
    rows = sorted({r for r, _ in seen})
    check("十二秒里持续有子弹进来", len(seen) >= 5, f"累计观测 {len(seen)} 次")
    check("全部朝左飞", bool(seen) and all(vx < 0 for _, vx in seen))
    check(
        "高度落在第 3..11 行（0 基）之间，且不止一个高度",
        bool(rows) and all(3 <= r <= 11 for r in rows) and len(rows) > 1,
        f"出现过的行: {rows}",
    )

    section("8. 同一关重放两次，随机序列必须一致（否则整套探针都不可重现）")
    seqs = []
    for _ in range(2):
        await rpc(ws, "game.setLevel", {"pack": "smb", "world": 5, "level": 3})
        await step(ws, 3)
        await stand_at(ws, 10)
        await survive(ws)
        seq = []
        for _ in range(30):
            await step(ws, 12)
            await rpc(ws, "game.setPlayerSize", {"size": "big"})
            s = await snap(ws)
            seq.extend(
                sorted(round(b["y"]) for b in of_type(s, "bullet_bill") if b["state"] == "walking")
            )
        seqs.append(seq)
    check(
        "两次重放的高度序列完全相同",
        seqs[0] == seqs[1] and seqs[0],
        f"{len(seqs[0])} 次观测 vs {len(seqs[1])} 次",
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
