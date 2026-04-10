mod server;
mod protocol;

pub use protocol::{VdpRequest, VdpResponse};
pub use server::VdpServer;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

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
