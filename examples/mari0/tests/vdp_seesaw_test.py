#!/usr/bin/env python3
"""
mari0 跷跷板验证脚本

实体 80，全游戏 9 座，分布在 3-3（类型 1、2）、4-3（3、4、5、6）、6-3（7、8、9），
**九座正好把 `seesawtype` 的九组参数各用一次**，而且全都在第 2 行。也就是说
参数表里任何一格写错，都只能靠那一关看出来 —— 别的关不会重复用到。

规则（`seesaw.lua` + `seesawplatform.lua`）：
  - 一座 = 一根横梁 + 两端各一个滑轮 + 各吊一块板，两块板**共用一根绳**：
    `seesawtype[t] = {range, dist1, dist2, size}`，两块板离横梁的距离之和永远等于
    `dist1 + dist2 - 2 - 2/16`。板子自己也比普通平台低 2/16 格（17/16 对 15/16）。
  - 速度是**累加的**，不是杠杆。站一边每秒给这边加 `seesawspeed = 4` 格/秒，
    对面减同样多，而且**没有上限** —— 站着不动它会越来越快，直到绳走完。
  - `seesawfriction = 4` 只在「当前重量不支持当前方向」时才生效，而且数值和
    seesawspeed 一样大，所以下来的瞬间正好抵掉自己刚才的拉力，是滑停不是回弹。
  - 绳走完时看**对面有没有人**：没人就两块板各自钉在绳两头停住；有人就整座塌 ——
    两块板速度先清零，再以 `seesawgravity = 30` 格/秒² 往下掉，是拉力的七倍多。
    塌的是**对面那块**，也就是踩着的人脚下那块。
  - 判定踩没踩上用 ±0.1 格，而且跳跃中的人不算重量也不被带着走。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_seesaw_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
DT = 1.0 / 60.0

# variables.lua:136-138
SEESAW_SPEED = 4.0
SEESAW_GRAVITY = 30.0
SEESAW_FRICTION = 4.0
# seesaw.lua:4-13 — (range, dist1, dist2, size)
SEESAW_TYPES = {
    1: (7, 4, 6, 3.0),
    2: (4, 2, 6, 3.0),
    3: (7, 3, 6, 3.0),
    4: (8, 3, 7, 3.0),
    5: (5, 3, 7, 3.0),
    6: (6, 3, 7, 3.0),
    7: (4, 3, 7, 1.5),
    8: (3, 3, 7, 1.5),
    9: (3, 4, 7, 1.5),
}
# 每关有哪几座，(列, 类型)
LEVELS = {
    (3, 3, 0): [(82, 1), (137, 2)],
    (4, 3, 0): [(49, 3), (81, 4), (92, 5), (103, 6)],
    (6, 3, 0): [(71, 7), (79, 8), (127, 9)],
}

HOLD_JUMP = [{"device": "keyboard", "action": "press", "key": "Space"}]
FREE_ALL = [
    {"device": "keyboard", "action": "release", "key": k}
    for k in ("Space", "Up", "Down", "Left", "Right")
]

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


async def setup(ws, world, level, sublevel=0):
    """装载关卡、清场、按回 playing。"""
    await rpc(
        ws,
        "game.setLevel",
        {"pack": "smb", "world": world, "level": level, "sublevel": sublevel},
    )
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    return await si(ws)


async def stand_on(ws, world, level, index, side):
    """重载关卡，再把人放到某座跷跷板的某块板上，返回站上去那一帧。

    **必须重载**。跷跷板的速度是累加的而且永不复位，塌了更是不可逆 —— 上一小节骑过
    的那座会带着几百 px/s 的速度（甚至已经塌掉的状态）进到这一节，量出来的东西就全
    是上一节的尾巴了。
    """
    await si(ws, 1, FREE_ALL)
    s = await setup(ws, world, level)
    p = s["seesaws"][index][side]
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": p["x"] + 8.0, "y": p["y"] - TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    await rpc(ws, "game.clearEnemies")
    return await si(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section("1. 九座跷跷板，九种类型各用一次，全在第 2 行")
    seen = []
    for (world, level, sub), expected in LEVELS.items():
        name = f"{world}-{level}"
        s = await setup(ws, world, level, sub)
        got = sorted((ss["col"], ss["kind"]) for ss in s["seesaws"])
        check(f"{name} 有 {len(expected)} 座，列与类型对得上", got == expected, f"{got}")
        check(
            f"{name} 全在第 2 行",
            all(ss["row"] == 2 for ss in s["seesaws"]),
            f"{[ss['row'] for ss in s['seesaws']]}",
        )
        seen += [k for _, k in got]
        # 几何：绳长、板宽、滑轮间距
        for ss in s["seesaws"]:
            rng, d1, d2, size = SEESAW_TYPES[ss["kind"]]
            ok_rope = abs(ss["rope"] - ((d1 + d2) * TILE_SIZE - (2 + 2 / 16) * TILE_SIZE)) < 0.01
            ok_w = abs(ss["left"]["w"] - size * TILE_SIZE) < 0.01
            gap = (ss["right"]["x"] + ss["right"]["w"] / 2) - (
                ss["left"]["x"] + ss["left"]["w"] / 2
            )
            ok_gap = abs(gap - rng * TILE_SIZE) < 0.01
            summed = ss["left"]["drop"] + ss["right"]["drop"]
            ok_sum = abs(summed - ss["rope"]) < 0.01
            check(
                f"  类型 {ss['kind']}: 绳长 {d1}+{d2}-2⅛ 格、板宽 {size} 格、滑轮间距 {rng} 格",
                ok_rope and ok_w and ok_gap,
                f"rope={ss['rope']:.1f} w={ss['left']['w']:.0f} gap={gap:.0f}",
            )
            check(
                f"  类型 {ss['kind']}: 两块板的下垂之和 = 绳长（这才叫一根绳）",
                ok_sum,
                f"{summed:.2f} vs {ss['rope']:.2f}",
            )
    check("九种类型不重不漏", sorted(seen) == list(range(1, 10)), f"{sorted(seen)}")
    check("6-3 那三座是 1.5 格窄板", all(SEESAW_TYPES[k][3] == 1.5 for k in (7, 8, 9)))

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 站上去：这边往下，对面往上，速度大小相等方向相反")
    s = await stand_on(ws, 3, 3, 0, "left")
    check("被算成一个重量", s["seesaws"][0]["left"]["riders"] == 1,
          f"riders={s['seesaws'][0]['left']['riders']}")
    s = await si(ws, 20)
    a = s["seesaws"][0]
    check("踩的那边在往下", a["left"]["vy"] > 0, f"vy={a['left']['vy']:.1f}")
    check("对面在往上", a["right"]["vy"] < 0, f"vy={a['right']['vy']:.1f}")
    check(
        "两边速度大小相等方向相反",
        abs(a["left"]["vy"] + a["right"]["vy"]) < 0.01,
        f"{a['left']['vy']:.2f} / {a['right']['vy']:.2f}",
    )
    check(
        "两块板的下垂之和一直等于绳长",
        abs(a["left"]["drop"] + a["right"]["drop"] - a["rope"]) < 0.5,
        f"{a['left']['drop'] + a['right']['drop']:.2f} vs {a['rope']:.2f}",
    )
    check("人被板带着一起下去，而且算站着（能起跳）", s["player"]["on_ground"])

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. 速度是累加的：每秒 +{SEESAW_SPEED} 格/秒，而且没有上限")
    v0 = a["left"]["vy"]
    s = await si(ws, 30)
    v1 = s["seesaws"][0]["left"]["vy"]
    rate = (v1 - v0) / TILE_SIZE / (30 * DT)
    check(
        f"加速度约 {SEESAW_SPEED} 格/秒²",
        abs(rate - SEESAW_SPEED) < 0.1,
        f"实测 {rate:.3f} 格/秒²",
    )
    s = await si(ws, 30)
    v2 = s["seesaws"][0]["left"]["vy"]
    check(
        "还在加速（没有终端速度，只有绳到头）",
        v2 > v1 > v0,
        f"{v0:.0f} → {v1:.0f} → {v2:.0f} px/s",
    )

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 跳跃中的人不算重量，也不被板带着走")
    s = await stand_on(ws, 3, 3, 0, "left")
    s = await si(ws, 4, HOLD_JUMP)
    check("起跳后不再算重量", s["seesaws"][0]["left"]["riders"] == 0,
          f"riders={s['seesaws'][0]['left']['riders']}")
    check("人确实离开了板（在往上）", s["player"]["vy"] < 0, f"vy={s['player']['vy']:.0f}")
    await si(ws, 1, FREE_ALL)

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. 下来以后摩擦力（{SEESAW_FRICTION}）把速度滑停到**正好 0**，不回弹")
    s = await stand_on(ws, 3, 3, 0, "left")
    await si(ws, 40)
    s = await si(ws)
    moving = s["seesaws"][0]["left"]["vy"]
    check("先让它跑起来", moving > 0, f"vy={moving:.0f}")
    # 把人挪到 3-3 的**另一座**跷跷板上待着。跷跷板都吊在坑上，随便找块地放人会掉下去
    # 摔死，而人一死 `update_playing` 就整个停了 —— 那样速度会**冻结**在半路上，
    # 看起来就像摩擦力停在了非零值。停到另一座板上既安全又互不干扰。
    other = s["seesaws"][1]["left"]
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": other["x"] + 8.0, "y": other["y"] - TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    await rpc(ws, "game.clearEnemies")
    settled = None
    for _ in range(60):
        s = await si(ws, 5)
        a = s["seesaws"][0]
        if a["left"]["vy"] == 0.0 and a["right"]["vy"] == 0.0:
            settled = a
            break
        if a["falloff"] is not None:
            break
    check("两边都停在正好 0（不是接近 0）", settled is not None,
          f"L={a['left']['vy']:.4f} R={a['right']['vy']:.4f} falloff={a['falloff']}")
    if settled:
        check(
            "停下来以后绳长关系还在",
            abs(settled["left"]["drop"] + settled["right"]["drop"] - settled["rope"]) < 0.5,
            f"{settled['left']['drop'] + settled['right']['drop']:.2f} vs {settled['rope']:.2f}",
        )

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 一路踩到绳到头：塌的是脚下那块，先清零再以 30 格/秒² 掉")
    s = await stand_on(ws, 3, 3, 0, "left")
    fell = None
    for _ in range(200):
        s = await si(ws, 2)
        if s["seesaws"][0]["falloff"] is not None:
            fell = s
            break
    check("整座塌了", fell is not None)
    if fell:
        a = fell["seesaws"][0]
        check(
            "塌的是踩着的那一边（脚下那块掉走）",
            a["falloff"] == "left",
            f"falloff={a['falloff']}",
        )
        check(
            "对面那块正好被拉到横梁上",
            abs(a["right"]["drop"]) < 4.0,
            f"drop={a['right']['drop']:.2f}",
        )
        # 塌了以后两块板一起以 seesawgravity 掉
        v0 = a["left"]["vy"]
        s = await si(ws, 20)
        b = s["seesaws"][0]
        g = (b["left"]["vy"] - v0) / TILE_SIZE / (20 * DT)
        check(
            f"以 {SEESAW_GRAVITY} 格/秒² 往下掉（拉力的七倍多）",
            abs(g - SEESAW_GRAVITY) < 0.5,
            f"实测 {g:.2f} 格/秒²",
        )
        check(
            "两块板一起掉，不是只掉一块",
            abs(b["left"]["vy"] - b["right"]["vy"]) < 0.01,
            f"L={b['left']['vy']:.1f} R={b['right']['vy']:.1f}",
        )
        check(
            "塌了以后重量不再起作用（只有重力）",
            b["left"]["vy"] > 0 and b["right"]["vy"] > 0,
        )
        # 一直掉到出界就不再是实体
        for _ in range(120):
            s = await si(ws, 4)
            if s["seesaws"][0]["left"]["gone"]:
                break
        check("掉出世界底部以后不再是可站立的实体",
              s["seesaws"][0]["left"]["gone"],
              f"y={s['seesaws'][0]['left']['y']:.0f}")

    # ── 7 ───────────────────────────────────────────────────────────
    section("7. 及时下来：绳到头时对面没人，就各自钉在绳两头停住，不塌")
    s = await stand_on(ws, 3, 3, 0, "left")
    # 踩 84 帧攒够动量，再在绳到头之前撤到另一座板上。撤得太早滑不到头（80 帧就差
    # 0.04px 到不了，摩擦力先把它耗停了），撤得太晚就变成第 6 节那种塌法 ——
    # 靠自身动力走完这段要 94 帧，所以留给「及时下来」的窗口就这么十帧。
    await si(ws, 84)
    other = s["seesaws"][1]["left"]
    await rpc(
        ws,
        "game.setPlayerPos",
        {"x": other["x"] + 8.0, "y": other["y"] - TILE_SIZE, "vx": 0.0, "vy": 0.0},
    )
    await rpc(ws, "game.clearEnemies")
    pinned = None
    for _ in range(200):
        s = await si(ws, 2)
        a = s["seesaws"][0]
        if a["falloff"] is not None:
            break
        if a["left"]["vy"] == 0.0 and a["right"]["drop"] < 1.0:
            pinned = a
            break
    check("没塌", s["seesaws"][0]["falloff"] is None,
          f"falloff={s['seesaws'][0]['falloff']}")
    check("靠惯性滑到了绳的尽头并停住", pinned is not None,
          f"right drop={s['seesaws'][0]['right']['drop']:.2f} "
          f"left vy={s['seesaws'][0]['left']['vy']:.2f}")
    if pinned:
        # 「钉住」比字面意思软：钉位置的那一步在板子移动**之前**跑，而且不清速度，
        # 所以还带着动量的板会被拽过界、下一帧再拉回来，最终停在离横梁一帧行程之内，
        # 等摩擦力把速度耗成 0 才真正贴上去。清速度的话滑行会变成一脚刹死。
        check("对面停在横梁上（误差在一帧行程之内）", abs(pinned["right"]["drop"]) < 1.0,
              f"drop={pinned['right']['drop']:.4f}")
        check(
            "踩过的那块停在绳的另一头",
            abs(pinned["left"]["drop"] - pinned["rope"]) < 1.0,
            f"drop={pinned['left']['drop']:.2f} rope={pinned['rope']:.2f}",
        )
        check(
            "绳长关系依然成立",
            abs(pinned["left"]["drop"] + pinned["right"]["drop"] - pinned["rope"]) < 0.5,
            f"{pinned['left']['drop'] + pinned['right']['drop']:.2f} vs {pinned['rope']:.2f}",
        )

    # ── 8 ───────────────────────────────────────────────────────────
    section("8. 站右边也一样，方向对称")
    s = await stand_on(ws, 3, 3, 0, "right")
    check("被算成一个重量", s["seesaws"][0]["right"]["riders"] == 1)
    s = await si(ws, 20)
    a = s["seesaws"][0]
    check("踩的右边往下", a["right"]["vy"] > 0, f"vy={a['right']['vy']:.1f}")
    check("左边往上", a["left"]["vy"] < 0, f"vy={a['left']['vy']:.1f}")
    fell = None
    for _ in range(200):
        s = await si(ws, 2)
        if s["seesaws"][0]["falloff"] is not None:
            fell = s
            break
    check(
        "踩右边塌的就是右边",
        fell is not None and fell["seesaws"][0]["falloff"] == "right",
        f"falloff={fell['seesaws'][0]['falloff'] if fell else None}",
    )

    # ── 9 ───────────────────────────────────────────────────────────
    section("9. 6-3 的窄板（1.5 格）一样能站能踩")
    await setup(ws, 6, 3)
    s = await si(ws)
    narrow = s["seesaws"][0]
    check("板宽 1.5 格 = 48px", abs(narrow["left"]["w"] - 48.0) < 0.01,
          f"{narrow['left']['w']:.0f}px")
    s = await stand_on(ws, 6, 3, 0, "left")
    check("站得上去", s["seesaws"][0]["left"]["riders"] == 1,
          f"riders={s['seesaws'][0]['left']['riders']}")
    s = await si(ws, 20)
    check("窄板也照样被踩下去", s["seesaws"][0]["left"]["vy"] > 0,
          f"vy={s['seesaws'][0]['left']['vy']:.1f}")

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
