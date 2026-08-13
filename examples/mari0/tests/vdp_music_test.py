#!/usr/bin/env python3
"""
mari0 音乐与计时规则验证脚本

原版有三处很容易做错、而单元测试观察不到（涉及真实音频引擎与逐帧时序）的行为：

  1. 时钟按 **2.5 单位/秒** 走，不是 1（`mariotime -= 2.5*dt`）。
  2. 跨过 99 时 **停掉全部音频** 并播警告音 —— 主题曲不在底下继续。
  3. 再过 **7.5 个时间单位**（3 秒实时）才从零开始播 `-fast` 变体，无交叉淡入。

用法：
  1. 先启动游戏: cargo run -p mari0 --features vdp
  2. 运行本脚本: python3 examples/mari0/tests/vdp_music_test.py

依赖: pip install websockets
"""
import asyncio
import json
import sys

import websockets

WS_URL = "ws://127.0.0.1:9229"
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
    """`engine.step` is queued, not synchronous — wait for the frames to land."""
    before = (await rpc(ws, "engine.getTime"))["frame_count"]
    await rpc(ws, "engine.step", {"frames": frames})
    for _ in range(400):
        now = (await rpc(ws, "engine.getTime"))["frame_count"]
        if now >= before + frames:
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


async def run(ws):
    await rpc(ws, "engine.pause")

    section("1. 关卡 music 字段决定曲目，时钟按 2.5 倍速走")
    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
    await rpc(ws, "game.reset")
    await step(ws)
    s = await snap(ws)
    check("1-1 是地上曲 (music=2)", s["level"]["music"] == 2, f"music={s['level']['music']}")
    check("1-1 限时 400", s["level"]["time_limit"] == 400.0)
    check("初始处于 normal 阶段", s["music_phase"] == "normal", s["music_phase"])

    before = (await snap(ws))["time_remaining"]
    await step(ws, 60)  # engine.step 按标定的 1/60s 推进，60 帧 = 1 秒
    after = (await snap(ws))["time_remaining"]
    elapsed = before - after
    check(
        "60 帧消耗 2.5 个时间单位（不是 1）",
        abs(elapsed - 2.5) < 0.1,
        f"实测 {elapsed:.3f}",
    )

    section("2. background / spriteset / music 曾被静默忽略，现在读得到")
    # 用 1-4 而不是 1-2：1-2 是 24 宽的**过场桩**（intermission），它自己就是
    # music=2 / background=1 的地上设定，真正的地下关是子关 1-2_1。城堡关 W-4 才是
    # 顶层关卡里唯一与 1-1 环境不同的一组。
    await rpc(ws, "game.setLevel", {"world": 1, "level": 4})
    await step(ws)
    s = await snap(ws)
    check("1-4 music=4（城堡）", s["level"]["music"] == 4, f"music={s['level']['music']}")
    check("1-4 spriteset=3（城堡贴图集）", s["level"]["spriteset"] == 3, f"spriteset={s['level']['spriteset']}")
    check("1-4 背景不是天空蓝", s["level"]["background"] == 2, f"bg={s['level']['background']}")

    section("3. 跨过 99 → warning；再过 7.5 单位 → fast")
    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
    await rpc(ws, "game.reset")
    await step(ws)
    # 直接把时钟放到 99 之上一点，省掉 7200 帧的空转。
    await rpc(ws, "game.setTime", {"time": 100.0})
    s = await snap(ws)
    check("时钟已设到 100", abs(s["time_remaining"] - 100.0) < 0.01, f"time={s['time_remaining']:.2f}")
    check("此时仍是 normal", s["music_phase"] == "normal", s["music_phase"])

    for _ in range(60):
        await step(ws, 6)
        s = await snap(ws)
        if s["music_phase"] != "normal":
            break
    check("跨过 99 后进入 warning 阶段", s["music_phase"] == "warning", s["music_phase"])
    warned_at = s["time_remaining"]
    check("warning 发生在 99 以下", warned_at <= 99.0, f"time={warned_at:.2f}")

    for _ in range(120):
        await step(ws, 6)
        s = await snap(ws)
        if s["music_phase"] == "fast":
            break
    check("随后切换到 fast 阶段", s["music_phase"] == "fast", s["music_phase"])
    gap = warned_at - s["time_remaining"]
    check(
        "warning → fast 间隔 7.5 个时间单位（3 秒实时）",
        7.0 < gap < 8.5,
        f"实测 {gap:.2f} 单位",
    )

    section("4. 时间归零 = 死亡")
    await rpc(ws, "game.setTime", {"time": 0.5})
    for _ in range(60):
        await step(ws, 6)
        s = await snap(ws)
        if s["state"] == "dead":
            break
    check(
        "时间耗尽后玩家死亡",
        s["state"] == "dead",
        f"state={s['state']}, time={s['time_remaining']:.2f}",
    )

    section("5. 无限时关卡（timelimit=0）时钟不走")
    # 实验室 mappack 全部不限时；时钟若照走会在进关瞬间杀死玩家。
    await rpc(ws, "game.setLevel", {"pack": "portal", "world": 1, "level": 1})
    await rpc(ws, "game.reset")
    await step(ws)
    s = await snap(ws)
    check("portal 1-1 不限时", s["level"]["time_limit"] == 0.0, f"limit={s['level']['time_limit']}")
    before = s["time_remaining"]
    await step(ws, 120)
    s = await snap(ws)
    check("120 帧后时钟没动", s["time_remaining"] == before, f"{before} → {s['time_remaining']}")
    check("玩家没有因超时死亡", s["state"] != "dead", f"state={s['state']}")

    # 复位到 smb 1-1，避免影响后续脚本。
    await rpc(ws, "game.setLevel", {"world": 1, "level": 1})
    await rpc(ws, "game.reset")
    await rpc(ws, "engine.resume")


async def main():
    print("=" * 60)
    print("mari0 音乐与计时规则验证")
    print("=" * 60)
    try:
        async with websockets.connect(WS_URL) as ws:
            await run(ws)
    except (OSError, websockets.exceptions.WebSocketException) as e:
        print(f"错误: 无法连接到游戏 ({e})。请先启动:")
        print("  cargo run -p mari0 --features vdp")
        return 1

    print()
    if FAILURES:
        print(f"失败 {len(FAILURES)} 项: {FAILURES}")
        return 1
    print("音乐与计时规则全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
