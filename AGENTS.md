# AGENTS.md — Vibe2D AI Agent 指南

本文件为 AI Agent（大语言模型、代码助手）提供 Vibe2D 项目的完整上下文。在修改任何代码之前请先阅读本文件。

## 项目概述

Vibe2D 是一个**对 AI 友好的 2D 游戏引擎**，使用 Rust 编写。它采用 **Ebiten/Love2D 风格**的 API —— 实现一个带有 `new()`、`update()`、`draw()` 方法的 `Game` trait 就能创建游戏。无需 ECS，无需模板代码。

- **语言**：Rust（edition 2024）
- **构建系统**：Cargo workspace
- **许可证**：MIT OR Apache-2.0
- **最小概念**：一个 `game.yaml` 配置文件 + 一个实现 `Game` trait 的 `main.rs`

## 仓库结构

```
vibe2d/
├── Cargo.toml                  # Workspace 根配置，列出所有成员
├── crates/
│   ├── vibe2d/                 # 主引擎 crate（Game trait、Context、Screen、run()）
│   │   └── src/
│   │       ├── lib.rs          # 入口：run()、GameBridge、VDP 请求路由
│   │       ├── game.rs         # Game trait 定义
│   │       ├── context.rs      # Context 结构体（assets、audio、ui_state）
│   │       ├── screen.rs       # Screen 结构体（draw_sprite、draw_text 等）
│   │       └── config.rs       # GameConfig，从 game.yaml 解析
│   ├── vibe_render/            # 基于 wgpu 的 sprite batch 渲染器 + 字体 atlas
│   │   └── src/
│   │       ├── renderer.rs     # Renderer、DrawCommand、sprite 批处理、截图
│   │       ├── font.rs         # Font（fontdue 字形 atlas）
│   │       ├── texture.rs      # Texture、TextureId
│   │       └── sprite.wgsl     # 顶点/片段着色器
│   ├── vibe_platform/          # 平台抽象层（winit + wgpu 桌面端）
│   ├── vibe_input/             # InputState + action 映射（键盘 + 鼠标）
│   ├── vibe_asset/             # AssetManager（纹理、字体，名称→ID 查找）
│   ├── vibe_audio/             # AudioEngine（rodio，WAV 播放）
│   ├── vibe_debug/             # VDP WebSocket 服务（tokio，JSON-RPC 2.0）
│   ├── vibe_ui/                # 即时模式 UI 系统
│   │   └── src/
│   │       ├── context.rs      # UiContext（label、button、panel、text_input、scroll_list）
│   │       ├── state.rs        # UiState（跨帧持久状态）
│   │       ├── layout.rs       # Anchor、LayoutDirection、定位
│   │       ├── style.rs        # PanelStyle、Color 常量
│   │       ├── response.rs     # WidgetResponse（clicked、submitted 等）
│   │       ├── id.rs           # WidgetId
│   │       └── vdp.rs          # VdpUiAction、WidgetSnapshot
│   ├── vibe_test/              # 测试工具库：VdpClient + GameHarness（游戏集成测试用，dev-dep）
│   └── vibe_physics/           # 占位 crate —— 尚未实现
├── tools/
│   └── vibe-cli/               # CLI 工具：vibe new/inspect/rpc/screenshot
├── examples/
│   ├── flappy-bird/            # 完整的 Flappy Bird 游戏（约 480 行）
│   ├── tetris/                 # 俄罗斯方块
│   ├── mari0/                  # 马里奥风格游戏
│   └── ui/                     # UI 系统演示
├── docs/
│   ├── architecture.md         # 详细架构文档
│   ├── vdp.md                  # VDP 协议规范
│   └── ui.md                   # UI 系统设计文档
└── skills/
    └── vdp.md                  # LLM skill 文件，用于 VDP 交互
```

## Crate 依赖关系

```
游戏 crate（如 flappy-bird）
  ├── [dependencies]
  │     └── vibe2d
  │           ├── vibe_render      （wgpu 渲染）
  │           ├── vibe_platform    （winit 窗口/事件循环）
  │           ├── vibe_input       （键盘/鼠标状态）
  │           ├── vibe_asset       （纹理/字体加载）
  │           ├── vibe_audio       （rodio 音效播放）
  │           ├── vibe_ui          （即时模式 UI）
  │           └── vibe_debug       （可选，通过 "vdp" feature 启用）
  └── [dev-dependencies]
        └── vibe_test              （可选，VDP 集成测试工具库；仅在 `cargo test` 时拉取）
```

## 核心 API

详细的 API 参考、配置格式和 VDP 协议用法见 **[docs/api.md](docs/api.md)**。

以下是最小概念速览：

- **`Game` trait**：实现 `new()` / `update()` / `draw()` 三个方法即可创建游戏
- **`Context`**：引擎上下文，包含 `assets`、`audio`、`ui_state`、`virtual_width/height`
- **`Screen`**：渲染目标，提供 `draw_sprite()`、`draw_text()` 等绘制方法
- **`InputState`**：输入查询，支持 action 映射（`is_action_just_pressed("jump")`）
- **`game.yaml`**：声明式配置窗口、资源、输入映射、VDP 调试

## 关键设计模式

### Take/Swap 模式

Rust 的借用检查器禁止同时可变借用。引擎使用 `std::mem::take()` 在每次回调前将 `AssetManager`、`AudioEngine` 和 `UiState` 从 `GameBridge` 移出到 `Context` 中，回调后再移回：

```rust
let mut ctx = Context {
    assets: std::mem::take(&mut self.assets),   // 移出
    audio: std::mem::take(&mut self.audio),
    ...
};
game.update(&mut ctx, dt, input);
self.assets = ctx.assets;                       // 移回
self.audio = ctx.audio;
```

这发生在 `crates/vibe2d/src/lib.rs` 的 `on_init()`、`on_update()` 和 `on_render()` 中。

**添加新引擎资源时**：遵循相同模式 —— 在 `GameBridge` 和 `Context` 中都添加字段，并在三个 take/swap 位置都包含它。

### PlatformCallbacks 解耦

引擎核心（`vibe2d`）不依赖任何特定窗口系统。`vibe_platform` 通过 `PlatformCallbacks` trait 回调引擎：

```
vibe_platform::run_desktop(config, callbacks)
    ├── callbacks.on_init()     → GameBridge 加载资源
    ├── callbacks.on_update()   → GameBridge 更新游戏
    └── callbacks.on_render()   → GameBridge 渲染帧
```

### 帧生命周期

```
每帧（约 60Hz）：
  1. 计算 dt
  2. [VDP] 自动释放上一帧的 tap/click 输入
  3. [VDP] 处理 VDP 请求（try_recv，非阻塞）
  4. [VDP] 将模拟输入注入 InputState
  5. game.update(ctx, dt, input)      — 游戏逻辑
  6. game.update_ui(ctx, input)       — 构建 UI（缓存绘制命令）
  7. game.draw(ctx, screen)           — 渲染游戏世界
  8. 回放 UI 缓存的绘制命令           — UI 绘制在最顶层
  9. GPU sprite batch 渲染            — 按纹理分批，draw_indexed
  10. input.begin_frame()             — 清空 just_pressed/just_released
```

### Sprite Batch 渲染

所有 `draw_sprite()` 调用会将 `DrawCommand` 结构体排入队列。渲染时，命令按 `texture_id` 分组，以最少的 GPU draw call 进行绘制。

### UI 系统（即时模式 + 持久状态）

UI 在 `update_ui()` 中构建（update 阶段，而非 draw 阶段），因为 `draw()` 的签名是 `&self` + `&Context`（不可变，无输入访问权限）。绘制命令缓存在 `UiState.cached_draw_commands` 中，渲染时回放。详细用法见 [docs/api.md](docs/api.md#ui-系统)。

## Feature Flags

### `vdp`（默认启用，可编译时剥离）

控制 Vibe Debug Protocol —— 用于运行时检查、状态修改、输入模拟、暂停/步进调试和截图的 WebSocket 服务。

```bash
cargo build                        # VDP 启用（默认）
cargo build --no-default-features  # 剥离 VDP，用于发布
```

所有 VDP 相关代码均使用 `#[cfg(feature = "vdp")]` 门控。添加 VDP 相关功能时，务必添加门控。

Feature 级联：游戏 crate → `vibe2d/vdp` → `vibe_debug` + `serde_json`

**命名统一**：整个 workspace 中凡是与 VDP 相关的 feature 一律叫 `vdp`（不要自造 `debug`、`harness`、`client` 等别名）：

| Crate | `vdp` feature 含义 |
|-------|------------------|
| `vibe2d` | 启用运行时 VDP WebSocket 服务端 |
| `vibe_test` | 启用 VDP 客户端 + 进程 harness（测试工具库） |
| 游戏 crate | 级联开启 `vibe2d/vdp` + `serde_json` |

## 线程模型

```
┌─────────────────────────┐     ┌──────────────────────┐
│  主线程                  │     │  VDP 线程（tokio）     │
│  winit 事件循环          │     │  WebSocket :9229      │
│  ├── 输入处理            │     │  JSON-RPC 解析        │
│  ├── VDP try_recv() ◄───┼─────┤                      │
│  ├── game.update()      │     │  recv_timeout(5s)     │
│  ├── game.draw()        │     │                      │
│  ├── GPU 渲染 ──────────┼─────┤                      │
│  └── VDP send() ────────┼────►│  发送 JSON 响应       │
└─────────────────────────┘     └──────────────────────┘
```

通信方式：双向 `std::sync::mpsc` channel。游戏线程不使用 async。

## 操作指南：常见任务

### 创建新游戏

1. 创建 `examples/my-game/Cargo.toml`：
   ```toml
   [package]
   name = "my-game"
   version.workspace = true
   edition.workspace = true

   [features]
   default = ["vdp"]
   vdp = ["vibe2d/vdp", "dep:serde_json"]

   [dependencies]
   vibe2d = { workspace = true }
   serde_json = { workspace = true, optional = true }
   ```
2. 创建 `examples/my-game/game.yaml`，配置窗口/资源/输入
3. 创建 `examples/my-game/src/main.rs`，实现 `Game` trait
4. 在根 `Cargo.toml` 的 workspace `members` 中添加 `"examples/my-game"`
5. 运行：`cargo run -p my-game`

### 添加新引擎功能/资源

1. 如果是新 crate，在 `crates/` 下创建，添加到 workspace `Cargo.toml`
2. 在 `crates/vibe2d/Cargo.toml` 中添加依赖
3. 在 `Context` 中添加字段（`crates/vibe2d/src/context.rs`）
4. 在 `GameBridge` 中添加字段（`crates/vibe2d/src/lib.rs`）
5. 在三个 take/swap 位置都包含该字段：`on_init()`、`on_update()`、`on_render()`
6. 如果是用户可见的 API，在 `prelude` 中重新导出（`crates/vibe2d/src/lib.rs`）

### 添加新 VDP 方法

**引擎级别**（所有游戏通用）：在 `crates/vibe2d/src/lib.rs` 的 `GameBridge::handle_vdp_request()` 中添加 match 分支。

**游戏级别**（特定游戏）：在游戏结构体上实现 `handle_vdp()`。示例见 [docs/api.md](docs/api.md#实现自定义-vdp-方法)。

### 添加新 UI 组件

1. 在 `crates/vibe_ui/src/context.rs` 的 `UiContext` 中添加组件方法
2. 如果有持久状态，在 `crates/vibe_ui/src/state.rs` 的 `UiState` 中添加字段
3. 如果需要 VDP 支持，在 `crates/vibe_ui/src/vdp.rs` 中添加 `VdpUiAction` 变体
4. 在 `GameBridge::handle_vdp_request()` 中添加 `"ui.*"` 下的 VDP 方法路由
5. 为 `ui.listWidgets` 添加 `WidgetSnapshot` 序列化

### 添加新资源类型

1. 在 `crates/vibe_asset/src/lib.rs`（`AssetManager`）中添加加载逻辑
2. 在 `crates/vibe2d/src/config.rs`（`AssetsConfig`）中添加配置字段
3. 在 `crates/vibe2d/src/lib.rs` 的 `GameBridge::on_init()` 中添加加载调用

### 为游戏编写测试

Vibe2D 分两层：

- **引擎层**：每个 crate 在 `#[cfg(test)] mod tests` 中写纯逻辑单元测试（无 GPU/窗口依赖），覆盖 `InputState`、`UiState`、`VdpResponse` 序列化、`GameConfig` 解析等。引擎本身**不写**针对具体游戏的集成测试——这是 example 自己的职责。
- **游戏层**：example 通过 `vibe_test` 工具库写 VDP 端到端集成测试，验证自己的业务功能（按钮点击、游戏状态转移、计分逻辑等）。

新游戏接入 Rust 集成测试的步骤：

1. 在游戏 `Cargo.toml` 中加入 dev-dependencies：
   ```toml
   [dev-dependencies]
   vibe_test = { workspace = true, features = ["vdp"] }
   tokio = { workspace = true, features = ["rt-multi-thread", "macros", "time"] }
   anyhow.workspace = true
   serde_json.workspace = true
   ```
2. 确保 `game.yaml` 中 `debug.vdp.enabled: true`，并使用**独立端口**避免与其他 example 冲突（如 flappy-bird=9229、ui-demo=9230、tetris=9231……）。
3. 在 `examples/<game>/tests/<name>.rs` 中写 `#[tokio::test(flavor = "multi_thread")] #[ignore]` 测试，用 `vibe_test::GameHarness::launch("<game>", <port>)` 启动进程并拿到一个带 VDP 客户端的 harness。
4. 运行：`cargo test -p <game> -- --ignored --test-threads=1`（必须串行，因为 harness 占用固定 VDP 端口）。

参考样例见 `examples/ui/tests/vdp_ui.rs`。

**注意事项**：
- `vibe_test` 是 **dev-dependency** —— `cargo build --release` **不会**把它编译进发布产物。
- `#[ignore]` 是刻意的——集成测试会冷启动 `cargo run -p <game>`，耗时长，不适合放在默认 `cargo test` 管线里。
- `GameHarness::step_and_wait(n)` 要优先于裸 `step(n)` —— `engine.step` 是非阻塞的，不等就发下一条 RPC 容易产生时序 race。
- Python 脚本（`examples/*/tests/*.py`）定位不同：Rust 测试是 **CI 门禁**（硬断言 + 退出码），Python 是 **交互探索 / LLM autopilot 演示**。两者互补，不要互相替代。

## 代码风格

### Rust 约定

- **Edition 2024** — 使用最新的 Rust 惯用法
- **引擎代码禁止 `unwrap()`** — 使用 `anyhow::Result`、`Option::map` 或显式错误处理
- **示例游戏中可以使用 `unwrap()`** — 为了简洁
- **使用描述性命名** — `scroll_offset` 而非 `so`，`pending_screenshot` 而非 `ps`
- **扁平控制流** — 提前返回、守卫子句，避免深层嵌套
- **注释解释"为什么"，而非"是什么"** — 代码应该是自文档化的

### 架构规则

- **引擎 crate 不依赖游戏代码** — 依赖方向是单向的
- **`vibe2d` 不依赖 `vibe_platform`** — 平台层通过 `PlatformCallbacks` 调用引擎
- **所有坐标使用虚拟分辨率** — 而非物理窗口像素
- **VDP 代码始终使用 `#[cfg(feature = "vdp")]` 门控** — 剥离后零开销
- **UI 通过 sprite batch 渲染** — 无单独的渲染管线；矩形使用 1×1 白色像素纹理 + 颜色着色

### 测试与验证（硬性要求）

**在交付任何代码修改前，必须跑通受影响范围的测试。** 这是非协商项，不是「有余力就做」。

最小自证路径（按作用范围选一组执行）：

| 改动范围 | 必须跑通的命令 |
|---------|---------------|
| 修改某个引擎 crate（如 `vibe_ui`、`vibe_input`） | `cargo test -p <crate>` |
| 修改 `vibe2d` 核心 / `GameBridge` / VDP 路由 | `cargo test -p vibe2d` + `cargo test -p ui-demo -- --ignored --test-threads=1` |
| 修改 `vibe_test` 工具库 | `cargo test -p vibe_test` + 至少一个使用它的 example 的集成测试 |
| 修改某个 example（如 `ui-demo`） | `cargo test -p <example> -- --ignored --test-threads=1` |
| 涉及 feature 门控或 workspace 结构变动 | 额外跑 `cargo build --no-default-features` 确认剥离路径能编译 |
| 无法判断影响范围 | 兜底：`cargo test --workspace` + `cargo test -p ui-demo -- --ignored --test-threads=1` |

规则细则：

- **测试失败 = 任务未完成**。不允许「代码写完了，测试没跑」或「编译通过就算完」。
- **不准通过删减/注释/放宽断言来让测试变绿**。如果断言确实算错了（例如 `examples/ui/tests/vdp_ui.rs` 里曾经漏算 `padding*2`），修正断言时必须在注释中说明「为什么旧断言错了」，而不是偷偷改数字。
- **不要引入新的 `#[ignore]` 来规避失败**。`#[ignore]` 只用于"需要真实窗口/GPU 的 VDP 集成测试"这一类本来就不适合默认跑的场景。
- **不要提交只跑了 `cargo check` / `cargo build` 就声称「完成」的改动**——这两个命令只验证类型，不验证行为。
- **手动编辑代码（尤其是 clippy 修复）后，必须再跑一次 `cargo fmt --all`**。clippy 建议的改写常常改变行长度，rustfmt 会重新选择换行点，仅看「编辑后肉眼像是格式化好的」不算数，CI 的 `cargo fmt --all -- --check` 才是唯一标准。
- **本地无法跑窗口/GPU 测试时**（比如纯 headless CI），至少跑所有单元测试 + `cargo test -p ui-demo --no-run` 保证集成测试编译通过，并在回复中显式说明「哪些测试因环境限制未跑」。
- **修复 bug 前应当有一条复现该 bug 的测试**；修复后这条测试从 red 变 green。如果原有测试覆盖不到这个 bug，补一条再修。

**反模式**（一旦出现立即回退）：

- 「编译通过了所以应该没问题」
- 「这个测试本来就是 flaky 的，忽略」
- 「改动很小，不用跑测试」
- 「用户没要求跑测试」 — 这条规则就是用户的要求

### 命名规范

| 类别 | 规范 | 示例 |
|------|------|------|
| Crate | `snake_case` | `vibe_render`、`vibe_input` |
| 结构体 | `PascalCase` | `GameBridge`、`DrawCommand` |
| 函数 | `snake_case` | `draw_sprite_tinted`、`is_action_just_pressed` |
| 常量 | `UPPER_SNAKE_CASE` | `PIPE_GAP`、`GRAVITY` |
| 配置键（YAML） | `snake_case` | `virtual_resolution`、`mouse_buttons` |
| VDP 方法 | `namespace.camelCase` | `engine.pause`、`game.setState`、`ui.scrollToBottom` |
| 纹理/字体名称 | `snake_case` | `"background"`、`"score"` |
| 组件 ID | `snake_case` | `"retry_btn"`、`"chat_input"` |

## 构建与运行

```bash
# 构建整个 workspace
cargo build

# 运行示例游戏
cargo run -p flappy-bird
cargo run -p tetris

# 不包含 VDP 的发布构建
cargo build --no-default-features --release

# 运行 CLI 工具
cargo run -p vibe-cli -- inspect
cargo run -p vibe-cli -- rpc engine.info
cargo run -p vibe-cli -- screenshot -o capture.png
```

## VDP（Vibe Debug Protocol）

VDP 是基于 WebSocket + JSON-RPC 2.0 的运行时调试协议（`ws://127.0.0.1:9229`）。完整方法列表、参数格式和使用示例见 [docs/api.md](docs/api.md#vdpvibe-debug-protocol)，协议规范见 [docs/vdp.md](docs/vdp.md)。

## 建议阅读顺序

首次接触本代码库时，建议按以下顺序阅读：

1. **`crates/vibe2d/src/game.rs`** — `Game` trait（整个用户面向的 API）
2. **`crates/vibe2d/src/context.rs`** — `Context` 结构体
3. **`crates/vibe2d/src/screen.rs`** — `Screen` 绘制 API
4. **`crates/vibe2d/src/lib.rs`** — `run()`、`GameBridge`、帧生命周期、VDP 路由
5. **`examples/flappy-bird/src/main.rs`** — 完整游戏示例
6. **`examples/flappy-bird/game.yaml`** — 配置文件示例
7. **`docs/architecture.md`** — 完整架构文档

## 文档同步规则

修改代码时，如果涉及以下内容，**必须同步更新对应文档**：

| 修改内容 | 需更新的文档 |
|----------|-------------|
| `Game` trait 签名或方法 | `docs/api.md`（Game Trait 章节） |
| `Context`、`Screen` 的字段或方法 | `docs/api.md`（Context / Screen 章节） |
| `InputState` 的公开方法 | `docs/api.md`（InputState 章节） |
| `game.yaml` 配置字段 | `docs/api.md`（game.yaml 章节） |
| VDP 新增/修改/删除方法 | `docs/api.md`（VDP 章节）+ `docs/vdp.md` + `skills/vdp.md` |
| UI 组件新增/修改 | `docs/api.md`（UI 系统章节）+ `docs/ui.md` |
| 新增 crate 或改变 crate 依赖关系 | `AGENTS.md`（仓库结构 / Crate 依赖关系） |
| 新增设计模式或架构变更 | `AGENTS.md`（关键设计模式）+ `docs/architecture.md` |
| 新增示例游戏 | `AGENTS.md`（仓库结构） |

**原则**：API 的详细用法维护在 `docs/api.md`，架构规则和开发指南维护在 `AGENTS.md`。两者各司其职，避免重复。

## 常见陷阱与注意事项

- **`draw()` 的签名是 `&self`** — 不能在 `draw()` 中修改游戏状态。所有状态修改在 `update()` 中进行。UI 构建在 `update_ui()` 中完成，而非 `draw()`。
- **虚拟坐标 vs 物理坐标** — 所有游戏代码使用虚拟坐标（如 512×288）。不要使用物理窗口像素。鼠标坐标由引擎自动转换。
- **字体配置格式** — YAML 中字体使用 `"路径:字号"` 格式：`"assets/fonts/font.ttf:32"`。
- **纹理名称必须匹配** — `ctx.assets.texture_id("player")` 查找的是 `game.yaml` 中 `assets.textures` 部分声明的名称。
- **`__vibe_ui_white`** — 引擎自动创建的 1×1 白色像素纹理，用于 UI 矩形绘制。游戏纹理不要使用此名称。
- **VDP `handle_vdp()` 兜底分支** — 始终以 `_ => Err(format!("Unknown method: {}", method))` 结尾，避免静默吞掉未知方法。
- **Feature 门控** — 任何涉及 `serde_json`、`vibe_debug`、`inspect()` 或 `handle_vdp()` 的代码都必须使用 `#[cfg(feature = "vdp")]`。
- **交付前必须跑测试** — 详见上面的《测试与验证》小节。改完代码不跑测试就交付属于未完成任务。
