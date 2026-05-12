//! VDP relay server for web games.
//!
//! Architecture:
//! ```text
//! ┌──────────────┐       ┌─────────────────────┐       ┌──────────────────┐
//! │  vibe-cli    │──ws──►│  vdp-relay (:9229)  │◄──ws──│  Browser/WASM    │
//! │  (tool)      │       │  (this server)      │       │  (game)          │
//! └──────────────┘       └─────────────────────┘       └──────────────────┘
//! ```
//!
//! - Game (browser) connects to `ws://<host>:<port>/game`
//! - Tools (vibe-cli rpc/inspect) connect to `ws://<host>:<port>/`
//! - Relay forwards tool requests → game, game responses → tool.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// Shared relay state.
struct RelayState {
    /// Sender to forward requests to the connected game.
    game_tx: Option<mpsc::UnboundedSender<String>>,
    /// Queue of pending tool requests waiting for a response from the game.
    /// Uses FIFO order: game processes requests sequentially.
    pending: Vec<oneshot::Sender<String>>,
}

pub async fn run(port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    println!("VDP relay listening on ws://{}", addr);
    println!("  Game connects to:  ws://{}/game", addr);
    println!("  Tools connect to:  ws://{}/", addr);

    let state = Arc::new(Mutex::new(RelayState {
        game_tx: None,
        pending: Vec::new(),
    }));

    loop {
        let (stream, peer) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer, state).await {
                // Connection errors are normal (client disconnect)
                let _ = e;
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> Result<()> {
    // Use accept_hdr_async to inspect the request URI and determine role.
    let mut is_game = false;

    let ws_stream = tokio_tungstenite::accept_hdr_async(
        stream,
        |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
         resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
            if req.uri().path() == "/game" {
                is_game = true;
            }
            Ok(resp)
        },
    )
    .await?;

    if is_game {
        handle_game_connection(ws_stream, peer, state).await
    } else {
        handle_tool_connection(ws_stream, peer, state).await
    }
}

type WsStream = tokio_tungstenite::WebSocketStream<TcpStream>;

/// Handle a game connection (browser/wasm).
/// The game receives JSON-RPC requests and sends back responses.
async fn handle_game_connection(
    ws_stream: WsStream,
    peer: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> Result<()> {
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this game connection
    {
        let mut s = state.lock().await;
        if s.game_tx.is_some() {
            eprintln!("Game already connected, rejecting {}", peer);
            return Ok(());
        }
        s.game_tx = Some(tx);
    }
    println!("Game connected: {}", peer);

    // Spawn a task to forward requests from relay → game WebSocket
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read responses from game and dispatch to the oldest waiting tool
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let mut s = state.lock().await;
                // FIFO: remove the first pending sender (oldest request)
                if !s.pending.is_empty() {
                    let sender = s.pending.remove(0);
                    let _ = sender.send(text.to_string());
                }
            }
            Ok(Message::Close(_)) => break,
            Err(_) => break,
            _ => {}
        }
    }

    // Cleanup: remove game and reject any pending requests
    {
        let mut s = state.lock().await;
        s.game_tx = None;
        for sender in s.pending.drain(..) {
            let _ = sender.send(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32000, "message": "Game disconnected" }
                })
                .to_string(),
            );
        }
    }
    write_handle.abort();
    println!("Game disconnected: {}", peer);
    Ok(())
}

/// Handle a tool connection (vibe-cli rpc/inspect).
/// The tool sends JSON-RPC requests and waits for responses.
async fn handle_tool_connection(
    ws_stream: WsStream,
    _peer: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> Result<()> {
    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Forward the request to the game and wait for a response
                let (resp_tx, resp_rx) = oneshot::channel();

                let sent = {
                    let mut s = state.lock().await;
                    if let Some(game_tx) = s.game_tx.clone() {
                        s.pending.push(resp_tx);
                        game_tx.send(text.to_string()).is_ok()
                    } else {
                        false
                    }
                };

                if !sent {
                    let error = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32000, "message": "No game connected to relay" }
                    });
                    write
                        .send(Message::Text(serde_json::to_string(&error)?.into()))
                        .await?;
                    continue;
                }

                // Wait for response with timeout
                match tokio::time::timeout(std::time::Duration::from_secs(5), resp_rx).await {
                    Ok(Ok(response)) => {
                        write.send(Message::Text(response.into())).await?;
                    }
                    _ => {
                        let error = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": { "code": -32000, "message": "Timeout waiting for game response" }
                        });
                        write
                            .send(Message::Text(serde_json::to_string(&error)?.into()))
                            .await?;
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(())
}
