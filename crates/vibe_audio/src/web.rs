use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::Result;
use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::AudioContext;

use vibe_asset::AssetBundle;

/// Audio engine that loads and plays sound effects (web backend via Web Audio API).
///
/// `Default` produces a silent instance. The `AudioContext` is created lazily
/// on first `play()` to comply with browser autoplay policies (user gesture required).
/// Interior mutability (`RefCell`) is used so `play()` can take `&self`, matching
/// the desktop API.
pub struct AudioEngine {
    context: RefCell<Option<AudioContext>>,
    /// Raw audio bytes keyed by name; decoded to AudioBuffer on demand.
    sounds: HashMap<String, Vec<u8>>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self {
            context: RefCell::new(None),
            sounds: HashMap::new(),
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

    /// Ensure the AudioContext is created (must happen after user gesture).
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

    /// Play a loaded sound by name (fire-and-forget).
    /// Decodes the audio bytes and plays via Web Audio API.
    pub fn play(&self, name: &str) {
        let data = match self.sounds.get(name) {
            Some(d) => d.clone(),
            None => return,
        };

        let ctx = match self.ensure_context() {
            Some(c) => c,
            None => return,
        };

        let name_owned = name.to_owned();

        // Decode and play asynchronously
        wasm_bindgen_futures::spawn_local(async move {
            let array_buffer = to_array_buffer(&data);
            let decode_promise = match ctx.decode_audio_data(&array_buffer) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Failed to start audio decode for '{}': {:?}", name_owned, e);
                    return;
                }
            };

            match JsFuture::from(decode_promise).await {
                Ok(decoded) => {
                    let audio_buffer: web_sys::AudioBuffer = decoded.unchecked_into();
                    let source = match ctx.create_buffer_source() {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("Failed to create buffer source: {:?}", e);
                            return;
                        }
                    };
                    source.set_buffer(Some(&audio_buffer));
                    if let Err(e) = source.connect_with_audio_node(&ctx.destination()) {
                        tracing::warn!("Failed to connect audio source: {:?}", e);
                        return;
                    }
                    if let Err(e) = source.start() {
                        tracing::warn!("Failed to start audio playback: {:?}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to decode audio '{}': {:?}", name_owned, e);
                }
            }
        });
    }
}

/// Convert a byte slice to a JS ArrayBuffer.
fn to_array_buffer(bytes: &[u8]) -> ArrayBuffer {
    let uint8_array = Uint8Array::new_with_length(bytes.len() as u32);
    uint8_array.copy_from(bytes);
    uint8_array.buffer()
}
