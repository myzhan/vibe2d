use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use rodio::Source;

/// Audio engine that loads and plays sound effects (desktop backend via rodio).
///
/// `Default` intentionally produces a silent instance (no device, no streams)
/// so `std::mem::take(&mut engine)` during the take/swap cycle in
/// `GameBridge::on_update`/`on_render` has something cheap to leave behind.
/// For an actually-playing engine, construct via [`AudioEngine::new`].
#[derive(Default)]
pub struct AudioEngine {
    _stream: Option<rodio::OutputStream>,
    handle: Option<rodio::OutputStreamHandle>,
    sounds: HashMap<String, Vec<u8>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => {
                tracing::info!("Audio initialized");
                Self {
                    _stream: Some(stream),
                    handle: Some(handle),
                    sounds: HashMap::new(),
                }
            }
            Err(e) => {
                tracing::warn!("Failed to initialize audio: {}", e);
                Self::default()
            }
        }
    }

    /// Load audio files from config (name -> relative path).
    pub fn load_sounds(
        &mut self,
        base_path: &Path,
        audio_configs: &HashMap<String, String>,
    ) -> Result<()> {
        for (name, rel_path) in audio_configs {
            let full_path = base_path.join(rel_path);
            let bytes = std::fs::read(&full_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to load sound '{}' from {:?}: {}",
                    name,
                    full_path,
                    e
                )
            })?;
            self.sounds.insert(name.clone(), bytes);
            tracing::info!("Loaded sound '{}'", name);
        }
        Ok(())
    }

    /// Load audio files from pre-fetched bytes (unified cross-platform path).
    pub fn load_sounds_from_bundle(
        &mut self,
        audio_configs: &HashMap<String, String>,
        bundle: &vibe_asset::AssetBundle,
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

    /// Play a loaded sound by name (fire-and-forget).
    pub fn play(&self, name: &str) {
        let handle = match &self.handle {
            Some(h) => h,
            None => return,
        };
        if let Some(data) = self.sounds.get(name) {
            let cursor = Cursor::new(data.clone());
            match rodio::Decoder::new(cursor) {
                Ok(source) => {
                    if let Err(e) = handle.play_raw(source.convert_samples()) {
                        tracing::warn!("Failed to play sound '{}': {}", name, e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to decode sound '{}': {}", name, e);
                }
            }
        }
    }
}
