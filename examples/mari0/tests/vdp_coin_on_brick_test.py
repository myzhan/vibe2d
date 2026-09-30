#!/usr/bin/env python3
"""
mari0 顶砖收砖上金币验证脚本

原版 `mario.lua` 的 `hitblock` 弹跳分支是一条**顺序执行**的长链：金币砖内容物 →
藤蔓 → 砸砖/顶砖 → 压死砖上的敌人（2487-2506）→ 让砖上的道具跳一下（2508-2518）
→ **检查砖顶上有没有金币**（2520-2524）：

    --check for coin on top
    if inmap(x, y-1) and tilequads[map[x][y-1][1]].coin then
        collectcoin(x, y-1)
        table.insert(coinblockanimations, coinblockanimation:new(x-0.5, y-1))
    end

`collectcoin`（mario.lua:3071-3086）= 清格子 + `addpoints(200)`（不带坐标，所以原版
这里**没有**飘分）+ 金币音效 + 金币计数（满 100 加命）。也就是说：小个子顶砖、大个子
砸砖、顶问号砖、顶藤蔓砖 —— 只要被顶的砖上面正好坐着一块金币，这块金币就跟着收进
口袋。移植版只做了 AABB 走过拾取（`game.rs` 的 coin 循环），人从砖**底下**顶上去永远
碰不到头顶那块金币的盒子 —— 这就是本脚本要钉死的 bug。

测试场地选 smb/5-2：第 8 行 (29..31) 三块金币坐在第 9 行一排砖上，砖下三层空气、
第 13 行是地板 —— 人贴砖底放下正好头顶砖，且摔下去也死不了。全 shipped 关卡里
金币直接坐在**问号砖**上的情形一处都没有，所以问号砖那一条臂只由插入位置保证
（和原版一样在 kind 判定之后、任何早退之前），脚本只测有数据可测的两条：小个子
顶砖弹一下、大个子砸砖。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_coin_on_brick_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0

# constants.rs
COIN_SCORE = 200
BRICK_BREAK_SCORE = 50

# 5-2 的测试格：金币第 8 行，砖第 9 行
BRICK_ROW = 9
COIN_ROW = 8
SMALL_COL = 29
BIG_COL = 30
NEIGHBOUR_COL = 31

# 人贴砖底放、给一点上速度：解算器当帧把人顶回砖底、vy 归零，也就触发顶砖
# （`game.rs` 的块命中检测吃 `vy == 0 && !on_ground`）。vy 会被归零，人不会
# 一路飞进金币那一行 —— 所以金币被收走只可能是「顶砖」这条路收的，不是走过拾取。
BUMP_VY = -300.0

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
    """Step and inspect in one call — the autopilot's trick, and much faster."""
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


def coin_at(s, col, row):
    """场上的金币实例（col,row 格左上角的世界像素坐标）。"""
    for c in s["coins"]:
        if abs(c["x"] - col * TILE_SIZE) < 0.01 and abs(c["y"] - row * TILE_SIZE) < 0.01:
            return c
    return None


async def setup(ws):
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": 5, "level": 2, "sublevel": 0},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    return await si(ws)


async def bump(ws, col):
    """贴着 (col, BRICK_ROW) 砖底顶一下，返回顶完的快照。"""
    await rpc(
        ws,
        "game.setPlayerPos",
        {
            "x": col * TILE_SIZE,
            "y": (BRICK_ROW + 1) * TILE_SIZE,
            "vx": 0.0,
            "vy": BUMP_VY,
        },
    )
    # 顶砖当帧生效；多走几帧只是容错，人 vy 已归零只会往下掉
    for _ in range(10):
        s = await si(ws)
        c = coin_at(s, col, COIN_ROW)
        if c and c["collected"]:
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 5-2 (29..31, 8) 三块金币坐在第 9 行的砖上")
    s = await setup(ws)
    for col in (SMALL_COL, BIG_COL, NEIGHBOUR_COL):
        c = coin_at(s, col, COIN_ROW)
        check(f"({col}, {COIN_ROW}) 有一块没被收的金币", c is not None and not c["collected"],
              str(c))
    base_score = s["score"]
    base_coins = s["coin_count"]

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 小个子从底下顶砖：砖上的金币跟着收（本 bug 的主体）")
    s = await bump(ws, SMALL_COL)
    c = coin_at(s, SMALL_COL, COIN_ROW)
    check("头顶那块金币被收走了", c is not None and c["collected"], str(c))
    check("金币计数 +1", s["coin_count"] == base_coins + 1,
          f"{base_coins} → {s['coin_count']}")
    check(f"分数 +{COIN_SCORE}", s["score"] == base_score + COIN_SCORE,
          f"{base_score} → {s['score']}")
    for col in (BIG_COL, NEIGHBOUR_COL):
        n = coin_at(s, col, COIN_ROW)
        check(f"旁边的 ({col}, {COIN_ROW}) 没被牵连", n is not None and not n["collected"],
              str(n))
    check("人没死，游戏还在跑", s["state"] == "playing", str(s["state"]))

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 大个子从底下砸砖：砖碎了，砖上的金币照样收")
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    s = await bump(ws, BIG_COL)
    c = coin_at(s, BIG_COL, COIN_ROW)
    check("头顶那块金币被收走了", c is not None and c["collected"], str(c))
    check("金币计数再 +1", s["coin_count"] == base_coins + 2,
          f"{base_coins} → {s['coin_count']}")
    # 砸砖本身 +50（BRICK_BREAK_SCORE），金币再 +200
    check(f"分数共 +{BRICK_BREAK_SCORE + COIN_SCORE}",
          s["score"] == base_score + COIN_SCORE + BRICK_BREAK_SCORE + COIN_SCORE,
          f"{base_score} → {s['score']}")
    n = coin_at(s, NEIGHBOUR_COL, COIN_ROW)
    check(f"旁边的 ({NEIGHBOUR_COL}, {COIN_ROW}) 还是没动", n is not None and not n["collected"],
          str(n))

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 修完之后走过拾取还在：人走进金币盒子照样收")
    await rpc(ws, "game.setPlayerSize", {"size": "small"})
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": NEIGHBOUR_COL * TILE_SIZE, "y": COIN_ROW * TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    s = await si(ws)
    c = coin_at(s, NEIGHBOUR_COL, COIN_ROW)
    check("走过去就收了", c is not None and c["collected"], str(c))
    check("金币计数再 +1", s["coin_count"] == base_coins + 3,
          f"{base_coins} → {s['coin_count']}")

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
