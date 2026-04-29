# Vibe2D UI 系统设计

## 概述

`vibe_ui` 是 Vibe2D 的即时模式（Immediate Mode）UI 系统，为**游戏内 HUD、菜单和聊天交互**提供完整的 UI 能力。

### 设计目标

- **默认内置**：作为 `vibe2d` 的必选依赖，开箱即用
- **即时模式 + 持久状态**：核心采用即时模式 API（每帧声明式描述），有状态组件（TextInput、ScrollList）由引擎自动管理持久状态
- **零额外渲染开销**：复用现有 sprite batch + font atlas 管线，不引入新的渲染路径
- **风格一致**：API 与 `Screen::draw_sprite()` / `draw_text()` 同级，零学习成本
- **VDP 全面可控**：所有 UI 控件通过 ID 标识，支持通过 VDP 协议进行检查、操作和自动化测试
- **聊天场景支持**：内置文本输入框和滚动列表，可直接构建游戏内聊天 UI

### 不做什么

- 富文本编辑器（粗体/斜体/图文混排）
- 窗口拖拽、停靠、多窗口管理
- CSS/样式表系统

---

## 架构

### Crate 结构

```
crates/
  vibe_ui/           — UI 组件、布局、交互、VDP 集成
    src/
      lib.rs         — 公共导出
      context.rs     — UiContext：即时模式 UI 上下文
      state.rs       — UiState：跨帧持久状态（TextInput 内容、ScrollList 位置、焦点）
      layout.rs      — 布局计算（锚点 + 堆叠）
      widgets/
        mod.rs       — 组件公共 trait 和导出
        label.rs     — Label 文本标签
        button.rs    — Button 可点击按钮
        panel.rs     — Panel 矩形容器
        progress.rs  — ProgressBar 进度条
        text_input.rs — TextInput 文本输入框
        scroll_list.rs — ScrollList 滚动列表
      style.rs       — 样式定义（颜色、间距、字体大小）
      response.rs    — 交互响应（点击、悬浮、文本变更）
      vdp.rs         — VDP 协议集成（UI 树检查、远程操作）
      id.rs          — 控件 ID 体系
```

### 依赖关系

```
vibe2d（必选依赖 vibe_ui）
  ├── vibe_render    （已有：sprite batch、font atlas）
  ├── vibe_input     （已有：鼠标位置、点击状态、键盘输入）
  └── vibe_ui        （新增，必选）
        ├── uses vibe_render::Renderer    — 提交 DrawCommand
        ├── uses vibe_render::Font        — 文本测量与渲染
        └── reads vibe_input::InputState  — 鼠标 + 键盘交互检测
```

`vibe_ui` 不创建新的 GPU 资源，所有绘制通过向现有 `Renderer` 提交 `DrawCommand` 完成。

### 依赖配置

```toml
# crates/vibe2d/Cargo.toml
[dependencies]
vibe_ui = { workspace = true }          # 必选，不是 optional
```

`vibe_ui` 作为引擎核心能力，始终编译，无 feature flag 门控。

---

## 控件 ID 体系

每个 UI 控件都有一个**字符串 ID**，用于：

1. **持久状态索引**：TextInput 和 ScrollList 通过 ID 在 `UiState` 中查找/存储跨帧状态
2. **VDP 控件定位**：VDP 协议通过 ID 定位和操作特定控件
3. **无状态组件也有 ID**：Label、Button 等虽然不需要跨帧状态，但通过 ID 可被 VDP 检查和操作

### ID 规则

- 用户显式提供（有状态组件必须提供）
- 无状态组件可省略（自动生成 `__auto_{序号}`）
- ID 在同一帧内必须唯一

```rust
/// 控件唯一标识符。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WidgetId(pub String);

impl WidgetId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn auto(index: usize) -> Self {
        Self(format!("__auto_{}", index))
    }
}
```

---

## 持久状态：UiState

有状态组件（TextInput、ScrollList）需要跨帧保持数据。`UiState` 存储在 `Context` 中，生命周期与游戏一致。

```rust
/// UI 跨帧持久状态，存储在 Context 中。
pub struct UiState {
    /// 当前获得键盘焦点的控件 ID
    focused: Option<WidgetId>,

    /// TextInput 状态：ID → 输入框状态
    text_inputs: HashMap<WidgetId, TextInputState>,

    /// ScrollList 状态：ID → 滚动位置
    scroll_lists: HashMap<WidgetId, ScrollListState>,

    /// 上一帧的 UI 控件树快照（供 VDP 检查）
    last_frame_widgets: Vec<WidgetSnapshot>,

    /// VDP 注入的待执行操作队列
    pending_vdp_actions: Vec<VdpUiAction>,

    /// 自动 ID 计数器（每帧重置）
    auto_id_counter: usize,
}

pub struct TextInputState {
    pub text: String,
    pub cursor_position: usize,
    pub selection_start: Option<usize>,
}

pub struct ScrollListState {
    pub scroll_offset: f32,
    pub total_content_height: f32,
}
```

### Context 集成

```rust
pub struct Context {
    pub assets: AssetManager,
    pub audio: AudioEngine,
    pub ui_state: UiState,              // 新增
    pub virtual_width: f32,
    pub virtual_height: f32,
}
```

`UiState` 使用与 `AssetManager` / `AudioEngine` 相同的 Take/Swap 模式，在 `on_update()` 和 `on_render()` 阶段移入/移出 `GameBridge`。

---

## API 设计

### 入口：`Screen::ui()`

```rust
fn draw(&mut self, ctx: &Context, screen: &mut Screen) {
    // 先画游戏世界...
    screen.draw_sprite(self.bg_tex, 0.0, 0.0, 512.0, 288.0);

    // 然后画 UI 层
    let ui_output = screen.ui(ctx, |ui| {
        // UI 代码...
    });

    // 可选：检查 UI 是否消费了鼠标事件
    if !ui_output.consumed_mouse {
        // 处理游戏世界的点击逻辑...
    }
}
```

`ui()` 接收 `&Context`（访问 `InputState`、`AssetManager`、`UiState`），返回 `UiOutput`。

### UiContext

```rust
/// 即时模式 UI 上下文，在每帧 draw 阶段使用。
pub struct UiContext<'a> {
    renderer: &'a mut Renderer,
    ui_state: &'a mut UiState,
    input: &'a InputState,
    virtual_width: f32,
    virtual_height: f32,

    // 本帧输入快照
    mouse_x: f32,
    mouse_y: f32,
    mouse_just_clicked: bool,
    mouse_pressed: bool,

    // 布局状态栈
    cursor_x: f32,
    cursor_y: f32,
    anchor: Anchor,
    layout_direction: LayoutDirection,
    style: Style,

    // 本帧控件记录（用于 VDP 快照和遮挡处理）
    frame_widgets: Vec<WidgetSnapshot>,
    consumed_mouse: bool,
}
```

闭包结束后，`frame_widgets` 写入 `UiState::last_frame_widgets`，供 VDP 在下一帧检查。

### UiOutput

```rust
pub struct UiOutput {
    /// UI 层是否消费了本帧的鼠标点击
    pub consumed_mouse: bool,
    /// UI 层是否消费了本帧的键盘输入（TextInput 获得焦点时）
    pub consumed_keyboard: bool,
}
```

---

## 组件

### Label — 文本标签

最基础的组件，显示一行文本。

```rust
// 基本用法（自动 ID）
ui.label(&font, "Score: 42");

// 带颜色
ui.label_colored(&font, "GAME OVER", Color::from_hex(0xFF4444));

// 显式 ID（方便 VDP 定位）
ui.label_with_id("score_label", &font, "Score: 42");
```

**实现**：调用 `Font::layout_text()` 获取字形信息，提交 `DrawCommand` 到 `Renderer`。与 `Screen::draw_text()` 共享同一条渲染路径。

### Button — 可点击按钮

带交互反馈的矩形区域 + 文本。

```rust
if ui.button(&font, "Restart").clicked() {
    self.restart();
}

// 显式 ID
if ui.button_with_id("retry_btn", &font, "Retry").clicked() {
    self.restart();
}

// 带自定义样式
if ui.button_styled(&font, "Quit", ButtonStyle {
    bg_color: Color::from_hex(0x333333),
    hover_color: Color::from_hex(0x555555),
    pressed_color: Color::from_hex(0x222222),
    text_color: Color::WHITE,
    padding: 8.0,
}).clicked() {
    // ...
}
```

**返回值**：`Response` 结构。

```rust
pub struct Response {
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
}

impl Response {
    pub fn clicked(&self) -> bool { self.clicked }
    pub fn hovered(&self) -> bool { self.hovered }
}
```

**交互检测逻辑**：
1. 用 `Font::text_width()` + padding 计算按钮矩形区域
2. 检测 `mouse_x, mouse_y` 是否在矩形内 → `hovered`
3. 检测 `mouse_just_clicked && hovered` → `clicked`
4. 根据状态选择背景色（normal / hover / pressed）
5. 如果 `clicked`，标记 `consumed_mouse = true`
6. 检查 `pending_vdp_actions` 中是否有针对此按钮的 `click` 操作 → 模拟点击

**渲染实现**：
- 背景矩形：使用 1×1 纯白像素纹理（引擎内置），通过 `draw_sprite_tinted` 着色
- 文本：标准 font atlas 渲染

### Panel — 矩形容器

半透明背景面板，用于视觉分组。

```rust
ui.panel(PanelStyle::default(), |ui| {
    ui.label(&font, "Game Over");
    ui.label(&font, "Score: 42");
    if ui.button(&font, "Retry").clicked() {
        self.restart();
    }
});
```

**实现**：
1. 先递归执行子组件，记录总尺寸（宽 = 最大子组件宽，高 = 子组件高度之和 + 间距）
2. 绘制背景矩形（1×1 白像素 + tint）
3. 绘制子组件

Panel 使用**两遍渲染**：第一遍计算布局尺寸，第二遍实际提交 DrawCommand。

```rust
pub struct PanelStyle {
    pub bg_color: Color,       // 默认 Color { r: 0.0, g: 0.0, b: 0.0, a: 0.7 }
    pub padding: f32,          // 默认 12.0
}
```

### ProgressBar — 进度条

用于血条、加载进度等。

```rust
// 基本用法：0.0 ~ 1.0
ui.progress_bar(self.health / self.max_health, 120.0, 12.0);

// 带颜色和 ID
ui.progress_bar_with_id("hp_bar", self.hp / self.max_hp, 120.0, 12.0,
    Color::from_hex(0x44FF44),  // 填充色
    Color::from_hex(0x333333),  // 背景色
);
```

**实现**：两个矩形叠加（背景 + 前景），前景宽度 = 总宽度 × progress。

### TextInput — 文本输入框

**有状态组件**，ID 必须显式提供。支持键盘输入、光标移动、文本选择。

```rust
// 基本用法
let response = ui.text_input("chat_input", &font, 200.0);
if response.submitted {
    let text = ui.text_input_value("chat_input");
    self.send_chat_message(&text);
    ui.text_input_clear("chat_input");
}

// 带 placeholder
let response = ui.text_input_with_placeholder("name_input", &font, 150.0, "Enter name...");
```

**TextInputResponse**：

```rust
pub struct TextInputResponse {
    /// 标准 UI 响应（hovered / pressed / clicked）
    pub response: Response,
    /// 文本内容是否发生变更
    pub changed: bool,
    /// 用户是否按下 Enter 提交
    pub submitted: bool,
}
```

**状态管理**：

```rust
// 读取当前输入内容
let text = ui.text_input_value("chat_input");

// 程序化设置内容
ui.text_input_set_value("chat_input", "Hello");

// 清空输入框
ui.text_input_clear("chat_input");
```

**焦点管理**：
- 点击输入框 → 获得焦点，键盘输入进入此组件
- 点击其他区域或按 Escape → 失去焦点
- `UiOutput::consumed_keyboard = true` 时，游戏代码应跳过键盘输入处理
- 同一时间只有一个 TextInput 可以获得焦点

**键盘事件处理**：

| 按键 | 行为 |
|---|---|
| 可打印字符 | 在光标处插入字符 |
| Backspace | 删除光标前一个字符 |
| Delete | 删除光标后一个字符 |
| Left / Right | 移动光标 |
| Home / End | 光标移到行首/行尾 |
| Enter | 触发 `submitted = true` |
| Escape | 失去焦点 |
| Ctrl+A | 全选 |
| Ctrl+C / Ctrl+V | 复制/粘贴（依赖平台剪贴板，后续实现） |

**渲染实现**：
- 背景矩形：1×1 白像素 + tint（区分 focused / unfocused 状态）
- 文本：font atlas 渲染，裁剪到输入框宽度范围
- 光标：闪烁竖线（1px 宽矩形），闪烁周期 0.5s
- 选中文本：蓝色半透明矩形覆盖

**需要扩展 InputState**：

TextInput 需要接收字符输入事件，当前 `InputState` 只追踪按键状态（pressed/released），需新增：

```rust
impl InputState {
    /// 本帧收到的字符输入（来自 winit ReceivedCharacter 事件）
    pub fn chars_this_frame(&self) -> &[char] { ... }

    /// 本帧收到的键盘事件（区分 key press 和 character input）
    pub fn on_char_received(&mut self, ch: char) { ... }
}
```

`vibe_platform` 层需要新增对 `WindowEvent::Ime` 或 `KeyEvent` 中 `text` 字段的处理，将可打印字符转发到 `InputState`。

### ScrollList — 滚动列表

**有状态组件**，ID 必须显式提供。在固定高度区域内显示可滚动的内容列表。

```rust
// 聊天消息列表
ui.scroll_list("chat_messages", 300.0, 200.0, |ui| {
    for msg in &self.chat_messages {
        ui.label(&font, &format!("{}: {}", msg.sender, msg.text));
    }
});

// 滚动到底部（新消息到来时）
ui.scroll_list_scroll_to_bottom("chat_messages");
```

**ScrollListResponse**：

```rust
pub struct ScrollListResponse {
    pub response: Response,
    /// 当前滚动偏移量（像素）
    pub scroll_offset: f32,
    /// 内容总高度
    pub content_height: f32,
    /// 可见区域高度
    pub visible_height: f32,
}
```

**滚动控制**：

```rust
// 程序化设置滚动位置
ui.scroll_list_set_offset("chat_messages", 0.0);   // 滚动到顶部

// 滚动到底部
ui.scroll_list_scroll_to_bottom("chat_messages");

// 读取当前滚动位置
let offset = ui.scroll_list_offset("chat_messages");
```

**交互**：
- 鼠标滚轮 → 垂直滚动（需要扩展 `InputState` 支持滚轮事件）
- 鼠标拖拽 → 可选的拖拽滚动
- 内容超出可见区域时自动裁剪

**需要扩展 InputState**：

```rust
impl InputState {
    /// 本帧的鼠标滚轮增量（正值 = 向上滚动）。
    pub fn mouse_scroll_delta(&self) -> f32 { ... }

    pub fn on_mouse_scroll(&mut self, delta: f32) { ... }
}
```

**渲染实现**：
- 背景矩形：1×1 白像素 + tint
- 内容渲染：子组件正常渲染，但 Y 坐标偏移 `-scroll_offset`
- 裁剪：超出可见区域的 DrawCommand 在提交前被丢弃（CPU 侧裁剪）
- 滚动条：可选的窄矩形指示器，显示当前滚动位置

**CPU 侧裁剪**：

ScrollList 内的子组件渲染到一个临时的 DrawCommand 缓冲区，提交前对每个 DrawCommand 的 `dst_rect` 进行裁剪判断：

```rust
fn clip_draw_command(cmd: &DrawCommand, clip_rect: [f32; 4]) -> Option<DrawCommand> {
    let [dx, dy, dw, dh] = cmd.dst_rect;
    let [cx, cy, cw, ch] = clip_rect;

    // 完全在裁剪区域外 → 丢弃
    if dx + dw <= cx || dx >= cx + cw || dy + dh <= cy || dy >= cy + ch {
        return None;
    }

    // 部分重叠 → 裁剪 dst_rect 和 src_rect
    // ...（调整 UV 坐标以匹配裁剪后的像素区域）
    Some(clipped_cmd)
}
```

---

## 布局系统

### 锚点定位（Anchor）

控制 UI 内容在屏幕上的位置：

```rust
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}
```

```rust
// 分数显示在右上角
ui.set_anchor(Anchor::TopRight);
ui.set_padding(8.0);
ui.label(&font, "Score: 42");

// Game Over 居中
ui.set_anchor(Anchor::Center);
ui.panel(PanelStyle::default(), |ui| {
    ui.label(&font, "Game Over");
    if ui.button(&font, "Retry").clicked() { ... }
});
```

**实现**：
- 锚点决定"参考点"在屏幕上的位置
- 例如 `TopRight` + padding=8 → 参考点为 `(virtual_width - 8, 8)`
- 子组件从参考点开始，向相应方向展开

### 堆叠布局（LayoutDirection）

```rust
pub enum LayoutDirection {
    Vertical,    // 默认：组件从上到下排列
    Horizontal,  // 组件从左到右排列
}
```

```rust
// 水平排列按钮
ui.set_anchor(Anchor::BottomCenter);
ui.set_layout(LayoutDirection::Horizontal);
ui.set_spacing(16.0);

if ui.button(&font, "Retry").clicked() { ... }
if ui.button(&font, "Quit").clicked() { ... }
```

**间距控制**：

```rust
ui.set_spacing(8.0);   // 组件之间的间距
ui.set_padding(12.0);  // 锚点边距（距屏幕边缘）
```

---

## 样式系统

### Style 结构

```rust
pub struct Style {
    pub text_color: Color,            // 默认 Color::WHITE
    pub spacing: f32,                 // 组件间距，默认 4.0
    pub padding: f32,                 // 边距，默认 8.0
    pub button: ButtonStyle,          // 按钮默认样式
    pub panel: PanelStyle,            // 面板默认样式
    pub text_input: TextInputStyle,   // 输入框默认样式
    pub scroll_list: ScrollListStyle, // 滚动列表默认样式
}

pub struct ButtonStyle {
    pub bg_color: Color,         // 默认 rgba(0.3, 0.3, 0.3, 0.8)
    pub hover_color: Color,      // 默认 rgba(0.5, 0.5, 0.5, 0.8)
    pub pressed_color: Color,    // 默认 rgba(0.2, 0.2, 0.2, 0.9)
    pub text_color: Color,       // 默认 Color::WHITE
    pub padding: f32,            // 文本到按钮边缘的内边距，默认 6.0
}

pub struct TextInputStyle {
    pub bg_color: Color,              // 默认 rgba(0.15, 0.15, 0.15, 0.9)
    pub focused_bg_color: Color,      // 默认 rgba(0.2, 0.2, 0.2, 0.95)
    pub border_color: Color,          // 默认 rgba(0.5, 0.5, 0.5, 0.8)
    pub focused_border_color: Color,  // 默认 rgba(0.4, 0.7, 1.0, 0.9)
    pub text_color: Color,            // 默认 Color::WHITE
    pub placeholder_color: Color,     // 默认 rgba(0.5, 0.5, 0.5, 1.0)
    pub cursor_color: Color,          // 默认 Color::WHITE
    pub selection_color: Color,       // 默认 rgba(0.3, 0.5, 0.8, 0.5)
    pub padding: f32,                 // 默认 4.0
    pub height: f32,                  // 默认由字体行高 + padding 决定
}

pub struct ScrollListStyle {
    pub bg_color: Color,              // 默认 rgba(0.1, 0.1, 0.1, 0.5)
    pub scrollbar_color: Color,       // 默认 rgba(0.5, 0.5, 0.5, 0.5)
    pub scrollbar_width: f32,         // 默认 4.0
    pub padding: f32,                 // 默认 4.0
}
```

用户可设置全局默认样式，也可在单个组件上覆盖。

---

## 1×1 白像素纹理

UI 的矩形（按钮背景、面板背景、进度条、输入框边框、光标、选中高亮、滚动条）通过一个 **1×1 纯白像素纹理** 实现，搭配 `draw_sprite_tinted` 着色。

这张纹理由 UI 系统**自管**：`UiState::init(renderer, assets)` 在引擎 `on_init` 阶段被自动调用一次，内部走和游戏代码完全相同的"动态纹理"路径——

```rust
// crates/vibe_ui/src/state.rs (节选)
pub fn init(&mut self, renderer: &Renderer, assets: &mut AssetManager) {
    if self.white_texture_id.is_some() { return; }
    let tex = renderer.create_white_pixel_texture();   // Renderer 公开 API
    let id = assets.register_texture("__vibe_ui_white", tex); // 普通注册
    self.white_texture_id = Some(id);
}
```

`UiContext::new` 直接从 `UiState.white_texture_id` 读取这个 `TextureId`，游戏代码无需感知。注册时用的 `__vibe_ui_white` 只是 debug label，避免和游戏 `game.yaml` 的纹理名碰撞——**没有"框架内置纹理"分类**，UI 只是另一个使用 `Renderer::create_white_pixel_texture` + `AssetManager::register_texture` 的客户端，与游戏侧自己注册圆环 / 渐变 / 等程序化纹理走的是同一条路径（详见 `docs/api.md` 的"圆形绘制"和"动态纹理"章节）。

优点：
- 无需额外着色器或渲染管线
- 矩形绘制和精灵绘制在同一个 batch 中，无 GPU 状态切换
- 通过 `color` 参数实现任意颜色的矩形

---

## 交互检测

### 鼠标交互

UI 交互依赖 `InputState` 提供的鼠标状态：

| InputState API | UI 用途 |
|---|---|
| `mouse_position()` → `(f32, f32)` | 获取虚拟坐标系下的鼠标位置 |
| `is_mouse_button_just_pressed(Left)` | 检测本帧是否点击（用于 `clicked`） |
| `is_mouse_button_pressed(Left)` | 检测是否按住（用于 `pressed` / 拖拽滚动） |
| `mouse_scroll_delta()` | 滚轮增量（ScrollList 滚动）**新增** |

所有坐标均为虚拟分辨率坐标，与游戏世界一致，无需额外转换。

### 键盘交互

TextInput 获得焦点时需要拦截键盘输入：

| InputState API | UI 用途 |
|---|---|
| `chars_this_frame()` → `&[char]` | 本帧收到的可打印字符 **新增** |
| `is_key_just_pressed(key)` | 检测方向键、Backspace、Enter 等功能键 |

### 焦点系统

```
点击 TextInput A → A 获得焦点
    → UiState.focused = Some("input_a")
    → 键盘字符输入到 A
    → UiOutput.consumed_keyboard = true

点击空白区域 / 按 Escape → 失去焦点
    → UiState.focused = None
    → UiOutput.consumed_keyboard = false
```

焦点存储在 `UiState::focused` 中，跨帧持久。

### 热区（Hit Test）

```rust
fn hit_test(mouse_x: f32, mouse_y: f32, rect: [f32; 4]) -> bool {
    let [x, y, w, h] = rect;
    mouse_x >= x && mouse_x <= x + w && mouse_y >= y && mouse_y <= y + h
}
```

### 遮挡处理

UI 层的组件按绘制顺序处理交互，**后绘制的组件优先响应**。`screen.ui()` 返回 `UiOutput`，包含 `consumed_mouse` 和 `consumed_keyboard` 标记。

---

## VDP 集成

### 设计原则

所有 UI 控件对 VDP 完全透明：

1. **可检查**：VDP 可查询当前帧的完整 UI 控件树（类型、ID、位置、内容、状态）
2. **可操作**：VDP 可模拟点击按钮、输入文本、滚动列表
3. **可自动化**：AI Agent 可通过 VDP 协议完整操控游戏 UI，实现自动化测试和游玩

### 控件快照（WidgetSnapshot）

每帧 `UiContext` 记录所有绘制的控件信息，帧结束后写入 `UiState::last_frame_widgets`：

```rust
pub struct WidgetSnapshot {
    pub id: WidgetId,
    pub widget_type: WidgetType,
    pub rect: [f32; 4],              // 屏幕上的位置和尺寸
    pub visible: bool,
    pub properties: WidgetProperties,
}

pub enum WidgetType {
    Label,
    Button,
    Panel,
    ProgressBar,
    TextInput,
    ScrollList,
}

pub enum WidgetProperties {
    Label {
        text: String,
        color: [f32; 4],
    },
    Button {
        text: String,
        hovered: bool,
        pressed: bool,
    },
    Panel {
        children: Vec<WidgetId>,
    },
    ProgressBar {
        progress: f32,
    },
    TextInput {
        text: String,
        placeholder: String,
        focused: bool,
        cursor_position: usize,
    },
    ScrollList {
        scroll_offset: f32,
        content_height: f32,
        visible_height: f32,
        children: Vec<WidgetId>,
    },
}
```

### VDP 协议方法

#### `ui.inspect` — 检查 UI 控件树

```
>>> {"jsonrpc": "2.0", "id": 1, "method": "ui.inspect"}
<<< {
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "focused": "chat_input",
    "widgets": [
      {
        "id": "score_label",
        "type": "label",
        "rect": [460.0, 8.0, 44.0, 16.0],
        "visible": true,
        "text": "Score: 42"
      },
      {
        "id": "retry_btn",
        "type": "button",
        "rect": [220.0, 160.0, 72.0, 28.0],
        "visible": true,
        "text": "Retry",
        "hovered": false,
        "pressed": false
      },
      {
        "id": "chat_input",
        "type": "text_input",
        "rect": [10.0, 260.0, 200.0, 20.0],
        "visible": true,
        "text": "Hello",
        "placeholder": "Type message...",
        "focused": true,
        "cursor_position": 5
      },
      {
        "id": "chat_messages",
        "type": "scroll_list",
        "rect": [10.0, 50.0, 200.0, 200.0],
        "visible": true,
        "scroll_offset": 120.5,
        "content_height": 450.0,
        "visible_height": 200.0,
        "children": ["__auto_3", "__auto_4", "__auto_5"]
      }
    ]
  }
}
```

#### `ui.inspectWidget` — 检查单个控件

```
>>> {"jsonrpc": "2.0", "id": 2, "method": "ui.inspectWidget", "params": {"id": "chat_input"}}
<<< {
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "id": "chat_input",
    "type": "text_input",
    "rect": [10.0, 260.0, 200.0, 20.0],
    "visible": true,
    "text": "Hello",
    "placeholder": "Type message...",
    "focused": true,
    "cursor_position": 5
  }
}
```

#### `ui.click` — 模拟点击控件

```
>>> {"jsonrpc": "2.0", "id": 3, "method": "ui.click", "params": {"id": "retry_btn"}}
<<< {
  "jsonrpc": "2.0",
  "id": 3,
  "result": {"clicked": true}
}
```

**实现**：将 `VdpUiAction::Click { id }` 加入 `UiState::pending_vdp_actions`。下一帧该按钮组件渲染时，检查队列并触发 `clicked = true`。

#### `ui.setText` — 设置输入框文本

```
>>> {"jsonrpc": "2.0", "id": 4, "method": "ui.setText", "params": {"id": "chat_input", "text": "Hello World"}}
<<< {
  "jsonrpc": "2.0",
  "id": 4,
  "result": {"id": "chat_input", "text": "Hello World"}
}
```

**实现**：直接修改 `UiState::text_inputs[id].text`，设置 `cursor_position` 到文本末尾。

#### `ui.submit` — 模拟 Enter 提交

```
>>> {"jsonrpc": "2.0", "id": 5, "method": "ui.submit", "params": {"id": "chat_input"}}
<<< {
  "jsonrpc": "2.0",
  "id": 5,
  "result": {"submitted": true, "text": "Hello World"}
}
```

**实现**：将 `VdpUiAction::Submit { id }` 加入队列，下一帧 TextInput 渲染时触发 `submitted = true`。

#### `ui.setFocus` — 设置焦点

```
>>> {"jsonrpc": "2.0", "id": 6, "method": "ui.setFocus", "params": {"id": "chat_input"}}
<<< {
  "jsonrpc": "2.0",
  "id": 6,
  "result": {"focused": "chat_input"}
}
```

#### `ui.scroll` — 设置滚动位置

```
>>> {"jsonrpc": "2.0", "id": 7, "method": "ui.scroll", "params": {"id": "chat_messages", "offset": 0.0}}
<<< {
  "jsonrpc": "2.0",
  "id": 7,
  "result": {"id": "chat_messages", "scroll_offset": 0.0}
}
```

#### `ui.scrollToBottom` — 滚动到底部

```
>>> {"jsonrpc": "2.0", "id": 8, "method": "ui.scrollToBottom", "params": {"id": "chat_messages"}}
<<< {
  "jsonrpc": "2.0",
  "id": 8,
  "result": {"id": "chat_messages", "scroll_offset": 250.0}
}
```

### VDP 操作队列

VDP 操作异步注入，在下一帧的 `screen.ui()` 中消费：

```rust
pub enum VdpUiAction {
    Click { id: WidgetId },
    SetText { id: WidgetId, text: String },
    Submit { id: WidgetId },
    SetFocus { id: WidgetId },
    ClearFocus,
    Scroll { id: WidgetId, offset: f32 },
    ScrollToBottom { id: WidgetId },
}
```

```
VDP 线程                        主线程（游戏循环）
    │                               │
    │  收到 ui.click(retry_btn)     │
    │  ──→ VdpRequest 入 channel    │
    │                               │
    │                     on_update: try_recv()
    │                       match "ui.click":
    │                         ui_state.pending_vdp_actions.push(Click{id})
    │                               │
    │                     on_render: screen.ui(ctx, |ui| {
    │                       button("retry_btn"):
    │                         检查 pending_vdp_actions → 找到 Click
    │                         → response.clicked = true
    │                         → 游戏代码执行 self.restart()
    │                     })
    │                               │
    │                     帧结束: pending_vdp_actions.clear()
```

### VDP 请求路由

在 `GameBridge::on_update()` 中新增 `ui.*` 方法的处理分支：

```rust
// 现有的 VDP 处理逻辑
match method {
    "engine.pause" => { ... }
    "engine.resume" => { ... }
    "game.*" => { game.handle_vdp(...) }

    // 新增 UI 方法
    "ui.inspect" => { serialize(ui_state.last_frame_widgets) }
    "ui.inspectWidget" => { find_widget_by_id(...) }
    "ui.click" => { ui_state.pending_vdp_actions.push(Click{...}) }
    "ui.setText" => { ui_state.text_inputs[id].text = ... }
    "ui.submit" => { ui_state.pending_vdp_actions.push(Submit{...}) }
    "ui.setFocus" => { ui_state.focused = Some(id) }
    "ui.scroll" => { ui_state.scroll_lists[id].scroll_offset = ... }
    "ui.scrollToBottom" => { ui_state.pending_vdp_actions.push(ScrollToBottom{...}) }
}
```

`ui.*` 方法由引擎内部直接处理，不需要游戏代码实现 `handle_vdp`。

---

## InputState 扩展

为支持 TextInput 和 ScrollList，需要扩展 `vibe_input`：

### 新增字段

```rust
pub struct InputState {
    // ...现有键盘和鼠标字段...

    // 新增：字符输入
    chars_received: Vec<char>,          // 本帧收到的可打印字符

    // 新增：鼠标滚轮
    scroll_delta: f32,                  // 本帧滚轮增量
}
```

### 新增方法

```rust
impl InputState {
    /// 本帧收到的可打印字符列表。
    pub fn chars_this_frame(&self) -> &[char] {
        &self.chars_received
    }

    /// 由平台层调用，记录一个字符输入事件。
    pub fn on_char_received(&mut self, ch: char) {
        self.chars_received.push(ch);
    }

    /// 本帧的鼠标滚轮增量（正值 = 向上滚动）。
    pub fn mouse_scroll_delta(&self) -> f32 {
        self.scroll_delta
    }

    /// 由平台层调用，记录滚轮事件。
    pub fn on_mouse_scroll(&mut self, delta: f32) {
        self.scroll_delta += delta;
    }

    /// begin_frame 扩展：清空字符输入和滚轮增量。
    pub fn begin_frame(&mut self) {
        // ...现有清空逻辑...
        self.chars_received.clear();
        self.scroll_delta = 0.0;
    }
}
```

### vibe_platform 扩展

```rust
// desktop.rs 事件处理中新增：
WindowEvent::MouseWheel { delta, .. } => {
    let scroll = match delta {
        MouseScrollDelta::LineDelta(_, y) => y * 20.0,  // 行滚动 → 像素
        MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
    };
    self.input.on_mouse_scroll(scroll);
}

WindowEvent::KeyboardInput { event, .. } => {
    // 现有按键处理...

    // 新增：提取可打印字符
    if event.state == ElementState::Pressed {
        if let Some(text) = &event.text {
            for ch in text.chars() {
                if !ch.is_control() {
                    self.input.on_char_received(ch);
                }
            }
        }
    }
}
```

---

## 完整使用示例

### 游戏内聊天 UI

```rust
struct ChatGame {
    messages: Vec<ChatMessage>,
    ui_has_keyboard: bool,
    // ...
}

struct ChatMessage {
    sender: String,
    text: String,
}

impl Game for ChatGame {
    fn draw(&mut self, ctx: &Context, screen: &mut Screen) {
        // 游戏世界渲染...

        let ui_output = screen.ui(ctx, |ui| {
            let font = ctx.assets.font("ui").unwrap();

            // 聊天面板 — 左下角
            ui.set_anchor(Anchor::BottomLeft);
            ui.set_padding(8.0);
            ui.panel(PanelStyle {
                bg_color: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.6 },
                padding: 8.0,
            }, |ui| {
                // 消息列表（可滚动）
                ui.scroll_list("chat_messages", 280.0, 160.0, |ui| {
                    for msg in &self.messages {
                        ui.label_colored(font,
                            &format!("{}: {}", msg.sender, msg.text),
                            Color::from_hex(0xCCCCCC),
                        );
                    }
                });

                // 输入框 + 发送按钮（水平排列）
                ui.set_layout(LayoutDirection::Horizontal);
                ui.set_spacing(4.0);

                let input_response = ui.text_input_with_placeholder(
                    "chat_input", font, 220.0, "Type message...",
                );

                if ui.button_with_id("send_btn", font, "Send").clicked()
                    || input_response.submitted
                {
                    let text = ui.text_input_value("chat_input");
                    if !text.is_empty() {
                        self.messages.push(ChatMessage {
                            sender: "Player".to_string(),
                            text: text.to_string(),
                        });
                        ui.text_input_clear("chat_input");
                        ui.scroll_list_scroll_to_bottom("chat_messages");
                    }
                }
            });
        });

        // 聊天 UI 活跃时跳过游戏输入
        self.ui_has_keyboard = ui_output.consumed_keyboard;
    }

    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
        if !self.ui_has_keyboard {
            // 正常处理游戏输入...
        }
    }
}
```

### VDP 自动化聊天测试

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

# 1. 检查 UI 状态
result = rpc("ui.inspect")
print("当前 UI 控件:", [w["id"] for w in result["result"]["widgets"]])

# 2. 聚焦输入框
rpc("ui.setFocus", {"id": "chat_input"})

# 3. 输入文本
rpc("ui.setText", {"id": "chat_input", "text": "Hello from VDP!"})

# 4. 提交（模拟 Enter）
rpc("ui.submit", {"id": "chat_input"})

# 5. 验证消息列表更新
import time
time.sleep(0.1)
result = rpc("ui.inspectWidget", {"id": "chat_messages"})
print("滚动位置:", result["result"]["scroll_offset"])

# 6. 点击发送按钮（另一种方式）
rpc("ui.setText", {"id": "chat_input", "text": "Sent via button click"})
rpc("ui.click", {"id": "send_btn"})

# 7. 截图验证
rpc("game.screenshot", {"path": "/tmp/chat_test.png"})
```

### Flappy Bird — 改造后

```rust
fn draw(&mut self, ctx: &Context, screen: &mut Screen) {
    // ...游戏世界渲染...

    screen.ui(ctx, |ui| {
        match self.state {
            GameState::Idle => {
                ui.set_anchor(Anchor::Center);
                ui.set_spacing(8.0);
                ui.label(&ctx.assets.font("score").unwrap(), "Flappy Bird");
                ui.label(&ctx.assets.font("ui").unwrap(), "Press SPACE to start");
                if self.best_score > 0 {
                    ui.label(
                        &ctx.assets.font("ui").unwrap(),
                        &format!("Best: {}", self.best_score),
                    );
                }
            }
            GameState::Playing => {
                ui.set_anchor(Anchor::TopCenter);
                ui.set_padding(10.0);
                ui.label_with_id("score_label",
                    &ctx.assets.font("score").unwrap(),
                    &self.score.to_string(),
                );
            }
            GameState::Dead => {
                ui.set_anchor(Anchor::Center);
                ui.panel(PanelStyle::default(), |ui| {
                    ui.label(&ctx.assets.font("score").unwrap(), "Game Over");
                    ui.label(
                        &ctx.assets.font("ui").unwrap(),
                        &format!("Score: {}", self.score),
                    );
                    if self.best_score > 0 {
                        ui.label(
                            &ctx.assets.font("ui").unwrap(),
                            &format!("Best: {}", self.best_score),
                        );
                    }
                    if ui.button_with_id("retry_btn",
                        &ctx.assets.font("ui").unwrap(), "Retry",
                    ).clicked() {
                        self.restart();
                    }
                });
            }
            _ => {}
        }
    });
}
```

---

## 渲染集成

### Update/Draw 分离架构

UI 系统严格遵循 **update/draw 分离** 原则：

- **`update_ui()` 阶段**（update 阶段）：用户构建 UI、处理交互。此时有 `&mut Context` + `&InputState`，可以修改 `UiState`
- **`draw()` 阶段**（render 阶段）：纯渲染，不涉及 UI 构建。UI 的 draw commands 由引擎自动回放

```
update 阶段                          render 阶段
┌─────────────┐                    ┌─────────────────┐
│ game.update │                    │ game.draw        │
│  (游戏逻辑)  │                    │  (场景/精灵渲染) │
├─────────────┤                    ├─────────────────┤
│ game.update_ui                   │ 引擎自动回放     │
│  (构建 UI)  │ → UiContext        │ cached commands  │
│             │   缓存 commands    │  (UI 叠加在顶层) │
│             │   → UiState        │                  │
└─────────────┘                    └─────────────────┘
```

**为什么不在 `draw()` 中构建 UI？**

`Game::draw()` 的签名是 `fn draw(&self, ctx: &Context, screen: &mut Screen)`，设计上只做纯渲染：

1. `ctx` 是不可变引用 — 无法修改 `UiState`（UI 交互需要修改焦点、文本缓冲等持久状态）
2. 没有 `&InputState` 参数 — UI 组件需要检测鼠标点击、键盘输入
3. 修改 `draw()` 签名会破坏现有 API，对所有游戏代码造成侵入式改动

因此引入了 `update_ui()` 方法，在 update 阶段调用，拥有完整的可变访问权限。

### 缓存 Draw Commands

`UiContext` **不依赖 `&mut Renderer`**，而是自己维护 `Vec<DrawCommand>` 内部缓冲区：

```rust
// UiContext 不再持有 Renderer 引用
pub struct UiContext<'a> {
    ui_state: &'a mut UiState,
    input: &'a InputState,
    draw_commands: Vec<DrawCommand>,  // 内部缓冲区
    // ...
}

impl UiContext<'_> {
    // finish() 时将 commands 存入 UiState
    pub fn finish(self) -> UiOutput {
        self.ui_state.cached_draw_commands = self.draw_commands;
        // ...
    }
}
```

引擎在 `on_render` 中 `game.draw()` 之后，自动回放缓存的 commands：

```rust
// GameBridge::on_render
fn on_render(&mut self, renderer: &mut Renderer) {
    game.draw(&ctx, &mut screen);

    // 自动回放 UI draw commands，叠加在游戏画面之上
    for cmd in &ctx.ui_state.cached_draw_commands {
        renderer.draw_sprite(*cmd);
    }
}
```

这种设计带来三个好处：
1. **UI 构建不需要 Renderer** — `update_ui()` 在 update 阶段调用，完全不依赖图形管线
2. **draw() 签名不变** — 现有游戏代码零改动
3. **UI 始终渲染在最顶层** — draw commands 在所有游戏精灵之后提交

### Game::update_ui() 方法

```rust
pub trait Game {
    fn new(ctx: &mut Context) -> Self;
    fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState);
    fn draw(&self, ctx: &Context, screen: &mut Screen);

    /// 在 update 阶段构建 UI。
    /// UI draw commands 自动缓存，在 draw 阶段回放到画面最顶层。
    fn update_ui(&mut self, _ctx: &mut Context, _input: &InputState) {}

    fn clear_color(&self) -> Color { Color::BLACK }
}
```

用户实现 `update_ui()` 时，需要 take/swap `ctx.ui_state` 来同时访问 `ctx.assets`：

```rust
fn update_ui(&mut self, ctx: &mut Context, input: &InputState) {
    // UI 自管自己的 1×1 白像素纹理（见上文「1×1 白像素纹理」章节），
    // UiContext 从 UiState 直接读取，无需游戏传入。

    // Take ui_state 出来，这样可以同时借用 ctx.assets
    let mut ui_state = std::mem::take(&mut ctx.ui_state);
    let mut ui = UiContext::new(&mut ui_state, input, vw, vh);

    // 构建 UI...
    ui.set_anchor(Anchor::TopCenter);
    if let Some(font) = ctx.assets.font("score") {
        ui.label(font, "Score: 42");
    }

    ui.finish();
    ctx.ui_state = ui_state;  // 放回
}
```

### 绘制顺序

```
每帧 DrawCommand 队列：
├── [0..N]   游戏世界精灵（game.draw 产生）
└── [N..M]   UI draw commands（从 UiState 缓存回放）
    ├── UI 矩形（1×1 白像素 tinted）
    └── UI 文本（font atlas 字形）
```

### Batch 优化

**优化策略**：Panel 组件内部先批量提交所有矩形 DrawCommand，再批量提交所有文本 DrawCommand，减少 batch 数量。

```
button_bg (tex: white) ─┐
panel_bg  (tex: white) ─┤── Batch: 1 次 draw call
progress  (tex: white) ─┘
label_a   (tex: font)  ─┐
label_b   (tex: font)  ─┤── Batch: 1 次 draw call
button_txt(tex: font)  ─┘
```

---

## 实现优先级

| 阶段 | 内容 | 状态 |
|---|---|---|
| **Phase 1** | UiContext + UiState + WidgetId + Label + Anchor + 1×1 白像素 | ✅ 完成 |
| **Phase 2** | Button + Response + 交互检测 + 焦点系统 | ✅ 完成 |
| **Phase 3** | Panel（两遍渲染 + 子组件布局）+ Spacing + Style | ✅ 完成 |
| **Phase 4** | ProgressBar + Horizontal 布局 + UiOutput | ✅ 完成 |
| **Phase 5** | InputState 扩展（chars + scroll）+ vibe_platform 事件转发 | ✅ 完成 |
| **Phase 6** | TextInput（键盘输入 + 光标 + 选中 + 渲染） | ✅ 完成 |
| **Phase 7** | ScrollList（滚动 + CPU 裁剪 + 滚动条） | ✅ 完成 |
| **Phase 8** | VDP 集成（WidgetSnapshot + ui.* 方法 + VdpUiAction 队列） | ✅ 完成 |
| **Phase 9** | 缓存 Draw Commands + update/draw 分离 + update_ui() | ✅ 完成 |
| **Phase 10** | IME 输入法支持 + 字体 lazy 加载（CJK） | ✅ 完成 |

---

## Phase 10: IME 输入法 + CJK 字体

### 背景

原始 `TextInput` 只通过 `WindowEvent::KeyboardInput::text` 收集字符，而该字段对中日韩输入法（IME）返回的是被吞掉的 ASCII 占位（多数情况下根本不触发），导致中文/日文/韩文/Emoji 全部无法输入。同时 `vibe_render::Font` 在加载时一次性预烘焙 ASCII 字符集，连显示中文都办不到。

Phase 10 解决这两个互相依赖的问题。

### 跨平台 IME 接入（vibe_input + vibe_platform）

`InputState` 新增两个字段：

```rust
pub struct InputState {
    // ... 既有字段 ...
    ime_commit: String,                 // 本帧 IME 提交的整段文本
    ime_preedit: Option<ImePreedit>,    // 进行中的合成（不写入 buffer）
}

pub struct ImePreedit {
    pub text: String,
    pub cursor_byte: Option<usize>,
}
```

关键约定：
- `ime_commit` 在 `begin_frame()` 中清空（与 `chars_received` 同语义：每帧消费一次）。
- `ime_preedit` **不**在 `begin_frame()` 中清空 —— 它是 IME 的多帧合成状态，由平台层在 `Ime::Disabled` 或显式 `clear_ime_preedit()` 时清除。
- IME 合成期间 `ime_preedit` 非空，平台层会**抑制**普通的 `chars_received`，避免把 ASCII 拼音回显进 buffer 又再被 commit 一次。

`vibe_platform::desktop` 中：

```rust
window.set_ime_allowed(true); // 关键开关，否则 winit 不会发 Ime 事件

WindowEvent::Ime(ime) => match ime {
    Ime::Enabled | Ime::Disabled => input.clear_ime_preedit(),
    Ime::Preedit(text, cursor_range) => {
        let cursor_byte = cursor_range.map(|(start, _end)| start);
        input.on_ime_preedit(text, cursor_byte);
    }
    Ime::Commit(text) => input.on_ime_commit(&text),
}
```

winit 的 `Ime` 事件在 macOS / Windows / Linux（X11、Wayland）上由各自的原生 IME 框架转发，对游戏代码完全透明。

### TextInput 的 IME 处理

`text_input_impl` 中的关键改动：

1. **在 IME 合成期间不消费 `chars_this_frame`**，避免重复输入。
2. **将 `ime_commit` 整段 `insert_str` 到光标位置**，而不是逐字符 push —— 中文一次提交往往是多字节的一个或多个汉字。
3. **渲染 preedit 浮层**：在光标右侧叠加显示合成文本，加底部下划线（贴合各 OS 原生 IME 视觉约定），且在 preedit 活跃时关闭光标闪烁。

### Font lazy 加载（vibe_render）

`Font` 重写为 lazy 字形 atlas：

```rust
pub struct Font {
    font: fontdue::Font,                  // 拥有的运行时 rasterizer
    pub atlas_texture_id: TextureId,
    glyphs: HashMap<char, GlyphInfo>,
    atlas_size: u32,                      // 256 → 512 → 1024 → 2048 → 4096（按需翻倍）
    atlas_data: Vec<u8>,
    packer: ShelfPacker,
    dirty: bool,
    pub size: f32,
    pub line_height: f32,
}
```

加载时 atlas 完全为空，所有字形按需生成。新增 `prepare_text(device, queue, layout, atlas_texture, text) -> PrepareOutcome`：

```rust
pub enum PrepareOutcome {
    NoChange,                  // 全部已缓存
    AtlasUpdated,              // 在原 GPU 纹理上 write_texture 上传新像素
    AtlasResized(Texture),     // atlas 翻倍，调用方需替换 textures[atlas_id]
}
```

满了之后翻倍 atlas 大小并**重新 rasterize 所有已缓存字形**（fontdue 的栅格化是确定性的，重 raster 比 memcpy 简单）。`MAX_ATLAS_SIZE = 4096`，单字号下能容纳约 16k 个 CJK 字形。

`text_width` / `layout_text` 对未 prepare 的字符以"半字号宽度"作为兜底 advance，使光标位置和测量结果保持一致 —— 这是简化版的 tofu，不绘制空白方框（绘制需要在 Font 内部反向引用 white_pixel 纹理，复杂度不值）。

### Renderer / AssetManager 边界

GPU 资源的所有权严格属于 `Renderer`。`vibe_render` 把 `device` / `queue` / `texture_bind_group_layout` 包在 `Arc` 里自持，并对外暴露三个高层 API：

| Renderer 方法 | 用途 |
|--------------|------|
| `load_texture(label, bytes) -> Texture` | 解码 PNG/JPG 并上传成 GPU 纹理 |
| `load_font(bytes, size, atlas_id) -> (Font, Texture)` | 创建 lazy atlas 并预热 ASCII |
| `prepare_text(font, atlas_slot, text)` | 把 `text` 中所有未栅格化的字符上传到 atlas，atlas 满了就翻倍并写回 `atlas_slot` |

`vibe_asset::AssetManager` **不依赖 wgpu**，只持有已上传的 `Texture` / `Font` 句柄和"名称→ID"索引。`AssetManager::load_textures` / `load_fonts` / `prepare_text` 这三个方法都接收 `&Renderer` 参数，把 GPU 工作委托回 `Renderer`。

### 游戏代码的调用入口

游戏应该用 **`ctx.prepare_text(font_name, text)`**（在 `update` / `update_ui` 中），而不是直接调 `ctx.assets.prepare_text(...)`。原因：`update` 阶段没有 `Renderer` 借用（take/swap 模式），所以 `Context::prepare_text` 只是把请求**排队**到 `Context::pending_text_prep`，由 `GameBridge` 在 `on_render` 开头 flush。

```rust
fn update(&mut self, ctx: &mut Context, dt: f32, input: &InputState) {
    // ...
    let mut to_prepare = String::new();
    for msg in &self.messages { to_prepare.push_str(msg); }
    if let Some(state) = ctx.ui_state.text_inputs.get(&chat_id) {
        to_prepare.push_str(&state.text);
    }
    if let Some(preedit) = input.ime_preedit() {
        to_prepare.push_str(&preedit.text);
    }
    ctx.prepare_text("cjk", &to_prepare); // 排队，零成本
}
```

**调用约定**：把本帧将要绘制的所有非 ASCII 文本（消息列表、输入框 buffer、当前 IME preedit）拼起来一次性 prepare。idempotent —— 已缓存字符不会重复栅格化。准备失败仅打 warn，缺字以空白渲染，不影响游戏继续运行。

**一帧抖动**：`prepare_text` 排队 → flush 在 `on_render` 开头 → 但 `update_ui` 阶段的 `text_width()` 测量发生在 flush 之前。所以某字符首次出现的那一帧，宽度测量用 fallback advance（`size * 0.5`），下一帧才精确。CJK 实际 advance ≈ `size * 1.0`，会有一帧约 50% 宽度的视觉抖动，仅在该字符首次出现时发生一次。

### 字体配置

`game.yaml` 中字体仍然是 `"路径:字号"` 格式，引擎本身**不自带字体**。

```yaml
assets:
  fonts:
    body: "assets/fonts/ui.ttf:14"
    cjk: "assets/fonts/SourceHanSansSC-Regular.otf:14"   # 用户自备 CJK 字体
```

`examples/ui` 自带一份 [思源黑体 SC Regular](https://github.com/adobe-fonts/source-han-sans)（OFL 授权）作为 CJK 输入演示用字体，**仅用于示例**，不作为引擎内置资源。生产游戏需要自行选择并放入字体文件。

---

## 设计决策总结

| 决策点 | 选择 | 原因 |
|---|---|---|
| 依赖方式 | 必选依赖，始终编译 | UI 是引擎核心能力，所有游戏都受益 |
| UI 模式 | 即时模式 + 持久状态 | 即时模式保持 draw() 哲学；TextInput/ScrollList 需要跨帧状态 |
| 持久状态位置 | `Context.ui_state` | 和 AssetManager 同级，Take/Swap 模式一致 |
| 矩形绘制 | 1×1 白像素 + color tint | 零额外着色器/管线，复用 sprite batch |
| 布局方式 | 锚点 + 线性堆叠 | 覆盖 90% 游戏 UI 场景，实现简单 |
| 交互方式 | 鼠标热区 + 键盘字符输入 | 利用现有 InputState，扩展字符/滚轮事件 |
| 文本输入 | 内建 TextInput 组件 | 支持聊天场景，无需外部 GUI 库 |
| 滚动列表 | CPU 侧裁剪 | 不依赖 GPU scissor，与 sprite batch 兼容 |
| VDP 集成 | 控件 ID + 快照 + 操作队列 | 所有控件对 AI Agent 完全透明可控 |
| 字体 | 复用 vibe_render::Font | 不引入新的文本渲染路径 |
| UI 构建阶段 | update 阶段（`update_ui()`） | `draw()` 签名为 `&self` + `&Context`，无法修改 UI 状态和获取 InputState |
| Draw Commands | 缓存在 UiState，render 阶段回放 | UiContext 不依赖 Renderer，update 阶段可独立构建 UI |
| draw() 签名 | 保持不变 `fn draw(&self, ctx: &Context, screen: &mut Screen)` | 零侵入，不破坏现有游戏代码 |
