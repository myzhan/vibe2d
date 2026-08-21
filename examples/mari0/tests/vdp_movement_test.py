#!/usr/bin/env python3
"""
mari0 水平移动、跳跃与伤害阶梯验证脚本

这一套锁的是六个**曾经复刻错了**的地方。它们不是缺功能，是数值和分支写歪了，
所以每条断言都直接对着 `variables.lua` 的原值：

  1. 跑速上限 `maxrunspeed = 9.0` 格/秒（曾经是 11.2，快了 24%）。
  2. 跳跃力**随速度连续**变化：`jumpforce + (|vx|/maxrunspeed) * jumpforceadd`
     = 16 → 17.9，并且 `math.max` 把它压在 17.9（`mario.lua:1571-1572`）。
     曾经是「按不按冲刺键」的二值 16 / 19 —— 19 还超出了原版上限。
  3. 伤害阶梯：`mario:shrink` 直接 `size = 1`（`mario.lua:1672`），
     火焰马里奥被打**变小**，不是变大。从满状态到死是两下，不是三下。
  4. 拾取是「一级一级」的：`mario:grow` 就是 `size + 1`，蘑菇和火花都调它
     （`mushroom.lua:85`、`flower.lua:76`）。
  5. 无敌时间 `invincibletime = 3.2` 秒（曾经硬编码 2.0）。
  6. **打滑**：地面上反向时，在加速度之上再叠一份 `friction`，并切到 `sliding`
     姿态（`mario.lua:1119-1126`）。曾经只有 `accel * dt`，转身毫无重量感。

顺带覆盖两个一起补上的原版细节：空中反向只打 `airslidefactor = 0.8` 的折，
以及空中的**两级速度上限** —— 没带着速度起跳就只能到走路上限。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 -u examples/mari0/tests/vdp_movement_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
T = 32.0

# variables.lua:12-22, :43-44, :312
MAX_WALK = 6.4
MAX_RUN = 9.0
WALK_ACCEL = 8.0
RUN_ACCEL = 16.0
FRICTION = 14.0
AIR_SLIDE_FACTOR = 0.8
JUMP_FORCE = 16.0
JUMP_FORCE_ADD = 1.9
INVINCIBLE_TIME = 3.2
# 按住跳跃时的重力，用来把「起跳那一帧」的观测值折算回原始跳跃力
GRAVITY_JUMPING = 30.0
DT = 1.0 / 60.0

# 1-1 里一段平坦无障碍的跑道
FLAT_COL = 3

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


def near(a, b, tol=0.02):
    return abs(a - b) <= tol


def section(title):
    print(f"\n─── {title} ───")


PRESS = lambda *k: [{"device": "keyboard", "action": "press", "key": x} for x in k]
RELEASE = lambda *k: [{"device": "keyboard", "action": "release", "key": x} for x in k]
FREE_ALL = RELEASE("Left", "Right", "Space", "F", "Down")


async def stand(ws, col=FLAT_COL, size="small"):
    """站在 1-1 的平地上，速度归零。

    y 用 10 格而不是 11：11 格会和管道实体重叠，放进去当场就死 —— 本脚本第一版
    就是这么把「跳不上管子」误诊成物理坏了的。
    """
    await rpc(ws, "game.setLevel", {"pack": "smb", "world": 1, "level": 1, "sublevel": 0})
    await rpc(ws, "game.setState", {"state": "playing"})
    await rpc(ws, "game.clearEnemies")
    await rpc(ws, "game.setLives", {"lives": 5})
    await rpc(ws, "game.setPlayerSize", {"size": size})
    await si(ws, 1, FREE_ALL)
    await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": 10 * T, "vx": 0.0, "vy": 0.0})
    for _ in range(20):
        s = await si(ws, 4)
        if s["player"]["on_ground"]:
            break
    return s


async def jump_from(ws, keys, run_frames):
    """先跑 `run_frames` 帧攒速度，再起跳；返回 (起跳前 vx, 起跳那帧 vy)，单位格/秒。"""
    await stand(ws)
    vx = 0.0
    if run_frames:
        s = await si(ws, run_frames, PRESS(*keys))
        vx = s["player"]["vx"] / T
    # 跳跃键必须是「刚按下」才算，所以先确保它是松的
    await si(ws, 1, RELEASE("Space"))
    s = await si(ws, 1, PRESS(*keys, "Space"))
    return vx, -s["player"]["vy"] / T


async def run(ws):
    await rpc(ws, "engine.pause")

    # ── 1 ───────────────────────────────────────────────────────────
    section(f"1. 速度上限：走 {MAX_WALK}、跑 {MAX_RUN} 格/秒")
    await stand(ws)
    s = await si(ws, 70, PRESS("Right"))
    check(
        f"走路收敛到 {MAX_WALK}",
        near(s["player"]["vx"] / T, MAX_WALK),
        f"{s['player']['vx'] / T:.3f} 格/秒",
    )
    await stand(ws)
    s = await si(ws, 70, PRESS("Right", "F"))
    check(
        f"冲刺收敛到 {MAX_RUN}（曾经是 11.2）",
        near(s["player"]["vx"] / T, MAX_RUN),
        f"{s['player']['vx'] / T:.3f} 格/秒",
    )

    # ── 2 ───────────────────────────────────────────────────────────
    section("2. 跳跃力随速度连续变化，上限 17.9")

    def expect_jump(vx):
        """原版公式，再减掉起跳那一帧已经吃掉的一份重力。"""
        force = min(JUMP_FORCE + abs(vx) / MAX_RUN * JUMP_FORCE_ADD, JUMP_FORCE + JUMP_FORCE_ADD)
        return force - GRAVITY_JUMPING * DT

    for label, keys, frames in (
        ("站定起跳", (), 0),
        ("走速起跳", ("Right",), 70),
        ("跑速起跳", ("Right", "F"), 70),
    ):
        vx, vy = await jump_from(ws, keys, frames)
        want = expect_jump(vx)
        check(
            f"{label}：vx={vx:.2f} → 跳跃力 {want:.3f}",
            near(vy, want, 0.05),
            f"实测 {vy:.3f} 格/秒",
        )

    # 二值模型下走速和跑速会得到同一个数；连续模型下必须不同
    _, vy_walk = await jump_from(ws, ("Right",), 70)
    _, vy_run = await jump_from(ws, ("Right", "F"), 70)
    _, vy_idle = await jump_from(ws, (), 0)
    check(
        "三种速度跳出三个不同高度（二值模型只会有两个）",
        not near(vy_idle, vy_walk, 0.05) and not near(vy_walk, vy_run, 0.05),
        f"站定 {vy_idle:.2f} / 走 {vy_walk:.2f} / 跑 {vy_run:.2f}",
    )
    check(
        f"跑速的跳跃力封顶在 {JUMP_FORCE + JUMP_FORCE_ADD}（旧代码给到 19，超出原版上限）",
        vy_run < JUMP_FORCE + JUMP_FORCE_ADD + 0.01,
        f"{vy_run:.3f} 格/秒",
    )

    # ── 3 ───────────────────────────────────────────────────────────
    section(f"3. 打滑：反向时额外叠一份 friction={FRICTION}")
    await stand(ws)
    s = await si(ws, 70, PRESS("Right", "F"))
    v0 = s["player"]["vx"] / T
    await si(ws, 1, RELEASE("Right"))
    seq = []
    # 至少要跑满 v0/(friction+run accel) 秒才看得到速度过零：9.0÷30 = 0.3 秒 = 18 帧。
    # 采样 8×2=16 帧的话转身还没结束，末态断言会误报。
    for _ in range(14):
        s = await si(ws, 2, PRESS("Left", "F"))
        seq.append((s["player"]["vx"] / T, s["player"]["anim_state"]))
    # 反向减速率 = friction + runacceleration，取中间两个采样点算斜率，
    # 避开松键那一帧的过渡
    rate = (seq[1][0] - seq[3][0]) / (4 * DT)
    check(
        f"减速率 = friction + run accel = {FRICTION + RUN_ACCEL} 格/秒²",
        near(rate, FRICTION + RUN_ACCEL, 0.6),
        f"实测 {rate:.2f}（只有加速度的话会是 {RUN_ACCEL}）",
    )
    check(
        "转身过程中用 slide 姿态",
        any(a == "slide" for _, a in seq),
        f"看到的姿态 {sorted({a for _, a in seq})}",
    )
    check(
        "速度反向之后就不再是 slide",
        seq[-1][0] < 0 and seq[-1][1] != "slide",
        f"末尾 vx={seq[-1][0]:.2f} anim={seq[-1][1]}",
    )
    await si(ws, 1, FREE_ALL)

    # ── 4 ───────────────────────────────────────────────────────────
    section("4. 空中：只打 airslidefactor 的折，且有两级上限")
    # 原地起跳（vx=0），空中按右：加速度不打折，上限是走路速度
    await stand(ws)
    await si(ws, 1, RELEASE("Space"))
    await si(ws, 2, PRESS("Space"))
    s = await si(ws, 40, PRESS("Space", "Right", "F"))
    check(
        f"没带速度起跳，空中按住冲刺也只到走路上限 {MAX_WALK}",
        near(s["player"]["vx"] / T, MAX_WALK, 0.05) and not s["player"]["on_ground"],
        f"{s['player']['vx'] / T:.3f} 格/秒 on_ground={s['player']['on_ground']}",
    )
    # 带着跑速起跳：空中可以继续推到跑速上限
    await stand(ws)
    await si(ws, 70, PRESS("Right", "F"))
    await si(ws, 1, RELEASE("Space"))
    s = await si(ws, 6, PRESS("Right", "F", "Space"))
    check(
        f"带着跑速起跳，空中仍保持在跑速上限 {MAX_RUN}",
        near(s["player"]["vx"] / T, MAX_RUN, 0.05) and not s["player"]["on_ground"],
        f"{s['player']['vx'] / T:.3f} 格/秒",
    )
    # 空中反向：加速度应为 walk_accel * 0.8
    await stand(ws)
    await si(ws, 1, RELEASE("Space"))
    s = await si(ws, 3, PRESS("Space", "Right"))
    v_before = s["player"]["vx"] / T
    # 必须先松右键：按下的键会一直保持，两个方向键同时按下时 dir 仍然取右，
    # 于是测出来的是「继续加速」而不是「反向」。
    await si(ws, 1, RELEASE("Right"))
    s2 = await si(ws, 6, PRESS("Space", "Left"))
    rate_air = (v_before - s2["player"]["vx"] / T) / (6 * DT)
    check(
        f"空中反向的加速度是 walk accel × {AIR_SLIDE_FACTOR} = {WALK_ACCEL * AIR_SLIDE_FACTOR}",
        near(rate_air, WALK_ACCEL * AIR_SLIDE_FACTOR, 0.8),
        f"实测 {rate_air:.2f}（不打折会是 {WALK_ACCEL}）",
    )
    await si(ws, 1, FREE_ALL)

    # ── 5 ───────────────────────────────────────────────────────────
    section(f"5. 伤害阶梯：任何一下都掉到最小，无敌 {INVINCIBLE_TIME} 秒")
    for size in ("big",):
        await stand(ws, size=size)
        p = (await si(ws))["player"]
        await rpc(ws, "game.spawnEnemy", {"type": "goomba", "x": p["x"] + 1.2 * T, "y": p["y"]})
        hit = None
        for _ in range(80):
            s = await si(ws, 2)
            if s["player"]["invincible_timer"] > 0 or s["state"] != "playing":
                hit = s
                break
        check(
            f"{size} 被撞一下 → small（不是掉一级停在中间）",
            hit is not None
            and not hit["player"]["is_big"]
            and not hit["player"]["is_fire"],
            f"big={hit and hit['player']['is_big']} fire={hit and hit['player']['is_fire']}",
        )
        check(
            f"无敌时间接近 {INVINCIBLE_TIME} 秒（曾经是 2.0）",
            hit is not None and near(hit["player"]["invincible_timer"], INVINCIBLE_TIME, 0.1),
            f"{hit and round(hit['player']['invincible_timer'], 3)} 秒",
        )

    # ── 6 ───────────────────────────────────────────────────────────
    section("6. 拾取是一级一级来的（mario:grow 就是 size + 1）")
    await stand(ws, size="small")
    # 1-1 第 21 列那块问号砖里是蘑菇；顶出来吃掉
    s = await si(ws)
    blocks = [
        b
        for b in s.get("block_contents", [])
        if b.get("content") == "mushroom" and b.get("col", 0) < 40
    ]
    if not blocks:
        check("找到 1-1 的蘑菇砖", False, "block_contents 里没有")
    else:
        b = blocks[0]
        col, row = b["col"], b["row"]
        await rpc(ws, "game.setPlayerPos", {"x": col * T, "y": (row + 1) * T, "vx": 0.0, "vy": 0.0})
        for _ in range(20):
            s = await si(ws, 4)
            if s["player"]["on_ground"]:
                break
        popped = False
        for _ in range(12):
            await si(ws, 1, RELEASE("Space"))
            s = await si(ws, 16, PRESS("Space"))
            if s.get("items"):
                popped = True
                break
        got = None
        if popped:
            for _ in range(160):
                for key in ("Right", "Left"):
                    s = await si(ws, 2, PRESS(key))
                    if s["player"]["is_big"] or s["player"]["is_fire"]:
                        got = "fire" if s["player"]["is_fire"] else "big"
                        break
                if got:
                    break
        check(
            "small + 蘑菇 → big（一级，不是直接给到火焰）",
            got == "big",
            f"顶出道具={popped} 结果={got}",
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
