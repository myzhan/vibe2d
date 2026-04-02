# VDP (Vibe Debug Protocol) 协议规范

版本：0.1.0

## 概述

VDP 是 Vibe2D 引擎的调试协议，基于 **WebSocket + JSON-RPC 2.0**，允许外部工具（CLI、AI agent、测试脚本）在运行时查询和修改游戏状态。设计灵感来自 Chrome DevTools Protocol。

## 连接

- **传输层**: WebSocket
- **默认地址**: `ws://127.0.0.1:9229`
- **连接模型**: 单连接（一次只接受一个客户端）

### 配置

在 `game.yaml` 中启用和配置 VDP：

```yaml
debug:
  vdp:
    enabled: true
    port: 9229       # 可选，默认 9229
```

## 消息格式

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "engine.info",
  "params": {}
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `jsonrpc` | string | 是 | 固定 `"2.0"` |
| `id` | number/string | 是 | 请求标识符，响应中原样返回 |
| `method` | string | 是 | 方法名 |
| `params` | object | 否 | 方法参数，默认 `{}` |

### 成功响应

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { ... }
}
```

### 错误响应

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "Method not found: foo.bar"
  }
}
```

### 错误码

| 错误码 | 含义 |
|--------|------|
| `-32700` | 解析错误（无效 JSON） |
| `-32601` | 方法不存在 |
| `-32000` | 服务端错误 / 超时（5 秒） |

---

## 内置方法

以下方法由引擎内部实现，所有游戏通用。

### `engine.info`

查询引擎版本和虚拟分辨率。

**参数**: 无

**响应**:

```json
{
  "engine": "vibe2d",
  "version": "0.1.0",
  "virtual_width": 512.0,
  "virtual_height": 288.0
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `engine` | string | 固定 `"vibe2d"` |
| `version` | string | 引擎版本号 |
| `virtual_width` | float | 虚拟分辨率宽度（像素） |
| `virtual_height` | float | 虚拟分辨率高度（像素） |

---

### `game.inspect`

查询当前游戏状态快照。返回内容由游戏的 `Game::inspect()` trait 方法定义。

**参数**: 无

**响应**: 游戏自定义 JSON 结构

**错误**: 游戏未初始化时返回 `"Game not initialized"`

---

### `game.screenshot`

请求截图，截图在下一帧渲染时捕获并保存为 PNG。

**参数**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | string | 否 | 输出路径，默认 `"screenshot.png"` |

**响应**:

```json
{
  "path": "/tmp/screenshot.png",
  "status": "queued"
}
```

**说明**:
- `status: "queued"` 表示截图已排队，将在下一帧渲染时执行
- 截图使用离屏纹理（RENDER_ATTACHMENT + COPY_SRC），不依赖 surface 纹理
- 输出分辨率为虚拟分辨率（如 512x288），PNG 格式

---

## 游戏自定义方法

游戏通过实现 `Game` trait 的 `handle_vdp()` 方法注册自定义 VDP 方法。方法名建议使用 `game.` 前缀。

### Game trait 接口

```rust
pub trait Game {
    /// 返回游戏状态快照（供 game.inspect 使用）
    fn inspect(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// 处理自定义 VDP 方法
    fn handle_vdp(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err(format!("Unknown method: {}", method))
    }
}
```

### 方法路由顺序

1. 先匹配内置方法（`engine.info`、`game.inspect`、`game.screenshot`）
2. 未匹配则转发到 `Game::handle_vdp()`
3. 仍未匹配则返回 `-32601 Method not found`

### 示例：Flappy Bird 自定义方法

#### `game.setBirdY`

设置小鸟 Y 坐标，可选重置速度。

**参数**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `y` | float | 是 | Y 坐标（0=顶部，正方向向下） |
| `vy` | float | 否 | 垂直速度，省略则不修改 |

**响应**:

```json
{
  "bird_y": 100.0,
  "bird_vy": 0.0
}
```

#### `game.setScore`

设置当前分数。

**参数**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `score` | integer | 是 | 分数值 |

**响应**:

```json
{
  "score": 42
}
```

#### `game.setState`

设置游戏状态机状态。

**参数**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `state` | string | 是 | 目标状态 |

**有效状态**: `"idle"`, `"countdown"`, `"playing"`, `"dead"`

**响应**:

```json
{
  "state": "playing"
}
```

**状态切换副作用**:

| 目标状态 | 副作用 |
|----------|--------|
| `idle` | 重置小鸟位置和速度 |
| `countdown` | 重置游戏（清管道、清分数）、启动 3 秒倒计时 |
| `playing` | 无 |
| `dead` | 若当前分数 > 最高分则更新最高分 |

---

## 架构

### 线程模型

```
┌──────────────┐    mpsc channel    ┌──────────────┐
│  Game Thread  │ ◄──── request ──── │  VDP Server  │
│  (main loop)  │ ──── response ───► │  (tokio)     │
└──────────────┘                    └──────┬───────┘
                                           │ WebSocket
                                    ┌──────┴───────┐
                                    │   Client     │
                                    │ (CLI/Python) │
                                    └──────────────┘
```

- **VDP Server**: 独立线程，tokio single-threaded runtime
- **Game Thread**: 主线程，每帧通过 `try_recv()` 非阻塞收取请求
- **通信**: 双向 `std::sync::mpsc` channel
- **超时**: 服务端等待游戏线程响应最多 5 秒

### 请求处理时序

```
Client                 VDP Server              Game Thread
  │                        │                        │
  │── WS text message ────►│                        │
  │                        │── mpsc send ──────────►│
  │                        │                        │── handle_vdp_request()
  │                        │                        │── build VdpResponse
  │                        │◄── mpsc send ──────────│
  │◄── WS text message ───│                        │
```

### 关键约束

- VDP 请求在 `game.update()` **之前**处理（每帧开头）
- 所有方法调用是**同步**的，阻塞当前帧
- 游戏线程无 async，不需要 tokio

---

## CLI 工具

`vibe` CLI 提供 VDP 的命令行接口。

### `vibe inspect`

```bash
vibe inspect [--addr ws://127.0.0.1:9229]
```

等价于调用 `game.inspect`，输出 pretty-printed JSON。

### `vibe rpc <method> [params]`

```bash
vibe rpc engine.info
vibe rpc game.setBirdY '{"y": 100, "vy": 0}'
vibe rpc game.setState '{"state": "idle"}'  --addr ws://127.0.0.1:9229
```

通用 RPC 调用，`params` 为 JSON 字符串，默认 `{}`。

### `vibe screenshot`

```bash
vibe screenshot [--output screenshot.png] [--addr ws://127.0.0.1:9229]
```

请求截图并等待 200ms 让文件写入完成。

---

## 实现自定义方法指南

### 1. 实现 `inspect()`

```rust
fn inspect(&self) -> serde_json::Value {
    serde_json::json!({
        "state": "playing",
        "player": {
            "x": self.player_x,
            "y": self.player_y,
            "hp": self.player_hp,
        },
        "enemies": self.enemies.len(),
    })
}
```

### 2. 实现 `handle_vdp()`

```rust
fn handle_vdp(
    &mut self,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "game.setPlayerPos" => {
            let x = params.get("x").and_then(|v| v.as_f64())
                .ok_or("Missing 'x' parameter")?;
            let y = params.get("y").and_then(|v| v.as_f64())
                .ok_or("Missing 'y' parameter")?;
            self.player_x = x as f32;
            self.player_y = y as f32;
            Ok(serde_json::json!({"x": self.player_x, "y": self.player_y}))
        }
        _ => Err(format!("Unknown method: {}", method)),
    }
}
```

### 命名约定

- 内置方法: `engine.*`
- 游戏状态查询: `game.inspect`（内置）
- 游戏自定义方法: `game.<camelCase>` （如 `game.setBirdY`、`game.setState`）
