mod protocol;
#[cfg(not(target_arch = "wasm32"))]
mod server;

pub use protocol::{JsonRpcRequest, VdpRequest, VdpResponse};
#[cfg(not(target_arch = "wasm32"))]
pub use server::VdpServer;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

/// Channel pair for communication between VDP server and game loop.
pub struct VdpChannel {
    pub receiver: mpsc::Receiver<VdpRequest>,
    pub sender: mpsc::Sender<VdpResponse>,
    /// Shared flag: `true` when a VDP client is connected.
    pub client_connected: Arc<AtomicBool>,
}

impl VdpChannel {
    /// Returns `true` if a VDP client is currently connected.
    pub fn is_client_connected(&self) -> bool {
        self.client_connected.load(Ordering::Relaxed)
    }
}

/// Create a VDP channel pair. Returns (game_side, server_side).
pub fn create_channel() -> (VdpChannel, VdpServerChannel) {
    let (req_tx, req_rx) = mpsc::channel();
    let (resp_tx, resp_rx) = mpsc::channel();
    let client_connected = Arc::new(AtomicBool::new(false));
    (
        VdpChannel {
            receiver: req_rx,
            sender: resp_tx,
            client_connected: Arc::clone(&client_connected),
        },
        VdpServerChannel {
            sender: req_tx,
            receiver: resp_rx,
            client_connected,
        },
    )
}

/// Server-side channel endpoints.
pub struct VdpServerChannel {
    pub sender: mpsc::Sender<VdpRequest>,
    pub receiver: mpsc::Receiver<VdpResponse>,
    /// Shared flag: set to `true` when a client connects, `false` on disconnect.
    pub client_connected: Arc<AtomicBool>,
}

// ─────────────────────────────────────────────────────────────────────
// Unit tests — protocol serialization & in-process channel routing
// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vdp_response_success_serializes_with_jsonrpc_2_0() {
        let resp = VdpResponse::success(json!(1), json!({"ok": true}));
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 1);
        assert_eq!(value["result"]["ok"], true);
        // success response must NOT include an error field
        assert!(value.get("error").is_none());
    }

    #[test]
    fn vdp_response_error_serializes_with_code_and_message() {
        let resp = VdpResponse::error(json!("req-7"), -32000, "boom");
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], "req-7");
        assert_eq!(value["error"]["code"], -32000);
        assert_eq!(value["error"]["message"], "boom");
        // error response must NOT include a result field
        assert!(value.get("result").is_none());
    }

    #[test]
    fn vdp_response_method_not_found_uses_code_minus_32601() {
        let resp = VdpResponse::method_not_found(json!(42), "does.not.exist");
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["error"]["code"], -32601);
        assert!(
            value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("does.not.exist")
        );
    }

    #[test]
    fn channel_round_trip_request_then_response() {
        let (game_side, server_side) = create_channel();

        // Server pushes a request, game side receives it
        let req = VdpRequest {
            id: json!(1),
            method: "engine.info".to_string(),
            params: json!({}),
        };
        server_side.sender.send(req.clone()).unwrap();
        let received = game_side.receiver.recv().unwrap();
        assert_eq!(received.id, json!(1));
        assert_eq!(received.method, "engine.info");

        // Game side replies, server receives the response
        game_side
            .sender
            .send(VdpResponse::success(json!(1), json!({"ok": true})))
            .unwrap();
        let resp = server_side.receiver.recv().unwrap();
        assert_eq!(resp.result.unwrap()["ok"], true);
    }

    #[test]
    fn client_connected_flag_is_shared() {
        let (game_side, server_side) = create_channel();
        assert!(!game_side.is_client_connected());

        server_side.client_connected.store(true, Ordering::Relaxed);
        assert!(game_side.is_client_connected());

        server_side.client_connected.store(false, Ordering::Relaxed);
        assert!(!game_side.is_client_connected());
    }

    #[test]
    fn json_rpc_request_can_be_parsed() {
        // Validates the wire format the WS server consumes
        let raw = r#"{"jsonrpc":"2.0","id":7,"method":"engine.pause","params":{}}"#;
        let parsed: protocol::JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.id, json!(7));
        assert_eq!(parsed.method, "engine.pause");
    }

    #[test]
    fn json_rpc_request_default_params_when_omitted() {
        let raw = r#"{"id":"abc","method":"engine.info"}"#;
        let parsed: protocol::JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.id, json!("abc"));
        assert!(parsed.params.is_null());
    }
}
