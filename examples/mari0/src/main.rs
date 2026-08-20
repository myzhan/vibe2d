// Mari0 — Portal meets Mario
// A tribute to the original Mari0 by Maurice (Stabyourself.net)
// Original game: https://stabyourself.net/mari0/
// Built with vibe2d engine.

// Data layer: tile properties (generated from the tilesheets), the 100-entry
// entity table, and the general level parser. Replaces the loader that was
// hardcoded to 1-1.
mod level;

mod atlas;
mod castle;
mod constants;
mod cube;
mod effects;
mod emancipation;
mod enemies;
mod flagpole;
mod game;
mod gel;
mod interlude;
mod items;
mod lab;
mod lab_render;
mod maze;
mod menu;
mod music;
mod physics;
mod pipe;
mod platform;
mod player;
mod portal;
mod portal_math;
mod render;
mod seesaw;
mod spring;
mod vine;
mod world;

#[cfg(feature = "vdp")]
mod vdp;

use game::Mari0Game;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    vibe2d::run::<Mari0Game>("game.yaml");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // No-op on web; the real entry point is web_main below.
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    wasm_bindgen_futures::spawn_local(async {
        vibe2d::run_web::<Mari0Game>("game.yaml").await;
    });
}
