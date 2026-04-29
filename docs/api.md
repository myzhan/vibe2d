# Vibe2D API 参考

本文档包含 Vibe2D 引擎的完整 API 参考、配置格式和 VDP 协议用法。

> **维护规则**：任何对引擎 API、配置格式、VDP 方法的修改，都必须同步更新本文档。详见 [AGENTS.md](../AGENTS.md) 中的文档同步规则。

---

## Game Trait

每个游戏都需要实现此 trait，定义在 `crates/vibe2d/src/game.rs`：

```rust
pub trait Game {
    /// 创建并初始化游戏。在此加载资源、设置初始状态。
    fn new(ctx: &mut Context) -> Self;

    /// 每帧调用。更新游戏逻辑、处理输入。
    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState);

    /// 每帧在 update 之后调用。将所有内容绘制到屏幕。
    fn draw(&self, ctx: &Context, screen: &mut Screen);

    /// 在 update 阶段构建 UI（即时模式）。
    /// UI 绘制命令会自动缓存，在渲染阶段回放到画面最顶层。
    fn update_ui(&mut self, _ctx: &mut Context, _input: &InputState) {}

    /// 背景清除颜色，可覆盖自定义。
    fn clear_color(&self) -> Color { Color::BLACK }

    /// 返回游戏状态的 JSON 快照，供 VDP game.inspect 使用。
    #[cfg(feature = "vdp")]
    fn inspect(&self) -> serde_json::Value { serde_json::Value::Null }

    /// 处理自定义 VDP 命令来修改游戏状态。
    #[cfg(feature = "vdp")]
    fn handle_vdp(&mut self, method: &str, params: &serde_json::Value)
        -> Result<serde_json::Value, String> {
        Err("Not implemented".to_string())
    }
}
```

### 程序入口

```rust
use vibe2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn new(_ctx: &mut Context) -> Self { Self }
    fn update(&mut self, _ctx: &mut Context, _dt: f32, _input: &InputState) {}
    fn draw(&self, _ctx: &Context, _screen: &mut Screen) {}
}

fn main() {
    vibe2d::run::<MyGame>("game.yaml");
}
```

---

## Context

引擎上下文，定义在 `crates/vibe2d/src/context.rs`：

```rust
pub struct Context {
    pub assets: AssetManager,      // 纹理、字体资源管理
    pub audio: AudioEngine,        // 音频引擎
    pub ui_state: UiState,         // UI 持久状态
    pub virtual_width: f32,        // 虚拟分辨率宽度
    pub virtual_height: f32,       // 虚拟分辨率高度
}
```

### AssetManager 常用方法

```rust
ctx.assets.texture_id("player")       // -> Option<TextureId>
ctx.assets.font("ui")                 // -> Option<&Font>
ctx.assets.all_textures()             // -> Vec<&Texture>
```

### AudioEngine 常用方法

```rust
ctx.audio.play("jump");               // 播放音效（即发即忘）
```

---

## Screen 绘制 API

渲染目标，定义在 `crates/vibe2d/src/screen.rs`。所有坐标使用**虚拟分辨率**。

### 基础绘制

```rust
// 绘制完整纹理
screen.draw_sprite(texture_id, x, y, width, height);

// 绘制翻转的纹理
screen.draw_sprite_flipped(texture_id, x, y, w, h);       // 垂直翻转
screen.draw_sprite_flipped_h(texture_id, x, y, w, h);     // 水平翻转
screen.draw_sprite_flipped_both(texture_id, x, y, w, h);  // 双轴翻转
```

### 区域绘制（sprite sheet）

```rust
// src_rect: [u, v, w, h]（0.0..1.0 UV 坐标）
// dst_rect: [x, y, w, h]（虚拟像素坐标）
screen.draw_sprite_region(texture_id, src_rect, dst_rect);
screen.draw_sprite_region_flipped(texture_id, src_rect, dst_rect, flip_x, flip_y);
```

### 着色绘制

```rust
screen.draw_sprite_tinted(texture_id, x, y, w, h, color);
screen.draw_sprite_region_tinted(texture_id, src_rect, dst_rect, color);
screen.draw_sprite_region_flipped_tinted(texture_id, src_rect, dst_rect, flip_x, flip_y, color);
```

### 文本绘制

```rust
screen.draw_text(font, "Hello", x, y);
screen.draw_text_centered(font, "Hello", y);   // 水平居中
```

### 圆形绘制

引擎不预置任何圆纹理。需要画圆的游戏在 `Game::new(ctx, renderer)` 里调 `Renderer` 的工厂方法生成并注册一次，把返回的 `TextureId` 存进游戏结构体，`draw` 时显式传给 `Screen`：

```rust
fn new(ctx: &mut Context, renderer: &Renderer) -> Self {
    let circle_filled = ctx.assets.register_texture(
        "my_disc",
        renderer.create_filled_circle_texture("my_disc", 256),
    );
    let circle_ring = ctx.assets.register_texture(
        "my_ring",
        // size, thickness_ratio（描边比例，0.08 ≈ 半径 8%）
        renderer.create_ring_texture("my_ring", 256, 0.08),
    );
    Self { circle_filled, circle_ring, /* … */ }
}

fn draw(&self, _ctx: &Context, screen: &mut Screen) {
    // 实心圆（纹理；中心 cx,cy；半径 radius；颜色 color）
    screen.draw_circle(self.circle_filled, cx, cy, radius, color);
    // 圆环 / 空心圆轮廓（描边比例由你创建纹理时指定）
    screen.draw_circle_outline(self.circle_ring, cx, cy, radius, color);
}
```

要点：

- `Renderer::create_filled_circle_texture` / `create_ring_texture` / `create_white_pixel_texture` / `create_rgba_texture` 是公开的"动态纹理工厂"API。任何能纯 CPU 算出 RGBA 像素的图像都可以走 `create_rgba_texture` 上传
- 256² 是常用默认值：在 [10, 100] px 半径范围下肉眼无锯齿；缓冲 256 KB，启动开销可忽略
- 两个 `Screen` API 内部都是单次 sprite blit，相同纹理的绘制会自动合批，开销与画一个 sprite 相同
- 需要多种描边宽度时，用同样的 API 注册多张不同 `thickness_ratio` 的圆环纹理，按需取用
- 颜色按 alpha 通道与底色混合，可放心叠在场景之上而不遮盖底层内容

---

## InputState 输入查询

定义在 `crates/vibe_input/src/lib.rs`。

### 键盘

```rust
input.is_key_pressed(KeyCode::Space)         // 当前帧按住
input.is_key_just_pressed(KeyCode::Space)    // 本帧刚按下
input.is_key_just_released(KeyCode::Space)   // 本帧刚松开
```

### Action 映射（推荐方式）

```rust
input.is_action_pressed("jump")              // 检查键盘和鼠标绑定
input.is_action_just_pressed("jump")
input.is_action_just_released("jump")
```

### 鼠标

```rust
input.mouse_x()                                         // 虚拟坐标 X
input.mouse_y()                                         // 虚拟坐标 Y
input.is_mouse_button_pressed(MouseButton::Left)
input.is_mouse_button_just_pressed(MouseButton::Left)
input.is_mouse_button_just_released(MouseButton::Left)
```

### 字符输入与滚轮（用于 UI）

```rust
input.chars_this_frame()       // -> &[char]，本帧收到的可打印字符
input.mouse_scroll_delta()     // -> f32，本帧滚轮增量（正值 = 向上）
```

---

## game.yaml 配置格式

每个游戏在其 crate 根目录下都有一个 `game.yaml`：

```yaml
meta:                            # 可选，项目元信息
  name: "My Game"
  version: "0.1.0"

window:                          # 必填，物理窗口配置
  width: 1280
  height: 720
  title: "My Game - Vibe2D"
  vsync: true

virtual_resolution:              # 可选，默认与 window 相同
  width: 512
  height: 288

assets:                          # 可选，资源声明
  textures:                      # 名称 → 路径
    player: "assets/sprites/player.png"
    background: "assets/images/bg.png"
  fonts:                         # 名称 → "路径:字号"
    ui: "assets/fonts/font.ttf:16"
    score: "assets/fonts/font.ttf:32"
  audio:                         # 名称 → 路径
    jump: "assets/sfx/jump.wav"

input:                           # 可选，输入映射
  actions:
    jump:
      keys: ["Space", "W"]
      mouse_buttons: ["Left"]    # 可选，鼠标按键绑定
    move_left:
      keys: ["Left", "A"]       # 多键绑定，任一触发

debug:                           # 可选，调试配置
  vdp:
    enabled: true
    port: 9229                   # 可选，默认 9229
```

### 配置说明

- **资源按名称加载**：在代码中使用 `ctx.assets.texture_id("player")` 或 `ctx.assets.font("ui")` 获取
- **字体格式**：`"路径:字号"`，如 `"assets/fonts/font.ttf:32"`
- **Action 映射**：支持键盘和鼠标混合绑定，`input.is_action_just_pressed("jump")` 会同时检查两者

---

## UI 系统

即时模式 UI，在 `update_ui()` 中构建（update 阶段）。

### 基本用法

```rust
fn update_ui(&mut self, ctx: &mut Context, input: &InputState) {
    let vw = ctx.virtual_width;
    let vh = ctx.virtual_height;

    // UI 系统自管自己的 1×1 白像素纹理（在引擎 on_init 时由
    // UiState::init 通过 Renderer::create_white_pixel_texture +
    // AssetManager::register_texture 注册），UiContext 直接从
    // UiState 读取，游戏代码无需感知。
    let mut ui_state = std::mem::take(&mut ctx.ui_state);
    let mut ui = UiContext::new(&mut ui_state, input, vw, vh);

    // 设置锚点和布局
    ui.set_anchor(Anchor::Center);
    ui.set_spacing(8.0);
    ui.set_padding(10.0);

    // 文本标签
    if let Some(font) = ctx.assets.font("ui") {
        ui.label(font, "Hello World");
    }

    // 按钮
    if let Some(font) = ctx.assets.font("ui") {
        if ui.button_with_id("start_btn", font, "Start").clicked() {
            self.start_game();
        }
    }

    // 面板（带背景的分组容器）
    ui.panel(PanelStyle::default(), |ui| {
        // 面板内的子组件...
    });

    // 文本输入
    let input_response = ui.text_input_with_placeholder("chat", font, 200.0, "Type...");
    if input_response.submitted {
        let text = ui.text_input_value("chat");
        // 处理提交...
    }

    // 可滚动列表
    ui.scroll_list("messages", 280.0, 160.0, |ui| {
        for msg in &self.messages {
            ui.label(font, msg);
        }
    });

    ui.finish();
    ctx.ui_state = ui_state;
}
```

### 锚点（Anchor）

控制 UI 在屏幕上的位置：`TopLeft`、`TopCenter`、`TopRight`、`CenterLeft`、`Center`、`CenterRight`、`BottomLeft`、`BottomCenter`、`BottomRight`

### 布局方向（LayoutDirection）

- `Vertical`（默认）— 子组件从上到下排列
- `Horizontal` — 子组件从左到右排列

### UiOutput

`update_ui()` 结束后可通过 `UiOutput` 检查 UI 是否消费了输入：

```rust
let output = ui.finish();
self.ui_has_keyboard = output.consumed_keyboard;
```

---

## VDP（Vibe Debug Protocol）

基于 WebSocket + JSON-RPC 2.0 的运行时调试协议，默认地址 `ws://127.0.0.1:9229`。

### 请求格式

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "engine.info",
  "params": {}
}
```

### 响应格式

```json
{ "jsonrpc": "2.0", "id": 1, "result": { ... } }
{ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "..." } }
```

### 内置方法一览

| 方法 | 参数 | 说明 |
|------|------|------|
| `engine.info` | — | 引擎版本 + 虚拟分辨率 |
| `engine.pause` | — | 暂停游戏循环（渲染仍继续） |
| `engine.resume` | — | 恢复游戏循环 |
| `engine.step` | `{"frames": N}` | 暂停时步进 N 帧（固定 dt=1/60） |
| `engine.getTime` | — | 帧计数 + 累计时间 + 暂停状态 |
| `engine.simulateInput` | 见下方 | 注入键盘/鼠标输入 |
| `engine.simulateInputBatch` | `{"inputs": [...]}` | 批量注入多个输入 |
| `engine.setRendering` | `{"enabled": bool}` | 启用/禁用渲染（用于无头步进） |
| `game.inspect` | — | 完整游戏状态 JSON |
| `game.screenshot` | `{"path": "..."}` | 截图保存为 PNG |
| `ui.listWidgets` | — | 列出所有 UI 组件及位置状态 |
| `ui.click` | `{"id": "..."}` | 模拟点击组件 |
| `ui.setText` | `{"id": "...", "text": "..."}` | 设置文本输入内容 |
| `ui.submit` | `{"id": "..."}` | 模拟 Enter 提交 |
| `ui.setFocus` | `{"id": "..."}` | 设置焦点 |
| `ui.clearFocus` | — | 清除焦点 |
| `ui.scroll` | `{"id": "...", "offset": N}` | 设置滚动位置 |
| `ui.scrollToBottom` | `{"id": "..."}` | 滚动到底部 |

### engine.simulateInput 参数

**键盘**：
```json
{"device": "keyboard", "action": "press|release|tap", "key": "Space"}
```
- `tap` = 按下后下一帧自动释放，触发 `just_pressed`
- 支持的键名：`Space`、`Enter`、`Escape`、`Up`、`Down`、`Left`、`Right`、`A`-`D`、`W`、`S`

**鼠标**：
```json
{"device": "mouse", "action": "move", "x": 256.0, "y": 144.0}
{"device": "mouse", "action": "press|release|click", "button": "Left|Right|Middle"}
```
- `click` = 按下后下一帧自动释放（等价于键盘的 `tap`）

### CLI 工具

```bash
vibe inspect                                                    # 查看游戏状态
vibe rpc engine.info                                            # 引擎信息
vibe rpc engine.pause                                           # 暂停
vibe rpc engine.step '{"frames": 5}'                            # 步进
vibe rpc engine.simulateInput '{"action": "tap", "key": "Space"}'  # 模拟输入
vibe screenshot -o capture.png                                  # 截图
```

### Python 示例

```python
import websocket, json

ws = websocket.WebSocket()
ws.connect("ws://127.0.0.1:9229")

def rpc(method, params=None):
    msg = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params:
        msg["params"] = params
    ws.send(json.dumps(msg))
    return json.loads(ws.recv())

# 查看游戏状态
result = rpc("game.inspect")

# 暂停 → 步进 → 截图 → 恢复
rpc("engine.pause")
rpc("engine.step", {"frames": 10})
rpc("game.screenshot", {"path": "/tmp/capture.png"})
rpc("engine.resume")
```

### 实现自定义 VDP 方法

```rust
#[cfg(feature = "vdp")]
fn inspect(&self) -> serde_json::Value {
    serde_json::json!({
        "state": "playing",
        "score": self.score,
        "player": { "x": self.player_x, "y": self.player_y },
    })
}

#[cfg(feature = "vdp")]
fn handle_vdp(&mut self, method: &str, params: &serde_json::Value)
    -> Result<serde_json::Value, String>
{
    match method {
        "game.setPlayerPos" => {
            let x = params["x"].as_f64().ok_or("Missing 'x'")? as f32;
            let y = params["y"].as_f64().ok_or("Missing 'y'")? as f32;
            self.player_x = x;
            self.player_y = y;
            Ok(serde_json::json!({"x": x, "y": y}))
        }
        _ => Err(format!("Unknown method: {}", method)),
    }
}
```

### VDP 方法命名约定

- 引擎内置方法：`engine.*`（pause/resume/step/getTime/simulateInput/info）
- 游戏状态查询：`game.inspect`（内置）
- 游戏截图：`game.screenshot`（内置）
- UI 操作：`ui.*`（listWidgets/click/setText/submit/setFocus/scroll 等，内置）
- AOI 空间查询：`aoi.*`（list/queryAabb/queryCircle/queryPoint/raycast/stats，**由游戏转发**，见下文）
- 游戏自定义方法：`game.<camelCase>`（如 `game.setBirdY`、`game.setState`）

## AOI 空间查询（`vibe_aoi`）

`vibe_aoi` 是一个**独立的可选工具库**，提供空间查询能力（broadphase、AOI、raycast、enter/leave 事件）。它故意**不进 `Context`、不属于 `vibe2d` 的子依赖**——需要的游戏自己在 `Cargo.toml` 里 `vibe_aoi.workspace = true` 后持有一个 `AoiWorld` 实例。完整设计动机见 `docs/aoi.md`，可运行示例见 `examples/aoi-demo`。

### 启用

```toml
# 游戏的 Cargo.toml
[features]
default = ["vdp"]
vdp = ["vibe2d/vdp", "vibe_aoi/vdp", "dep:serde_json"]

[dependencies]
vibe2d = { workspace = true }
vibe_aoi = { workspace = true }
serde_json = { workspace = true, optional = true }
```

`vibe_aoi/vdp` feature 开启时会同时启用 JSON 序列化与 `AoiWorld::handle_vdp` helper；关闭后 `serde_json` 完全不会被拉取，库回到纯 CPU 计算的最小形态。

### 核心类型

| 类型 | 说明 |
|------|------|
| `Shape::Point(Vec2)` / `Circle{center, radius}` / `Aabb{center, half_extents}` | 实体或查询区域的几何形状 |
| `EntityId(u32)` | 实体句柄；`remove` 后会被复用 |
| `ObserverId(u32)` | 观察者句柄；同上 |
| `AoiEvent::Enter(EntityId)` / `Leave(EntityId)` | 观察者每帧产出的事件 |
| `RaycastHit { entity, distance }` | `raycast` 的命中结果 |
| `AoiStats` | 实体数 / cell 数 / 每 cell 最大与平均实体数 |

### 常用 API

```rust
use glam::Vec2;
use vibe_aoi::{AoiWorld, Shape, AoiEvent};

// 选择后端：
let mut world = AoiWorld::new(Vec2::new(512.0, 288.0));   // 默认 UniformGrid，自动 cell_size
let mut world = AoiWorld::with_grid(Vec2::splat(1024.0), 32.0); // 显式 cell_size
let mut world = AoiWorld::with_bruteforce();              // 线性扫描（< ~200 实体或测试 oracle）

// 实体管理：
let id = world.insert(Shape::circle(Vec2::ZERO, 5.0));
world.update(id, Shape::circle(Vec2::new(10.0, 0.0), 5.0));
world.remove(id);

// 一次性查询（注意：返回 `&mut self`，因为 grid 后端用了 dedupe scratchpad）：
let hits: Vec<EntityId> = world.query_aabb(Vec2::ZERO, Vec2::splat(50.0));
let hits = world.query_circle(Vec2::new(100.0, 100.0), 30.0);
let hits = world.query_point(Vec2::new(50.0, 50.0));    // 鼠标拾取

// 观察者（跨帧持久查询，自动产 enter/leave 事件）：
let obs = world.create_observer(Shape::circle(player_pos, 100.0));
// 每帧：
world.update_observer(obs, Shape::circle(new_player_pos, 100.0));
for ev in world.drain_events(obs) {
    match ev {
        AoiEvent::Enter(id) => { /* 该实体本帧进入了观察区域 */ }
        AoiEvent::Leave(id) => { /* 该实体本帧离开了观察区域 */ }
    }
}
world.remove_observer(obs);

// 射线投射（返回最近命中）：
if let Some(hit) = world.raycast(origin, dir, max_dist) {
    println!("命中实体 {:?}，距离 {}", hit.entity, hit.distance);
}

// 诊断：
let s = world.stats();
```

**`drain_events` 的语义**：调用后队列被清空。事件**不**跨 `drain_events` 累积；如果你某帧不调它，下一帧的 diff 仍然只反映「上次 drain 时的 current 集 → 当前集」，中间过渡的事件会被合并。

**`AoiWorld::new(bounds)` 的 `cell_size` 启发式**：`bounds.max_element() / 32.0`，clamp 到 `[16.0, 256.0]`。绝大多数 2D 游戏不需要手动调，把世界尺寸传进去即可。

### 与 `vibe_physics` 的边界

`vibe_aoi` 回答「**谁在哪儿**」（broadphase + 区域查询）；`vibe_physics`（尚未实现）将回答「**接下来怎么动**」（narrowphase 接触信息 + 动力学积分）。未来 `vibe_physics` 会**直接复用 `vibe_aoi` 的 grid + 几何判定**，不允许另写一套。详见 `docs/aoi.md`。

### VDP 方法

游戏在自己的 `handle_vdp()` 里转发 `aoi.*` 命名空间到 `AoiWorld::handle_vdp`：

```rust
#[cfg(feature = "vdp")]
fn handle_vdp(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    if method.starts_with("aoi.") {
        return self.aoi.handle_vdp(method, params);
    }
    // ...其它 game.* 方法
    Err(format!("Unknown method: {method}"))
}
```

| 方法 | 参数 | 返回 |
|------|------|------|
| `aoi.list` | — | `{ entities: [{id, shape}, ...] }` |
| `aoi.queryAabb` | `{ min: [x,y], max: [x,y] }` | `{ hits: [id, ...] }` |
| `aoi.queryCircle` | `{ center: [x,y], radius: f }` | `{ hits: [id, ...] }` |
| `aoi.queryPoint` | `{ point: [x,y] }` | `{ hits: [id, ...] }` |
| `aoi.raycast` | `{ origin: [x,y], dir: [x,y], maxDist: f }` | `{ hit: { entity, distance } | null }` |
| `aoi.stats` | — | `AoiStats` 全字段 |

`Shape` 在 wire 上序列化为扁平对象：`{"type": "circle", "center": [..], "radius": ..}` / `{"type": "point", "position": [..]}` / `{"type": "aabb", "center": [..], "halfExtents": [..]}`。`EntityId` / `ObserverId` 序列化为裸数字。
