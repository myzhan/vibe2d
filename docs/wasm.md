# WebAssembly (WASM) 支持

Vibe2D 支持编译为 WebAssembly 并在浏览器中运行。同一份游戏代码可以同时构建桌面端和 Web 端，无需修改游戏逻辑。

## 快速开始

### 前置条件

```bash
# 安装 wasm32 target
rustup target add wasm32-unknown-unknown

# 安装 Trunk（WASM 构建工具）
cargo install trunk
```

### 构建并运行

```bash
cd examples/flappy-bird
trunk serve --port 8080
```

打开浏览器访问 `http://localhost:8080` 即可。

### 发布构建

```bash
trunk build --release
```

生成的文件在 `dist/` 目录下，可直接部署到任何静态文件服务器。

## 项目配置

### Cargo.toml

游戏的 `Cargo.toml` 需要添加 wasm32 专用依赖：

```toml
[dependencies]
vibe2d = { workspace = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
getrandom = { version = "0.3", features = ["wasm_js"] }  # 如果用了 rand
```

### index.html

创建 `index.html` 作为 Trunk 的入口：

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>My Game - Vibe2D</title>
  <style>
    html, body { margin: 0; padding: 0; overflow: hidden; height: 100%; background: #000; }
    canvas { width: 100vw; height: 100vh; display: block; }
  </style>
</head>
<body>
  <canvas id="vibe2d-canvas"></canvas>
  <link data-trunk rel="rust" data-wasm-opt="z" />
  <link data-trunk rel="copy-dir" href="assets" />
  <link data-trunk rel="copy-file" href="game.yaml" />
</body>
</html>
```

关键点：
- Canvas 的 id 必须是 `vibe2d-canvas`（引擎通过此 id 获取绘制目标）
- `data-trunk rel="copy-dir"` 将 assets 目录复制到输出
- `data-trunk rel="copy-file"` 将 game.yaml 复制到输出
- `data-wasm-opt="z"` 优化 WASM 体积

### Trunk.toml

```toml
[build]
target = "index.html"

[serve]
address = "0.0.0.0"
port = 8080
```

### main.rs 入口

游戏代码需要同时支持桌面和 Web 两个入口：

```rust
use vibe2d::prelude::*;

struct MyGame { /* ... */ }
impl Game for MyGame { /* ... */ }

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    vibe2d::run::<MyGame>("game.yaml");
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    wasm_bindgen_futures::spawn_local(async {
        vibe2d::run_web::<MyGame>("game.yaml").await;
    });
}
```

- 桌面端使用 `vibe2d::run()` （同步）
- Web 端使用 `vibe2d::run_web()` （异步，通过 `wasm_bindgen(start)` 作为入口）
- `main()` 在 wasm32 上留空（实际入口是 `web_main`）

## 架构差异

### 桌面端 vs Web 端

| 方面 | 桌面端 | Web 端 |
|------|--------|--------|
| 入口 | `vibe2d::run()` | `vibe2d::run_web()` |
| 窗口系统 | winit（原生窗口） | winit（Canvas） |
| GPU 后端 | Vulkan / Metal / DX12 | WebGL2 |
| 资源加载 | 文件系统读取 | HTTP fetch |
| 音频 | rodio | 静默（待实现） |
| 时间 | `std::time::Instant` | `web-time::Instant` |
| VDP 连接 | 游戏内嵌 WebSocket 服务端 | 通过 relay 连接 |
| 截图 | 写入本地 PNG 文件 | GPU readback → base64 返回 |
| 线程 | 多线程（VDP 独立 tokio 线程） | 单线程 |

### 资源加载流程

Web 端的资源加载是异步的：

1. `run_web()` 首先通过 HTTP fetch 获取 `game.yaml`
2. 解析配置后，并行 fetch 所有声明的资源文件（纹理、字体、音频）
3. 所有资源下载完成后，才启动 winit 事件循环和 GPU 初始化

资源路径（`game.yaml` 中的 `assets.textures` 等）被视为相对于页面 URL 的路径。Trunk 的 `copy-dir` 指令会将 assets 目录复制到 dist 根目录，因此路径如 `assets/textures/bird.png` 可以直接通过 HTTP 访问。

### GPU 后端

Web 端使用 wgpu 的 WebGL2 后端：

```toml
# vibe_render/Cargo.toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wgpu = { workspace = true, features = ["webgl"] }
```

WebGL2 是 wgpu 在浏览器中的默认后端，兼容性好（支持所有现代浏览器）。WebGPU 后端 (`features = ["webgpu"]`) 仅在 Chrome 113+ 等浏览器支持，目前未启用。

## VDP 调试（Web 端）

Web 端的 VDP 调试通过 relay 中继架构实现，详见 [docs/vdp.md](vdp.md)。

### 架构

```
┌────────────────┐    WebSocket     ┌───────────────┐    WebSocket     ┌────────────────┐
│  Python 脚本   │◄────────────────►│  vdp-relay    │◄────────────────►│  浏览器游戏     │
│  vibe-cli rpc  │    ws://:9229/   │  :9229        │    ws://:9229/   │  (WASM)        │
│  ...           │                  │               │    /game         │                │
└────────────────┘                  └───────────────┘                  └────────────────┘
       工具端                              中继                              游戏端
```

### 使用步骤

1. 启动 relay：
   ```bash
   cargo run -p vibe-cli -- vdp-relay --port 9229
   ```

2. 启动游戏 Web 服务：
   ```bash
   cd examples/flappy-bird && trunk serve --port 8080
   ```

3. 打开浏览器访问 `http://localhost:8080`

4. 使用工具连接（与桌面端命令完全一致）：
   ```bash
   # inspect
   vibe inspect --addr ws://127.0.0.1:9229
   
   # screenshot
   vibe screenshot --output capture.png --addr ws://127.0.0.1:9229
   
   # Python autopilot
   python examples/flappy-bird/tests/autopilot_max_score.py
   ```

### 自动连接

游戏加载后会自动连接到 relay（前提是 `game.yaml` 中启用了 VDP）：

```yaml
debug:
  vdp:
    enabled: true
```

连接 URL 确定规则：
1. 如果页面 URL 含 `?vdp_relay=ws://host:port` 参数，使用该地址
2. 否则默认连接 `ws://<当前页面主机名>:9229/game`

### 截图

Web 端截图通过 GPU buffer readback 实现，返回 base64 编码的 PNG 数据：

```bash
vibe screenshot --output capture.png --addr ws://127.0.0.1:9229
```

`vibe-cli` 会自动检测响应格式并解码保存。对工具端完全透明。

## 平台差异注意事项

### 代码兼容性

游戏代码通常不需要 `#[cfg(target_arch = "wasm32")]` 门控（除了入口 `main`/`web_main`）。但以下场景需要注意：

1. **文件系统**：Web 端无本地文件系统。引擎已封装资源加载流程，游戏不应直接 `std::fs::read`。
2. **多线程**：Web 端是单线程。避免使用 `std::thread::spawn`，使用引擎的 `update()` 循环管理逻辑。
3. **时间**：引擎已统一为 `web-time`（在 wasm32 上）和 `std::time`（在桌面）。游戏通过 `dt` 参数获取帧时间，无需直接调 `Instant`。
4. **随机数**：使用 `rand` crate 时需添加 `getrandom = { version = "0.3", features = ["wasm_js"] }` 依赖。

### 音频

Web 端音频暂未实现（`vibe_audio` 在 wasm32 上为静默 no-op）。游戏中调 `ctx.audio.play()` 不会报错，只是没有声音。

### 手柄

手柄**输入**在 Web 端可用：gilrs 有 wasm 后端，读浏览器的 `navigator.getGamepads()`，
所以按键、摇杆、扳机和连接/断开都能正常工作，游戏侧代码与桌面完全一致。

**震动在 Web 端不可用**（`ctx.rumble()` 为 no-op）：gilrs-core 的 wasm
`FfDevice::set_ff_state` 是空实现，力反馈的 worker 线程也被 `cfg` 掉了。

另外注意浏览器为防指纹识别，通常会**在用户首次按键之前隐藏手柄** ——
所以 Web 端多数手柄不是在启动时被发现，而是稍后以 `Connected` 事件到达。

### 性能

- Debug 构建的 WASM 体积较大且运行慢。发布时务必用 `trunk build --release` 或 `data-wasm-opt="z"`。
- WebGL2 性能通常低于原生 Vulkan/Metal。对于复杂游戏，注意 draw call 数量。
- Sprite batch 渲染在 Web 端同样有效——按纹理分批可以显著减少 draw call。

## 完整示例目录结构

```
examples/flappy-bird/
├── Cargo.toml          # 含 wasm32 依赖
├── Trunk.toml          # Trunk 构建配置
├── index.html          # Web 入口 HTML
├── game.yaml           # 游戏配置（桌面/Web 共用）
├── assets/             # 资源文件
│   ├── textures/
│   └── fonts/
├── src/
│   └── main.rs         # 游戏代码（桌面/Web 共用）
└── tests/
    └── autopilot_max_score.py  # Python 测试脚本（桌面/Web 通用）
```
