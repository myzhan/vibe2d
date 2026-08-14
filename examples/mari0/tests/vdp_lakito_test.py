#!/usr/bin/env python3
"""
mari0 lakitu + 刺猬（spiny）验证脚本

**为什么这两个必须一起做**：实体表里有 `spikey`(98) 和 `spikeyhalf`(99)，编辑器也
提供它们，但**73 个关卡文件里一次都没出现**。走路的刺猬只有一条来路 ——
lakitu 扔出来的蛋落地（`lakito.lua:72` → `goomba.lua:250`）。所以只加 spawn 分支
等于加了个玩家永远见不到的敌人。

lakitu 只在 **4-1 / 6-1 / 8-2** 三关，每关都配一个 `lakitoend`。

本脚本钉住的规则（都来自原版，不是自创）：
  1. 三关都有 lakitu，且 `lakitoend` 都在；spawn 表里没有 spikey
  2. lakitu 追的是**玩家 1.5 秒后的位置**（`lakitodistancetime`），不是当前位置
  3. 左右**不对称**：往右速度 = max(2, round((距离-3)*2))，往左恒为 -2
     （`lakito.lua:98-102`）—— 这就是"你能往前甩开他、但永远摆脱不掉"的原因
  4. 转向有回差：过了 `lakitospace = 4` 格才掉头，所以他绕着玩家慢慢摆
  5. 每 `lakitothrowtime = 4` 秒扔一个蛋，且**同屏刺猬 < 3** 才扔
  6. 蛋是**往上抛**的（speedy = -10）、自身无水平速度、重力只有 30（全场唯一）
  7. 蛋落地变成走路的刺猬，并且**朝玩家走**
  8. 刺猬**踩不死** —— 原版判据是 `b.t ~= "goomba"` 一个不等式（`mario.lua:1778`），
     所以从上面踩下去照样掉一级
  9. 踩 lakitu 不是杀死他：16 秒后他从屏幕右缘飞回来（`lakitorespawn`）
 10. 玩家走过 `lakitoend` 那一列后他**永久**转被动：不再扔蛋、以 3 格/秒一路飘左

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_lakito_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
TILE_SIZE = 32.0
FPS = 60.0

# variables.lua:143-149
LAKITO_THROW_TIME = 4.0
LAKITO_HIDE_TIME = 0.5
LAKITO_RESPAWN = 16.0
LAKITO_SPACE = 4.0
LAKITO_PASSIVE_SPEED = 3.0
LAKITO_MAX_SPINIES = 3

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
    for _ in range(800):
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


def of_type(snapshot, *kinds):
    return [e for e in snapshot["enemies"] if e["type"] in kinds]


def one_lakito(snapshot):
    got = of_type(snapshot, "lakito")
    return got[0] if got else None


async def load_with_lakito(ws, world, level, player_col, player_row=12):
    """Load a lakitu level and drag the camera far enough to reveal him.

    Standing the player on the ground first matters: dropping him in mid-level
    mid-air lets the lazy spawner put an enemy next to him, and a death freezes
    `update_playing` so nothing moves and every later assertion reads as a bug in
    the code under test.
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": world, "level": level})
    await step(ws)
    await rpc(ws, "game.setPlayerPos", {"x": player_col * TILE_SIZE, "y": player_row * TILE_SIZE})
    await step(ws, 30)
    return await snap(ws)


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. lakitu 只在 4-1 / 6-1 / 8-2，且都在镜头推过去之后才出现")
    for world, level, col in ((4, 1, 20), (6, 1, 18), (8, 2, 12)):
        s = await load_with_lakito(ws, world, level, col)
        lak = one_lakito(s)
        check(f"{world}-{level} 出现了 lakitu", lak is not None, f"敌人 {[e['type'] for e in s['enemies']]}")
        check(f"{world}-{level} 还没走到 lakitoend", s["lakito_retired"] is False)

    section("2. 玩家站定不动：lakitu 以 2 格/秒飘左（往左那一侧是恒速）")
    s = await load_with_lakito(ws, 4, 1, 20)
    lak = one_lakito(s)
    if lak:
        x0 = lak["x"]
        await step(ws, 30)
        lak = one_lakito(await snap(ws))
        drift = (x0 - lak["x"]) / TILE_SIZE / 0.5
        check(
            "往左恒为 2 格/秒（而不是按距离加速）",
            abs(drift - 2.0) < 0.15,
            f"实测 {drift:.2f} 格/秒",
        )
        check("方向标记为左", lak["facing_right"] is False)

    section("3. 掉头有 4 格回差，掉头后往右按距离加速")
    # 玩家站在 20 列，lakitu 从 26 列往左飘；他会一路飘到 16 列（20 - 4）才掉头。
    # 他从 26 列起飘，2 格/秒，要到 16 列得走 5 秒 —— 留到 7 秒。
    s = await load_with_lakito(ws, 4, 1, 20)
    turned_at = None
    for _ in range(70):
        await step(ws, 6)
        lak = one_lakito(await snap(ws))
        if lak is None:
            break
        if lak["facing_right"]:
            turned_at = lak["x"] / TILE_SIZE
            break
    check(
        f"在玩家左侧 {LAKITO_SPACE:.0f} 格附近掉头",
        turned_at is not None and 14.5 <= turned_at <= 16.5,
        f"掉头于第 {turned_at:.2f} 列（玩家在 20）" if turned_at else "一直没掉头",
    )
    if turned_at is not None:
        lak = one_lakito(await snap(ws))
        check(
            "往右的速度至少 2 格/秒（max(2, …) 的下限）",
            lak["vx"] / TILE_SIZE >= 2.0 - 1e-3,
            f"vx={lak['vx'] / TILE_SIZE:.2f}",
        )

    section("4. 往右追的速度随距离增长（不是常数）")
    # 只能挪 6 格：再远镜头一跳就会把 lakitu 甩到"左边 200px 外"而被剔除 ——
    # 那是对的行为，但会让这一节看起来像 lakitu 消失了。
    s = await load_with_lakito(ws, 4, 1, 20)
    lak = one_lakito(s)
    if lak:
        gap = 6.0
        await rpc(
            ws,
            "game.setPlayerPos",
            {"x": (lak["x"] / TILE_SIZE + gap) * TILE_SIZE, "y": 12 * TILE_SIZE},
        )
        await step(ws, 4)
        lak2 = one_lakito(await snap(ws))
        check("玩家在右侧时他转身去追", lak2 is not None and lak2["facing_right"] is True)
        # round((6-3)*2) = 6 格/秒。
        expected = round((gap - 3.0) * 2.0)
        check(
            f"距离 {gap:.0f} 格时速度是 round((距离-3)×2) = {expected} 格/秒",
            lak2 is not None and abs(lak2["vx"] / TILE_SIZE - expected) < 1.2,
            f"vx={lak2['vx'] / TILE_SIZE:.2f} 格/秒" if lak2 else "",
        )

    section("5. 每 4 秒扔一个蛋，蛋是往上抛的、水平速度为 0、重力只有 30")
    s = await load_with_lakito(ws, 4, 1, 20)
    egg = None
    for _ in range(60):
        await step(ws, 6)
        s = await snap(ws)
        eggs = of_type(s, "spikey_fall")
        if eggs:
            egg = eggs[0]
            break
    check("四秒后出现了一个蛋", egg is not None)
    if egg:
        check("蛋是往上抛的（speedy = -10）", egg["vy"] < 0.0, f"vy={egg['vy'] / TILE_SIZE:.1f} 格/秒")
        check("蛋自身没有水平速度", abs(egg["vx"]) < 1e-6, f"vx={egg['vx']}")
        # 30 格/秒² 而不是 80：连续两帧的 vy 差就是重力 × dt。
        vy0 = egg["vy"]
        await step(ws, 6)
        eggs = of_type(await snap(ws), "spikey_fall")
        if eggs:
            g = (eggs[0]["vy"] - vy0) / TILE_SIZE / (6 / FPS)
            check("蛋的重力是 30 格/秒²（全场唯一比 80 小的）", abs(g - 30.0) < 2.0, f"实测 {g:.1f}")

    section("6. 蛋落地变成走路的刺猬，并且朝玩家走")
    hatched = None
    for _ in range(80):
        await step(ws, 6)
        s = await snap(ws)
        walking = of_type(s, "spikey")
        if walking:
            hatched = (walking[0], s["player"]["x"])
            break
    check("蛋落地孵成了刺猬", hatched is not None)
    if hatched:
        spiny, px = hatched
        toward = (spiny["vx"] > 0) == (spiny["x"] < px)
        check(
            "刺猬朝玩家走（而不是随便挑一边）",
            toward,
            f"刺猬在 {spiny['x'] / TILE_SIZE:.1f}，玩家在 {px / TILE_SIZE:.1f}，vx={spiny['vx'] / TILE_SIZE:.1f}",
        )

    section("7. 刺猬踩不死：同样的下落，栗子怪被踩死，刺猬把你打小")
    for kind, expect_bounce in (("goomba", True), ("spikey", False)):
        await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
        await step(ws)
        await rpc(ws, "game.clearEnemies")
        await rpc(ws, "game.setPlayerPos", {"x": 6 * TILE_SIZE, "y": 9 * TILE_SIZE})
        await rpc(ws, "game.setPlayerSize", {"size": "big"})
        await rpc(ws, "game.spawnEnemy", {"type": kind, "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False})
        bounced = False
        hurt = False
        for _ in range(20):
            await step(ws, 2)
            s = await snap(ws)
            if s["player"]["vy"] < -1.0:
                bounced = True
            if not s["player"]["is_big"]:
                hurt = True
            if bounced or hurt:
                break
        if expect_bounce:
            check("踩栗子怪会弹起来", bounced and not hurt, f"bounce={bounced} hurt={hurt}")
        else:
            check("踩刺猬不弹、反而掉一级", hurt and not bounced, f"bounce={bounced} hurt={hurt}")

    section("8. 踩 lakitu 只是把他赶走：16 秒后从屏幕右缘飞回来")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1})
    await step(ws)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerPos", {"x": 6 * TILE_SIZE, "y": 9 * TILE_SIZE})
    await rpc(ws, "game.setPlayerSize", {"size": "big"})
    await rpc(ws, "game.spawnEnemy", {"type": "lakito", "x": 6 * TILE_SIZE, "y": 12 * TILE_SIZE, "facing_right": False})
    downed = None
    for _ in range(30):
        await step(ws, 2)
        s = await snap(ws)
        lak = one_lakito(s)
        if lak and lak["state"] == "dead":
            downed = lak
            break
    check("从上面落下把他赶出了云", downed is not None)
    if downed:
        check(
            f"他的计时器是 {LAKITO_RESPAWN:.0f} 秒的归队倒计时，不是 0.5 秒的死亡动画",
            downed["death_timer"] > LAKITO_RESPAWN - 1.0,
            f"death_timer={downed['death_timer']:.2f}",
        )
        # 掉出世界底部也不能把他删掉 —— 他是在等，不是没了。
        await step(ws, 120)
        s = await snap(ws)
        check("掉出画面后他仍然在场（等着归队）", one_lakito(s) is not None)

    section(f"9. 同屏刺猬到 {LAKITO_MAX_SPINIES} 只就停手")
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 4, "level": 1})
    await step(ws)
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setPlayerPos", {"x": 20 * TILE_SIZE, "y": 12 * TILE_SIZE})
    await rpc(ws, "game.spawnEnemy", {"type": "lakito", "x": 20 * TILE_SIZE, "y": 2 * TILE_SIZE, "facing_right": False})
    for _ in range(3):
        await rpc(ws, "game.spawnEnemy", {"type": "spikey_fall", "x": 40 * TILE_SIZE, "y": 2 * TILE_SIZE, "facing_right": False})
    await step(ws, int(FPS * LAKITO_THROW_TIME) + 30)
    s = await snap(ws)
    spinies = len(of_type(s, "spikey", "spikey_fall"))
    check(
        "已有三只时四秒过去也没有第四只",
        spinies <= LAKITO_MAX_SPINIES,
        f"场上 {spinies} 只",
    )

    section("10. 走过 lakitoend：不再扔蛋，一路飘左，且永久生效")
    # 4-1 的 lakitoend 在第 209 列（1 基），即 0 基第 208 列。
    s = await load_with_lakito(ws, 4, 1, 20)
    check("还没到 lakitoend 时是 false", s["lakito_retired"] is False)
    await rpc(ws, "game.setPlayerPos", {"x": 209 * TILE_SIZE, "y": 12 * TILE_SIZE})
    await step(ws, 4)
    s = await snap(ws)
    check("过了那一列就 retired", s["lakito_retired"] is True)
    # 往回走也不会取消 —— 它是个只置位的闩。
    await rpc(ws, "game.setPlayerPos", {"x": 40 * TILE_SIZE, "y": 12 * TILE_SIZE})
    await step(ws, 4)
    s = await snap(ws)
    check("往回走不取消（只置位的闩）", s["lakito_retired"] is True)

    # 原来那只早被"飘到镜头左边 200px 外"的剔除规则清掉了（这正是它该做的），
    # 所以就地放一只来量被动漂移。**必须按镜头定位而不是按玩家** —— 镜头永不回退
    # （和原版一样），玩家瞬移回第 40 列时镜头还停在第 203 列附近，放在玩家旁边
    # 等于放在镜头左边几千像素外，一帧就被剔除。
    await rpc(ws, "game.clearEnemies")
    cam_col = (await snap(ws))["camera_x"] / TILE_SIZE
    await rpc(
        ws,
        "game.spawnEnemy",
        {"type": "lakito", "x": (cam_col + 4) * TILE_SIZE, "y": 2 * TILE_SIZE, "facing_right": False},
    )
    await step(ws, 2)
    lak = one_lakito(await snap(ws))
    check("补放的 lakitu 在场", lak is not None)
    if lak:
        x0 = lak["x"]
        await step(ws, 30)
        lak = one_lakito(await snap(ws))
        if lak:
            speed = (x0 - lak["x"]) / TILE_SIZE / 0.5
            check(
                f"被动漂移是 {LAKITO_PASSIVE_SPEED:.0f} 格/秒（比追击时的 2 格/秒更快）",
                abs(speed - LAKITO_PASSIVE_SPEED) < 0.3,
                f"实测 {speed:.2f} 格/秒",
            )
        await step(ws, int(FPS * LAKITO_THROW_TIME) + 30)
        s = await snap(ws)
        check(
            "退休后一个蛋都不扔（哪怕过了整个投掷周期）",
            not of_type(s, "spikey", "spikey_fall"),
            f"场上 {len(of_type(s, 'spikey', 'spikey_fall'))} 只",
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
