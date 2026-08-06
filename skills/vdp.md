# Vibe2D VDP Skill

This skill enables AI assistants to inspect and control a running Vibe2D game via the Vibe Debug Protocol (VDP).

## Prerequisites

- A Vibe2D game must be running with VDP enabled in `game.yaml`:
  ```yaml
  debug:
    vdp:
      enabled: true
      port: 9229
  ```
- The game must be compiled with the `vdp` feature (enabled by default). Use `--no-default-features` to strip VDP for release builds.

## Available Commands

### Inspect Game State
```bash
vibe inspect
```
Returns the full game state as JSON, including:
- Game state machine state (idle, countdown, playing, dead)
- Score and best score
- Entity positions (bird, pipes, etc.)

### Send Custom RPC
```bash
vibe rpc <method> [params_json]
```
Examples:
```bash
# Get engine info
vibe rpc engine.info

# Pause / resume / step
vibe rpc engine.pause
vibe rpc engine.resume
vibe rpc engine.step '{"frames": 1}'

# Get time info
vibe rpc engine.getTime

# Simulate keyboard input (tap = press + auto-release next frame)
vibe rpc engine.simulateInput '{"device": "keyboard", "action": "tap", "key": "Space"}'

# Simulate mouse input
vibe rpc engine.simulateInput '{"device": "mouse", "action": "move", "x": 256, "y": 144}'
vibe rpc engine.simulateInput '{"device": "mouse", "action": "click", "button": "Left"}'

# Get game state
vibe rpc game.inspect

# Set score
vibe rpc game.setScore '{"score": 42}'

# Change game state
vibe rpc game.setState '{"state": "playing"}'
```

### Create New Project
```bash
vibe new my-game
```

## VDP Protocol Reference

VDP uses WebSocket + JSON-RPC 2.0 on `ws://127.0.0.1:9229`.

### Request Format
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "game.inspect",
  "params": {}
}
```

### Response Format
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { ... }
}
```

### Built-in Methods

| Method | Params | Description |
|--------|--------|-------------|
| `engine.info` | — | Engine version, virtual resolution |
| `engine.pause` | — | Pause game loop (rendering continues) |
| `engine.resume` | — | Resume game loop |
| `engine.step` | `{"frames": N}` | Execute N frames while paused (default 1) |
| `engine.getTime` | — | Get frame count + elapsed time |
| `engine.simulateInput` | see below | Inject keyboard/mouse input |
| `engine.screenshot` | `{"path": "..."}` | Save screenshot to file |
| `game.inspect` | — | Full game state JSON |

#### engine.simulateInput

Keyboard:
```json
{"device": "keyboard", "action": "press|release|tap", "key": "Space"}
```

Mouse:
```json
{"device": "mouse", "action": "move", "x": 256.0, "y": 144.0}
{"device": "mouse", "action": "press|release|click", "button": "Left"}
```

- `tap` = press this frame, auto-release next frame
- `click` = press this frame, auto-release next frame (mouse equivalent of tap)

### Game-specific Methods (Flappy Bird)

| Method | Params | Description |
|--------|--------|-------------|
| `game.setScore` | `{"score": int}` | Set current score |
| `game.setState` | `{"state": string}` | Set game state (idle/countdown/playing/dead) |

## Implementing VDP in Your Game

Override `inspect()` and `handle_vdp()` in your Game trait using
`#[derive(Serialize)]` snapshots for `inspect` and the
`#[vibe2d::vdp::vdp_methods]` macro for `handle_vdp`. Add `dep:serde` to the
game's `vdp` feature.

```rust
// inspect: a derived snapshot struct reproduces the wire shape
#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
struct MyInspect { state: &'static str, score: u32, player: PlayerView }

#[cfg(feature = "vdp")]
#[derive(serde::Serialize)]
struct PlayerView { x: f32, y: f32 }

#[cfg(feature = "vdp")]
fn inspect(&self) -> serde_json::Value {
    let view = MyInspect {
        state: "playing",
        score: self.score,
        player: PlayerView { x: self.x, y: self.y },
    };
    serde_json::to_value(&view).unwrap_or(serde_json::Value::Null)
}

// handle_vdp: typed params + declarative dispatch
#[cfg(feature = "vdp")]
#[derive(serde::Deserialize)]
struct SetPlayerPos { x: f32, y: f32 }

#[cfg(feature = "vdp")]
#[vibe2d::vdp::vdp_methods]
impl MyGame {
    #[vdp("game.setPlayerPos")]
    fn vdp_set_player_pos(&mut self, p: SetPlayerPos) -> Result<serde_json::Value, String> {
        self.x = p.x;
        self.y = p.y;
        Ok(serde_json::json!({"ok": true}))
    }
}

#[cfg(feature = "vdp")]
fn handle_vdp(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    self.dispatch_vdp(method, params)
        .unwrap_or_else(|| Err(format!("Unknown: {}", method)))
}
```

The macro deserializes params via `vibe2d::vdp::from_params` and serializes
results via `vibe2d::vdp::to_result`; unmatched methods return `None` so the
forwarder can fall back (or first forward a namespace like `aoi.*`).
