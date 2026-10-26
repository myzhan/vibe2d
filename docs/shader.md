# 自定义 Shader（片段着色器）

Vibe2D 支持游戏级**自定义片段着色器**（Love2D pixel shader 风格）。本特性在
`dev.shader` 分支实现。

设计目标是在**不破坏引擎三大约束**的前提下加入 shader：

1. **sprite 批处理** —— 自定义 shader 只是多一个「断批 key」，顶点/索引缓冲与内置管线共用；
2. **VDP 截图确定性** —— 主渲染 pass 与离屏截图 pass 走同一条 `execute_draw_commands`，uniform 在 `render()` 开头一次性写入；
3. **`DrawCommand` 是纯数据** —— 只新增一个 `shader_id` 字段，不引入闭包 / GPU 句柄。

---

## 最小用法

### 1. 编译 shader（在 `Game::new`，拿得到 `&Renderer`）

```rust
const MY_FS: &str = r#"
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // uniforms / t_texture / t_sampler / VertexOutput 都由引擎 preamble 提供
    return vec4<f32>(in.color.rgb, in.color.a) * (0.5 + 0.5 * sin(uniforms.time));
}
"#;

let my_shader = renderer
    .create_fragment_shader("my_shader", MY_FS)   // -> Result<ShaderId>
    .expect("shader failed to compile");
```

游戏只需写 **`@fragment fn fs_main`** 这一个函数（外加自己的辅助函数）。其余
（顶点阶段、纹理/采样器绑定、uniform 块）由引擎的 preamble 自动前置拼接。

### 2. 绘制时选中并喂参数（在 `Game::draw`，通过 `Screen`）

```rust
screen.set_shader(my_shader);
screen.set_shader_uniforms(
    my_shader,
    ShaderUniforms::new().with_params([strength, 0.0, 0.0, 0.0]),
);
screen.draw_sprite(tex, x, y, w, h);   // 这张 sprite 走自定义管线

screen.clear_shader();                  // 切回内置管线，之后的绘制恢复正常 sprite
```

`on_shader(shader, |screen| { ... })` 可把一段绘制包进某个 shader 并自动复位。

---

## 运行时契约

自定义 WGSL 会被前置拼接 `crates/vibe_render/src/shader_common.wgsl`，因此以下符号
**在 `fs_main` 里直接可用**：

| 符号 | 说明 |
|------|------|
| `VertexOutput` | 顶点入参：`tex_coords: vec2<f32>`、`color: vec4<f32>` |
| `t_texture` / `t_sampler` | `@group(1)`，sprite 所属图集纹理 + 采样器 |
| `uniforms` | `@group(2) @binding(0)` 的 `ShaderUniforms` 块（见下） |
| `in.color` | 顶点色（`draw_sprite_tinted` / UI tint 会写入） |

### `ShaderUniforms` 结构

```wgsl
struct ShaderUniforms {
    time:    f32,   // 引擎自动推进：自 Renderer 创建起经过的秒数（类比 love_Time）
    _pad0.._pad2,   // 3 个填充 float，把下面两个 vec4 顶到 16 字节对齐
    params:  vec4<f32>,   // 游戏自定义槽位 A
    params2: vec4<f32>,   // 游戏自定义槽位 B
};
```

- **`time` 无需游戏干预** —— `Renderer::render()` 每帧用 `Instant::elapsed()` 统一写入
  所有自定义 shader 的 uniform buffer。
- **`params` / `params2`** 是游戏侧传参通道，用 `ShaderUniforms::new().with_params([f32;4]).with_params2([f32;4])`
  设置，再通过 `Screen::set_shader_uniforms(shader, uniforms)` 应用。当前只有这
  **两个固定 `vec4` 槽**（共 8 个 float），不支持任意命名 uniform。

Rust 端 `ShaderUniforms` 与 WGSL 内存布局（48 字节、`params`@16、`params2`@32）
由 `vibe_render` 单测 `shader_uniforms_mirrors_wgsl_layout` 锁定，二者不能漂移。

---

## 批处理如何保持

每个管线（内置 + 每个自定义 shader）共享同一套 bind group layout 与顶点布局，
`DrawCommand` 新增的 `shader_id` 与 `texture_id`、`clip` 一样是断批条件：

- 连续、同纹理、同 shader 的绘制合并成一个 batch；
- shader 变化 → 断批（触发一次 `set_pipeline` + `set_bind_group(2, …)`）。

因此混用自定义 shader 与内置管线时，draw call 数量只按「实际切换次数」增长，
`vibe_render` 的 `shader_change_breaks_the_batch_even_with_one_texture` /
`default_shader_commands_still_coalesce_by_texture` 两个单测覆盖此行为。

---

## 编译错误处理

`create_fragment_shader` 返回 `Result<ShaderId>`：

- **原生平台**：用 wgpu 的 validation error scope 捕获非法 WGSL，返回 `Err(...)`
  （附带 wgpu 的编译错误信息），不会 panic、也不会往注册表塞坏管线。
- **wasm**：`pop_error_scope` 的 future 无法同步阻塞，编译错误落到浏览器控制台
  的 error handler，Rust 侧仍返回 `Ok`。

---

## 已知限制（v1）

- **仅片段着色器**：不支持自定义顶点阶段。
- **固定 uniform 块**：只有 `time` + 两个 `vec4`，无任意命名 uniform / 纹理数组。
- **无后处理 canvas**：不能把场景先渲到离屏 RT 再全屏过一遍 shader（需要
  render-to-texture，尚未实现）。
- **shader 生命周期 = Renderer 生命周期**：注册后不回收；适合启动时一次性编译。

---

## 相关文件

- `crates/vibe_render/src/shader.rs` —— `ShaderId`、`ShaderUniforms`、`create_fragment_shader`
- `crates/vibe_render/src/shader_common.wgsl` —— 所有管线共享的 preamble
- `crates/vibe_render/src/sprite.wgsl` —— 内置管线只剩的默认 `fs_main`
- `crates/vibe_render/src/renderer.rs` —— `DrawCommand.shader_id`、`compute_batches`、`execute_draw_commands`
- `crates/vibe2d/src/screen.rs` —— `set_shader` / `clear_shader` / `on_shader` / `set_shader_uniforms`
- `examples/shader-demo/` —— 可运行示例（等离子背景 + ripple 弹跳球 + 内置 HUD）及其 VDP 端到端测试 `tests/vdp_shader.rs`

运行示例：

```bash
cargo run -p shader-demo
```

跑 shader-demo 集成测试（会真开一个窗口，需 GPU）：

```bash
cargo test -p shader-demo -- --ignored --test-threads=1
```
