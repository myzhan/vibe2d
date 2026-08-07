//! [`ScreenshotPacer`] — paces `tokio::time::sleep` with periodic
//! `game.screenshot` calls so a playthrough scenario produces a usable
//! image sequence for video assembly.
//!
//! Why not a background task? Vibe2D's VDP server (see
//! `crates/vibe_debug/src/server.rs:42`) accepts one WS client at a
//! time, so a recorder that opens its own VdpClient connection
//! deadlocks waiting for accept. Instead the pacer runs synchronously
//! on the test's existing harness connection, interleaving screenshots
//! with the scenario's own VDP calls.
//!
//! No-ops when `VIBE_TEST_RECORDING_DIR` is unset — local runs sleep
//! normally and don't litter `/tmp` with PNGs.
//!
//! Used by playthrough tests via:
//!
//! ```ignore
//! let mut pacer = ScreenshotPacer::new(GAME_PACKAGE, 15);
//! pacer.sleep(&mut h, Duration::from_millis(700)).await;  // ~10 frames
//! pacer.snap(&mut h).await;                               // one explicit shot
//! ```

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use crate::harness::GameHarness;

pub struct ScreenshotPacer {
    /// Per-frame interval (1000ms / fps). 0 when recording is disabled.
    interval: Duration,
    /// Output dir for this label, or `None` when recording is disabled.
    dir: Option<PathBuf>,
    /// Next frame index; `dir.join(format!("{:04}.png", frame))`.
    frame: usize,
}

impl ScreenshotPacer {
    /// New pacer for `label`, recording at `fps` when CI sets
    /// `VIBE_TEST_RECORDING_DIR`. Creates the per-label output dir
    /// eagerly so callers don't have to.
    pub fn new(label: &str, fps: u32) -> Self {
        let interval = Duration::from_millis((1000 / fps.max(1)) as u64);
        let dir = std::env::var("VIBE_TEST_RECORDING_DIR")
            .ok()
            .map(|root| PathBuf::from(root).join(label));
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        Self {
            interval,
            dir,
            frame: 0,
        }
    }

    /// `true` when CI has wired up `VIBE_TEST_RECORDING_DIR`.
    pub fn recording(&self) -> bool {
        self.dir.is_some()
    }

    /// Sleep for `dur`, taking one screenshot per `self.interval`.
    /// When recording is disabled this is just `tokio::time::sleep`.
    pub async fn sleep(&mut self, h: &mut GameHarness, dur: Duration) {
        if !self.recording() {
            tokio::time::sleep(dur).await;
            return;
        }
        let ticks = (dur.as_millis() / self.interval.as_millis().max(1)).max(1) as u32;
        for _ in 0..ticks {
            self.snap(h).await;
            tokio::time::sleep(self.interval).await;
        }
    }

    /// Take a single screenshot now if recording is enabled, otherwise no-op.
    /// Errors are swallowed — the artifact has whatever frames landed.
    pub async fn snap(&mut self, h: &mut GameHarness) {
        let Some(dir) = &self.dir else {
            return;
        };
        let path = dir.join(format!("{:04}.png", self.frame));
        let _ = h
            .call_ok("game.screenshot", json!({ "path": path.to_string_lossy() }))
            .await;
        self.frame += 1;
    }

    /// How many frames have been requested so far.
    pub fn frame_count(&self) -> usize {
        self.frame
    }
}
