# Vibe2D 项目架构

## 概述

Vibe2D 是一个用 Rust 编写的模块化 2D 游戏引擎，面向 AI 辅助的快速游戏原型开发。采用 Love2D/Ebiten 风格的简洁 `Game` trait API，支持 YAML 声明式配置、wgpu GPU 渲染、字体渲染、音频播放，以及 WebSocket 调试协议（VDP）。

核心引擎代码约 2000 行 Rust。

## Workspace 结构

```
vibe2d/
├── Cargo.toml                         # Workspace 根配置（Edition 2024）
├── crates/
│   ├── vibe2d/          (494 行)      # 核心引擎：Game trait、run()、Context、Screen
│   ├── vibe_render/     (847 行)      # wgpu 渲染：sprite batch、字体 atlas、截图
│   ├── vibe_platform/   (210 行)      # 桌面平台：winit 窗口与事件循环
│   ├── vibe_input/      (105 行)      # 输入系统：键盘状态追踪 + action 映射
│   ├── vibe_asset/      (118 行)      # 资源管理：纹理/字体加载与缓存
│   ├── vibe_audio/       (85 行)      # 音频引擎：rodio WAV 播放
│   ├── vibe_debug/      (206 行)      # VDP 服务：WebSocket + JSON-RPC 2.0
│   └── vibe_physics/      (1 行)      # 物理系统（占位）
├── tools/
│   └── vibe-cli/                      # CLI 工具：inspect、rpc、screenshot
├── examples/
│   └── flappy-bird/                   # 完整 Flappy Bird 示例（~488 行）
├── docs/                              # 文档
└── skills/
    └── vdp.md                         # LLM skill 文档
```

## crate 依赖关系

```
                    ┌─────────────────────────────┐
                    │        vibe2d (核心)          │
                    │  GameBridge 协调所有子系统     │
                    └──────────┬──────────────────┘
          ┌──────────┬────────┼────────┬───────────┐
          ▼          ▼        ▼        ▼           ▼
     vibe_asset  vibe_audio  vibe_debug  vibe_input  vibe_platform
       │                                    │         │     │
       ▼                                    │         ▼     ▼
     vibe_render ◄──────────────────────────┘       winit  wgpu
```

| crate | 依赖 | 职责 |
|-------|------|------|
| vibe2d | render, platform, input, asset, audio, debug | 顶层协调器 |
| vibe_platform | render, input, wgpu, winit | 事件循环与窗口管理 |
| vibe_render | wgpu, image, fontdue, glam | GPU 渲染管线 |
| vibe_input | winit (KeyCode) | 键盘状态追踪 |
| vibe_asset | vibe_render | 资源加载与缓存 |
| vibe_audio | rodio | 音效播放 |
| vibe_debug | tokio, tokio-tungstenite | VDP WebSocket 服务 |

## 关键外部依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| wgpu | 24 | GPU 抽象层（Vulkan/Metal/DX12） |
| winit | 0.30 | 跨平台窗口与事件 |
| image | 0.25 | PNG 纹理加载 |
| glam | 0.29 | 向量/矩阵运算 |
| fontdue | 0.9 | TTF 字形光栅化 |
| rodio | 0.20 | WAV 音频播放 |
| tokio | 1.0 | 异步运行时（VDP 服务端） |
| tokio-tungstenite | 0.26 | WebSocket 服务端 |
| serde + serde_yaml | 1.0 / 0.9 | YAML 配置解析 |

---

## 各 crate 详解

### vibe2d — 核心引擎（494 行）

引擎的入口和协调层，链接所有子系统并提供公共 API。

#### Game trait

用户实现此 trait 来创建游戏：

```rust
pub trait Game {
    fn new(ctx: &mut Context) -> Self;
    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState);
    fn draw(&mut self, ctx: &Context, screen: &mut Screen);
    fn clear_color(&self) -> Color { Color::BLACK }
    fn inspect(&self) -> serde_json::Value { Value::Null }
    fn handle_vdp(&mut self, method: &str, params: &Value) -> Result<Value, String>;
}
```

#### Context

运行时上下文，传递给游戏代码：

```rust
pub struct Context {
    pub assets: AssetManager,   // 资源管理器
    pub audio: AudioEngine,     // 音频引擎
    pub virtual_width: f32,     // 虚拟分辨率宽度
    pub virtual_height: f32,    // 虚拟分辨率高度
}
```

#### GameBridge

内部结构，桥接 Game 与平台层：

```rust
struct GameBridge<G: Game> {
    game: Option<G>,
    assets: AssetManager,
    audio: AudioEngine,
    vdp: Option<VdpChannel>,
    config: GameConfig,
    base_path: PathBuf,
    virtual_width: f32,
    virtual_height: f32,
    pending_screenshot: Option<PathBuf>,
}
```

GameBridge 实现 `PlatformCallbacks` trait，由平台事件循环在 init/update/render 各阶段调用。

#### run() 入口

```rust
pub fn run<G: Game + 'static>(config_path: &str)
```

初始化流程：
1. 加载 game.yaml 配置
2. 提取虚拟/窗口分辨率
3. 创建 InputState + action 映射
4. 可选启动 VDP WebSocket 服务
5. 创建 GameBridge
6. 调用 `vibe_platform::run_desktop()` 进入事件循环

---

### vibe_render — GPU 渲染（847 行）

基于 wgpu 的 2D sprite batch 渲染器。

#### 渲染管线

```
每帧 draw_sprite() 调用 → 排入 DrawCommand 队列
                              │
                              ▼
                     按 texture_id 分组（batch）
                              │
                              ▼
                     生成顶点数据 → 写入 GPU buffer
                              │
                              ▼
                     wgpu RenderPass 逐 batch 绘制
                              │
                              ▼
                     Present 到屏幕
                              │
                              ▼ （如有 pending_screenshot）
                     离屏纹理渲染 → staging buffer → PNG
```

#### 核心结构

```rust
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pipeline: wgpu::RenderPipeline,
    draw_commands: Vec<DrawCommand>,    // 每帧绘制命令队列
    vertex_buffer: wgpu::Buffer,        // 预分配，最多 10000 个 quad
    index_buffer: wgpu::Buffer,
    virtual_width: f32,
    virtual_height: f32,
    pending_screenshot: Option<PathBuf>,
}

pub struct DrawCommand {
    texture_id: TextureId,
    src_rect: [f32; 4],    // UV 坐标 [u, v, w, h]（0..1）
    dst_rect: [f32; 4],    // 像素坐标 [x, y, w, h]
    color: [f32; 4],       // RGBA 着色
    flip_y: bool,          // 垂直翻转
}
```

#### Sprite Batch 优化

相邻的同纹理 DrawCommand 合并为一次 `draw_indexed()` 调用，将 10000 个 draw call 减少到约 10 次（取决于纹理数量）。

```
draw_sprite(tex_0, ...)  ─┐
draw_sprite(tex_0, ...)  ─┤── Batch 1: draw_indexed(0..12)
draw_sprite(tex_1, ...)  ─┐
draw_sprite(tex_1, ...)  ─┤── Batch 2: draw_indexed(12..24)
draw_sprite(tex_1, ...)  ─┘
```

GPU 内存预分配：
- 顶点缓冲：10000 quad × 4 顶点 × 32 字节 ≈ 1.28 MB
- 索引缓冲：10000 quad × 6 索引 × 2 字节 ≈ 120 KB

#### 虚拟分辨率

所有游戏代码使用虚拟坐标（如 512×288），渲染器通过正交投影矩阵映射到 clip space：

```rust
fn orthographic_projection(width: f32, height: f32) -> [f32; 16] {
    // (0, 0) 为左上角，Y 轴向下
    // 映射到 (-1, -1) ~ (1, 1) clip space
}
```

实际窗口大小（如 1280×720）由 wgpu surface 层处理缩放。

#### 字体 Atlas

fontdue 光栅化 ASCII 字符集 → 打包到 512×N RGBA 纹理 → 上传 GPU：

```
字符集 "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789..."
                              │
                              ▼
                     逐字形光栅化（fontdue）
                              │
                              ▼
                     行优先打包到 512 宽 atlas
                              │
                              ▼
                     上传为 GPU 纹理（与 sprite 共享管线）
```

字体 atlas 作为普通纹理存入 AssetManager，文本绘制通过 sprite batch 管线完成，无额外开销。

#### 截图实现

Surface 纹理不支持 COPY_SRC，因此使用离屏渲染：

1. 创建虚拟分辨率大小的离屏纹理（RENDER_ATTACHMENT + COPY_SRC）
2. 重新执行相同的绘制命令
3. 复制到 staging buffer（MAP_READ），注意 256 字节行对齐
4. 读回像素数据，BGRA → RGBA 转换
5. 编码为 PNG 保存

---

### vibe_platform — 桌面平台（210 行）

封装 winit 事件循环和窗口创建。

#### PlatformCallbacks trait

平台层通过此 trait 回调游戏引擎：

```rust
pub trait PlatformCallbacks {
    fn on_init(&mut self, renderer: &Renderer);
    fn on_input_event(&mut self, input: &mut InputState);
    fn on_update(&mut self, dt: f32, input: &InputState);
    fn on_render(&mut self, renderer: &mut Renderer);
    fn clear_color(&self) -> [f32; 4];
    fn get_textures(&self) -> Vec<&Texture>;
}
```

这个 trait 实现了平台与引擎的解耦：vibe2d 不依赖 vibe_platform，未来可扩展到 WASM/Web 平台。

#### 初始化流程

```
EventLoop resumed()
  ├─ 创建 winit Window
  ├─ 创建 wgpu Instance
  ├─ 创建 Surface
  ├─ 请求 Adapter（物理 GPU）
  ├─ 请求 Device + Queue
  ├─ 配置 Surface（sRGB，VSync）
  ├─ 创建 Renderer（编译着色器，分配缓冲）
  └─ 回调 on_init()（加载资源，创建游戏实例）
```

---

### vibe_input — 输入系统（105 行）

键盘状态追踪，支持通过 YAML 配置 action 映射。

```rust
pub struct InputState {
    pressed: HashMap<KeyCode, bool>,        // 当前按住
    just_pressed: HashMap<KeyCode, bool>,   // 本帧刚按下
    just_released: HashMap<KeyCode, bool>,  // 本帧刚松开
    actions: HashMap<String, Vec<KeyCode>>, // action → 按键列表
}
```

每帧生命周期：
1. 事件循环分发按键事件 → 更新 pressed / just_pressed / just_released
2. 游戏代码查询：`input.is_action_just_pressed("flap")`
3. 帧末清理：`begin_frame()` 清空 just_pressed / just_released

Action 映射配置：

```yaml
input:
  actions:
    flap:
      keys: ["Space"]
    move_left:
      keys: ["Left", "A"]   # 多键绑定，任一触发
```

---

### vibe_asset — 资源管理（118 行）

纹理和字体的加载、缓存与查找。

```rust
pub struct AssetManager {
    textures: Vec<Texture>,                    // 密集数组
    texture_names: HashMap<String, TextureId>, // 名称 → 索引
    fonts: HashMap<String, Font>,              // 字体缓存
}
```

- 纹理以 `Vec<Texture>` 存储，`TextureId(usize)` 为索引
- 字体 atlas 也作为纹理存入同一 Vec，与 sprite 共享渲染管线
- 字体配置格式：`"path/to/font.ttf:32"`（路径:字号）

---

### vibe_audio — 音频引擎（85 行）

基于 rodio 的即时音效播放。

```rust
pub struct AudioEngine {
    _stream: Option<rodio::OutputStream>,
    handle: Option<rodio::OutputStreamHandle>,
    sounds: HashMap<String, Vec<u8>>,   // WAV 字节缓存
}
```

- `load_sounds()`: 从磁盘读取 WAV 文件到内存
- `play()`: 克隆字节 → Decoder 解码 → `play_raw()` 即时播放
- 即发即忘模式，暂无音量控制和循环功能

---

### vibe_debug — VDP 调试服务（206 行）

详细协议规范见 [docs/vdp.md](vdp.md)。

核心架构：双向 mpsc channel 连接游戏线程和 VDP 服务线程。

```rust
pub struct VdpChannel {          // 游戏线程持有
    pub receiver: Receiver<VdpRequest>,
    pub sender: Sender<VdpResponse>,
}

pub struct VdpServerChannel {    // 服务线程持有
    pub sender: Sender<VdpRequest>,
    pub receiver: Receiver<VdpResponse>,
}
```

---

## 关键设计模式

### Take/Swap 模式

**问题**：Rust 借用检查器不允许同时可变借用。AssetManager 和 AudioEngine 存在于 GameBridge 中，但游戏代码需要通过 `&mut Context` 访问它们。

**方案**：每次回调前将资源移出 bridge，回调后移回。

```rust
// on_update() 中：
let mut ctx = Context {
    assets: std::mem::take(&mut self.assets),  // 移出
    audio: std::mem::take(&mut self.audio),
    virtual_width: self.virtual_width,
    virtual_height: self.virtual_height,
};
game.update(&mut ctx, dt, input);   // 游戏代码使用
self.assets = ctx.assets;           // 移回
self.audio = ctx.audio;
```

三处应用：`on_init()`、`on_update()`、`on_render()`。

**优点**：零拷贝（栈上移动），类型安全，单一数据源。

### PlatformCallbacks 解耦

引擎核心（vibe2d）不依赖具体窗口系统。通过 trait 反转控制，平台层调用引擎：

```
vibe_platform::run_desktop(config, callbacks)
    │
    ├─ callbacks.on_init()     → GameBridge 加载资源
    ├─ callbacks.on_update()   → GameBridge 更新游戏
    └─ callbacks.on_render()   → GameBridge 渲染帧
```

未来可扩展到 WASM 平台，只需实现不同的 `run_wasm()` 入口。

---

## 主循环时序

```
每帧（约 60Hz）:

┌─ 计算 dt ──────────────────────────────────────┐
│                                                 │
├─ VDP 请求处理（非阻塞 try_recv）               │
│   while channel.try_recv() {                    │
│       response = handle_vdp_request(req)        │
│       channel.send(response)                    │
│   }                                             │
│                                                 │
├─ on_update(dt, input)                           │
│   take assets/audio → Context                   │
│   game.update(&mut ctx, dt, input)              │
│   swap assets/audio 回 bridge                   │
│                                                 │
├─ on_render(renderer)                            │
│   take assets/audio → Context                   │
│   game.draw(&ctx, &mut screen)                  │
│     → screen.draw_sprite() 排入队列             │
│   swap assets/audio 回 bridge                   │
│                                                 │
├─ renderer.render(clear_color, textures)         │
│   生成顶点数据 → 按纹理分 batch → GPU 绘制     │
│   如有 pending_screenshot → 离屏渲染 → PNG      │
│   draw_commands.clear()                         │
│                                                 │
├─ input.begin_frame()                            │
│   清空 just_pressed / just_released             │
│                                                 │
└─ window.request_redraw() ──────────────────────┘
```

---

## game.yaml 配置结构

```yaml
meta:                         # 可选，项目元信息
  name: "Flappy Bird"
  version: "0.1.0"

window:                       # 必填，物理窗口配置
  width: 1280
  height: 720
  title: "Flappy Bird - Vibe2D"
  vsync: true

virtual_resolution:           # 可选，默认与 window 相同
  width: 512
  height: 288

assets:                       # 可选，资源声明
  textures:                   # 名称 → 路径
    background: "assets/images/background/10_background.png"
    bird: "assets/sprites/bird.png"
  fonts:                      # 名称 → "路径:字号"
    score: "assets/fonts/flappy.ttf:32"
  audio:                      # 名称 → 路径
    flap: "assets/sfx/bird-flap.wav"

input:                        # 可选，输入映射
  actions:
    flap:
      keys: ["Space"]

debug:                        # 可选，调试配置
  vdp:
    enabled: true
    port: 9229
```

对应 Rust 结构：

```rust
pub struct GameConfig {
    pub meta: Option<MetaConfig>,
    pub window: WindowConfig,
    pub virtual_resolution: Option<VirtualResolutionConfig>,
    pub assets: Option<AssetsConfig>,
    pub input: Option<InputConfig>,
    pub debug: Option<DebugConfig>,
    pub constants: Option<HashMap<String, Value>>,
}
```

---

## 线程模型

```
┌────────────────────────────┐     ┌──────────────────────┐
│     主线程（游戏循环）       │     │  VDP 线程（tokio）    │
│                            │     │                      │
│  winit 事件循环             │     │  TcpListener :9229   │
│  ├─ 输入处理               │     │  WebSocket 升级      │
│  ├─ VDP try_recv() ◄──────┼─────┤  JSON-RPC 解析       │
│  ├─ game.update()          │     │                      │
│  ├─ game.draw()            │     │  recv_timeout(5s) ◄──┼──┐
│  ├─ GPU 渲染              │     │  发送 JSON 响应      │  │
│  └─ VDP send() ───────────┼─────┤                      │  │
│                            │     └──────────────────────┘  │
│  rodio 音频流（后台线程）    │                               │
└────────────────────────────┘     客户端（CLI/Python/AI）───┘
```

- 主线程：winit 事件循环 + 游戏逻辑 + GPU 渲染
- VDP 线程：tokio single-thread runtime，WebSocket 服务
- 音频线程：rodio OutputStream 自动管理
- 线程间通信：`std::sync::mpsc` 双向 channel
