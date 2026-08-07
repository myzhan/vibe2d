//! Playthrough scenario for the UI demo — drives the game through a short
//! human-paced sequence so a video recorder (Xvfb + ffmpeg, see
//! `.github/workflows/playthrough-record.yml`) can capture a representative
//! "what does this PR actually look like" GIF.
//!
//! Unlike [`vdp_ui.rs`], this scenario deliberately does NOT pause the
//! engine — the recorder needs live frames. Each VDP action is followed by a
//! short sleep so the resulting GIF reads at human speed instead of looking
//! like a sped-up demo reel.
//!
//! Run locally with:
//!
//!     cargo test -p ui-demo --test playthrough -- --ignored --nocapture

use std::time::Duration;

use vibe_test::{GameHarness, ScreenshotPacer};

const GAME_PACKAGE: &str = "ui-demo";
// Matches `examples/ui/game.yaml` -> debug.vdp.port.
const VDP_PORT: u16 = 9230;
// A human "beat" — long enough for a viewer's eye to track what just
// happened. 700ms keeps the full scenario under 15s.
const BEAT: Duration = Duration::from_millis(700);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "demo recording — used by .github/workflows/playthrough.yml"]
async fn ui_demo_full_playthrough() {
    let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT)
        .await
        .expect("launch ui-demo");

    // No-op locally; in CI (VIBE_TEST_RECORDING_DIR set) every
    // `pacer.sleep(...)` below interleaves screenshots at 15fps onto
    // the harness's existing VDP connection. VDP server only accepts
    // one client at a time so we cannot use a background task.
    let mut pacer = ScreenshotPacer::new(GAME_PACKAGE, 15);

    // Let the title / initial layout settle on screen.
    pacer.sleep(&mut h, BEAT).await;
    pacer.sleep(&mut h, BEAT).await;

    // Click the counter button a few times — viewer sees the "Clicks: N"
    // label tick up.
    for _ in 0..3 {
        h.ui_click("btn_click").await.unwrap();
        pacer.sleep(&mut h, BEAT).await;
    }

    // Type and submit a CJK message — exercises both the text input widget
    // and the IME font path, which is one of the demo's main reasons to exist.
    h.ui_set_text("chat_input", "你好 vibe2d").await.unwrap();
    pacer.sleep(&mut h, BEAT).await;
    h.ui_submit("chat_input").await.unwrap();
    pacer.sleep(&mut h, BEAT).await;

    // Follow up with an English message so the scroll list visibly grows.
    h.ui_set_text("chat_input", "hello world").await.unwrap();
    pacer.sleep(&mut h, BEAT).await;
    h.ui_submit("chat_input").await.unwrap();
    pacer.sleep(&mut h, BEAT).await;

    // Fill enough lines to overflow the visible area, then scroll-to-bottom
    // so the GIF ends on the scroll animation.
    for i in 0..6 {
        h.ui_set_text("chat_input", &format!("line {}", i))
            .await
            .unwrap();
        h.ui_submit("chat_input").await.unwrap();
        // No pacer here — submit-spam happens fast on purpose so the
        // GIF shows a burst of activity, then we slow back down for
        // the scroll. Skipping screenshots during the burst is fine.
    }
    pacer.sleep(&mut h, BEAT).await;
    h.ui_scroll_to_bottom("msg_list").await.unwrap();
    pacer.sleep(&mut h, BEAT).await;
    pacer.sleep(&mut h, BEAT).await;
}
