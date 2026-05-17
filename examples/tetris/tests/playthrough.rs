//! Playthrough scenario for tetris — let pieces drop naturally while
//! sprinkling a few moves + rotations so the GIF shows actual play.
//!
//! Used by `.github/workflows/playthrough.yml`. Does NOT pause the
//! engine — the recorder needs live frames.
//!
//! Run locally with:
//!
//!     cargo test -p tetris --test playthrough -- --ignored --nocapture

use std::time::Duration;

use vibe_test::{GameHarness, ScreenshotPacer};

const GAME_PACKAGE: &str = "tetris";
// Matches `examples/tetris/game.yaml` -> debug.vdp.port.
const VDP_PORT: u16 = 9229;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "demo recording — used by .github/workflows/playthrough.yml"]
async fn tetris_playthrough() {
    let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT)
        .await
        .expect("launch tetris");

    // See ui-demo's playthrough.rs for why this is synchronous.
    let mut pacer = ScreenshotPacer::new(GAME_PACKAGE, 15);

    pacer.sleep(&mut h, Duration::from_millis(800)).await;

    // Drift the first piece left a few cells.
    for _ in 0..3 {
        h.simulate_key_tap("Left").await.unwrap();
        pacer.sleep(&mut h, Duration::from_millis(180)).await;
    }
    // Rotate once (Up = rotate CW per game.yaml).
    h.simulate_key_tap("Up").await.unwrap();
    pacer.sleep(&mut h, Duration::from_millis(400)).await;

    // Soft drop until it locks-ish.
    h.simulate_key_press("Down").await.unwrap();
    pacer.sleep(&mut h, Duration::from_millis(1500)).await;
    h.simulate_key_release("Down").await.unwrap();
    pacer.sleep(&mut h, Duration::from_millis(400)).await;

    // Second piece: drift right + rotate twice.
    for _ in 0..3 {
        h.simulate_key_tap("Right").await.unwrap();
        pacer.sleep(&mut h, Duration::from_millis(180)).await;
    }
    h.simulate_key_tap("Up").await.unwrap();
    pacer.sleep(&mut h, Duration::from_millis(220)).await;
    h.simulate_key_tap("Up").await.unwrap();
    pacer.sleep(&mut h, Duration::from_millis(400)).await;

    // Let the rest of the GIF play out naturally as pieces drop.
    pacer.sleep(&mut h, Duration::from_millis(2500)).await;
}
