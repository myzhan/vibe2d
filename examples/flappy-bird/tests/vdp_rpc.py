#!/usr/bin/env python3
"""
VDP 单次 RPC 调用工具

用法：
  python3 tests/vdp_rpc.py <method> [params_json] [id]

示例：
  python3 tests/vdp_rpc.py engine.info
  python3 tests/vdp_rpc.py engine.pause
  python3 tests/vdp_rpc.py engine.resume
  python3 tests/vdp_rpc.py engine.step '{"frames": 3}'
  python3 tests/vdp_rpc.py engine.getTime
  python3 tests/vdp_rpc.py engine.simulateInput '{"device": "keyboard", "action": "tap", "key": "Space"}'
  python3 tests/vdp_rpc.py engine.simulateInput '{"device": "mouse", "action": "move", "x": 100, "y": 50}'
  python3 tests/vdp_rpc.py engine.simulateInput '{"device": "mouse", "action": "click", "button": "Left"}'
  python3 tests/vdp_rpc.py game.inspect
  python3 tests/vdp_rpc.py game.setState '{"state": "idle"}'
  python3 tests/vdp_rpc.py game.setBirdY '{"y": 100.0}'
  python3 tests/vdp_rpc.py game.setScore '{"score": 10}'
  python3 tests/vdp_rpc.py game.screenshot '{"path": "/tmp/screenshot.png"}'

依赖: pip install websockets
"""
import asyncio
import json
import sys
import websockets

async def call(method, params=None, req_id=1):
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    async with websockets.connect("ws://127.0.0.1:9229") as ws:
        payload = json.dumps(msg)
        print(f">>> {payload}")
        await ws.send(payload)
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        print(f"<<< {resp}")
        return json.loads(resp)

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    method = sys.argv[1]
    params = json.loads(sys.argv[2]) if len(sys.argv) > 2 else None
    req_id = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    asyncio.run(call(method, params, req_id))
