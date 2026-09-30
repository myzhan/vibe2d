#!/usr/bin/env python3
"""
mari0 踩龟翻壳渲染验证脚本

原版 `koopa.lua` 的 stomp（212-240）里没有补间——「翻过来」就是换帧：第 224 行把
quad 换成 3 号格，一只四脚朝天、肚皮朝上的壳，hitbox 缩成 small。被火球/星星/
飞壳打死的龟也一样先换 3 号格再翻着面飞走（`koopa.lua:246`）。

移植版的 bug 全在 `render.rs` 的 Shell 分支：
  - 画的是 `koopa_uv(4, 0)`——0 起算第 4 列，即原版 quad 5 的**带翅膀飞行帧**，
    所以踩完看到的不是翻壳，而是一只被压进 32×32 hitbox 的振翅飞龟；
  - 纹理写死 `tex_koopa`，红龟和甲虫一进壳就变绿；
  - 整张 16×24 的格硬塞 32×32，纵向压扁。

状态机（Walking → Shell）本来就是对的，所以本脚本只钉渲染：对 koopa / koopa_red /
beetle 各踩出一只静止的壳，与一张没有敌人的同机位底图做**像素差分**（把灌木、砖、
天空这些背景全部减掉），再对差出来的壳像素做颜色分类：

  koopa      ：壳是全表最绿的一格（绿 > 红）；bug 的飞行帧是皮肤+翅膀为主（红 > 绿）。
  koopa_red  ：红壳一个绿像素都不该有；bug 时画的是绿龟（绿一大片）。
  beetle     ：深色壳，绿、白都为零；bug 时绿身白翅一大片。

用法：
  1. 先启动游戏: cargo run -p mari0
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_koopa_shell_render_test.py

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
SHOT_DIR = "/tmp/mari0_koopa_shell"

# 差分阈值：同通道差小于这个数算「背景没变」；分类下限沿袭对三张图的实测
# （koopa.png/koopared.png/beetle.png 第 2 列的像素统计 ×2 渲染放大）。机位钉死
# 后背景全部抵消，红壳 ≈456 红像素、甲虫壳 ≈624 深像素；bug 时分别只剩 ~112 / ~0。
SAME = 12
GREEN_MAX = 150
RED_MIN = 300
DARK_MIN = 400

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


def classify_diff(shell_img, base_img, rect):
    """矩形里**和底图不同**的像素按颜色分类计数。

    先减底图再分类，灌木/砖/天空这些背景就都进不了账——壳后面有没有装饰
    跟壳长什么样是两码事。
    """
    x0, y0, x1, y1 = rect
    a, b = shell_img.load(), base_img.load()
    counts = {"green": 0, "red": 0, "white": 0, "dark": 0}
    for y in range(max(y0, 0), min(y1, shell_img.height)):
        for x in range(max(x0, 0), min(x1, shell_img.width)):
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
    """把人停到 6 列的平地上等站稳——两次截图机位相同，差分才成立。"""
    await rpc(ws, "game.setPlayerPos", {"x": 6 * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 2)
        if s["player"]["on_ground"]:
            return s
    return s


async def run(ws):
    await rpc(ws, "engine.pause")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.setLives", {"lives": 9})
    await si(ws, 1, [{"device": "keyboard", "action": "release", "key": k}
                     for k in ("Left", "Right", "Space", "F", "Down")])
    # 相机永不回卷（game.rs:1438「never scroll back」）：踩踏时把人 teleport 到
    # 12 列会让机位前移并停在那里，底图若在移动前拍，差分里就混进整片背景。
    # 所以先把人送过去让机位一次到位、清掉这趟 reveal 出来的原生敌人（clearEnemies
    # 不重置 spawn 前沿，相机不再前进就不会再刷），再停回 6 列拍底图——此后三张
    # 壳截图与底图机位完全一致，差分里剩下的就只有壳本身。
    await rpc(ws, "game.setPlayerPos", {"x": 12 * T, "y": 8 * T, "vx": 0.0, "vy": 0.0})
    await si(ws, 4)
    await rpc(ws, "game.clearEnemies")
    await park_player(ws)
    os.makedirs(SHOT_DIR, exist_ok=True)
    base = await screenshot(ws, f"{SHOT_DIR}/base.png")

    for etype in ("koopa", "koopa_red", "beetle"):
        section(f"{etype}：踩成静止的壳")
        await rpc(ws, "game.clearEnemies")
        await rpc(ws, "game.spawnEnemy", {"type": etype, "x": 12 * T, "y": 11 * T})
        await si(ws, 4)
        # 从正上方砸下去：踩一下，Walking → Shell
        await rpc(ws, "game.setPlayerPos", {"x": 12 * T, "y": 8 * T, "vx": 0.0, "vy": 200.0})
        shelled = False
        for _ in range(60):
            s = await si(ws, 2)
            if any(e["type"] == etype and e["state"] == "shell" for e in s["enemies"]):
                shelled = True
                break
        # 壳一成形就把人挪走——让他落回壳上会把壳踢飞；多走 1.5s 等踩分飘字
        # 散掉、壳落稳，截图里就只剩壳。
        s = await park_player(ws)
        for _ in range(45):
            s = await si(ws, 2)
        enemy = next((e for e in s["enemies"] if e["type"] == etype), None)
        check("踩成了静止的壳并落了地",
              shelled and enemy is not None and enemy["state"] == "shell"
              and abs(enemy["vx"]) < 1.0 and abs(enemy["vy"]) < 1.0,
              f"state={enemy and enemy['state']} vx={enemy and enemy['vx']} "
              f"vy={enemy and enemy['vy']}")

        img = await screenshot(ws, f"{SHOT_DIR}/{etype}.png")
        sx, sy = img.width / VIRTUAL_W, img.height / VIRTUAL_H
        # 壳的 hitbox 是 32×32；底部让出 3 虚拟像素，不把地砖边缘扫进来
        x0 = round((enemy["x"] - s["camera_x"]) * sx)
        y0 = round(enemy["y"] * sy)
        rect = (x0, y0, x0 + round(32 * sx), y0 + round(29 * sy))
        c = classify_diff(img, base, rect)
        detail = f"counts={c}"

        if etype == "koopa":
            check("绿龟的壳：绿像素多于红像素（bug 的飞行帧皮肤+翅膀为主，红多于绿）",
                  c["green"] > c["red"], detail)
        elif etype == "koopa_red":
            check("红龟的壳：一个绿像素都没有（bug 时整只变绿龟）",
                  c["green"] < GREEN_MAX, detail)
            check("红龟的壳：确实是红的", c["red"] > RED_MIN, detail)
        else:
            check("甲虫的壳：是深色的", c["dark"] > DARK_MIN, detail)
            check("甲虫的壳：没有绿也没有白（bug 时绿身白翅）",
                  c["green"] < GREEN_MAX and c["white"] < GREEN_MAX, detail)

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
