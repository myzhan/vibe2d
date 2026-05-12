#!/usr/bin/env python3
"""
Tetris AI 玩家 — 通过 VDP 协议自动玩俄罗斯方块

使用 Pierre Dellacherie 评估函数，在 pause+step 模式下逐帧控制，
尝试获得尽可能高的分数。

用法：
  1. 先启动游戏: cd examples/tetris && cargo run -p tetris
  2. 运行本脚本: python3 tests/ai_player.py [--lookahead]

依赖: pip install websockets
"""
import asyncio
import json
import sys
import time
import websockets

WS_URL = "ws://127.0.0.1:9229"
req_id = 0
TOTAL_ROWS = 40
COLS = 10

# ── SRS piece shapes (must match Rust code) ─────────────────────────
# SHAPES[piece_type_idx][rotation_idx] = [(dr, dc), (dr, dc), (dr, dc), (dr, dc)]
SHAPES = {
    "I": [
        [(0,-1),(0,0),(0,1),(0,2)],
        [(-1,1),(0,1),(1,1),(2,1)],
        [(1,-1),(1,0),(1,1),(1,2)],
        [(-1,0),(0,0),(1,0),(2,0)],
    ],
    "O": [
        [(0,0),(0,1),(1,0),(1,1)],
        [(0,0),(0,1),(1,0),(1,1)],
        [(0,0),(0,1),(1,0),(1,1)],
        [(0,0),(0,1),(1,0),(1,1)],
    ],
    "T": [
        [(-1,0),(0,-1),(0,0),(0,1)],
        [(-1,0),(0,0),(0,1),(1,0)],
        [(0,-1),(0,0),(0,1),(1,0)],
        [(-1,0),(0,-1),(0,0),(1,0)],
    ],
    "S": [
        [(-1,0),(-1,1),(0,-1),(0,0)],
        [(-1,0),(0,0),(0,1),(1,1)],
        [(0,0),(0,1),(1,-1),(1,0)],
        [(-1,-1),(0,-1),(0,0),(1,0)],
    ],
    "Z": [
        [(-1,-1),(-1,0),(0,0),(0,1)],
        [(-1,1),(0,0),(0,1),(1,0)],
        [(0,-1),(0,0),(1,0),(1,1)],
        [(-1,0),(0,-1),(0,0),(1,-1)],
    ],
    "J": [
        [(-1,-1),(0,-1),(0,0),(0,1)],
        [(-1,0),(-1,1),(0,0),(1,0)],
        [(0,-1),(0,0),(0,1),(1,1)],
        [(-1,0),(0,0),(1,-1),(1,0)],
    ],
    "L": [
        [(-1,1),(0,-1),(0,0),(0,1)],
        [(-1,0),(0,0),(1,0),(1,1)],
        [(0,-1),(0,0),(0,1),(1,-1)],
        [(-1,-1),(-1,0),(0,0),(1,0)],
    ],
}

# ── Evaluation weights (Pierre Dellacherie style) ───────────────────
W_AGGREGATE_HEIGHT = -0.510066
W_COMPLETE_LINES   =  0.760666
W_HOLES            = -0.356630
W_BUMPINESS        = -0.184483

# ── RPC helpers ──────────────────────────────────────────────────────

async def rpc_quiet(ws, method, params=None):
    global req_id
    req_id += 1
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    await ws.send(json.dumps(msg))
    resp = await asyncio.wait_for(ws.recv(), timeout=5)
    data = json.loads(resp)
    if "error" in data:
        raise ConnectionError(data["error"].get("message", "RPC error"))
    return data


async def step_and_wait(ws, frames=1):
    r = await rpc_quiet(ws, "engine.getTime")
    fc_before = r["result"]["frame_count"]
    await rpc_quiet(ws, "engine.step", {"frames": frames})
    for _ in range(200):
        r = await rpc_quiet(ws, "engine.getTime")
        if r["result"]["frame_count"] >= fc_before + frames:
            return r
        await asyncio.sleep(0.005)
    return r


async def tap_key(ws, key):
    await rpc_quiet(ws, "engine.simulateInput",
                    {"device": "keyboard", "action": "tap", "key": key})
    await step_and_wait(ws, 1)


async def inspect(ws):
    r = await rpc_quiet(ws, "game.inspect")
    return r.get("result", {})


# ── Grid simulation (Python) ────────────────────────────────────────

def get_cells(piece_type, rotation, x, y):
    """Get absolute cell positions for a piece."""
    shape = SHAPES[piece_type][rotation]
    return [(y + dr, x + dc) for (dr, dc) in shape]


def collides(piece_type, rotation, x, y, grid):
    """Check if piece collides with grid or walls."""
    for (r, c) in get_cells(piece_type, rotation, x, y):
        if c < 0 or c >= COLS:
            return True
        if r >= TOTAL_ROWS:
            return True
        if 0 <= r < TOTAL_ROWS and grid[r][c] is not None:
            return True
    return False


def drop_y(piece_type, rotation, x, y, grid):
    """Find the lowest valid Y position (ghost position)."""
    while not collides(piece_type, rotation, x, y + 1, grid):
        y += 1
    return y


def simulate_placement(grid, piece_type, rotation, x, y):
    """Place piece on grid and return (new_grid, lines_cleared)."""
    # Deep copy grid
    new_grid = [row[:] for row in grid]

    # Lock piece
    for (r, c) in get_cells(piece_type, rotation, x, y):
        if 0 <= r < TOTAL_ROWS and 0 <= c < COLS:
            new_grid[r][c] = piece_type

    # Clear lines
    lines = 0
    rows_to_keep = []
    for r in range(TOTAL_ROWS):
        if all(cell is not None for cell in new_grid[r]):
            lines += 1
        else:
            rows_to_keep.append(new_grid[r])

    # Rebuild grid
    empty_rows = [[None] * COLS for _ in range(TOTAL_ROWS - len(rows_to_keep))]
    result_grid = empty_rows + rows_to_keep

    return result_grid, lines


def column_heights(grid):
    """Get height of each column (from bottom)."""
    heights = [0] * COLS
    for c in range(COLS):
        for r in range(TOTAL_ROWS):
            if grid[r][c] is not None:
                heights[c] = TOTAL_ROWS - r
                break
    return heights


def count_holes(grid):
    """Count holes (empty cells with filled cell above in same column)."""
    holes = 0
    for c in range(COLS):
        found_block = False
        for r in range(TOTAL_ROWS):
            if grid[r][c] is not None:
                found_block = True
            elif found_block:
                holes += 1
    return holes


def bumpiness(heights):
    """Sum of absolute height differences between adjacent columns."""
    bump = 0
    for i in range(len(heights) - 1):
        bump += abs(heights[i] - heights[i + 1])
    return bump


def evaluate(grid, lines_cleared):
    """Evaluate a board state. Higher is better."""
    heights = column_heights(grid)
    agg_height = sum(heights)
    holes = count_holes(grid)
    bump = bumpiness(heights)

    return (W_AGGREGATE_HEIGHT * agg_height +
            W_COMPLETE_LINES * lines_cleared +
            W_HOLES * holes +
            W_BUMPINESS * bump)


# ── AI decision making ──────────────────────────────────────────────

def find_best_placement(piece_type, grid, next_type=None, use_lookahead=False):
    """
    Find the best placement for the current piece.
    Returns (rotation, target_x, score).
    """
    best = None
    best_score = float('-inf')

    for rot in range(4):
        # Skip duplicate rotations for O piece
        if piece_type == "O" and rot > 0:
            break

        for x in range(-2, COLS + 2):
            # Check if piece can exist at spawn position (roughly)
            if collides(piece_type, rot, x, 0, grid):
                # Try higher spawn
                if collides(piece_type, rot, x, -2, grid):
                    continue

            # Find drop position
            # Start from top
            start_y = 0
            while collides(piece_type, rot, x, start_y, grid) and start_y > -4:
                start_y -= 1
            if collides(piece_type, rot, x, start_y, grid):
                continue

            dy = drop_y(piece_type, rot, x, start_y, grid)
            new_grid, lines = simulate_placement(grid, piece_type, rot, x, dy)

            if use_lookahead and next_type:
                # Two-piece lookahead
                inner_best_score = float('-inf')
                for rot2 in range(4):
                    if next_type == "O" and rot2 > 0:
                        break
                    for x2 in range(-2, COLS + 2):
                        if collides(next_type, rot2, x2, 0, new_grid):
                            if collides(next_type, rot2, x2, -2, new_grid):
                                continue
                        start_y2 = 0
                        while collides(next_type, rot2, x2, start_y2, new_grid) and start_y2 > -4:
                            start_y2 -= 1
                        if collides(next_type, rot2, x2, start_y2, new_grid):
                            continue
                        dy2 = drop_y(next_type, rot2, x2, start_y2, new_grid)
                        grid2, lines2 = simulate_placement(new_grid, next_type, rot2, x2, dy2)
                        score2 = evaluate(grid2, lines + lines2)
                        if score2 > inner_best_score:
                            inner_best_score = score2

                score = inner_best_score if inner_best_score > float('-inf') else evaluate(new_grid, lines)
            else:
                score = evaluate(new_grid, lines)

            if score > best_score:
                best_score = score
                best = (rot, x, score)

    return best


async def execute_placement(ws, current_rot, current_x, target_rot, target_x):
    """Execute a placement by sending input commands."""
    # Calculate rotations needed (CW)
    rot_diff = (target_rot - current_rot) % 4
    if rot_diff == 3:
        # CCW is faster
        await tap_key(ws, "Z")
    else:
        for _ in range(rot_diff):
            await tap_key(ws, "Up")

    # Calculate horizontal movement
    dx = target_x - current_x
    key = "Left" if dx < 0 else "Right"
    for _ in range(abs(dx)):
        await tap_key(ws, key)

    # Hard drop
    await tap_key(ws, "Space")


# ── Main loop ────────────────────────────────────────────────────────

async def main():
    use_lookahead = "--lookahead" in sys.argv

    print("=" * 60)
    print(f"Tetris AI Player ({'两步前瞻' if use_lookahead else '单步评估'})")
    print("=" * 60)

    try:
        async with websockets.connect(WS_URL) as ws:
            await rpc_quiet(ws, "engine.pause")
            await rpc_quiet(ws, "game.reset")
            await step_and_wait(ws, 3)

            pieces_placed = 0
            start_time = time.time()

            while True:
                state = await inspect(ws)

                if state["phase"] != "playing":
                    print(f"\nGame Over!")
                    break

                if state["current"] is None:
                    await step_and_wait(ws, 3)
                    continue

                current = state["current"]
                piece_type = current["type"]
                cur_rot = current["rotation"]
                cur_x = current["x"]
                grid = state["grid"]
                next_type = state["next"][0] if state["next"] else None

                # Find best placement
                result = find_best_placement(
                    piece_type, grid, next_type, use_lookahead)

                if result is None:
                    print(f"  [#{pieces_placed}] 无法找到有效放置!")
                    await tap_key(ws, "Space")  # Just hard drop
                    await step_and_wait(ws, 3)
                    pieces_placed += 1
                    continue

                target_rot, target_x, score = result

                # Execute
                await execute_placement(ws, cur_rot, cur_x, target_rot, target_x)
                pieces_placed += 1

                # Wait for next piece to spawn
                await step_and_wait(ws, 3)

                # Periodic status
                if pieces_placed % 10 == 0:
                    state = await inspect(ws)
                    elapsed = time.time() - start_time
                    pps = pieces_placed / elapsed if elapsed > 0 else 0
                    print(f"  [#{pieces_placed:3d}] score={state['score']:6d}  "
                          f"level={state['level']:2d}  lines={state['lines']:3d}  "
                          f"({pps:.1f} pieces/sec)")

            # Final stats
            state = await inspect(ws)
            elapsed = time.time() - start_time
            print(f"\n{'─' * 55}")
            print(f"最终结果:")
            print(f"  分数:     {state['score']}")
            print(f"  等级:     {state['level']}")
            print(f"  消行:     {state['lines']}")
            print(f"  方块数:   {pieces_placed}")
            print(f"  耗时:     {elapsed:.1f}s")
            print(f"  速度:     {pieces_placed/elapsed:.1f} pieces/sec")
            print(f"{'─' * 55}")

            # Take screenshot
            await rpc_quiet(ws, "game.screenshot",
                           {"path": "/tmp/tetris_ai_final.png"})

            # Resume engine
            await rpc_quiet(ws, "engine.resume")

    except ConnectionRefusedError:
        print("错误: 无法连接到游戏。请先启动游戏:")
        print("  cd examples/tetris && cargo run -p tetris")
        sys.exit(1)
    except (ConnectionError, websockets.exceptions.ConnectionClosed,
            asyncio.TimeoutError) as e:
        print(f"\n连接断开: {e}")

asyncio.run(main())
