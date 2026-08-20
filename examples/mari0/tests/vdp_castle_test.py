#!/usr/bin/env python3
"""
mari0 斧头结局验证脚本（城堡关的第二条过关线）

八个城堡关（1-4 … 7-4、8-4_4）都不是旗杆过关，而是斧头。数据布局完全一致：
`firestart` 第 93 列、bowser 第 128 列、**axe 第 141 列**，桥是第 10 行一排 13 个
tile 11，右端第 9 列上方挂着**唯一一个** tile 10（链子）。

序列（`mario.lua:476-560`），全部挂在**一个**计时器上，而这个计时器**只被重置一次**
（Bowser 开始坠落时，`:517`）—— 所以后面的节拍是从**坠落**算起，不是从斧头算起。
这就是 `castleanimationmariomove = 1.07` 看起来那么小的原因：光是桥塌就要
0.38 + 13×0.06 = 1.16 秒，比 1.07 还长。少了这次重置，马里奥会在桥还在塌的时候
就被放开，然后跑到正在消失的 tile 上。

  1. 拿到斧头：传送门清空、**所有平台被删掉**、控制权交出、速度归零。
  2. `castleanimationchaindisappear = 0.38` 后链子消失，桥开始塌。
  3. 每 `0.06` 秒从右往左消失一个 tile 11（顺带清掉它上面的 tile 10）。
  4. 桥塌完 → Bowser 坠落，**重力 27.5**（他自己的是 10.9，将近三倍）。
  5. 坠落后 `1.07` 秒放开马里奥，他以 4.27 格/秒往右跑（**没有输入**，
     原版整段 `controlsenabled = false`）。
  6. 停在 `mapwidth - 8` 格处（蘑菇头站的地方）。
  7. `castleanimationnextlevel = 9.47`（同样从坠落算起）后进下一关。

**一个真 bug 是这次做出来的**：Bowser 在原版里**没有** `autodelete`，而移植的通用
"滚出屏幕左边 200px 就删"规则把他删了 —— 他在第 121-128 列踱步，斧头在 141 列，
玩家走到斧头时镜头早把他甩出剔除线了。于是桥塌下去底下没人，**整个斧头结局在正常
游玩里根本走不到**。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_castle_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:346-353
CHAIN_DISAPPEAR = 0.38
BRIDGE_DELAY = 0.06
MARIO_MOVE = 1.07
MARIO_SPEED = 4.27
STOP_FROM_END = 8
NEXT_LEVEL = 9.47

AXE_COL = 141
BRIDGE_ROW = 10
CHAIN_ROW = 9
BRIDGE_TILE = 11
CHAIN_TILE = 10

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


async def si(ws, frames=1):
    return await rpc(ws, "engine.stepAndInspect", {"frames": frames})


def check(label, ok, detail=""):
    print(f"    {'OK  ' if ok else 'FAIL'} {label}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(label)


def section(title):
    print(f"\n─── {title} ───")


def bowser(s):
    return [e for e in s["enemies"] if e["type"] == "bowser"]


async def at_the_axe(ws, world=1):
    """Load a castle, walk up to Bowser, then stand on the pillar the axe sits on.

    Row 7, not row 9: columns 141-143 are a **solid pillar** from row 9 down, and the
    axe stands on top of it. Dropping the player at row 9 wedges him inside the stone
    and he never moves, which reads as the sequence being broken.
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": world, "level": 4})
    await si(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 136 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await si(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    return await si(ws, 2)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. Bowser 必须活到玩家走到斧头 —— 他没有 autodelete")
    s = await at_the_axe(ws)
    check(
        "走到斧头前 Bowser 还在场（通用剔除规则会把他删掉）",
        bool(bowser(s)),
        f"bowser={[round(e['x'] / TILE_SIZE, 1) for e in bowser(s)]}",
    )
    # "他在剔除线之外"这一条**不能在这里量** —— 这时玩家还没走到斧头，镜头也还没把他
    # 甩出去。真正越线是在玩家到了第 141 列之后（镜头 ~135.7、剔除线 ~129.4，
    # Bowser 在 ~128.8）。所以那条断言放到第 5 节坠落时去做。

    section("2. 拿斧头：控制权交出、平台清空、序列从 chain 开始")
    await rpc(ws, "game.setPlayerPos", {"x": AXE_COL * TILE_SIZE, "y": 7 * TILE_SIZE})
    s = await si(ws, 2)
    c = s["castle"]
    check("序列开始了", c is not None)
    if not c:
        return
    check("第一拍是 chain", c["phase"] == "chain", f"{c['phase']}")
    check("平台被清空", not s["platforms"], f"{len(s['platforms'])} 个")
    check("传送门被清掉", s["portals"]["blue"] is None and s["portals"]["orange"] is None)
    check(
        "扫除起点在斧头左边一格、下面两行",
        c["bridge"] == [AXE_COL - 1, BRIDGE_ROW],
        f"{c['bridge']}",
    )
    # **这一刻就是那条豁免承重的地方。** 玩家到斧头（141 列）时镜头到 ~135.7，
    # 剔除线在 ~129.4，而 Bowser 还在 ~128.8 —— 通用规则正好在此刻删掉他。
    # 撑过这一刻之后他自己会往右踱回窗口里，所以之后再量就量不到了；
    # 第一次做这个功能时就是在这里丢了 Bowser，桥塌下去底下没人。
    if bowser(s):
        bx = bowser(s)[0]["x"] / TILE_SIZE
        cull = s["camera_x"] / TILE_SIZE - 200.0 / TILE_SIZE
        check(
            "拿到斧头的这一刻，Bowser 在剔除线之外但还活着",
            bx < cull,
            f"bowser 在 {bx:.1f} 列，剔除线在 {cull:.1f} 列",
        )
    else:
        check("拿到斧头的这一刻 Bowser 还活着", False, "他被剔除规则删掉了")

    section(f"3. {CHAIN_DISAPPEAR} 秒后链子消失，桥开始从右往左塌")
    phases, bridges = [], []
    for _ in range(60):
        s = await si(ws, 3)
        c = s["castle"]
        if not c:
            break
        phases.append((c["phase"], c["timer"]))
        bridges.append(tuple(c["bridge"]))
        if c["phase"] not in ("chain", "bridge"):
            break
    chain_end = max((t for p, t in phases if p == "chain"), default=None)
    check(
        f"chain 拍持续约 {CHAIN_DISAPPEAR} 秒",
        chain_end is not None and abs(chain_end - CHAIN_DISAPPEAR) < 0.08,
        f"最后一次采到 chain 时 timer={chain_end}",
    )
    cols = [b[0] for b in bridges]
    check(
        "扫除是往左走的（列号单调不增）",
        all(b >= a for a, b in zip(cols[1:], cols)),
        f"{cols[:6]}…{cols[-3:]}",
    )
    check(
        "一直扫到桥的左端（1-4 的桥是第 128..140 列）",
        min(cols) <= 128,
        f"最左扫到 {min(cols)}",
    )

    section("4. 桥和链子的 tile 真的被清掉了")
    # 桥塌完之后整排都该是空的，链子那一格也是。
    for _ in range(40):
        s = await si(ws, 4)
        if not s["castle"] or s["castle"]["phase"] != "bridge":
            break
    # 没有读 tile 的 VDP 方法，所以用可见后果代替：桥扫到尽头这件事本身就是
    # "下一格不是 tile 11" 判出来的，而它的效果就是 Bowser 开始坠落。
    check("桥塌完就进入了 Bowser 坠落", s["castle"] and s["castle"]["phase"] in ("bowser_falls", "mario_runs"),
          f"{s['castle']['phase'] if s['castle'] else None}")

    section("5. Bowser 以将近三倍的重力坠入岩浆")
    ys, vys = [], []
    for _ in range(40):
        s = await si(ws, 2)
        for e in bowser(s):
            ys.append(e["y"] / TILE_SIZE)
            vys.append(e["vy"] / TILE_SIZE)
        if not bowser(s) and ys:
            break
    check("他往下掉了", len(ys) > 2 and ys[-1] > ys[0], f"y {ys[0]:.1f} → {ys[-1]:.1f}" if ys else "没采到")
    check("速度一路增加（不是匀速）", len(vys) > 2 and vys[-1] > vys[0] + 5.0,
          f"vy {vys[0]:.1f} → {vys[-1]:.1f}" if vys else "")
    check("最后掉出世界（进了岩浆）", not bowser(s))


    section(f"6. 坠落后 {MARIO_MOVE} 秒放开马里奥，他自己跑到 mapwidth-{STOP_FROM_END}")
    ran = None
    for _ in range(60):
        s = await si(ws, 4)
        c = s["castle"]
        if not c:
            break
        if c["phase"] == "mario_runs":
            ran = s
            break
    check("进入了 mario_runs", ran is not None)
    if ran:
        # 他从柱子上跑下来，落到城堡地板上。
        seen_speed = []
        stop = None
        for _ in range(80):
            s = await si(ws, 4)
            c = s["castle"]
            if not c:
                break
            seen_speed.append(s["player"]["vx"] / TILE_SIZE)
            if c["phase"] == "done":
                stop = s["player"]["x"] / TILE_SIZE
                break
        check(
            f"跑动速度是 {MARIO_SPEED} 格/秒",
            seen_speed and max(seen_speed) > MARIO_SPEED - 0.3,
            f"最快 {max(seen_speed):.2f}" if seen_speed else "",
        )
        width = s["level"]["width"]
        check(
            f"停在第 {width - STOP_FROM_END} 列（mapwidth - {STOP_FROM_END}）",
            stop is not None and abs(stop - (width - STOP_FROM_END)) < 0.1,
            f"停在 {stop}" if stop else "没停下",
        )

    section("7. 整段结束后进下一关")
    advanced = None
    for _ in range(90):
        s = await si(ws, 8)
        if s["level"]["name"] != "1-4":
            advanced = s["level"]["name"]
            break
    check("进到了 2-1", advanced == "2-1", f"{advanced}")
    check("序列状态被清掉了", s["castle"] is None)

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
