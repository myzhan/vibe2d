use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Result;
use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode, GainNode};

use vibe_asset::AssetBundle;

/// Audio engine: sound effects plus a single music channel (web backend via Web Audio API).
///
/// The public API is identical to the desktop backend — see `desktop.rs` for why
/// SFX are unmanaged fire-and-forget while music is one owned channel with no
/// crossfade. Game code must compile against both unchanged.
///
/// Web-specific constraints:
/// - The `AudioContext` is created lazily on first playback, because browsers
///   require a user gesture before audio may start.
/// - Decoding is **asynchronous**, so `play_music` cannot install its nodes
///   synchronously. Each request takes a generation number; a decode that
///   finishes after a newer request has been made discards itself instead of
///   stacking a second track onto the channel. Without this, quickly switching
///   tracks (which Mari0 does at the hurry-up threshold) could leave two
///   playing at once.
pub struct AudioEngine {
    /// `RefCell` because `play(&self)` may need to create the context.
    context: RefCell<Option<AudioContext>>,
    sounds: HashMap<String, Vec<u8>>,

    /// Live music nodes. `Rc` so an in-flight decode can publish into it, and
    /// `RefCell` so it can be replaced/stopped from `&self` paths.
    music: Rc<RefCell<Option<MusicNodes>>>,
    /// Shared with in-flight decodes so they can tell whether they're stale.
    music_generation: Rc<Cell<u64>>,
    /// Plain field: only `&mut self` methods ever write it, so no cell needed —
    /// which also lets `current_music()` return `Option<&str>` exactly like the
    /// desktop backend does.
    music_name: Option<String>,

    master_volume: f32,
    sfx_volume: f32,
    music_volume: f32,
}

struct MusicNodes {
    /// Stored as the parent type: `AudioBufferSourceNode`'s own `stop` family is
    /// deprecated in web-sys 0.3.98 in favour of `AudioScheduledSourceNode`'s.
    source: AudioScheduledSourceNode,
    gain: GainNode,
}

const UNITY: f32 = 1.0;

impl Default for AudioEngine {
    fn default() -> Self {
        Self {
            context: RefCell::new(None),
            sounds: HashMap::new(),
            music: Rc::new(RefCell::new(None)),
            music_generation: Rc::new(Cell::new(0)),
            music_name: None,
            master_volume: UNITY,
            sfx_volume: UNITY,
            music_volume: UNITY,
        }
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load audio files from pre-fetched bytes (WASM path).
    pub fn load_sounds_from_bundle(
        &mut self,
        audio_configs: &HashMap<String, String>,
        bundle: &AssetBundle,
    ) -> Result<()> {
        for (name, rel_path) in audio_configs {
            let bytes = bundle.files.get(rel_path).ok_or_else(|| {
                anyhow::anyhow!("Asset bundle missing sound '{}' (path: {})", name, rel_path)
            })?;
            self.sounds.insert(name.clone(), bytes.clone());
            tracing::info!("Loaded sound '{}'", name);
        }
        Ok(())
    }

    pub fn has_sound(&self, name: &str) -> bool {
        self.sounds.contains_key(name)
    }

    /// Ensure the AudioContext exists (must happen after a user gesture).
    fn ensure_context(&self) -> Option<AudioContext> {
        let mut ctx_ref = self.context.borrow_mut();
        if ctx_ref.is_none() {
            match AudioContext::new() {
                Ok(ctx) => {
                    tracing::info!("Web AudioContext created");
                    *ctx_ref = Some(ctx);
                }
                Err(e) => {
                    tracing::warn!("Failed to create AudioContext: {:?}", e);
                    return None;
                }
            }
        }
        ctx_ref.as_ref().cloned()
    }

    // ── Sound effects ────────────────────────────────────────────────

    pub fn play(&self, name: &str) {
        self.play_with_volume(name, UNITY);
    }

    pub fn play_with_volume(&self, name: &str, relative: f32) {
        let Some(data) = self.sounds.get(name).cloned() else {
            return;
        };
        let Some(ctx) = self.ensure_context() else {
            return;
        };
        let gain_value = relative.max(0.0) * self.sfx_volume * self.master_volume;
        if gain_value <= 0.0 {
            return;
        }
        let name_owned = name.to_owned();

        wasm_bindgen_futures::spawn_local(async move {
            let Some(buffer) = decode(&ctx, &data, &name_owned).await else {
                return;
            };
            let Ok(source) = ctx.create_buffer_source() else {
                return;
            };
            source.set_buffer(Some(&buffer));
            if connect_via_gain(&ctx, &source, gain_value).is_none() {
                tracing::warn!("Failed to wire gain for '{}'", name_owned);
                return;
            }
            if let Err(e) = source.start() {
                tracing::warn!("Failed to start audio playback: {:?}", e);
            }
        });
    }

    // ── Music channel ───────────────────────────────────────────────

    pub fn play_music(&mut self, name: &str) {
        if self.music_name.as_deref() == Some(name) {
            return;
        }
        self.start_music(name, true);
    }

    pub fn play_music_once(&mut self, name: &str) {
        self.start_music(name, false);
    }

    pub fn restart_music(&mut self, name: &str) {
        self.start_music(name, true);
    }

    fn start_music(&mut self, name: &str, looping: bool) {
        self.stop_music();
        self.music_name = Some(name.to_string());

        let Some(data) = self.sounds.get(name).cloned() else {
            tracing::warn!("No such music track '{}'", name);
            return;
        };
        let Some(ctx) = self.ensure_context() else {
            return;
        };

        // Claim a generation; the decode below only publishes if it's still current.
        let generation = self.music_generation.get() + 1;
        self.music_generation.set(generation);

        let gain_value = self.music_volume * self.master_volume;
        let name_owned = name.to_owned();
        let music_slot = Rc::clone(&self.music);
        let gen_slot = Rc::clone(&self.music_generation);

        wasm_bindgen_futures::spawn_local(async move {
            let Some(buffer) = decode(&ctx, &data, &name_owned).await else {
                return;
            };
            // A newer request landed while we were decoding — drop this one.
            if gen_slot.get() != generation {
                return;
            }
            let Ok(source) = ctx.create_buffer_source() else {
                return;
            };
            source.set_buffer(Some(&buffer));
            source.set_loop(looping);
            let Some(gain) = connect_via_gain(&ctx, &source, gain_value) else {
                return;
            };
            if let Err(e) = source.start() {
                tracing::warn!("Failed to start music '{}': {:?}", name_owned, e);
                return;
            }
            *music_slot.borrow_mut() = Some(MusicNodes {
                source: source.unchecked_into::<AudioScheduledSourceNode>(),
                gain,
            });
        });
    }

    pub fn stop_music(&mut self) {
        // Bump the generation so any in-flight decode discards itself.
        self.music_generation.set(self.music_generation.get() + 1);
        if let Some(nodes) = self.music.borrow_mut().take() {
            // `stop_with_when(0.0)` rather than the deprecated no-arg `stop()`;
            // 0.0 means "as soon as possible" in Web Audio's timeline.
            // 0.0 means "as soon as possible" on the Web Audio timeline.
            let _ = nodes.source.stop_with_when(0.0);
        }
        self.music_name = None;
    }

    pub fn pause_music(&self) {
        // Web Audio has no pause on a BufferSource; muting is the closest
        // equivalent that keeps the source's timeline running.
        if let Some(nodes) = self.music.borrow().as_ref() {
            nodes.gain.gain().set_value(0.0);
        }
    }

    pub fn resume_music(&self) {
        if let Some(nodes) = self.music.borrow().as_ref() {
            nodes
                .gain
                .gain()
                .set_value(self.music_volume * self.master_volume);
        }
    }

    pub fn current_music(&self) -> Option<&str> {
        self.music_name.as_deref()
    }

    pub fn is_music_playing(&self) -> bool {
        self.music_name.is_some()
    }

    pub fn stop_all(&mut self) {
        self.stop_music();
    }

    // ── Volume ──────────────────────────────────────────────────────

    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
        self.apply_music_volume();
    }

    pub fn set_music_volume(&mut self, volume: f32) {
        self.music_volume = volume.clamp(0.0, 1.0);
        self.apply_music_volume();
    }

    pub fn set_sfx_volume(&mut self, volume: f32) {
        self.sfx_volume = volume.clamp(0.0, 1.0);
    }

    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }
    pub fn music_volume(&self) -> f32 {
        self.music_volume
    }
    pub fn sfx_volume(&self) -> f32 {
        self.sfx_volume
    }

    fn apply_music_volume(&self) {
        if let Some(nodes) = self.music.borrow().as_ref() {
            nodes
                .gain
                .gain()
                .set_value(self.music_volume * self.master_volume);
        }
    }
}

/// Decode raw bytes into an `AudioBuffer`.
async fn decode(ctx: &AudioContext, data: &[u8], name: &str) -> Option<web_sys::AudioBuffer> {
    let array_buffer = to_array_buffer(data);
    let promise = match ctx.decode_audio_data(&array_buffer) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to start audio decode for '{}': {:?}", name, e);
            return None;
        }
    };
    match JsFuture::from(promise).await {
        Ok(decoded) => Some(decoded.unchecked_into()),
        Err(e) => {
            tracing::warn!("Failed to decode audio '{}': {:?}", name, e);
            None
        }
    }
}

/// Wire `source -> gain -> destination` and return the gain node.
fn connect_via_gain(
    ctx: &AudioContext,
    source: &AudioBufferSourceNode,
    gain_value: f32,
) -> Option<GainNode> {
    let gain = ctx.create_gain().ok()?;
    gain.gain().set_value(gain_value);
    source.connect_with_audio_node(&gain).ok()?;
    gain.connect_with_audio_node(&ctx.destination()).ok()?;
    Some(gain)
}

/// Convert a byte slice to a JS ArrayBuffer.
fn to_array_buffer(bytes: &[u8]) -> ArrayBuffer {
    let uint8_array = Uint8Array::new_with_length(bytes.len() as u32);
    uint8_array.copy_from(bytes);
    uint8_array.buffer()
}
