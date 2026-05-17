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

mod client;
mod harness;
mod recorder;

pub use client::VdpClient;
pub use harness::{GameHarness, LaunchOptions};
pub use recorder::ScreenshotPacer;
