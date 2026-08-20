#!/usr/bin/env python3
"""
mari0 旗杆结局验证脚本

抓住旗杆不是「走到那一列就结算」，而是把整关交给一段**六拍的过场**，全程没有输入。
这和斧头结局（`castle.rs`）是全游戏唯一两种通关方式。

六拍（`mario.lua:359-473`，全部由**同一个从不重置**的计时器驱动）：
  1. `slide`   抓着杆往下滑 `flagdescendtime = 0.9` 秒，旗子在同一段时间里降下同样的
               距离 —— 所以看起来是他把旗拽下来的。攀爬帧按 0.07 秒交替（藤蔓的两倍快）。
  2. `hang`    滑到底挂着 `flaganimationdelay = 0.6` 秒，人绕到杆的另一侧。
  3. `run`     放开，以**定速** 4.27 格/秒（不是他自己的最高速）跑向城堡，
               跑到旗杆右边 6 格进门消失。
  4. `countdown` 剩余时间按每帧 1 点换 50 分 —— 是**逐格滚**而不是一次结算，那个滚动
               本身就是奖励（scorering 音效）。
  5. `castle_flag` 城堡自己的旗升起 1.5 格。但有个下限 `castlemintime = 7`：
               整段不到 7 秒旗子不动，所以时间烧光的那一关也有同样的节奏。
  6. `fireworks` 每 0.55 秒一发，共 `fireworkcount` 发，最后一发后再 2 秒进下一关。

分数有个坑：`mario.lua:462` 的注释写「500 points per firework」，但**真正加分的代码**
`firework.lua:7` 是 `marioscore + 200`。端口原先信了注释。NES 原版确实是 500，
但这里对标的是 Mari0，Mari0 给 200。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_flagpole_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
DT = 1.0 / 60.0

# variables.lua:322-333
FLAG_DESCEND_TIME = 0.9
FLAG_ANIM_DELAY = 0.6
FLAG_RUN_SPEED = 4.27
FLAG_CASTLE_DIST = 6.0
CASTLE_MIN_TIME = 7.0
FIREWORK_DELAY = 0.55
FLAG_END_TIME = 2.0
FIREWORK_SCORE = 200  # firework.lua:7 —— 不是注释里的 500
CASTLE_FLAG_START_Y = 1.5

PHASES = ["slide", "hang", "run", "countdown", "castle_flag", "fireworks"]

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


async def grab_pole(ws, clock=43, row=9):
    """装载 1-1、把人直接放到触发线上、设好时钟，返回抓杆前那一帧。

    触发线就是「人的右边缘碰到旗杆列」。直接放上去而不是跑过去，是因为跑过去的这几十帧
    会把时钟走掉一两点 —— 而烟花数量取的是**抓杆瞬间**时钟的末位数字，差一点就全没了。
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await rpc(ws, "game.setState", {"state": "playing"})
    await si(ws)
    await rpc(ws, "game.clearEnemies")
    s = await si(ws)
    flag_col = s["level"]["flag_x"] / TILE_SIZE
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": (flag_col - 1) * TILE_SIZE, "y": row * TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    await rpc(ws, "game.setTime", {"time": clock})
    # 故意**不**再 step：放上去的那一帧就会触发，调用方需要先看到抓杆前的样子。
    return await rpc(ws, "game.inspect"), flag_col


async def walk_sequence(ws, max_steps=800):
    """跑完整段，返回 {阶段: 首次进入时的快照} 和最后一帧。"""
    first = {}
    last = None
    for _ in range(max_steps):
        s = await si(ws, 2)
        last = s
        f = s["flag"]
        if f and f["phase"] not in first:
            first[f["phase"]] = (f, s)
        if s["state"] == "level_complete":
            break
    return first, last


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 抓杆：交出控制权，人被吸到杆上，按高度记分")
    s, flag_col = await grab_pole(ws)
    check("抓杆前还没有序列", s["flag"] is None, f"flag={s['flag']}")
    score0 = s["score"]
    s = await si(ws, 2)
    f = s["flag"]
    check("抓上了", f is not None and f["phase"] == "slide", str(f and f["phase"]))
    if not f:
        return
    check("是攀爬姿势", s["player"]["anim_state"] == "climb", s["player"]["anim_state"])
    check(
        "人被吸到杆上（贴着杆的近侧）",
        abs(s["player"]["x"] / TILE_SIZE - (flag_col - 2.0 / 16.0)) < 0.01,
        f"x={s['player']['x'] / TILE_SIZE:.4f} 格，杆在 {flag_col:.0f}",
    )
    check("按抓杆高度记了分", s["score"] > score0, f"{score0} → {s['score']}")
    check("城堡旗从低位开始（1.5 格）",
          abs(f["castle_flag_y"] / TILE_SIZE - CASTLE_FLAG_START_Y) < 0.01,
          f"{f['castle_flag_y'] / TILE_SIZE:.2f} 格")

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 六拍按顺序走完，时间点都对得上")
    first, last = await walk_sequence(ws)
    check("六拍一个都不少", list(first.keys()) == PHASES, f"{list(first.keys())}")
    if list(first.keys()) != PHASES:
        return
    t = {p: first[p][0]["timer"] for p in PHASES}
    check(
        f"slide → hang 用了 {FLAG_DESCEND_TIME} 秒",
        abs(t["hang"] - FLAG_DESCEND_TIME) < 0.05,
        f"实测 {t['hang']:.3f}",
    )
    check(
        f"hang → run 再等 {FLAG_ANIM_DELAY} 秒",
        abs(t["run"] - (FLAG_DESCEND_TIME + FLAG_ANIM_DELAY)) < 0.05,
        f"实测 {t['run']:.3f}",
    )
    check(
        f"城堡升旗不早于第 {CASTLE_MIN_TIME} 秒（整段的硬下限）",
        t["fireworks"] >= CASTLE_MIN_TIME,
        f"烟花在 t={t['fireworks']:.2f} 开始",
    )
    check("最后进了 level_complete", last["state"] == "level_complete", last["state"])

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 滑杆：旗子和人同步下降，滑完人绕到杆的另一侧")
    slide_f, slide_s = first["slide"]
    hang_f, hang_s = first["hang"]
    check(
        "旗子跟着降下来了",
        hang_f["flag_y"] > slide_f["flag_y"],
        f"flag_y {slide_f['flag_y']:.1f} → {hang_f['flag_y']:.1f}",
    )
    check(
        "人也降下来了",
        hang_s["player"]["y"] > slide_s["player"]["y"],
        f"y {slide_s['player']['y'] / TILE_SIZE:.2f} → {hang_s['player']['y'] / TILE_SIZE:.2f} 格",
    )
    check(
        "滑完绕到了杆的另一侧（往右挪了）",
        hang_s["player"]["x"] > slide_s["player"]["x"],
        f"x {slide_s['player']['x'] / TILE_SIZE:.4f} → {hang_s['player']['x'] / TILE_SIZE:.4f} 格",
    )
    check("绕过去以后朝右（准备跑）", hang_s["player"]["facing_right"])

    # ── 4 ───────────────────────────────────────────────────────────
    section(f"4. 定速 {FLAG_RUN_SPEED} 格/秒跑向城堡，进门就不画了")
    run_f, run_s = first["run"]
    check("跑步姿势", run_s["player"]["anim_state"] == "run", run_s["player"]["anim_state"])
    cd_f, cd_s = first["countdown"]
    check(
        f"进门点是旗杆右边 {FLAG_CASTLE_DIST:.0f} 格",
        cd_s["player"]["x"] / TILE_SIZE >= flag_col + FLAG_CASTLE_DIST - 0.5,
        f"进门时 x={cd_s['player']['x'] / TILE_SIZE:.2f} 格，杆在 {flag_col:.0f}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. 时间换分：逐格滚，每点 50 分，滚到 0")
    check(
        "进城堡时时钟还没动",
        abs(cd_s["time_remaining"] - 43.0) < 0.6,
        f"time={cd_s['time_remaining']:.1f}",
    )
    cf_f, cf_s = first["castle_flag"]
    check("到城堡升旗那一拍时钟已经归零", cf_s["time_remaining"] == 0.0,
          f"time={cf_s['time_remaining']}")
    gained = cf_s["score"] - cd_s["score"]
    check(
        "换来的分 = 时钟点数 × 50",
        abs(gained - 43 * 50) <= 50,
        f"时钟 43 点换了 {gained} 分（期望 {43 * 50}）",
    )

    # ── 6 ───────────────────────────────────────────────────────────
    section(f"6. 烟花：时钟末位是 3 就放 3 发，每发 {FIREWORK_SCORE} 分（不是注释说的 500）")
    check("抓杆时算出 3 发", cf_f["total"] == 3, f"total={cf_f['total']}")
    check("最后确实放完了", last["fireworks"] == 3, f"{last['fireworks']}")
    fw_gain = last["score"] - cf_s["score"]
    check(
        f"3 发烟花 = {3 * FIREWORK_SCORE} 分",
        fw_gain == 3 * FIREWORK_SCORE,
        f"实测 {fw_gain} 分（若是 500/发 会是 1500）",
    )
    cfg = first["fireworks"][0]["castle_flag_y"]
    check("放烟花时城堡旗已经升到顶", abs(cfg) < 0.01, f"castle_flag_y={cfg:.2f}")

    # ── 7 ───────────────────────────────────────────────────────────
    section("7. 时钟末位不是 1/3/6 就一发都没有，序列照样走完")
    await grab_pole(ws, clock=42)
    first2, last2 = await walk_sequence(ws)
    check("六拍照样走完", list(first2.keys()) == PHASES, f"{list(first2.keys())}")
    check("0 发烟花", last2["fireworks"] == 0, f"{last2['fireworks']}")
    check("也照样进了 level_complete", last2["state"] == "level_complete", last2["state"])

    # ── 8 ───────────────────────────────────────────────────────────
    section("8. 时钟只剩 1 点：换分一瞬间就完，但 castlemintime 让节奏不变")
    # 不能测「时钟正好 0」：那在真实游戏里等于已经超时死了（`tick_clock` 会直接判死），
    # 根本走不到抓杆。剩 1 点是能真实发生的最短情况，而且末位 1 刚好给 1 发烟花。
    await grab_pole(ws, clock=1)
    first3, last3 = await walk_sequence(ws)
    check("六拍还是齐的", list(first3.keys()) == PHASES, f"{list(first3.keys())}")
    check("只放 1 发烟花", last3["fireworks"] == 1, f"{last3['fireworks']}")
    if "fireworks" in first3:
        check(
            f"而且照样不早于第 {CASTLE_MIN_TIME} 秒 —— 这就是 castlemintime 的意义",
            first3["fireworks"][0]["timer"] >= CASTLE_MIN_TIME,
            f"t={first3['fireworks'][0]['timer']:.2f}",
        )
    check("进了 level_complete", last3["state"] == "level_complete", last3["state"])

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
