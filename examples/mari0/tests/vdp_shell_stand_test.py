#!/usr/bin/env python3
"""
mari0 与静止乌龟壳接触的回归测试。

原版中，马里奥碰到静止的乌龟壳只会把它踢飞，自己不会死亡。移植版早期在“从
侧面走上壳”和“从上方落到壳上”两条路径里都没把壳瞬移出马里奥的碰撞框，导致
下一帧马里奥和刚被踢飞的移动壳仍然重叠，从而死亡。

本测试钉死三种情形：
  1. 马里奥静止站在刚缩进壳的乌龟壳正上方。
  2. 马里奥从左侧以低速走上乌龟壳。
  3. 马里奥跳起来，落地时踩中静止的乌龟壳。
三者都应存活。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_shell_stand_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

SHELL_KICK_SCORE = 500

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


async def setup(ws):
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 9})
    await rpc(ws, "game.setPlayerSize", {"size": "small"})
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
    return await si(ws)


def koopa(snap):
    return next((e for e in snap["enemies"] if e["type"] == "koopa"), None)


async def make_shell(ws, x, y):
    """生成一只 koopa，把它踩成静止的壳，再把马里奥移走，避免反弹落回壳上。"""
    await rpc(ws, "game.spawnEnemy", {"type": "koopa", "x": x, "y": y})
    await si(ws, 4)
    s = await si(ws)
    k = koopa(s)
    player_h = s["player"]["height"]
    await rpc(ws, "game.setPlayerPos", {"x": k["x"], "y": k["y"] - player_h - 4.0, "vx": 0.0, "vy": 80.0})
    for _ in range(40):
        s = await si(ws, 2)
        k = koopa(s)
        if k is not None and k["state"] == "shell":
            # 壳化完成的当帧就把马里奥拉走，否则踩踏后的反弹会让他落回壳上，
            # 把刚静止的壳又踢出去，导致测试目标不是“站在静止壳上”。
            await rpc(ws, "game.setPlayerPos", {"x": x - 4 * T, "y": y - 4 * T, "vx": 0.0, "vy": 0.0})
            break
    # 让壳安定下来
    for _ in range(20):
        s = await si(ws, 2)
    return s, koopa(s)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 静止站在乌龟壳正上方")
    s = await setup(ws)
    lives_before = s["lives"]
    s, k = await make_shell(ws, 12 * T, 11 * T)
    if k is None:
        check("壳存在", False, "no koopa")
        return

    score_after_shell = s["score"]
    player_h = s["player"]["height"]
    await rpc(ws, "game.setPlayerPos", {"x": k["x"], "y": k["y"] - player_h, "vx": 0.0, "vy": 0.0})
    for _ in range(60):
        s = await si(ws, 2)
        if s["state"] != "playing":
            break

    check("马里奥站在静止壳上存活",
          s["lives"] == lives_before and s["state"] == "playing",
          f"lives={s['lives']} state={s['state']}")
    check("踢壳得分 +500",
          s["score"] == score_after_shell + SHELL_KICK_SCORE,
          f"{score_after_shell} → {s['score']}")

    section("2. 从左侧走上静止的乌龟壳")
    s = await setup(ws)
    lives_before = s["lives"]
    s, k = await make_shell(ws, 12 * T, 11 * T)
    if k is None:
        check("壳存在", False, "no koopa")
        return

    score_after_shell = s["score"]
    await rpc(ws, "game.setPlayerPos", {"x": k["x"] - 40.0, "y": k["y"] - s['player']['height'], "vx": 120.0, "vy": 0.0})
    for _ in range(60):
        s = await si(ws, 2)
        if s["state"] != "playing":
            break

    check("马里奥走上静止壳后存活",
          s["lives"] == lives_before and s["state"] == "playing",
          f"lives={s['lives']} state={s['state']}")
    # 从侧面走上壳时，先触发踢壳 +500，马里奥被弹起后可能再次落到移动壳上触发踩踏
    # +100，所以这里只要求至少有踢壳分，不钉死恰好 500。
    check("得分至少增加踢壳分",
          s["score"] >= score_after_shell + SHELL_KICK_SCORE,
          f"{score_after_shell} → {s['score']}")

    section("3. 跳起来落地踩中静止的乌龟壳")
    s = await setup(ws)
    lives_before = s["lives"]
    s, k = await make_shell(ws, 12 * T, 11 * T)
    if k is None:
        check("壳存在", False, "no koopa")
        return

    score_after_shell = s["score"]
    player_h = s["player"]["height"]
    # 把马里奥放到壳正上方并给一个向下的速度，模拟落地踩中。
    await rpc(ws, "game.setPlayerPos", {"x": k["x"], "y": k["y"] - player_h - 80.0, "vx": 0.0, "vy": 200.0})
    for _ in range(60):
        s = await si(ws, 2)
        if s["state"] != "playing":
            break

    check("马里奥落回静止壳上存活",
          s["lives"] == lives_before and s["state"] == "playing",
          f"lives={s['lives']} state={s['state']}")
    # 从上方踩中壳走的是踩踏分支，会把壳踢出去并沿踩踏梯子计分（100/200/...），
    # 不一定恰好是踢壳的 500 分，所以只要求分数增加即可。
    check("得分增加",
          s["score"] > score_after_shell,
          f"{score_after_shell} → {s['score']}")

    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
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
