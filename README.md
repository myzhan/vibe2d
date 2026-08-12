# Vibe2D

一个对 AI 友好的 2D 游戏引擎，使用 Rust 构建。

用纯 Rust 编写游戏逻辑，采用 Ebiten/Love2D 风格的 `Game` trait，通过 YAML 声明式配置，并通过 **Vibe Debug Protocol (VDP)** 让 AI Agent 在运行时检查和控制游戏。

## 特性

- **极简游戏 API** — 实现 `new()`、`update()`、`draw()` 三个方法就是一个完整的游戏。无需 ECS，无需模板代码。
- **YAML 声明式配置** — 窗口大小、虚拟分辨率、资源、输入映射、调试设置，全部在 `game.yaml` 中声明。
- **Sprite Batch 渲染** — 基于 wgpu 的 GPU 渲染器，自动纹理批处理、正交投影、虚拟分辨率缩放。
- **文本渲染** — 通过 fontdue 加载 TTF 字体，字形 atlas 光栅化，`draw_text()` / `draw_text_centered()`。
- **输入系统** — 基于 Action 的输入映射（如 `jump: ["Space"]`），支持键盘、鼠标和手柄的 pressed/held/released 状态追踪。鼠标坐标自动转换为虚拟分辨率。
- **手柄支持** — 按键 / 摇杆（径向死区）/ 模拟扳机 / 连接断开 / 震动，支持多手柄与本地多人。一个 action 可同时绑定键盘、十字键和摇杆方向。桌面与 Web 通用（Web 无震动）。
- **即时模式 UI** — 内建 UI 系统，支持 Label、Button、Panel、TextInput、ScrollList，锚点布局，通过 VDP 可完全自动化操控。
- **音频引擎** — 基于 rodio 的音效播放，WAV 格式，即发即忘模式。
- **Vibe Debug Protocol (VDP)** — WebSocket + JSON-RPC 2.0 服务，支持运行时状态检查、状态修改、输入模拟（键盘/鼠标）、暂停/步进调试、截图。可通过 `--no-default-features` 在编译时完全剥离。
- **CLI 工具** — `vibe inspect`、`vibe rpc`、`vibe screenshot`，从终端与运行中的游戏交互。
- **纯 CLI 工作流** — 无 GUI 编辑器。代码、配置、运行、调试，全在命令行完成。

## 截图

Flappy Bird 示例运行在 Vibe2D 上：

![Flappy Bird on Vibe2D](screenshot.png)

## 快速开始

```rust
use vibe2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn new(_ctx: &mut Context) -> Self {
        Self
    }

    fn update(&mut self, _ctx: &mut Context, _dt: f32, _input: &InputState) {}

    fn draw(&self, _ctx: &Context, _screen: &mut Screen) {}
}

fn main() {
    vibe2d::run::<MyGame>("game.yaml");
}
```

## 项目结构

```
crates/
  vibe2d/         — 主引擎 crate（Game trait、Context、Screen、配置）
  vibe_render/    — wgpu sprite batch 渲染器、字体 atlas
  vibe_platform/  — 平台抽象（winit + wgpu 桌面端）
  vibe_input/     — 输入状态追踪 + action 映射
  vibe_asset/     — 资源管理器（纹理、字体）
  vibe_audio/     — 音频引擎（rodio，WAV 播放）
  vibe_debug/     — VDP WebSocket 服务 + JSON-RPC 协议
  vibe_ui/        — 即时模式 UI 系统
  vibe_physics/   — 物理引擎（占位）
tools/
  vibe-cli/       — CLI 工具（vibe new/inspect/rpc/screenshot）
examples/
  flappy-bird/    — 完整的 Flappy Bird 游戏
  tetris/         — 俄罗斯方块
  mari0/          — 马里奥风格游戏
  ui/             — UI 系统演示
  aoi-demo/       — AOI 空间查询演示
  gamepad/        — 手柄测试器（按键网格 / 摇杆 / 扳机 / action / 震动）
docs/
  architecture.md — 详细架构文档
  api.md          — API 参考
  vdp.md          — VDP 协议规范
  ui.md           — UI 系统设计文档
skills/
  vdp.md          — LLM skill 文件
```

## VDP（Vibe Debug Protocol）

在 `game.yaml` 中启用 VDP：

```yaml
debug:
  vdp:
    enabled: true
    port: 9229
```

然后与运行中的游戏交互：

```bash
# 查看游戏状态
vibe inspect

# 截图
vibe screenshot -o capture.png

# 暂停 / 步进 / 恢复
vibe rpc engine.pause
vibe rpc engine.step '{"frames": 1}'
vibe rpc engine.resume

# 模拟键盘输入
vibe rpc engine.simulateInput '{"action": "tap", "key": "Space"}'

# 模拟鼠标输入
vibe rpc engine.simulateInput '{"device": "mouse", "action": "move", "x": 256, "y": 144}'

# 模拟手柄输入（按键按位置命名，South = Xbox 的 A）
vibe rpc engine.simulateInput '{"device": "gamepad", "action": "tap", "button": "South"}'

# 发送自定义 RPC
vibe rpc game.setState '{"state": "Playing"}'
```

### Feature Flags

`vdp` 与 `gamepad` 默认启用，都可在编译时剥离：

```bash
cargo build --no-default-features --release          # 同时剥离 VDP 和手柄
cargo build --no-default-features --features gamepad # 只保留手柄
```

剥离 `gamepad` 会连带移除 gilrs 依赖 —— 在 Linux 上这也就免掉了 `libudev` 的构建期依赖。

## 文档

- [架构文档](docs/architecture.md) — 引擎内部设计和关键决策
- [API 参考](docs/api.md) — Game trait、Screen、InputState、game.yaml 配置、VDP 方法
- [VDP 协议规范](docs/vdp.md) — 完整的 WebSocket + JSON-RPC 协议细节
- [UI 系统设计](docs/ui.md) — 即时模式 UI 的架构和组件
- [AI Agent 指南](AGENTS.md) — 面向大模型的项目开发指南

## License

MIT