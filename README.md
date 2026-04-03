# Vibe2D

An AI-friendly 2D game engine built in Rust. Designed for simplicity — write game logic in pure Rust with an Ebiten/Love2D-style `Game` trait, configure via YAML, and let AI agents inspect and control the running game via the **Vibe Debug Protocol (VDP)**.

## Features

- **Simple Game API** — Implement `new()`, `update()`, `draw()` and you have a game. No ECS, no boilerplate.
- **YAML Configuration** — Window size, virtual resolution, assets, input mappings, debug settings — all in `game.yaml`.
- **Sprite Batch Rendering** — wgpu-powered GPU renderer with automatic texture batching, orthographic projection, and virtual resolution scaling.
- **Text Rendering** — TTF font loading via `fontdue`, glyph atlas rasterization, `draw_text()` / `draw_text_centered()`.
- **Input System** — Action-based input mapping (e.g., `jump: ["Space"]`) with keyboard and mouse pressed/held/released state tracking. Mouse coordinates use virtual resolution.
- **Vibe Debug Protocol (VDP)** — WebSocket + JSON-RPC 2.0 server for real-time game inspection, state mutation, input simulation (keyboard/mouse), pause/step debugging, and screenshots from external tools or AI agents. Compile-time strippable via `--no-default-features`.
- **CLI Tool** — `vibe inspect`, `vibe rpc`, `vibe screenshot` for interacting with the running game from the terminal.
- **Pure CLI Workflow** — No GUI editor. Code, configure, run, debug — all from the command line.

## Screenshot

Flappy Bird example running on Vibe2D:

![Flappy Bird on Vibe2D](screenshot.png)

## Quick Start

```rust
use vibe2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn new(_ctx: &mut Context) -> Self {
        Self
    }

    fn update(&mut self, _ctx: &mut Context, _dt: f32, _input: &InputState) {}

    fn draw(&mut self, _ctx: &Context, _screen: &mut Screen) {}
}

fn main() {
    vibe2d::run::<MyGame>("game.yaml");
}
```

## Project Structure

```
crates/
  vibe2d/         — Main engine crate (Game trait, Context, Screen, config)
  vibe_render/    — wgpu sprite batch renderer, font atlas
  vibe_platform/  — Platform abstraction (winit + wgpu desktop)
  vibe_input/     — Input state tracking with action mapping
  vibe_asset/     — Asset manager (textures, fonts)
  vibe_debug/     — VDP WebSocket server + JSON-RPC protocol
  vibe_physics/   — Physics (placeholder)
  vibe_audio/     — Audio (placeholder)
tools/
  vibe-cli/       — CLI tool (vibe new/inspect/rpc/screenshot)
examples/
  flappy-bird/    — Complete Flappy Bird game
skills/
  vdp.md          — LLM skill file for VDP protocol
```

## VDP (Vibe Debug Protocol)

Enable VDP in `game.yaml`:

```yaml
debug:
  vdp:
    enabled: true
    port: 9229
```

Then interact with the running game:

```bash
# Inspect game state
vibe inspect

# Take a screenshot
vibe screenshot -o capture.png

# Pause / step / resume
vibe rpc engine.pause
vibe rpc engine.step '{"frames": 1}'
vibe rpc engine.resume

# Simulate keyboard input
vibe rpc engine.simulateInput '{"device": "keyboard", "action": "tap", "key": "Space"}'

# Simulate mouse input
vibe rpc engine.simulateInput '{"device": "mouse", "action": "move", "x": 256, "y": 144}'

# Send custom RPC
vibe rpc game.setState '{"state": "Playing"}'
```

### Feature Flags

VDP is enabled by default. To build without VDP (for release):

```bash
cargo build --no-default-features
```

## License

MIT OR Apache-2.0
