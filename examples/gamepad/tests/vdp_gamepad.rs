//! VDP integration tests for the gamepad tester.
//!
//! These cold-start `gamepad-tester`, drive simulated gamepad input over VDP,
//! and assert on the tester's `inspect()` snapshot — which is the very same
//! snapshot the on-screen readout renders from, so a green test means the
//! screen is telling the truth too. Run with:
//!
//!     cargo test -p gamepad-tester -- --ignored --test-threads=1
//!
//! `--test-threads=1` is mandatory because every `GameHarness` grabs the same
//! VDP port (9233 here).
//!
//! Note none of this needs the `gamepad` feature or libudev: simulated input is
//! injected straight into `InputState` and never touches gilrs. That's
//! deliberate — it means CI validates the whole gamepad stack on a machine with
//! no controller attached.

use serde_json::json;
use vibe_test::{GameHarness, LaunchOptions};

const GAME_PACKAGE: &str = "gamepad-tester";
// Matches `examples/gamepad/game.yaml` -> debug.vdp.port.
const VDP_PORT: u16 = 9233;

/// Launch, pause, and settle one frame so state is stable across RPCs.
///
/// The game is deliberately built with **`gamepad` OFF** (`vdp` only). Every
/// assertion below is about pad *counts* and *ids*, so a controller physically
/// plugged into the developer's machine would otherwise be enumerated at startup
/// and shift them — `pad_count == 1` becomes 2, and a real pad occupying id 0
/// silently absorbs the simulated pad 0. Stripping the feature makes these tests
/// depend on nothing but the VDP input we inject.
async fn launch_paused() -> GameHarness {
    let mut h = GameHarness::launch_with(
        LaunchOptions::new(GAME_PACKAGE, VDP_PORT)
            .without_default_features()
            .with_features(&["vdp"]),
    )
    .await
    .expect("launch gamepad-tester");
    h.pause().await.unwrap();
    h.step_and_wait(1).await.unwrap();
    h
}

/// The first connected pad in the inspect snapshot.
fn pad0(snapshot: &serde_json::Value) -> &serde_json::Value {
    snapshot["pads"]
        .as_array()
        .and_then(|pads| pads.first())
        .unwrap_or_else(|| panic!("no pads in snapshot: {snapshot}"))
}

/// Look up one action's row in the snapshot.
fn action<'a>(snapshot: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    snapshot["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or_else(|| panic!("action {name} missing from snapshot: {snapshot}"))
}

/// Does this pad currently hold `button`?
fn holds(pad: &serde_json::Value, button: &str) -> bool {
    pad["pressed"]
        .as_array()
        .expect("pressed array")
        .iter()
        .any(|b| b == button)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn connect_shows_up_in_pad_list() {
    let mut h = launch_paused().await;

    h.simulate_gamepad_connect(0, "Test Pad").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert_eq!(snap["pad_count"].as_u64().unwrap(), 1);
    let pad = pad0(&snap);
    assert_eq!(pad["name"].as_str().unwrap(), "Test Pad");
    assert_eq!(pad["id"].as_u64().unwrap(), 0);
    assert!(pad["connected"].as_bool().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn button_press_and_release_tracked() {
    let mut h = launch_paused().await;

    // No explicit `connect` — pressing a button must auto-vivify the pad, which
    // is what lets simple test scripts skip the connect step entirely.
    h.simulate_gamepad_press("South").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert_eq!(snap["pad_count"].as_u64().unwrap(), 1);
    assert!(holds(pad0(&snap), "South"));
    // "South" is bound to `jump` in game.yaml.
    assert!(action(&snap, "jump")["pressed"].as_bool().unwrap());

    h.simulate_gamepad_release("South").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert!(!holds(pad0(&snap), "South"));
    assert!(!action(&snap, "jump")["pressed"].as_bool().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn tap_auto_releases_next_frame() {
    // Regression test for `pending_gamepad_auto_releases`: without it a tap
    // would stay held forever.
    let mut h = launch_paused().await;

    h.simulate_gamepad_tap("South").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert!(
        action(&snap, "jump")["just_pressed"].as_bool().unwrap(),
        "tap should produce a just_pressed edge: {snap}"
    );

    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    assert!(
        !action(&snap, "jump")["pressed"].as_bool().unwrap(),
        "tap should auto-release on the following frame: {snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn axis_past_threshold_drives_action_with_edges() {
    let mut h = launch_paused().await;

    // `move_left` binds LeftStickX- (as "LeftStickLeft") in game.yaml.
    h.simulate_gamepad_axis("LeftStickX", -1.0).await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    let a = action(&snap, "move_left");
    assert!(a["pressed"].as_bool().unwrap(), "{snap}");
    assert!(a["just_pressed"].as_bool().unwrap(), "{snap}");

    // Hold the same value one more frame. Still pressed, but NOT a fresh edge —
    // this is what proves `prev_axes_raw` is snapshotted in `begin_frame` rather
    // than recomputed from the current value.
    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    let a = action(&snap, "move_left");
    assert!(a["pressed"].as_bool().unwrap(), "{snap}");
    assert!(!a["just_pressed"].as_bool().unwrap(), "{snap}");

    // Return to centre: released, with an edge.
    h.simulate_gamepad_axis("LeftStickX", 0.0).await.unwrap();
    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    let a = action(&snap, "move_left");
    assert!(!a["pressed"].as_bool().unwrap(), "{snap}");
    assert!(a["just_released"].as_bool().unwrap(), "{snap}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn axis_inside_deadzone_is_inert_but_raw_value_survives() {
    let mut h = launch_paused().await;

    // 0.05 is well inside the 0.15 deadzone from game.yaml.
    h.simulate_gamepad_axis("LeftStickX", -0.05).await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert!(!action(&snap, "move_left")["pressed"].as_bool().unwrap());

    let pad = pad0(&snap);
    // Deadzoned reading is exactly zero...
    assert_eq!(pad["axes"]["lx"].as_f64().unwrap(), 0.0, "{snap}");
    // ...while the raw reading is preserved, which is what lets the tester show
    // both and what makes the deadzone runtime-tunable.
    assert!(
        (pad["axes_raw"]["lx"].as_f64().unwrap() + 0.05).abs() < 1e-4,
        "{snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn axis_respects_up_positive_y_convention() {
    let mut h = launch_paused().await;

    // `aim_up` binds RightStickUp == RightStickY+, because Y is up-positive.
    h.simulate_gamepad_axis("RightStickY", 1.0).await.unwrap();
    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    assert!(
        action(&snap, "aim_up")["pressed"].as_bool().unwrap(),
        "{snap}"
    );

    h.simulate_gamepad_axis("RightStickY", -1.0).await.unwrap();
    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    assert!(
        !action(&snap, "aim_up")["pressed"].as_bool().unwrap(),
        "pushing DOWN must not trigger aim_up: {snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn disconnect_clears_held_state() {
    // The "unplugged while holding right, character walks into a wall forever"
    // regression.
    let mut h = launch_paused().await;

    h.simulate_gamepad_connect(0, "Doomed Pad").await.unwrap();
    h.simulate_gamepad_press("DPadRight").await.unwrap();
    h.step_and_wait(1).await.unwrap();
    let snap = h.inspect().await.unwrap();
    assert!(action(&snap, "move_right")["pressed"].as_bool().unwrap());

    h.simulate_gamepad_disconnect(0).await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    // The pad drops out of the connected list entirely...
    assert_eq!(snap["pad_count"].as_u64().unwrap(), 0, "{snap}");
    // ...and the action it was driving is no longer held.
    assert!(
        !action(&snap, "move_right")["pressed"].as_bool().unwrap(),
        "held input must not survive a disconnect: {snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn analog_trigger_value_round_trips() {
    let mut h = launch_paused().await;

    h.simulate_gamepad_button_value("RightTrigger", 0.75)
        .await
        .unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    let rt = pad0(&snap)["rt"].as_f64().unwrap();
    assert!(
        (rt - 0.75).abs() < 1e-4,
        "expected rt≈0.75, got {rt}: {snap}"
    );

    // A value update alone must NOT fake a digital press — gilrs sends
    // ButtonPressed separately, and synthesizing one here would double-fire.
    assert!(
        !action(&snap, "fire")["pressed"].as_bool().unwrap(),
        "an analog value must not imply a digital press: {snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn two_pads_are_independent() {
    let mut h = launch_paused().await;

    h.simulate_gamepad_press_on(0, "South").await.unwrap();
    h.simulate_gamepad_press_on(1, "North").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert_eq!(snap["pad_count"].as_u64().unwrap(), 2, "{snap}");

    let pads = snap["pads"].as_array().unwrap();
    // Ascending id order is the BTreeMap guarantee behind "player 1 = pad 0".
    assert_eq!(pads[0]["id"].as_u64().unwrap(), 0);
    assert_eq!(pads[1]["id"].as_u64().unwrap(), 1);

    assert!(holds(&pads[0], "South"), "{snap}");
    assert!(!holds(&pads[0], "North"), "{snap}");
    assert!(holds(&pads[1], "North"), "{snap}");
    assert!(!holds(&pads[1], "South"), "{snap}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn select_pad_rejects_out_of_range_index() {
    let mut h = launch_paused().await;

    h.simulate_gamepad_connect(0, "Only Pad").await.unwrap();
    h.step_and_wait(1).await.unwrap();

    h.game_call("tester.selectPad", json!({ "index": 0 }))
        .await
        .expect("selecting the only connected pad should succeed");

    let resp = h
        .call("tester.selectPad", json!({ "index": 7 }))
        .await
        .unwrap();
    assert!(
        resp.get("error").is_some(),
        "selecting a nonexistent pad should error: {resp}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn rumble_button_queues_a_request() {
    // A headless run can't observe the gilrs call, but it can prove the
    // Context -> GameBridge -> platform queue is wired end to end.
    let mut h = launch_paused().await;

    let before = h.inspect().await.unwrap()["rumble_sent"].as_u64().unwrap();
    h.ui_click("rumble_both").await.unwrap();
    h.step_and_wait(2).await.unwrap();

    let after = h.inspect().await.unwrap()["rumble_sent"].as_u64().unwrap();
    assert_eq!(after, before + 1, "clicking [Both] should queue one rumble");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn unknown_gamepad_button_is_rejected() {
    // Proves the old `-32000 "Gamepad simulation not yet supported"` stub is
    // really gone and that bad names now produce a typed parameter error.
    let mut h = launch_paused().await;

    let resp = h
        .call(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "press", "button": "Bogus" }),
        )
        .await
        .unwrap();

    let code = resp["error"]["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("expected an error envelope, got {resp}"));
    assert_eq!(code, -32602, "{resp}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn unknown_gamepad_action_is_rejected() {
    let mut h = launch_paused().await;

    let resp = h
        .call(
            "engine.simulateInput",
            json!({ "device": "gamepad", "action": "levitate", "button": "South" }),
        )
        .await
        .unwrap();
    assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32602, "{resp}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn batch_input_accepts_gamepad_entries() {
    // `parse_and_queue_input` used to silently drop `device: "gamepad"`, so
    // batches and stepAndInspect ignored pads even once single-shot worked.
    let mut h = launch_paused().await;

    h.call_ok(
        "engine.simulateInputBatch",
        json!({
            "inputs": [
                { "device": "gamepad", "action": "connect", "pad": 0, "name": "Batch Pad" },
                { "device": "gamepad", "action": "press", "button": "Start" },
            ]
        }),
    )
    .await
    .unwrap();
    h.step_and_wait(1).await.unwrap();

    let snap = h.inspect().await.unwrap();
    assert_eq!(snap["pad_count"].as_u64().unwrap(), 1, "{snap}");
    assert_eq!(pad0(&snap)["name"].as_str().unwrap(), "Batch Pad");
    // "Start" is bound to `pause` in game.yaml.
    assert!(
        action(&snap, "pause")["pressed"].as_bool().unwrap(),
        "{snap}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real gamepad-tester window; run with --ignored"]
async fn action_bindings_are_read_from_game_yaml() {
    // The action panel is a live readout of the config, not a hardcoded list.
    let mut h = launch_paused().await;

    let snap = h.inspect().await.unwrap();
    let names: Vec<&str> = snap["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    // Sorted union of every binding map.
    assert_eq!(
        names,
        vec![
            "aim_up",
            "cycle_pad",
            "fire",
            "jump",
            "move_left",
            "move_right",
            "pause",
        ],
        "{snap}"
    );

    // `move_left` mixes all three binding kinds, which is the whole point of
    // the mixed config.
    let bindings = action(&snap, "move_left")["bindings"].as_str().unwrap();
    assert!(bindings.contains("keys["), "{bindings}");
    assert!(bindings.contains("pad[DPadLeft]"), "{bindings}");
    assert!(bindings.contains("LeftStickX-"), "{bindings}");
}
