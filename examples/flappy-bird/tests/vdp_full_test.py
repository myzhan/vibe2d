#!/usr/bin/env python3
"""
Vibe2D VDP 全流程验证脚本（v2）
通过 VDP 协议验证 debugger 功能（pause/resume/step/simulateInput/getTime）
并使用键盘模拟自动操控 Flappy Bird 完成得分。

用法：
  1. 先启动游戏: cd examples/flappy-bird && cargo run -p flappy-bird
  2. 运行本脚本: python3 tests/vdp_full_test.py

依赖: pip install websockets
"""
import asyncio
import json
import time
import websockets

WS_URL = "ws://127.0.0.1:9229"
req_id = 0

# ── 游戏常量（与 main.rs 对应）──
BIRD_X = 128.0
BIRD_W = 36.0
BIRD_H = 27.0
BIRD_LEFT = BIRD_X - BIRD_W / 2.0
BIRD_RIGHT = BIRD_X + BIRD_W / 2.0
PIPE_W = 50.0
PIPE_GAP = 70.0
GRAVITY = 500.0
JUMP_VY = -200.0
PIPE_SPEED = 200.0
GAP_HALF = PIPE_GAP / 2.0
DT = 1.0 / 60.0
TARGET_SCORE = 2
# ground_top 从 inspect 动态获取，这里仅作为 fallback
DEFAULT_GROUND_TOP = 258.0


async def rpc(ws, method, params=None):
    """发送 JSON-RPC 请求并打印请求/响应。"""
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    payload = json.dumps(msg)
    print(f"\n>>> {payload}")
    await ws.send(payload)
    resp = await asyncio.wait_for(ws.recv(), timeout=5)
    parsed = json.loads(resp)
    print(f"<<< {json.dumps(parsed, indent=2, ensure_ascii=False)}")
    return parsed


async def rpc_quiet(ws, method, params=None):
    """发送 JSON-RPC 请求，不打印（用于高频控制循环）。"""
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    await ws.send(json.dumps(msg))
    resp = await asyncio.wait_for(ws.recv(), timeout=5)
    return json.loads(resp)


def section(num, title):
    print(f"\n{'─' * 50}")
    print(f"【测试 {num}】{title}")
    print("─" * 50)


async def step_and_wait(ws, frames=1):
    """请求步进 N 帧并等待完成。"""
    r = await rpc_quiet(ws, "engine.getTime")
    fc_before = r["result"]["frame_count"]
    await rpc_quiet(ws, "engine.step", {"frames": frames})
    for _ in range(200):
        r = await rpc_quiet(ws, "engine.getTime")
        if r["result"]["frame_count"] >= fc_before + frames:
            return r
        await asyncio.sleep(0.005)
    return r


# ── 轨迹模拟辅助函数 ──

def sim_trajectory(y, vy, n_frames, ground_top):
    """
    模拟 n 帧无拍翅轨迹，返回 [(y, vy), ...]。
    物理逻辑与 main.rs 完全一致：先加重力，再更新位置。
    """
    out = []
    for _ in range(n_frames):
        vy += GRAVITY * DT
        y += vy * DT
        if y < 0.0:
            y, vy = 0.0, 0.0
        out.append((y, vy))
    return out


def sim_trajectory_with_flap_at(y, vy, n_frames, flap_frame, ground_top):
    """
    模拟 n 帧轨迹，在第 flap_frame 帧（0-indexed）执行拍翅。
    拍翅 = 将 vy 设为 JUMP_VY（在该帧的物理更新之前）。
    """
    out = []
    for i in range(n_frames):
        if i == flap_frame:
            vy = JUMP_VY
        vy += GRAVITY * DT
        y += vy * DT
        if y < 0.0:
            y, vy = 0.0, 0.0
        out.append((y, vy))
    return out


def check_collision(traj, pipe_snapshot, ground_top):
    """
    检测轨迹是否撞管道或地面。
    pipe_snapshot: [(x, gap_y), ...] — 管道当前位置快照
    返回碰撞帧索引（从 1 开始），0 表示无碰撞。
    """
    pipe_spd = PIPE_SPEED * DT
    for i, (y, _) in enumerate(traj):
        step = i + 1
        # 地面碰撞
        if y + BIRD_H > ground_top:
            return step
        for (px0, gy) in pipe_snapshot:
            px = px0 - pipe_spd * step
            # 水平重叠检查
            if px < BIRD_RIGHT and px + PIPE_W > BIRD_LEFT:
                # 垂直碰撞检查（上管道 / 下管道）
                if y < gy - GAP_HALF or y + BIRD_H > gy + GAP_HALF:
                    return step
    return 0


def check_pipe_collision_only(traj, pipe_snapshot):
    """
    仅检测轨迹是否撞管道（不检测地面）。
    用于决策时排除远期地面碰撞的干扰。
    返回碰撞帧索引（从 1 开始），0 表示无碰撞。
    """
    pipe_spd = PIPE_SPEED * DT
    for i, (y, _) in enumerate(traj):
        step = i + 1
        for (px0, gy) in pipe_snapshot:
            px = px0 - pipe_spd * step
            if px < BIRD_RIGHT and px + PIPE_W > BIRD_LEFT:
                if y < gy - GAP_HALF or y + BIRD_H > gy + GAP_HALF:
                    return step
    return 0


def check_pipe_collision_with_margin(traj, pipe_snapshot, margin=0.0):
    """
    检测轨迹是否撞管道（带安全边距，不检测地面）。
    margin: 间隙每侧缩小的像素数，增加安全裕度。
    返回碰撞帧索引（从 1 开始），0 表示无碰撞。
    """
    pipe_spd = PIPE_SPEED * DT
    for i, (y, _) in enumerate(traj):
        step = i + 1
        for (px0, gy) in pipe_snapshot:
            px = px0 - pipe_spd * step
            if px < BIRD_RIGHT and px + PIPE_W > BIRD_LEFT:
                if y < gy - GAP_HALF + margin or y + BIRD_H > gy + GAP_HALF - margin:
                    return step
    return 0


def get_relevant_pipes(pipes):
    """获取还没完全通过鸟的管道，按 x 排序。"""
    return sorted(
        [p for p in pipes if p["x"] + PIPE_W > BIRD_LEFT],
        key=lambda p: p["x"]
    )


def score_trajectory(traj, pipe_snapshot, ground_top):
    """
    给轨迹打分：评估轨迹的安全性。
    返回值越高越好。负值表示会碰撞。
    """
    pipe_spd = PIPE_SPEED * DT
    min_clearance = float('inf')

    for i, (y, _) in enumerate(traj):
        step = i + 1
        # 地面距离
        ground_clearance = ground_top - (y + BIRD_H)
        if ground_clearance < 0:
            return -1000 + step  # 撞地面，越早越差
        min_clearance = min(min_clearance, ground_clearance)

        # 天花板距离
        if y < 0:
            return -1000 + step
        min_clearance = min(min_clearance, y)

        # 管道距离
        for (px0, gy) in pipe_snapshot:
            px = px0 - pipe_spd * step
            if px < BIRD_RIGHT and px + PIPE_W > BIRD_LEFT:
                top_clearance = y - (gy - GAP_HALF)
                bot_clearance = (gy + GAP_HALF) - (y + BIRD_H)
                if top_clearance < 0 or bot_clearance < 0:
                    return -1000 + step  # 撞管道
                min_clearance = min(min_clearance, top_clearance, bot_clearance)

    return min_clearance


def find_target_y_for_pipes(pipes, ground_top, pipe_spd_frame):
    """
    根据当前管道状态，计算鸟应该瞄准的目标 Y 位置（鸟顶部坐标）。
    目标是让鸟中心对准最近管道的间隙中心。
    """
    relevant = get_relevant_pipes(pipes)
    if not relevant:
        return (ground_top / 2.0) - BIRD_H / 2.0

    # 找到最近的还没通过的管道
    nearest = relevant[0]
    target_gap_y = nearest["gap_y"]

    # 如果最近管道快通过了，开始混合下一个管道的目标
    if len(relevant) > 1:
        dist_to_clear = nearest["x"] + PIPE_W - BIRD_LEFT
        frames_to_clear = dist_to_clear / pipe_spd_frame
        blend_window = 35.0  # 提前更多帧开始过渡
        if frames_to_clear < blend_window:
            blend = max(0.0, 1.0 - frames_to_clear / blend_window)
            target_gap_y = nearest["gap_y"] * (1.0 - blend) + relevant[1]["gap_y"] * blend

    # 鸟的目标 Y = 间隙中心 - 鸟高度的一半
    return target_gap_y - BIRD_H / 2.0


def check_collision_full(traj, snap, ground_top):
    """
    检测轨迹是否撞管道或地面。
    返回碰撞帧索引（从 1 开始），0 表示无碰撞。
    """
    pipe_spd = PIPE_SPEED * DT
    for i, (y, _) in enumerate(traj):
        step = i + 1
        # 地面碰撞
        if y + BIRD_H > ground_top:
            return step
        # 管道碰撞
        for (px0, gy) in snap:
            px = px0 - pipe_spd * step
            if px < BIRD_RIGHT and px + PIPE_W > BIRD_LEFT:
                if y < gy - GAP_HALF or y + BIRD_H > gy + GAP_HALF:
                    return step
    return 0


def predict_y_at_pipe(by, bvy, pipe_x, ground_top):
    """
    预测鸟在管道到达时的 y 位置（不拍翅）。
    返回 (y, vy) 在管道前缘到达鸟右边缘时的状态。
    """
    frames_to_pipe = max(0, (pipe_x - BIRD_RIGHT) / (PIPE_SPEED * DT))
    n = int(frames_to_pipe)
    cy, cvy = by, bvy
    for _ in range(n):
        cvy += GRAVITY * DT
        cy += cvy * DT
        if cy < 0:
            cy, cvy = 0.0, 0.0
        if cy + BIRD_H > ground_top:
            cy = ground_top - BIRD_H
            cvy = 0.0
    return cy, cvy


def sim_future_with_rule(y, vy, target_y, pipe_x, gap_y, n_frames, ground_top):
    """
    用简单规则模拟 n_frames 帧，返回存活帧数。
    简单规则：y >= target_y and vy > 0 → 拍翅
    """
    cy, cvy = y, vy
    pipe_spd = PIPE_SPEED * DT
    for i in range(n_frames):
        flap = False
        if cy < 5.0 and cvy <= 0:
            flap = False
        elif cy > ground_top - BIRD_H - 15 and cvy >= 0:
            flap = True
        elif cy >= target_y and cvy > 0:
            flap = True
        if flap:
            cvy = JUMP_VY
        cvy += GRAVITY * DT
        cy += cvy * DT
        if cy < 0:
            cy, cvy = 0.0, 0.0
        cpx = pipe_x - pipe_spd * (i + 1)
        if cy + BIRD_H > ground_top:
            return i + 1
        if cpx < BIRD_RIGHT and cpx + PIPE_W > BIRD_LEFT:
            if cy < gap_y - GAP_HALF or cy + BIRD_H > gap_y + GAP_HALF:
                return i + 1
    return n_frames + 1


async def autopilot_keyboard(ws):
    """
    Pause+Step 逐帧控制 autopilot（v18 策略）。

    核心策略："落到下管道正上方再拍翅"
    1. 简单规则：y >= target_y and vy > 0 → 拍翅
       target_y = gap_y + 4.0（安全窗口内，给上下都留余量）
    2. 碰撞前瞻：模拟两种选择各 120 帧（内部用简单规则）
       - 两种都安全 → 用简单规则
       - 否则 → 选存活更久的
    3. 地面/天花板保护
    """
    last_score = 0
    frame = 0
    ground_top = DEFAULT_GROUND_TOP
    LOOKAHEAD = 120
    FLAP_TARGET_OFFSET = 4.0  # target_y = gap_y + 4.0

    print(f"    autopilot 开始（v18 策略），目标: {TARGET_SCORE} 分")

    while True:
        r = await rpc_quiet(ws, "game.inspect")
        res = r.get("result", {})
        state, score = res.get("state", ""), res.get("score", 0)
        bird, pipes = res.get("bird", {}), res.get("pipes", [])

        if state != "playing":
            await rpc_quiet(ws, "engine.resume")
            print(f"    [f{frame}] state={state}, exit")
            return score, state

        by = bird.get("y", 0.0)
        bvy = bird.get("vy", 0.0)

        if score != last_score:
            print(f"    [f{frame}] ** score={score} (y={by:.1f} vy={bvy:.1f})")
            last_score = score

        if score >= TARGET_SCORE:
            await rpc_quiet(ws, "engine.resume")
            print(f"    [f{frame}] target reached: score={score}")
            return score, "playing"

        # 找最近的管道
        relevant_pipes = get_relevant_pipes(pipes)
        nearest = relevant_pipes[0] if relevant_pipes else None

        want_flap = False
        if nearest:
            pipe_x = nearest["x"]
            gap_y = nearest["gap_y"]
            target_y = gap_y + FLAP_TARGET_OFFSET

            # 碰撞前瞻：模拟两种选择
            survive_coast = sim_future_with_rule(
                by, bvy, target_y, pipe_x, gap_y, LOOKAHEAD, ground_top)
            survive_flap = sim_future_with_rule(
                by, JUMP_VY, target_y, pipe_x, gap_y, LOOKAHEAD, ground_top)

            if survive_coast > LOOKAHEAD and survive_flap > LOOKAHEAD:
                # 两种都安全：用简单规则
                if by < 5.0 and bvy <= 0:
                    want_flap = False
                elif by > ground_top - BIRD_H - 15 and bvy >= 0:
                    want_flap = True
                elif by >= target_y and bvy > 0:
                    want_flap = True
            elif survive_flap > survive_coast:
                want_flap = True
            else:
                want_flap = False
        else:
            # 没有管道：保持在屏幕中间
            target = ground_top / 2.0
            if by >= target and bvy > 0:
                want_flap = True

        if want_flap:
            await rpc_quiet(ws, "engine.simulateInput",
                            {"device": "keyboard", "action": "tap", "key": "Space"})

        # 日志
        if frame % 60 == 0:
            p_info = ""
            if nearest:
                p_info = f" pipe=({nearest['x']:.0f},gap={nearest['gap_y']:.0f})"
            print(f"    [f{frame}] y={by:.1f} vy={bvy:.1f} sc={score}{p_info}")

        await step_and_wait(ws, 1)
        frame += 1

        if frame > 3000:
            await rpc_quiet(ws, "engine.resume")
            print(f"    [f{frame}] frame limit reached")
            return score, "timeout"


async def wait_for_death(ws, timeout=10.0):
    """停止操控后，等待小鸟自然死亡。"""
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        r = await rpc_quiet(ws, "game.inspect")
        state = r.get("result", {}).get("state", "")
        if state == "dead":
            return r
        await asyncio.sleep(0.05)
    return None


async def main():
    print("=" * 60)
    print("Vibe2D VDP 全流程验证 v2（debugger + 键盘模拟）")
    print("=" * 60)

    async with websockets.connect(WS_URL) as ws:
        # ━━━━━━━━ 阶段一：引擎信息 ━━━━━━━━
        section(1, "engine.info — 查询引擎基本信息")
        await rpc(ws, "engine.info")

        # ━━━━━━━━ 阶段二：getTime ━━━━━━━━
        section(2, "engine.getTime — 查询时间信息")
        r = await rpc(ws, "engine.getTime")
        result = r.get("result", {})
        assert "frame_count" in result, "缺少 frame_count"
        assert "elapsed_time" in result, "缺少 elapsed_time"
        assert "paused" in result, "缺少 paused"
        print("    OK getTime 字段完整")

        # ━━━━━━━━ 阶段三：Pause/Resume ━━━━━━━━
        section(3, "engine.pause — 暂停引擎")
        r = await rpc(ws, "engine.pause")
        assert r["result"]["paused"] is True
        fc1 = r["result"]["frame_count"]
        print(f"    OK 已暂停, frame_count={fc1}")

        await asyncio.sleep(0.5)

        r = await rpc(ws, "engine.getTime")
        fc2 = r["result"]["frame_count"]
        assert fc2 == fc1, f"帧数应冻结: {fc2} != {fc1}"
        print(f"    OK 暂停期间 frame_count 未变化: {fc2}")

        section(4, "engine.resume — 恢复引擎")
        r = await rpc(ws, "engine.resume")
        assert r["result"]["paused"] is False
        print("    OK 已恢复")

        await asyncio.sleep(0.3)

        r = await rpc(ws, "engine.getTime")
        fc3 = r["result"]["frame_count"]
        assert fc3 > fc2, f"恢复后帧数应增加: {fc3} <= {fc2}"
        print(f"    OK 恢复后 frame_count 增加: {fc2} -> {fc3}")

        # ━━━━━━━━ 阶段四：Step ━━━━━━━━
        section(5, "engine.step — 精确步进")
        await rpc(ws, "engine.pause")
        r = await rpc(ws, "engine.getTime")
        fc_before = r["result"]["frame_count"]

        step_count = 5
        r = await rpc(ws, "engine.step", {"frames": step_count})
        assert r["result"]["frames"] == step_count
        print(f"    OK 请求步进 {step_count} 帧")

        await asyncio.sleep(0.3)

        r = await rpc(ws, "engine.getTime")
        fc_after = r["result"]["frame_count"]
        assert fc_after == fc_before + step_count, \
            f"步进后帧数不正确: {fc_after} != {fc_before} + {step_count}"
        print(f"    OK 步进后 frame_count: {fc_before} -> {fc_after} (+{step_count})")

        # step 未暂停时应报错
        await rpc(ws, "engine.resume")
        r = await rpc(ws, "engine.step", {"frames": 1})
        assert "error" in r, "未暂停时 step 应返回错误"
        print("    OK 未暂停时 step 正确返回错误")

        # ━━━━━━━━ 阶段五：键盘模拟 + Step ━━━━━━━━
        section(6, "simulateInput + step — 暂停状态下模拟键盘触发状态变化")
        await rpc(ws, "game.setState", {"state": "idle"})
        await rpc(ws, "engine.pause")

        r = await rpc(ws, "engine.simulateInput",
                       {"device": "keyboard", "action": "tap", "key": "Space"})
        assert r["result"]["queued"] is True
        print("    OK 模拟输入已入队")

        await rpc(ws, "engine.step", {"frames": 1})
        await asyncio.sleep(0.1)

        r = await rpc(ws, "game.inspect")
        state = r["result"]["state"]
        assert state == "countdown", f"期望 countdown，实际 {state}"
        print(f"    OK tap Space 后状态变为: {state}")

        await rpc(ws, "engine.resume")

        # ━━━━━━━━ 阶段六：鼠标模拟 ━━━━━━━━
        section(7, "simulateInput(mouse) — 鼠标输入模拟")
        r = await rpc(ws, "engine.simulateInput",
                       {"device": "mouse", "action": "move", "x": 100.0, "y": 50.0})
        assert r["result"]["queued"] is True
        print("    OK 鼠标移动已入队")

        r = await rpc(ws, "engine.simulateInput",
                       {"device": "mouse", "action": "click", "button": "Left"})
        assert r["result"]["queued"] is True
        print("    OK 鼠标点击已入队")

        r = await rpc(ws, "engine.simulateInput",
                       {"device": "gamepad", "action": "press", "button": "A"})
        assert "error" in r
        print("    OK gamepad 正确返回 not yet supported 错误")

        # ━━━━━━━━ 阶段七：键盘模拟 Autopilot ━━━━━━━━
        section(8, "键盘模拟 Autopilot — 自动操控小鸟通过管道")
        # 先暂停，再重置游戏（避免初始化期间帧跑掉）
        await rpc(ws, "engine.pause")
        await rpc(ws, "game.setState", {"state": "idle"})
        await rpc(ws, "game.setState", {"state": "countdown"})  # reset_game
        await rpc(ws, "game.setState", {"state": "playing"})    # 跳过倒计时
        # 设置初始状态：屏幕中央，平稳起步
        await rpc(ws, "game.setBirdY", {"y": 130.0, "vy": 0.0})

        r = await rpc(ws, "game.inspect")
        state = r.get("result", {}).get("state", "")
        print(f"    -> 当前状态: {state}")

        if state == "playing":
            final_score, exit_state = await autopilot_keyboard(ws)

            # 验证得分
            section(9, "验证 Autopilot 结果")
            print(f"    最终分数: {final_score}, 退出状态: {exit_state}")
            assert final_score >= TARGET_SCORE, \
                f"Autopilot 未达到目标分数: {final_score} < {TARGET_SCORE}"
            print(f"    OK 得分 {final_score} >= {TARGET_SCORE}，Autopilot 验证通过!")

            if exit_state == "playing":
                print("    小鸟不再受控，等待重力和碰撞...")
                death_result = await wait_for_death(ws)
                if death_result:
                    result = death_result["result"]
                    print(f"    -> 小鸟已死亡! 最终分数: {result['score']}")
                else:
                    print("    -> 超时，小鸟仍未死亡")
            else:
                print(f"    小鸟已在控制循环中死亡 (state={exit_state})")
        else:
            print(f"    WARNING: 未进入 playing 状态，跳过 autopilot")
            assert False, f"期望 playing 状态，实际 {state}"

        # ━━━━━━━━ 阶段八：远程修改验证 ━━━━━━━━
        section(10, "远程修改验证 — setBirdY / setScore / setState")
        await rpc(ws, "game.setBirdY", {"y": 100.0})
        await rpc(ws, "game.setScore", {"score": 99})
        await rpc(ws, "game.inspect")

        # ━━━━━━━━ 阶段九：截图 ━━━━━━━━
        section(11, "game.screenshot — VDP 远程截图")
        await rpc(ws, "game.screenshot", {"path": "/tmp/vdp_v2_screenshot.png"})
        await asyncio.sleep(0.5)

        # ━━━━━━━━ 阶段十：错误处理 ━━━━━━━━
        section(12, "错误处理")
        await rpc(ws, "game.nonexistent", {"foo": "bar"})
        await rpc(ws, "engine.simulateInput",
                   {"device": "keyboard", "action": "tap", "key": "InvalidKey"})

    print("\n" + "=" * 60)
    print("VDP v2 全流程验证完成")
    print("=" * 60)

asyncio.run(main())
