//! [`VdpClient`] — JSON-RPC 2.0 client over a live VDP WebSocket.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

pub(crate) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A thin JSON-RPC 2.0 client over a live VDP WebSocket.
///
/// All methods are `async`. Requests are serialized with auto-incrementing
/// ids and responses are correlated by id, so calls may technically be
/// pipelined — though in practice each helper awaits its own response before
/// returning.
pub struct VdpClient {
    ws: WsStream,
    next_id: AtomicU64,
}

impl VdpClient {
    /// Connect to a VDP server that is already running.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let url = format!("ws://{}", addr);
        let (ws, _) = connect_async(&url)
            .await
            .with_context(|| format!("WS handshake failed for {}", url))?;
        Ok(Self {
            ws,
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC call and await the response with matching id.
    ///
    /// The returned `Value` is the full envelope (contains either `result`
    /// or `error`). Use [`VdpClient::call_ok`] when you want the `result`
    /// unwrapped and any error turned into an `Err`.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.ws
            .send(Message::Text(req.to_string().into()))
            .await
            .context("send VDP request")?;

        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.ws.next())
                .await
                .map_err(|_| anyhow!("timed out waiting for VDP response to {}", method))?
                .ok_or_else(|| anyhow!("VDP WebSocket closed unexpectedly"))?
                .context("VDP read error")?;
            if let Message::Text(text) = msg {
                let value: Value = serde_json::from_str(&text)
                    .with_context(|| format!("invalid VDP JSON: {}", text))?;
                if value.get("id") == Some(&json!(id)) {
                    return Ok(value);
                }
                // Out-of-order frame — keep reading (shouldn't happen with
                // the current VDP server, but be defensive).
            }
        }
    }

    /// Like [`call`](Self::call) but unwraps `result` and converts any
    /// JSON-RPC `error` into an `Err`.
    pub async fn call_ok(&mut self, method: &str, params: Value) -> Result<Value> {
        let resp = self.call(method, params).await?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("VDP error from {}: {}", method, err));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("VDP response for {} missing `result`", method))
    }

    // ── Engine built-in methods ─────────────────────────────────────

    pub async fn engine_info(&mut self) -> Result<Value> {
        self.call_ok("engine.info", json!({})).await
    }

    pub async fn pause(&mut self) -> Result<Value> {
        self.call_ok("engine.pause", json!({})).await
    }

    pub async fn resume(&mut self) -> Result<Value> {
        self.call_ok("engine.resume", json!({})).await
    }

    /// Single-step N frames. Requires the engine to be paused.
    pub async fn step(&mut self, frames: u32) -> Result<Value> {
        self.call_ok("engine.step", json!({ "frames": frames }))
            .await
    }

    pub async fn get_time(&mut self) -> Result<Value> {
        self.call_ok("engine.getTime", json!({})).await
    }

    /// Convenience: step `frames` and block until `engine.getTime`
    /// confirms the counter advanced. Useful because `engine.step` is
    /// non-blocking — the main thread consumes frames asynchronously.
    pub async fn step_and_wait(&mut self, frames: u32) -> Result<u64> {
        let before = self.frame_count().await?;
        self.step(frames).await?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let now = self.frame_count().await?;
            if now >= before + frames as u64 {
                return Ok(now);
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "step({}) never advanced frame_count ({} -> {})",
                    frames,
                    before,
                    now
                ));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn frame_count(&mut self) -> Result<u64> {
        let t = self.get_time().await?;
        t["frame_count"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing frame_count in getTime response: {}", t))
    }

    pub async fn simulate_key_tap(&mut self, key: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "keyboard", "action": "tap", "key": key }),
        )
        .await
    }

    pub async fn simulate_key_press(&mut self, key: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "keyboard", "action": "press", "key": key }),
        )
        .await
    }

    pub async fn simulate_key_release(&mut self, key: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "keyboard", "action": "release", "key": key }),
        )
        .await
    }

    pub async fn simulate_mouse_move(&mut self, x: f32, y: f32) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "mouse", "action": "move", "x": x, "y": y }),
        )
        .await
    }

    pub async fn simulate_mouse_click(&mut self, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "mouse", "action": "click", "button": button }),
        )
        .await
    }

    // ── UI methods ──────────────────────────────────────────────────

    /// Returns the raw `widgets` array from `ui.listWidgets`.
    pub async fn list_widgets(&mut self) -> Result<Vec<Value>> {
        let v = self.call_ok("ui.listWidgets", json!({})).await?;
        v["widgets"]
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("ui.listWidgets returned non-array: {}", v))
    }

    /// Find a widget by its id, returning the snapshot if present.
    pub async fn find_widget(&mut self, id: &str) -> Result<Option<Value>> {
        let widgets = self.list_widgets().await?;
        Ok(widgets
            .into_iter()
            .find(|w| w.get("id").and_then(|v| v.as_str()) == Some(id)))
    }

    pub async fn ui_click(&mut self, id: &str) -> Result<Value> {
        self.call_ok("ui.click", json!({ "id": id })).await
    }

    pub async fn ui_set_text(&mut self, id: &str, text: &str) -> Result<Value> {
        self.call_ok("ui.setText", json!({ "id": id, "text": text }))
            .await
    }

    pub async fn ui_submit(&mut self, id: &str) -> Result<Value> {
        self.call_ok("ui.submit", json!({ "id": id })).await
    }

    pub async fn ui_set_focus(&mut self, id: &str) -> Result<Value> {
        self.call_ok("ui.setFocus", json!({ "id": id })).await
    }

    pub async fn ui_clear_focus(&mut self) -> Result<Value> {
        self.call_ok("ui.clearFocus", json!({})).await
    }

    pub async fn ui_scroll(&mut self, id: &str, offset: f32) -> Result<Value> {
        self.call_ok("ui.scroll", json!({ "id": id, "offset": offset }))
            .await
    }

    pub async fn ui_scroll_to_bottom(&mut self, id: &str) -> Result<Value> {
        self.call_ok("ui.scrollToBottom", json!({ "id": id })).await
    }

    // ── Game-level passthrough ──────────────────────────────────────

    pub async fn inspect(&mut self) -> Result<Value> {
        self.call_ok("game.inspect", json!({})).await
    }

    pub async fn game_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.call_ok(method, params).await
    }
}
