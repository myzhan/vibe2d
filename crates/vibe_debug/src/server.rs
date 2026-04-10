use std::net::SocketAddr;
use std::sync::atomic::Ordering;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{JsonRpcRequest, VdpResponse};
use crate::{VdpRequest, VdpServerChannel};

pub struct VdpServer;

impl VdpServer {
    /// Start the VDP WebSocket server on a background thread.
    /// Returns immediately. The server runs until the process exits.
    pub fn start(port: u16, channel: VdpServerChannel) -> Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        tracing::info!("VDP server starting on ws://{}", addr);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create VDP tokio runtime");

            rt.block_on(async move {
                if let Err(e) = run_server(addr, channel).await {
                    tracing::error!("VDP server error: {}", e);
                }
            });
        });

        Ok(())
    }
}

async fn run_server(addr: SocketAddr, channel: VdpServerChannel) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("VDP server listening on ws://{}", addr);

    // Accept one connection at a time (simple model for AI tool)
    loop {
        let (stream, peer) = listener.accept().await?;
        tracing::info!("VDP client connected: {}", peer);
        channel.client_connected.store(true, Ordering::Relaxed);
        if let Err(e) = handle_connection(stream, &channel).await {
            tracing::warn!("VDP connection closed: {}", e);
        }
        channel.client_connected.store(false, Ordering::Relaxed);
        tracing::info!("VDP client disconnected: {}", peer);
    }
}

async fn handle_connection(stream: TcpStream, channel: &VdpServerChannel) -> Result<()> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => {
                // Parse JSON-RPC request
                let rpc_req: JsonRpcRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(e) => {
                        let error = VdpResponse::error(
                            serde_json::Value::Null,
                            -32700,
                            format!("Parse error: {}", e),
                        );
                        let json = serde_json::to_string(&error)?;
                        write.send(Message::Text(json.into())).await?;
                        continue;
                    }
                };

                // Send request to game thread
                let vdp_req = VdpRequest {
                    id: rpc_req.id.clone(),
                    method: rpc_req.method,
                    params: rpc_req.params,
                };

                channel.sender.send(vdp_req)?;

                // Wait for response from game thread (blocking with timeout)
                match channel.receiver.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(response) => {
                        let json = serde_json::to_string(&response)?;
                        write.send(Message::Text(json.into())).await?;
                    }
                    Err(_) => {
                        let error = VdpResponse::error(
                            rpc_req.id,
                            -32000,
                            "Timeout waiting for game response",
                        );
                        let json = serde_json::to_string(&error)?;
                        write.send(Message::Text(json.into())).await?;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    Ok(())
}
