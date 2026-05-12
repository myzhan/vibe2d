//! Web VDP client: connects to vdp-relay via WebSocket as the game endpoint.
//!
//! On wasm32, VDP works through a relay server:
//! - Browser game connects to `ws://<relay>/game` and receives JSON-RPC requests
//! - Game processes requests and sends responses back through the same WebSocket
//!
//! This module provides [`connect_to_relay`] which establishes the WebSocket
//! connection and returns a [`VdpChannel`](vibe_debug::VdpChannel) compatible
//! with the desktop VDP channel interface.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// Connect to a VDP relay server.
///
/// Returns a `vibe_debug::VdpChannel` that the game loop can use identically
/// to the desktop VDP channel (via `try_recv` for requests, `send` for responses).
///
/// The `relay_url` should be the full WebSocket URL including the `/game` path,
/// e.g. `ws://127.0.0.1:9229/game`.
pub fn connect_to_relay(relay_url: &str) -> Option<vibe_debug::VdpChannel> {
    let ws = match web_sys::WebSocket::new(relay_url) {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!("Failed to connect to VDP relay: {:?}", e);
            return None;
        }
    };
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    let (req_tx, req_rx) = mpsc::channel::<vibe_debug::VdpRequest>();
    let (resp_tx, resp_rx) = mpsc::channel::<vibe_debug::VdpResponse>();
    let client_connected = Arc::new(AtomicBool::new(false));
    let connected_flag = Arc::clone(&client_connected);

    // ── onopen: mark as connected ──
    let connected_open = Arc::clone(&client_connected);
    let onopen = Closure::wrap(Box::new(move |_: web_sys::js_sys::Object| {
        connected_open.store(true, Ordering::Relaxed);
        tracing::info!("VDP relay WebSocket connected");
    }) as Box<dyn FnMut(web_sys::js_sys::Object)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // ── onclose: mark as disconnected ──
    let connected_close = Arc::clone(&client_connected);
    let onclose = Closure::wrap(Box::new(move |_: web_sys::js_sys::Object| {
        connected_close.store(false, Ordering::Relaxed);
        tracing::info!("VDP relay WebSocket disconnected");
    }) as Box<dyn FnMut(web_sys::js_sys::Object)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    // ── onerror ──
    let onerror = Closure::wrap(Box::new(move |e: web_sys::ErrorEvent| {
        tracing::warn!("VDP relay WebSocket error: {:?}", e.message());
    }) as Box<dyn FnMut(web_sys::ErrorEvent)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // ── onmessage: parse JSON-RPC request, push to channel ──
    let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        if let Ok(text) = e.data().dyn_into::<js_sys::JsString>() {
            let text: String = text.into();
            match serde_json::from_str::<vibe_debug::JsonRpcRequest>(&text) {
                Ok(rpc_req) => {
                    let vdp_req = vibe_debug::VdpRequest {
                        id: rpc_req.id,
                        method: rpc_req.method,
                        params: rpc_req.params,
                    };
                    let _ = req_tx.send(vdp_req);
                }
                Err(err) => {
                    tracing::warn!("VDP relay: invalid JSON-RPC request: {}", err);
                }
            }
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // ── Spawn a "task" to drain responses and send them back via WebSocket ──
    // Since wasm is single-threaded, we use a polling approach:
    // The game loop will call `drain_responses()` each frame.
    // We store the WebSocket and resp_rx in a shared structure.
    let ws_clone = ws.clone();
    RESPONSE_DRAIN.with(|cell| {
        *cell.borrow_mut() = Some(ResponseDrain {
            ws: ws_clone,
            resp_rx,
        });
    });

    Some(vibe_debug::VdpChannel {
        receiver: req_rx,
        sender: resp_tx,
        client_connected: connected_flag,
    })
}

/// Must be called each frame to flush VDP responses back to the relay WebSocket.
pub fn drain_responses() {
    RESPONSE_DRAIN.with(|cell| {
        let borrow = cell.borrow();
        if let Some(drain) = borrow.as_ref() {
            while let Ok(response) = drain.resp_rx.try_recv() {
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = drain.ws.send_with_str(&json);
                }
            }
        }
    });
}

struct ResponseDrain {
    ws: web_sys::WebSocket,
    resp_rx: mpsc::Receiver<vibe_debug::VdpResponse>,
}

thread_local! {
    static RESPONSE_DRAIN: std::cell::RefCell<Option<ResponseDrain>> = std::cell::RefCell::new(None);
}

/// Determine the relay WebSocket URL from the current page location.
/// Convention: relay runs on the same host, port 9229, path `/game`.
/// If the page URL has a `vdp_relay` query parameter, use that instead.
pub fn default_relay_url() -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();

    // Check for ?vdp_relay=ws://host:port query param
    if let Ok(search) = location.search() {
        for param in search.trim_start_matches('?').split('&') {
            if let Some(value) = param.strip_prefix("vdp_relay=") {
                let url = js_sys::decode_uri_component(value)
                    .ok()
                    .map(|s| String::from(s))?;
                // Ensure it ends with /game
                if url.ends_with("/game") {
                    return Some(url);
                } else {
                    return Some(format!("{}/game", url.trim_end_matches('/')));
                }
            }
        }
    }

    // Default: same hostname, port 9229
    let hostname = location.hostname().ok()?;
    Some(format!("ws://{}:9229/game", hostname))
}
