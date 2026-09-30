#!/usr/bin/env python3
"""
mari0 飞行龟踩踏验证脚本（两次踩踏：先去翅，再进壳）

原版 `koopa.lua` 里带翅膀的龟是独立对象（`t = "flying"` 绿 / `"redflying"` 红，
:37-48），踩踏序列比普通龟长一步（stomp :212-240）：
  第一下  → 清掉 flying 标记、quad 换回走路帧、重力回世界重力 —— 变成一只
            同色的普通走路龟（去翅红龟仍然拒走悬崖，:106 的谓词测的是去掉
            flying 后的 type 字符串）；
  第二下  → 才进 shell（quad[3]，hitbox 缩小）。

移植版的 bug：
  - 渲染从来没画过翅膀帧（walking 分支落进普通龟 fallback），踩飞行龟自然
    「看不出原版效果」；
  - 关卡里的红飞行龟（1-3 有两只）被映射成绿的；
  - 绿龟应该边走边跳（重力 30 + 落地弹起 10，`koopa.lua:367-375`），红龟才是
    原地悬停（余弦 7.5 格 / 7 秒，`:122-130`），移植版两种都在悬停。

本脚本对 `koopa_flying` / `koopa_red_flying` 各验证一遍完整序列：
  1. 行为：绿翅龟有水平速度（走路），红翅龟 vx≈0 且 y 随时间上下（悬停）；
  2. 翅膀真的画出来了：与无敌人底图做像素差分，翅帧的白像素明显多于去翅后
     （翅膀是白的，走路龟只有少量高光）；
  3. 翅膀是同色的：红翅龟的差分矩形里红多于绿（bug 时整只变绿龟）；
  4. 第一下踩：inspect 的 type 变成 koopa / koopa_red，state 仍是 walking；
  5. 第二下踩：state 变 shell 且静止；
  6. 壳的颜色：绿壳绿多于红；红壳红够多且没有绿。

用法：
  1. 先启动游戏: cargo run -p mari0
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_winged_koopa_test.py

依赖: pip install websockets pillow
"""
import asyncio
import json
import os
import sys

import websockets
from PIL import Image

WS_URL = "ws://127.0.0.1:9229"
T = 32.0
VIRTUAL_W, VIRTUAL_H = 512.0, 480.0
SHOT_DIR = "/tmp/mari0_winged_koopa"

# 差分/分类阈值沿袭 vdp_koopa_shell_render_test.py 的实测标定（机位钉死后背景
# 全部抵消）。翅膀差量按翅格 vs 走路格的白像素实测（源图 ~45 格差 ×4 渲染
# 放大）再留出余量取得。
SAME = 12
GREEN_MAX = 150
RED_MIN = 300
WHITE_DROP = 60

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


async def screenshot(ws, path):
    """截图并等它落盘（渲染线程下一帧才写文件）。

    文件先被创建、PNG 字节随后才写完，所以存在时直接开也会撞上半个文件——
    开失败就接着等，而不是当作永久失败。
    """
    if os.path.exists(path):
        os.remove(path)
    await rpc(ws, "game.screenshot", {"path": path})
    for _ in range(300):
        await asyncio.sleep(0.02)
        if os.path.exists(path):
            try:
                return Image.open(path).convert("RGB")
            except Exception:
                pass
    raise RuntimeError(f"screenshot never landed: {path}")


def classify_diff(enemy_img, base_img, rect):
    """矩形里**和底图不同**的像素按颜色分类计数。

    先减底图再分类，灌木/砖/天空这些背景就都进不了账。
    """
    x0, y0, x1, y1 = rect
    a, b = enemy_img.load(), base_img.load()
    counts = {"green": 0, "red": 0, "white": 0, "dark": 0}
    for y in range(max(y0, 0), min(y1, enemy_img.height)):
        for x in range(max(x0, 0), min(x1, enemy_img.width)):
            pr, pg, pb = a[x, y]
            qr, qg, qb = b[x, y]
            if abs(pr - qr) < SAME and abs(pg - qg) < SAME and abs(pb - qb) < SAME:
                continue
            if pg > pr + 20 and pg > pb + 20:
                counts["green"] += 1
            elif pr > pg + 20 and pr > pb + 20:
                counts["red"] += 1
            elif pr > 200 and pg > 200 and pb > 200:
                counts["white"] += 1
            elif pr < 80 and pg < 80 and pb < 80:
                counts["dark"] += 1
    return counts


async def park_player(ws):
    """把人停到 6 列的平地上等站稳——截图与底图机位相同，差分才成立。"""
    await rpc(ws, "game.setPlayerPos", {"x": 6 * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 2)
        if s["player"]["on_ground"]:
            return s
    return s


async def reset_level_and_pin(ws):
    """整套重置 1-1、钉死机位、清光敌人，返回无敌人底图。

    每节开头都重跑一遍，而不是全程只跑一次，原因有二：
      - 相机永不回卷（game.rs「never scroll back」），而 setLevel 重载关卡会把
        相机重置回起点，所以每节的底图必须跟着重拍，否则差分矩形对不上背景；
      - 上一节若有踩踏没被及时检测到，玩家可能已进入死亡流程——
        GameState::Dead 下 update_playing 不跑、敌人全部冻结（首跑红翅龟
        Δy=0.0 的根因）。重载关卡会清掉 death 动画（load_current 里
        `self.death = None`），玩家和关卡一起回到干净状态。
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setLives", {"lives": 9})
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
    # 先把人送过去让机位一次到位、清掉这趟 reveal 出来的原生敌人
    # （clearEnemies 不重置 spawn 前沿，相机不再前进就不会再刷），再停回
    # 6 列拍底图——此后本节所有截图与底图机位完全一致。
    await rpc(ws, "game.setPlayerPos", {"x": 12 * T, "y": 8 * T, "vx": 0.0, "vy": 0.0})
    await si(ws, 4)
    await rpc(ws, "game.clearEnemies")
    await park_player(ws)
    return await screenshot(ws, f"{SHOT_DIR}/base.png")


async def find_enemy(s, *etypes):
    return next((e for e in s["enemies"] if e["type"] in etypes), None)


async def drop_stomp(ws, etypes, done):
    """从正上方把人砸下去，直到 done(snapshot, enemy) 成立。

    每次砸之前都按敌人**当前** x 重新瞄准——去翅后的龟会走开，第一次砸完
    位置就对不上了。砸完短暂轮询：踩踏在落地后几帧内就会发生，轮询太久只会
    让走路的敌人撞上落在地上的马里奥。没踩中就把人挪走再重砸。
    """
    for _ in range(8):
        s = await si(ws, 1)
        enemy = await find_enemy(s, *etypes)
        if enemy is None:
            return None
        await rpc(ws, "game.setPlayerPos",
                  {"x": enemy["x"], "y": 8 * T, "vx": 0.0, "vy": 200.0})
        hit = None
        for _ in range(10):
            s = await si(ws, 2)
            enemy = await find_enemy(s, *etypes)
            if enemy is not None and done(s, enemy):
                hit = (s, enemy)
                break
        if hit is not None:
            return hit
        await park_player(ws)
    return None


async def run(ws):
    await rpc(ws, "engine.pause")
    os.makedirs(SHOT_DIR, exist_ok=True)

    for winged, plain, spawn_y in (("koopa_flying", "koopa", 11 * T),
                                   ("koopa_red_flying", "koopa_red", 10 * T)):
        section(f"{winged}：两次踩踏（先去翅，再进壳）")
        base = await reset_level_and_pin(ws)
        # 红翅龟悬停（无重力），得放在半空；绿翅龟有重力，放地上让它跳。
        await rpc(ws, "game.spawnEnemy", {"type": winged, "x": 12 * T, "y": spawn_y})
        await si(ws, 4)

        # ── 行为：绿翅龟走路，红翅龟原地悬停 ──
        s = await si(ws, 2)
        enemy = await find_enemy(s, winged)
        if winged == "koopa_flying":
            check("绿翅龟带翅膀走路（有水平速度），不是悬停",
                  enemy is not None and abs(enemy["vx"]) > 1.0,
                  f"vx={enemy and enemy['vx']}")
        else:
            check("红翅龟悬停原地（没有水平速度）",
                  enemy is not None and abs(enemy["vx"]) < 1.0,
                  f"vx={enemy and enemy['vx']}")
            y0 = enemy["y"] if enemy else 0.0
            for _ in range(15):
                s = await si(ws, 2)
            enemy = await find_enemy(s, winged)
            check("红翅龟沿余弦上下移动（悬停不是钉死）",
                  enemy is not None and abs(enemy["y"] - y0) > 4.0,
                  f"Δy={enemy and enemy['y'] - y0:.1f}")

        # ── 翅膀画出来了：翅帧的白像素 vs 去翅后的白像素 ──
        s = await si(ws, 2)
        enemy = await find_enemy(s, winged)
        check(f"{winged} 存活且在 walking 状态",
              enemy is not None and enemy["state"] == "walking",
              f"state={enemy and enemy['state']}")
        winged_img = await screenshot(ws, f"{SHOT_DIR}/{winged}.png")
        sx, sy = winged_img.width / VIRTUAL_W, winged_img.height / VIRTUAL_H
        # 翅龟的 hitbox 是 32×48；底部让出 3 虚拟像素，不把地砖边缘扫进来
        wx0 = round((enemy["x"] - s["camera_x"]) * sx)
        wy0 = round(enemy["y"] * sy)
        wrect = (wx0, wy0, wx0 + round(32 * sx), wy0 + round(45 * sy))
        wc = classify_diff(winged_img, base, wrect)
        if winged == "koopa_flying":
            # 旧断言「绿多于红」是错的：绿龟的橙黄皮肤在本分类器里落进 "red"
            # （pr>pg+20 且 pr>pb+20），带翅帧里皮肤+翅膀像素数超过绿壳，
            # 渲染正确时 red 也会 > green（首跑实测 red=316 / green=224）。
            # 判别式改成绿像素量级：绿皮 sprite 必然 >150，红皮 sprite 必然
            # <150（与红壳检查对称），同样能抓住「用错颜色贴图」这个 bug。
            check("绿翅龟用的是绿皮贴图（绿像素量级达标）",
                  wc["green"] > GREEN_MAX, f"counts={wc}")
        else:
            check("红翅龟是红的（bug 时整只变绿龟）",
                  wc["red"] > wc["green"], f"counts={wc}")

        # ── 第一下踩：去掉翅膀，变同色走路龟 ──
        # 过滤集合要含 plain：踩中的瞬间 type 就从 winged 变成 plain，只盯
        # winged 会让敌人从 find_enemy 里「消失」、done 永远不触发——首跑两
        # 个颜色都因此误报「没踩中」（probe 实测 6 帧内 type 已变）。
        hit = await drop_stomp(ws, (winged, plain),
                               lambda s, e: e["type"] == plain and e["state"] == "walking")
        check(f"第一下踩去翅：type 变 {plain}，仍是 walking",
              hit is not None,
              "8 次砸击都没等到 type 变化" if hit is None else "")
        if hit is None:
            continue
        s = await park_player(ws)
        for _ in range(45):
            s = await si(ws, 2)
        enemy = await find_enemy(s, plain)
        check("去翅后的龟活着、在走路", enemy is not None and enemy["state"] == "walking",
              f"type={enemy and enemy['type']} state={enemy and enemy['state']}")

        plain_img = await screenshot(ws, f"{SHOT_DIR}/{plain}_dewinged.png")
        sx, sy = plain_img.width / VIRTUAL_W, plain_img.height / VIRTUAL_H
        px0 = round((enemy["x"] - s["camera_x"]) * sx)
        py0 = round(enemy["y"] * sy)
        prect = (px0, py0, px0 + round(32 * sx), py0 + round(45 * sy))
        pc = classify_diff(plain_img, base, prect)
        check("去翅后翅膀消失（白像素大幅减少）",
              wc["white"] - pc["white"] > WHITE_DROP,
              f"翅帧 white={wc['white']} → 去翅后 white={pc['white']}")

        # ── 第二下踩：进壳 ──
        hit = await drop_stomp(ws, (plain,),
                               lambda s, e: e["type"] == plain and e["state"] == "shell")
        check(f"第二下踩进壳：state 变 shell", hit is not None,
              "8 次砸击都没等到 shell" if hit is None else "")
        if hit is None:
            continue
        # 壳一成形就把人挪走——让他落回壳上会把壳踢飞；多走 1.5s 等踩分飘字
        # 散掉、壳落稳，截图里就只剩壳。
        s = await park_player(ws)
        for _ in range(45):
            s = await si(ws, 2)
        enemy = await find_enemy(s, plain)
        check("壳静止落地",
              enemy is not None and enemy["state"] == "shell"
              and abs(enemy["vx"]) < 1.0 and abs(enemy["vy"]) < 1.0,
              f"state={enemy and enemy['state']} vx={enemy and enemy['vx']} "
              f"vy={enemy and enemy['vy']}")

        shell_img = await screenshot(ws, f"{SHOT_DIR}/{plain}_shell.png")
        sx, sy = shell_img.width / VIRTUAL_W, shell_img.height / VIRTUAL_H
        # 壳的 hitbox 是 32×32；底部让出 3 虚拟像素，不把地砖边缘扫进来
        x0 = round((enemy["x"] - s["camera_x"]) * sx)
        y0 = round(enemy["y"] * sy)
        rect = (x0, y0, x0 + round(32 * sx), y0 + round(29 * sy))
        c = classify_diff(shell_img, base, rect)
        detail = f"counts={c}"
        if plain == "koopa":
            check("绿龟的壳：绿像素多于红像素", c["green"] > c["red"], detail)
        else:
            check("红龟的壳：一个绿像素都没有", c["green"] < GREEN_MAX, detail)
            check("红龟的壳：确实是红的", c["red"] > RED_MIN, detail)

    await rpc(ws, "engine.resume")


async def main():
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except (OSError, websockets.exceptions.WebSocketException) as exc:
        print(f"\n无法连接 {WS_URL}: {exc}")
        print("先启动: cargo run -p mari0")
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
