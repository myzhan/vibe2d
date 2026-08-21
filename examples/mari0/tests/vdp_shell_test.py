#!/usr/bin/env python3
"""
mari0 龟壳验证脚本

壳有四种交互，原版全都收在同一个 `koopa:stomp`（`koopa.lua:212-240`）里，按顺序判：
掉翅膀 → 变壳 → 踢动静止的壳 → 停下移动的壳。本脚本锁的是后两条，加上壳滑行时
撞死别人的连杀链 —— 这三件事之前一件都没做。

规则：
  - **走路撞上静止的壳会把它踢开**，给 500 分定值，而且**不掉血**
    （`mario.lua:1899-1911`）。这一条在受伤后的无敌期里同样生效 —— 原版
    `self.invincible` 那个分支里也带着同样的特判。
  - 但**移动中的壳没有特判**，会走到 `self:die()`：你踢出去的东西回头能弄死你。
  - **踩住移动的壳会把它停下**，并且 `self.combo = 1` 把它的连杀链清零
    （`koopa.lua:236-239`）。把壳卡在墙角反复踢的刷分套路就靠这半边成立。
  - **滑行的壳撞到谁谁死**（`koopa.lua:277-293`），按 `koopacombo`
    = {500, 800, 1000, 2000, 4000, 5000, 8000} 计分。注意两点：
      · 这是**壳自己的**计数器，跟马里奥连踩的 `mariocombo` 是两套，起点 500 而非 100；
      · 走完这张表之后每多杀一个给一条**命**，不再给分。
  - 壳不挑食：火球免疫的 beetle、踩不死的 spiny 都照杀。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_shell_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

# variables.lua:25, :102
KOOPA_COMBO = [500, 800, 1000, 2000, 4000, 5000, 8000]
SHELL_KICK_SCORE = 500
SHELL_SPEED = 12.0

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


PRESS = lambda *k: [{"device": "keyboard", "action": "press", "key": x} for x in k]
RELEASE = lambda *k: [{"device": "keyboard", "action": "release", "key": x} for x in k]
FREE_ALL = RELEASE("Left", "Right", "Space", "F", "Down")


def shells(snap):
    return [e for e in snap["enemies"] if e["type"].startswith("koopa")]


async def setup(ws, score=0):
    """1-1，清场，站在第 6 列的平地上。y 用 10 格 —— 11 格会撞进管道实体里。"""
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setScore", {"score": score})
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": 6 * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 4)
        if s["player"]["on_ground"]:
            break
    return s


async def resting_shell(ws, col=12):
    """踩一只 koopa，得到一枚静止的壳。"""
    await rpc(ws, "game.spawnEnemy", {"type": "koopa", "x": col * T, "y": 11 * T})
    await si(ws, 4)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": 8 * T, "vx": 0.0, "vy": 200.0})
    for _ in range(60):
        s = await si(ws, 2)
        k = shells(s)
        if k and k[0]["state"] == "shell":
            return s
    return s


async def kick_from_left(ws):
    """走到壳左边撞它一下，返回撞完那一帧。"""
    k = shells(await si(ws))[0]
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": k["x"] - 1.5 * T, "y": 11 * T, "vx": 0.0, "vy": 0.0},
    )
    for _ in range(20):
        s = await si(ws, 2)
        if s["player"]["on_ground"]:
            break
    for _ in range(40):
        s = await si(ws, 2, PRESS("Right"))
        k = shells(s)
        if k and k[0]["state"] == "shell_moving":
            break
    await si(ws, 1, FREE_ALL)
    return s


async def stomp_moving_shell(ws):
    """踩住一枚正在滑行的壳。

    壳走 12 格/秒，直接把人摆在它「现在」的位置上必然扑空 —— 所以这里逐帧把人
    重新对到壳的正上方，再让他落下去。摆位是造作的，但结果是确定的。
    """
    for _ in range(90):
        k = shells(await si(ws))
        if not k:
            return None
        k = k[0]
        if k["state"] != "shell_moving":
            return await si(ws)
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": k["x"], "y": k["y"] - 1.2 * T, "vx": 0.0, "vy": 400.0},
        )
        s = await si(ws, 1)
        k2 = shells(s)
        if k2 and k2[0]["state"] == "shell":
            return s
    return None


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section(f"1. 走路撞静止壳：踢开 + {SHELL_KICK_SCORE} 分，不掉血")
    await setup(ws)
    s = await resting_shell(ws)
    k = shells(s)
    check(
        "踩一下先变成静止的壳",
        bool(k) and k[0]["state"] == "shell" and abs(k[0]["vx"]) < 1.0,
        f"state={k and k[0]['state']} vx={k and round(k[0]['vx'] / T, 2)}",
    )
    await rpc(ws, "game.setScore", {"score": 0})
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    s = await kick_from_left(ws)
    k = shells(s)
    check(
        f"撞上去把它踢向右侧（{SHELL_SPEED} 格/秒）",
        bool(k) and k[0]["state"] == "shell_moving" and k[0]["vx"] / T > SHELL_SPEED - 0.5,
        f"state={k and k[0]['state']} vx={k and round(k[0]['vx'] / T, 1)}",
    )
    check(
        f"给 {SHELL_KICK_SCORE} 分定值（不是连击阶梯的任何一级）",
        s["score"] == SHELL_KICK_SCORE,
        f"得分 {s['score']}",
    )
    check(
        "踢壳不掉血 —— 大马里奥还是大的",
        s["player"]["is_big"] and s["state"] == "playing",
        f"big={s['player']['is_big']} state={s['state']}",
    )

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 踩住移动的壳：停下，并把它的连杀链清零")
    s = await stomp_moving_shell(ws)
    k = shells(s) if s else None
    check(
        "踩下去之后停住",
        bool(k) and k[0]["state"] == "shell" and abs(k[0]["vx"]) < 1.0,
        f"state={k and k[0]['state']} vx={k and round(k[0]['vx'] / T, 2)}",
    )
    check(
        "shell_combo 归零（卡墙刷分之所以每轮从 500 重新开始）",
        bool(k) and k[0]["shell_combo"] == 0,
        f"shell_combo={k and k[0]['shell_combo']}",
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 滑行的壳撞谁谁死，按 koopacombo 计分")
    await setup(ws)
    s = await resting_shell(ws)
    sx = shells(s)[0]["x"] / T
    victims = 4
    for i in range(victims):
        await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": (sx + 4 + i * 3) * T, "y": 11 * T})
    await si(ws, 2)
    await rpc(ws, "game.setScore", {"score": 0})
    s = await kick_from_left(ws)
    after_kick = s["score"]
    # 让壳跑一段，把这一排都撞掉
    for _ in range(90):
        s = await si(ws, 2)
    k = shells(s)
    chain = k[0]["shell_combo"] if k else 0
    gained = s["score"] - after_kick
    want = sum(KOOPA_COMBO[:chain])
    check(
        f"壳的连杀数 >= 放的 {victims} 只（1-1 自己还会补生成几只）",
        chain >= victims,
        f"shell_combo={chain}",
    )
    check(
        f"得分等于 koopacombo 前 {chain} 项之和 = {want}",
        gained == want and chain > 0,
        f"踢完之后又拿了 {gained} 分",
    )
    check(
        f"起点是 {KOOPA_COMBO[0]} 而不是踩敌人的 100",
        chain >= 1 and gained >= KOOPA_COMBO[0],
        f"第一级 {KOOPA_COMBO[0]}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section(f"4. 连杀走完 {len(KOOPA_COMBO)} 级之后改给命")
    await setup(ws)
    s = await resting_shell(ws)
    sx = shells(s)[0]["x"] / T
    # 密排：壳 12 格/秒，一格一只才能在它撞到管子弹回来之前把这一串走完。
    # 撒得太开的话远处那几只会被镜头剔出场，连杀链就断在半路。
    for i in range(len(KOOPA_COMBO) + 4):
        await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": (sx + 3 + i) * T, "y": 11 * T})
    await si(ws, 2)
    await rpc(ws, "game.setLives", {"lives": 3})
    s = await kick_from_left(ws)
    # 逐帧跟：壳弹回来撞死玩家会让命数掉下去，把 1UP 盖掉，所以要在**发生的当下**记录
    best_chain, oneup = 0, False
    lives = s["lives"]
    for _ in range(220):
        s = await si(ws, 1)
        k = shells(s)
        if k:
            best_chain = max(best_chain, k[0]["shell_combo"])
        if s["lives"] > lives:
            oneup = True
        lives = s["lives"]
        if best_chain >= len(KOOPA_COMBO) and oneup:
            break
    check(
        f"连杀链爬到 {len(KOOPA_COMBO)} 级封顶",
        best_chain == len(KOOPA_COMBO),
        f"最高 shell_combo={best_chain}",
    )
    check(
        "封顶之后多杀的换成命，不再给分",
        oneup,
        f"过程中命数有没有涨: {oneup}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. 移动中的壳没有特判：撞上去照样掉血")
    await setup(ws)
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    s = await resting_shell(ws)
    s = await kick_from_left(ws)
    k = shells(s)[0]
    # 站到壳的前进方向上等它撞回来
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": k["x"] + 5 * T, "y": 11 * T, "vx": 0.0, "vy": 0.0},
    )
    for _ in range(20):
        s = await si(ws, 2)
        if s["player"]["on_ground"]:
            break
    hurt = False
    for _ in range(60):
        s = await si(ws, 1)
        if not s["player"]["is_big"] or s["state"] != "playing":
            hurt = True
            break
    check(
        "被自己踢出去的壳撞到会掉一级（原版这里不特判，直接走 die 分支）",
        hurt,
        f"big={s['player']['is_big']} state={s['state']}",
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
