//! VDP end-to-end integration tests for the shader demo.
//!
//! These spawn a real `shader-demo` window and prove the custom fragment-shader
//! pipeline actually renders — not just that it compiles. The signal we assert
//! on is the *plasma* background: it is a full-screen field of saturated,
//! time-varying colour. The built-in pipeline (textured sprite * vertex colour)
//! could never produce that from this demo's near-uniform textures, so a high
//! chroma ratio across the frame can only come from the custom pipeline running
//! end to end (create shader → build pipeline → bind group → batch break →
//! execute).
//!
//! Heavy tests are marked `#[ignore]` so plain `cargo test` stays fast. Run:
//!
//!     cargo test -p shader-demo -- --ignored --test-threads=1

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use vibe_test::GameHarness;

const GAME_PACKAGE: &str = "shader-demo";
// Matches `examples/shader-demo/game.yaml` -> debug.vdp.port.
const VDP_PORT: u16 = 9234;

/// Ask the running engine to capture a screenshot to a unique temp path and
/// block until the file actually exists (the capture is scheduled onto the next
/// render frame, so it is not synchronous with the RPC response).
async fn capture_screenshot(h: &mut GameHarness) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("vibe_shader_test_{VDP_PORT}.png"));
    // Drop any stale artifact from a previous run before requesting a new one.
    let _ = std::fs::remove_file(&path);

    h.client
        .call_ok("game.screenshot", json!({ "path": path.to_string_lossy() }))
        .await
        .expect("game.screenshot RPC");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::fs::metadata(&path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            return path;
        }
        assert!(
            Instant::now() < deadline,
            "screenshot never appeared at {path:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Return `(non_black_ratio, chromatic_ratio)` for a PNG.
///
/// `non_black` counts pixels whose channels sum above a low threshold (so the
/// dark clear colour / unlit regions are excluded). `chromatic` counts pixels
/// whose max-min channel spread exceeds 40 — i.e. genuinely colourful pixels.
fn color_stats(path: &Path) -> (f32, f32) {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("failed to decode screenshot {path:?}: {e}"))
        .to_rgb8();

    let total = (img.width() as usize) * (img.height() as usize);
    assert!(total > 0, "screenshot had zero dimensions");

    let mut non_black = 0usize;
    let mut chromatic = 0usize;
    for px in img.pixels() {
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        if r + g + b > 30 {
            non_black += 1;
        }
        let spread = r.max(g).max(b) - r.min(g).min(b);
        if spread > 40 {
            chromatic += 1;
        }
    }
    (
        non_black as f32 / total as f32,
        chromatic as f32 / total as f32,
    )
}

/// The plasma custom shader must fill the frame with colour. This is the core
/// proof that a `create_fragment_shader` pipeline is selected and executed by
/// the real renderer, not merely compiled.
///
/// We let the engine run normally (no pause/step needed): the colour-ratio
/// assertions are timing-agnostic, and `capture_screenshot` blocks until a real
/// render frame has actually written the buffer — so a custom-shader frame is
/// guaranteed to have been submitted by the time we decode the PNG.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real shader-demo window; run with --ignored"]
async fn shader_demo_renders_custom_plasma_shader() {
    let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT)
        .await
        .expect("launch shader-demo");

    let path = capture_screenshot(&mut h).await;
    let (non_black, chromatic) = color_stats(&path);

    // Full-screen plasma ⇒ almost every pixel is lit and colourful. A default
    // pipeline rendering this demo's near-uniform textures would score ~0 here.
    assert!(
        non_black > 0.85,
        "expected a mostly-lit frame from the plasma shader, non_black={non_black:.3}"
    );
    assert!(
        chromatic > 0.40,
        "expected a colourful frame (custom shader ran), chromatic={chromatic:.3}"
    );

    let _ = std::fs::remove_file(&path);
}

/// Switching pipelines every frame (plasma → ripple → default HUD) exercises the
/// batch-break / set-bind-group path repeatedly. Let the engine run long enough
/// to shake that path and confirm it neither stalls nor degrades to a colourless
/// frame.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real shader-demo window; run with --ignored"]
async fn shader_demo_pipeline_switching_is_stable() {
    let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT)
        .await
        .expect("launch shader-demo");

    let before = h.frame_count().await.unwrap();
    // ~2.2s at 60Hz ≈ 130 frames; assert a conservative floor so a slow/locked
    // CI frame rate can't spuriously fail the progress check.
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let after = h.frame_count().await.unwrap();
    assert!(
        after >= before + 100,
        "engine stalled during pipeline switching: {before} -> {after}"
    );

    let path = capture_screenshot(&mut h).await;
    let (_, chromatic) = color_stats(&path);
    assert!(
        chromatic > 0.40,
        "frame lost its custom-shader colour after many steps: chromatic={chromatic:.3}"
    );

    let _ = std::fs::remove_file(&path);
}
