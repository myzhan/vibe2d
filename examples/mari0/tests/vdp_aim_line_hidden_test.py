#!/usr/bin/env python3
"""
mari0 过场期间瞄准激光隐藏验证脚本

原版只在「玩家握有控制权、且没挂在藤蔓上」时画出传送门 UI（点阵轨迹 + 准星）：

    --PORTAL UI STUFF (game.lua:1600-1603)
    if levelfinished == false then
        for pl = 1, players do
            if objects["player"][pl].controlsenabled and objects["player"][pl].t == "portal"
               and objects["player"][pl].vine == false then

而 `controlsenabled` 正是原版「交出控制权」的那面旗，凡是过场都把它放下：

  - 进管 `mario.lua:2887`（出管升完才还回来 `:326`）
  - 旗杆 `:2921`、斧头结局 `:2987`（两段还同时置 `levelfinished`）
  - 过场关的脚本走位 `:201`（`game.lua:2107` 在载入时把它设成 intermission 动画）
  - 藤蔓的两段过场：bonusstage 入场 `:208`、离场上升动画 `:3034`

移植版只按 `state == Playing` 判断，于是马里奥被藏进管子 / 走廊 / 城堡时，
那串激光还挂在屏幕上 —— 这就是本脚本要钉死的 bug。

`vine == false` 是第三个条件，它和 `controlsenabled` 的取法**不同**：爬藤时控制权
还在（`rightkey`/`leftkey` 要求 `controlsenabled and self.vine`，`mario.lua:1603-1622`），
但激光照样不画。所以移植版的镜像条件是 `self.vine.is_none()`，而不是
「爬藤时才画」。另外变大变小的定格只设 `noupdate`、不碰 `controlsenabled`，
那段定格里激光照画是对的，别把它一起藏了。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_aim_line_hidden_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0

# 1-1 "抄近路" 的下行水管口（0-based），与 vdp_pipe_test.py 一致
PIPE_COL = 58
PIPE_ROW = 9

# 城堡关的斧头/桥（0-based），与 vdp_castle_test.py 一致
AXE_COL = 141

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


async def key(ws, action, name):
    await rpc(ws, "engine.simulateInput", {"device": "keyboard", "action": action, "key": name})


async def load(ws, world, level, sublevel=0, pack="smb"):
    await rpc(
        ws,
        "game.setLevel",
        {"pack": pack, "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    return await si(ws, 2)


def laser_off(s):
    return s["aim_line_visible"] is False


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 基线：正常游玩时激光在场（否则下面的断言都没意义）")
    s = await load(ws, 1, 1)
    check("载入 1-1 并开始游玩", s["state"] == "playing" and s["level"]["name"] == "1-1")
    check("正常游玩时 laser 在场", s["aim_line_visible"] is True)
    check("没有管子/旗杆/城堡/藤蔓", s["pipe"] is None and s["flag"] is None
          and s["castle"] is None and s["vine"] is None)

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 进水管：整段（滑入 / 等待 / 换关 / 升起）都不画激光")
    # 站上 1-1 的 (58,9) 管口按下键 —— 和 vdp_pipe_test.py 同一套动作。
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": (PIPE_COL - 0.5) * TILE_SIZE, "y": PIPE_ROW * TILE_SIZE - TILE_SIZE},
    )
    await si(ws, 3)
    await key(ws, "press", "Down")
    s = await si(ws, 6)
    await key(ws, "release", "Down")
    check("进了管子（down）", s["pipe"] == "down", str(s["pipe"]))
    check("进管瞬间激光就没了", laser_off(s), f"aim_line_visible={s['aim_line_visible']}")

    pipe_samples, violations = 0, []
    for _ in range(200):
        s = await si(ws, 2)
        if s["pipe"] is not None:
            pipe_samples += 1
            if not laser_off(s):
                violations.append(f"{s['level']['name']}/{s['pipe']}")
        elif pipe_samples:
            break
    check("管中采到足够多帧", pipe_samples >= 20, f"{pipe_samples} 次采样（每次 2 帧）")
    check("管中每一帧都没有激光", not violations, f"{violations[:4]}{'…' if len(violations) > 4 else ''}")
    # 落地后还有 0.2 秒的「子关黑屏」卡（`SUBLEVELSCREEN_TIME`）：状态是 interlude，
    # 整段游戏世界都不画。等它看完、回到 playing 再断言恢复。
    for _ in range(40):
        if s["state"] == "playing":
            break
        s = await si(ws, 2)
    check(
        "出管落地（1-1_1）后激光恢复",
        s["level"]["name"] == "1-1_1" and s["state"] == "playing" and s["aim_line_visible"] is True,
        f"{s['level']['name']}/{s['state']}, aim={s['aim_line_visible']}",
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section("3. 旗杆结局：从抓杆到烟花整段都不画激光")
    s = await load(ws, 1, 1)
    await rpc(ws, "game.clearEnemies")
    s = await si(ws)
    flag_col = s["level"]["flag_x"] / TILE_SIZE
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": (flag_col - 1) * TILE_SIZE, "y": 9 * TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    await rpc(ws, "game.setTime", {"time": 43})
    # 放上去的那一帧就抓住旗杆（见 vdp_flagpole_test.py 的 grab_pole）。
    s = await si(ws, 2)
    f = s["flag"]
    check("抓上了旗杆", f is not None and f["phase"] == "slide", str(f and f["phase"]))
    check("抓杆瞬间激光就没了", laser_off(s), f"aim_line_visible={s['aim_line_visible']}")

    flag_samples, phases, violations = 0, [], []
    for _ in range(800):
        s = await si(ws, 2)
        f = s["flag"]
        if f is None:
            break
        flag_samples += 1
        phases.append(f["phase"])
        if not laser_off(s):
            violations.append(f["phase"])
    check("六拍都采到了", len(set(phases)) >= 5, f"{sorted(set(phases))}")
    check("旗杆整段没有激光", not violations, f"{sorted(set(violations))}")
    check(
        "整段结束进 level_complete，仍然没有激光",
        s["state"] == "level_complete" and laser_off(s),
        f"{s['state']}, aim={s['aim_line_visible']}",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 斧头结局：从链子消失到马里奥跑进城堡整段都不画激光")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 4})
    await rpc(ws, "game.setState", {"state": "playing"})
    await si(ws)
    await rpc(ws, "game.setPlayerPos", {"x": 136 * TILE_SIZE, "y": 8 * TILE_SIZE})
    await si(ws, 25)
    await rpc(ws, "game.setStar", {"seconds": 999})
    s = await si(ws, 2)
    check("还没拿到斧头", s["castle"] is None, str(s["castle"]))
    check("在此之前激光在场", s["aim_line_visible"] is True)

    await rpc(ws, "game.setPlayerPos", {"x": AXE_COL * TILE_SIZE, "y": 7 * TILE_SIZE})
    s = await si(ws, 2)
    c = s["castle"]
    check("城堡序列开始（chain）", c is not None and c["phase"] == "chain", str(c and c["phase"]))
    check("拿斧头瞬间激光就没了", laser_off(s), f"aim_line_visible={s['aim_line_visible']}")

    castle_samples, violations = 0, []
    for _ in range(400):
        s = await si(ws, 4)
        c = s["castle"]
        if c is None:
            break
        castle_samples += 1
        if not laser_off(s):
            violations.append(c["phase"])
    check("序列中采到足够多帧", castle_samples >= 20, f"{castle_samples} 次采样（每次 4 帧）")
    check("斧头结局整段没有激光", not violations, f"{sorted(set(violations))}")
    # 城堡结局自己换关，2-1 之前还有它的「世界 2-1」黑屏卡（`LEVELSCREEN_TIME`，
    # 世界/关卡起始关加长 1.5 倍），状态同样是 interlude。等卡看完再断言恢复。
    for _ in range(150):
        s = await si(ws, 8)
        if s["level"]["name"] == "2-1" and s["state"] == "playing":
            break
    check(
        "结束后进 2-1，激光恢复",
        s["level"]["name"] == "2-1" and s["state"] == "playing" and s["aim_line_visible"] is True,
        f"{s['level']['name']}/{s['state']}, aim={s['aim_line_visible']}",
    )

    # ── 5 ───────────────────────────────────────────────────────────
    section("5. 过场关 1-2：脚本走位（controlsenabled=false）全程不画激光")
    s = await load(ws, 1, 2)
    check("1-2 是过场关", s["level"]["intermission"] is True, str(s["level"].get("intermission")))
    check("开始就在被脚本推着走", s["player"]["vx"] > 0.0, f"vx={s['player']['vx']:.1f}")
    check("走位期间没有激光", laser_off(s), f"aim_line_visible={s['aim_line_visible']}")

    walk_samples, violations = 0, []
    for _ in range(120):
        s = await si(ws, 4)
        if s["level"]["name"] != "1-2":
            break
        walk_samples += 1
        if not laser_off(s):
            violations.append(f"第 {s['player']['x'] / TILE_SIZE:.1f} 列")
    check("采到足够多帧", walk_samples >= 10, f"{walk_samples} 次采样（每次 4 帧）")
    check("脚本走位整段没有激光", not violations, f"{violations[:4]}")

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. bonusstage 藤蔓入场：整段攀爬动画不画激光")
    # 2-1 的藤蔓（砖块 (83,5)）通往子关 2-1_1，它一载入就是 vinestart 攀爬入场
    # （`game.lua:2139-2141`）。直接载入它，省掉顶砖和爬藤两段前戏。
    s = await load(ws, 2, 1, sublevel=1)
    check("2-1_1 是 bonusstage", s["bonusstage"] is True, str(s["bonusstage"]))
    v = s["vine"]
    check(
        "入场是脚本攀爬（has_control=false）",
        v is not None and v["phase"] == "intro" and v["has_control"] is False,
        str(v),
    )
    check("入场瞬间没有激光", laser_off(s), f"aim_line_visible={s['aim_line_visible']}")

    intro_samples, violations = 0, []
    for _ in range(250):
        s = await si(ws, 4)
        if s["vine"] is None:
            break
        intro_samples += 1
        if not laser_off(s):
            violations.append(s["vine"]["phase"])
    check("入场中采到足够多帧", intro_samples >= 20, f"{intro_samples} 次采样（每次 4 帧）")
    check("入场整段没有激光", not violations, f"{sorted(set(violations))}")
    check(
        "爬完落到房间里，激光恢复",
        s["vine"] is None and s["aim_line_visible"] is True,
        f"vine={s['vine']}, aim={s['aim_line_visible']}",
    )

    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 过场期间瞄准激光隐藏验证")
    print("=" * 60)
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except (OSError, websockets.exceptions.WebSocketException) as exc:
        print(f"\n无法连接 {WS_URL}: {exc}")
        print("先启动: cargo run -p mari0 --features vdp")
        return 2

    print()
    if FAILURES:
        print(f"✗ {len(FAILURES)} 项失败:")
        for f in FAILURES:
            print(f"    - {f}")
        return 1
    print("✓ 全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
