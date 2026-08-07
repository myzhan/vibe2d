//! Playthrough scenario for flappy-bird — tap Space at a roughly human
//! rhythm so the GIF shows the bird actually flapping (and probably
//! eventually dying, which is itself a fine demo).
//!
//! Used by `.github/workflows/playthrough.yml`. Does NOT pause the
//! engine — recording wants live frames.
//!
//! Run locally with:
//!
//!     cargo test -p flappy-bird --test playthrough -- --ignored --nocapture

use std::time::Duration;

use vibe_test::{GameHarness, ScreenshotPacer};

const GAME_PACKAGE: &str = "flappy-bird";
// Matches `examples/flappy-bird/game.yaml` -> debug.vdp.port.
const VDP_PORT: u16 = 9229;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "demo recording — used by .github/workflows/playthrough.yml"]
async fn flappy_bird_playthrough() {
    let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT)
        .await
        .expect("launch flappy-bird");

    // See ui-demo's playthrough.rs for why this is synchronous.
    let mut pacer = ScreenshotPacer::new(GAME_PACKAGE, 15);

    // Initial pause so the title / first frame is visible in the GIF.
    pacer.sleep(&mut h, Duration::from_millis(800)).await;

    // Tap to start (Space is also the "flap" key after game start).
    // 12 flaps over ~5 s — roughly the rhythm a human plays at.
    for _ in 0..12 {
        h.simulate_key_tap("Space").await.unwrap();
        pacer.sleep(&mut h, Duration::from_millis(420)).await;
    }

    // Hold the last frame a beat so a viewer registers the score / fail.
    pacer.sleep(&mut h, Duration::from_millis(1200)).await;
}
