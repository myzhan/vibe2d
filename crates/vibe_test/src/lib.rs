//! Test helpers for writing VDP-driven integration tests against Vibe2D games.
//!
//! Vibe2D's engine does not ship game-specific integration tests — games are
//! expected to own their own tests. This crate provides the shared primitives
//! every game-level test needs:
//!
//! * [`GameHarness`] — spawns a game binary as a child process, waits for its
//!   VDP port to become reachable, and kills it on drop.
//! * [`VdpClient`] — a minimal JSON-RPC 2.0 client over a WebSocket with
//!   semantic helpers for the engine's built-in VDP methods
//!   (`engine.*`, `ui.*`, `game.*`).
//!
//! Typical usage (in `examples/<game>/tests/integration.rs`):
//!
//! ```ignore
//! use vibe_test::GameHarness;
//!
//! #[tokio::test(flavor = "multi_thread")]
//! #[ignore = "spawns a real game window"]
//! async fn mytest() {
//!     let mut h = GameHarness::launch("my-game", 9229).await.unwrap();
//!     h.pause().await.unwrap();
//!     h.step(10).await.unwrap();
//!     let widgets = h.list_widgets().await.unwrap();
//!     assert!(!widgets.is_empty());
//! }
//! ```
//!
//! Run with: `cargo test -p <game> -- --ignored --test-threads=1`.
//!
//! The entire crate is gated behind the `vdp` feature (default-on). This
//! matches the engine-wide `vdp` feature convention: a game that strips
//! VDP for release has no reason to pull in VDP test helpers either.

#![cfg(feature = "vdp")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

    // ── Gamepad simulation ──────────────────────────────────────────
    //
    // The engine defaults `pad` to 0, so the un-suffixed helpers drive the
    // virtual pad 0; the `_on` variants take an explicit index for
    // local-multiplayer tests.
    //
    // Note these work regardless of whether the engine was built with the
    // `gamepad` feature: simulated input writes straight into `InputState` and
    // never touches gilrs.

    pub async fn simulate_gamepad_connect(&mut self, pad: u32, name: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "connect", "pad": pad, "name": name }),
        )
        .await
    }

    pub async fn simulate_gamepad_disconnect(&mut self, pad: u32) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "disconnect", "pad": pad }),
        )
        .await
    }

    pub async fn simulate_gamepad_press(&mut self, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "press", "button": button }),
        )
        .await
    }

    pub async fn simulate_gamepad_release(&mut self, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "release", "button": button }),
        )
        .await
    }

    /// Press now, auto-release next frame (the gamepad analogue of
    /// `simulate_key_tap`).
    pub async fn simulate_gamepad_tap(&mut self, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "tap", "button": button }),
        )
        .await
    }

    /// Set a button's analog value (0.0..=1.0) — meaningful for the triggers.
    pub async fn simulate_gamepad_button_value(
        &mut self,
        button: &str,
        value: f32,
    ) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "value", "button": button, "value": value }),
        )
        .await
    }

    /// Set a stick axis (-1.0..=1.0). Remember Y is up-positive.
    pub async fn simulate_gamepad_axis(&mut self, axis: &str, value: f32) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "axis", "axis": axis, "value": value }),
        )
        .await
    }

    pub async fn simulate_gamepad_press_on(&mut self, pad: u32, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "press", "pad": pad, "button": button }),
        )
        .await
    }

    pub async fn simulate_gamepad_release_on(&mut self, pad: u32, button: &str) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "release", "pad": pad, "button": button }),
        )
        .await
    }

    pub async fn simulate_gamepad_axis_on(
        &mut self,
        pad: u32,
        axis: &str,
        value: f32,
    ) -> Result<Value> {
        self.call_ok(
            "engine.simulateInput",
            json!({
                "device": "gamepad", "action": "axis",
                "pad": pad, "axis": axis, "value": value,
            }),
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

/// Owns a spawned game child process plus a connected [`VdpClient`].
///
/// The child process is killed and reaped when the harness drops, so each
/// test gets a clean slate — just make sure tests don't race on the same
/// VDP port (use `--test-threads=1`, or bind distinct ports in `game.yaml`).
pub struct GameHarness {
    child: Option<Child>,
    pub client: VdpClient,
    pub port: u16,
}

/// Options for [`GameHarness::launch_with`].
pub struct LaunchOptions<'a> {
    /// Workspace package name, e.g. `"ui-demo"` or `"flappy-bird"`.
    pub package: &'a str,
    /// VDP port the game is expected to listen on (must match `game.yaml`).
    pub port: u16,
    /// How long to wait for the game to become VDP-ready. Cold `cargo run`
    /// builds can take a while.
    pub ready_timeout: Duration,
    /// If `Some`, sets `CARGO_TARGET_DIR` for the child — useful in CI to
    /// reuse a shared build cache.
    pub target_dir: Option<PathBuf>,
    /// Pass `--no-default-features` to the child `cargo run`.
    pub no_default_features: bool,
    /// Features to enable explicitly (`--features a,b`).
    pub features: &'a [&'a str],
}

impl<'a> LaunchOptions<'a> {
    pub fn new(package: &'a str, port: u16) -> Self {
        Self {
            package,
            port,
            ready_timeout: Duration::from_secs(180),
            target_dir: None,
            no_default_features: false,
            features: &[],
        }
    }

    /// Build the game under test with `--no-default-features`.
    ///
    /// Useful for pinning a deterministic feature set: e.g. a test that asserts
    /// on gamepad state wants `gamepad` OFF so a controller physically attached
    /// to the developer's machine can't be enumerated into the assertions.
    pub fn without_default_features(mut self) -> Self {
        self.no_default_features = true;
        self
    }

    /// Enable specific features (combine with [`Self::without_default_features`]
    /// to get an exact set).
    pub fn with_features(mut self, features: &'a [&'a str]) -> Self {
        self.features = features;
        self
    }
}

impl GameHarness {
    /// Launch a workspace package with default options and connect to its
    /// VDP port. The game is invoked via `cargo run -p <package>` so its
    /// compiled artifacts are reused from the workspace target cache.
    pub async fn launch(package: &str, port: u16) -> Result<Self> {
        Self::launch_with(LaunchOptions::new(package, port)).await
    }

    pub async fn launch_with(opts: LaunchOptions<'_>) -> Result<Self> {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["run", "--quiet", "-p", opts.package])
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if opts.no_default_features {
            cmd.arg("--no-default-features");
        }
        if !opts.features.is_empty() {
            cmd.args(["--features", &opts.features.join(",")]);
        }
        if let Some(dir) = &opts.target_dir {
            cmd.env("CARGO_TARGET_DIR", dir);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", opts.package))?;

        let addr: SocketAddr = ([127, 0, 0, 1], opts.port).into();
        let client = wait_for_vdp(addr, opts.ready_timeout)
            .await
            .with_context(|| {
                format!(
                    "`{}` did not become VDP-ready on port {} within {:?}",
                    opts.package, opts.port, opts.ready_timeout
                )
            })?;

        Ok(Self {
            child: Some(child),
            client,
            port: opts.port,
        })
    }
}

impl std::ops::Deref for GameHarness {
    type Target = VdpClient;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl std::ops::DerefMut for GameHarness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Drop for GameHarness {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Poll until a WebSocket handshake succeeds and the VDP server answers
/// `engine.info` with a vibe2d identity — then return the open client.
async fn wait_for_vdp(addr: SocketAddr, timeout: Duration) -> Result<VdpClient> {
    let deadline = Instant::now() + timeout;
    loop {
        // Flatten the probe via guard clauses + `?` on a helper; the nested
        // `if let` chain it replaces would otherwise trip `clippy::collapsible_if`.
        if let Some(client) = try_handshake(addr).await {
            return Ok(client);
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("timed out waiting for VDP at {}", addr));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// One probe attempt: TCP reachable → WS upgrade → `engine.info` identity check.
/// Returns `Some(client)` only when all three steps succeed and the identity
/// matches vibe2d. Any failure is swallowed so the caller can retry.
async fn try_handshake(addr: SocketAddr) -> Option<VdpClient> {
    TcpStream::connect(addr).await.ok()?;
    let mut client = VdpClient::connect(addr).await.ok()?;
    let info = client.engine_info().await.ok()?;
    let engine = info.get("engine").and_then(|v| v.as_str())?;
    (engine == "vibe2d").then_some(client)
}
