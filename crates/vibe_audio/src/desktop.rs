use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use rodio::Source;

/// Audio engine: sound effects plus a single music channel (desktop backend via rodio).
///
/// ## Two deliberately different playback models
///
/// **Sound effects** are fire-and-forget and unmanaged: every [`AudioEngine::play`]
/// mixes an independent stream, so the same sound can overlap itself without limit
/// and nothing can stop it early. That matches how a 2D game actually wants SFX to
/// behave (ten coins in ten frames should sound like ten coins).
///
/// **Music** is a single owned channel. There is exactly one music [`rodio::Sink`];
/// starting a new track stops the old one immediately with no crossfade. That is
/// not a simplification — it's the semantics classic games rely on (e.g. the
/// hurry-up track restarts from zero rather than resuming), so a fade would be
/// wrong, not nicer.
///
/// `Default` intentionally produces a silent instance (no device, no streams) so
/// `std::mem::take(&mut engine)` during the take/swap cycle in
/// `GameBridge::on_update`/`on_render` has something cheap to leave behind. Every
/// method is a safe no-op on a silent instance, which also makes the engine usable
/// in headless tests.
#[derive(Default)]
pub struct AudioEngine {
    _stream: Option<rodio::OutputStream>,
    handle: Option<rodio::OutputStreamHandle>,
    sounds: HashMap<String, Vec<u8>>,

    /// The one music sink. `None` until a track is played.
    music_sink: Option<rodio::Sink>,
    /// Name of the track currently on the music channel, for `current_music()`.
    music_name: Option<String>,

    master_volume: f32,
    sfx_volume: f32,
    music_volume: f32,
}

/// Volumes start at unity; `Default` can't express that for `f32` so `new()` and
/// `default_volumes()` both go through this.
const UNITY: f32 = 1.0;

impl AudioEngine {
    pub fn new() -> Self {
        let mut engine = match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => {
                tracing::info!("Audio initialized");
                Self {
                    _stream: Some(stream),
                    handle: Some(handle),
                    ..Default::default()
                }
            }
            Err(e) => {
                tracing::warn!("Failed to initialize audio: {}", e);
                Self::default()
            }
        };
        engine.master_volume = UNITY;
        engine.sfx_volume = UNITY;
        engine.music_volume = UNITY;
        engine
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

    /// Is a sound with this name loaded?
    pub fn has_sound(&self, name: &str) -> bool {
        self.sounds.contains_key(name)
    }

    // ── Sound effects ────────────────────────────────────────────────

    /// Play a loaded sound effect (fire-and-forget, at the current SFX volume).
    pub fn play(&self, name: &str) {
        self.play_with_volume(name, UNITY);
    }

    /// Play a loaded sound effect at a per-call relative volume.
    ///
    /// `relative` is multiplied by the SFX and master volumes, so `0.3` means
    /// "30% of however loud SFX currently are" rather than an absolute level.
    /// (Mari0 mixes its four portal sounds at 0.3 this way.)
    pub fn play_with_volume(&self, name: &str, relative: f32) {
        let Some(handle) = &self.handle else { return };
        let Some(data) = self.sounds.get(name) else {
            return;
        };
        let gain = (relative.max(0.0)) * self.sfx_volume * self.master_volume;
        if gain <= 0.0 {
            return;
        }
        match rodio::Decoder::new(Cursor::new(data.clone())) {
            Ok(source) => {
                if let Err(e) = handle.play_raw(source.convert_samples().amplify(gain)) {
                    tracing::warn!("Failed to play sound '{}': {}", name, e);
                }
            }
            Err(e) => tracing::warn!("Failed to decode sound '{}': {}", name, e),
        }
    }

    // ── Music channel ───────────────────────────────────────────────

    /// Start a looping music track, replacing whatever is on the music channel.
    ///
    /// Re-requesting the track that is *already* playing is a no-op, so calling
    /// this every frame from a state machine won't restart the music. Use
    /// [`AudioEngine::restart_music`] when you specifically need a restart.
    pub fn play_music(&mut self, name: &str) {
        if self.music_name.as_deref() == Some(name) && self.is_music_playing() {
            return;
        }
        self.start_music(name, true);
    }

    /// Start a music track that plays once and then stops (jingles: level end,
    /// game over, the hurry-up alarm).
    pub fn play_music_once(&mut self, name: &str) {
        self.start_music(name, false);
    }

    /// Restart the given track from the beginning even if it's already playing.
    pub fn restart_music(&mut self, name: &str) {
        self.start_music(name, true);
    }

    fn start_music(&mut self, name: &str, looping: bool) {
        self.stop_music();
        let Some(handle) = &self.handle else {
            // Still record the name so `current_music()` is meaningful in
            // headless/no-device runs (and in tests).
            self.music_name = Some(name.to_string());
            return;
        };
        let Some(data) = self.sounds.get(name) else {
            tracing::warn!("No such music track '{}'", name);
            return;
        };
        let sink = match rodio::Sink::try_new(handle) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to create music sink for '{}': {}", name, e);
                return;
            }
        };
        let decoded = match rodio::Decoder::new(Cursor::new(data.clone())) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to decode music '{}': {}", name, e);
                return;
            }
        };
        // `repeat_infinite` needs the source buffered, which is why music is a
        // separate path from SFX rather than a flag on `play`.
        if looping {
            sink.append(decoded.convert_samples::<f32>().repeat_infinite());
        } else {
            sink.append(decoded.convert_samples::<f32>());
        }
        sink.set_volume(self.music_volume * self.master_volume);
        self.music_sink = Some(sink);
        self.music_name = Some(name.to_string());
    }

    /// Stop the music channel. Sound effects keep playing.
    pub fn stop_music(&mut self) {
        if let Some(sink) = self.music_sink.take() {
            sink.stop();
        }
        self.music_name = None;
    }

    pub fn pause_music(&self) {
        if let Some(sink) = &self.music_sink {
            sink.pause();
        }
    }

    pub fn resume_music(&self) {
        if let Some(sink) = &self.music_sink {
            sink.play();
        }
    }

    /// Name of the track on the music channel, if any.
    pub fn current_music(&self) -> Option<&str> {
        self.music_name.as_deref()
    }

    /// Is the music channel actively playing?
    ///
    /// A non-looping track that has run to completion reports `false` here while
    /// [`AudioEngine::current_music`] still reports its name — that pair is how
    /// you detect "the jingle finished".
    pub fn is_music_playing(&self) -> bool {
        match &self.music_sink {
            Some(sink) => !sink.empty() && !sink.is_paused(),
            // No sink but a recorded name means a silent (no-device) instance;
            // treat it as playing so state machines behave identically headless.
            None => self.handle.is_none() && self.music_name.is_some(),
        }
    }

    /// Stop everything — music and (as far as the backend allows) effects.
    ///
    /// Note fire-and-forget effects already in flight cannot be recalled; this
    /// stops the music channel and is the closest analogue to Löve's
    /// `love.audio.stop()`, which Mari0 fires when the timer hits 99.
    pub fn stop_all(&mut self) {
        self.stop_music();
    }

    // ── Volume ──────────────────────────────────────────────────────

    /// Master volume, clamped to 0.0..=1.0. Applies to music and to future SFX.
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
        self.apply_music_volume();
    }

    pub fn set_music_volume(&mut self, volume: f32) {
        self.music_volume = volume.clamp(0.0, 1.0);
        self.apply_music_volume();
    }

    /// SFX volume, clamped to 0.0..=1.0. Only affects sounds started afterwards
    /// (already-playing effects are unmanaged by design).
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
        if let Some(sink) = &self.music_sink {
            sink.set_volume(self.music_volume * self.master_volume);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A silent engine with volumes at unity — what `new()` produces when there
    /// is no audio device, which is also the CI case.
    fn silent() -> AudioEngine {
        let mut e = AudioEngine::default();
        e.set_master_volume(UNITY);
        e.set_music_volume(UNITY);
        e.set_sfx_volume(UNITY);
        e
    }

    #[test]
    fn default_instance_is_silent_and_every_method_is_a_noop() {
        // `Default` is load-bearing: GameBridge `mem::take`s the engine every
        // frame, so the leftover must be safe to call into.
        let mut e = AudioEngine::default();
        e.play("nope");
        e.play_with_volume("nope", 0.3);
        e.play_music("nope");
        e.play_music_once("nope");
        e.pause_music();
        e.resume_music();
        e.stop_music();
        e.stop_all();
        assert!(!e.has_sound("nope"));
    }

    #[test]
    fn volumes_are_clamped() {
        let mut e = silent();
        e.set_master_volume(5.0);
        assert_eq!(e.master_volume(), 1.0);
        e.set_master_volume(-1.0);
        assert_eq!(e.master_volume(), 0.0);
        e.set_music_volume(2.5);
        assert_eq!(e.music_volume(), 1.0);
        e.set_sfx_volume(-0.5);
        assert_eq!(e.sfx_volume(), 0.0);
    }

    #[test]
    fn music_channel_tracks_the_current_track_name() {
        let mut e = silent();
        assert_eq!(e.current_music(), None);
        e.play_music("overworld");
        assert_eq!(e.current_music(), Some("overworld"));
        // Switching replaces rather than layers — there is only ever one track.
        e.play_music("underground");
        assert_eq!(e.current_music(), Some("underground"));
        e.stop_music();
        assert_eq!(e.current_music(), None);
    }

    #[test]
    fn replaying_the_same_track_does_not_restart_it() {
        // This is what lets a game call `play_music(track_for_current_level())`
        // unconditionally every frame without stuttering.
        let mut e = silent();
        e.play_music("overworld");
        assert!(e.is_music_playing());
        e.play_music("overworld");
        assert_eq!(e.current_music(), Some("overworld"));
        assert!(e.is_music_playing());
    }

    #[test]
    fn stop_all_clears_the_music_channel() {
        let mut e = silent();
        e.play_music("castle");
        e.stop_all();
        assert_eq!(e.current_music(), None);
        assert!(!e.is_music_playing());
    }

    #[test]
    fn loading_registers_sounds_by_name() {
        let mut e = silent();
        let bundle = vibe_asset::AssetBundle {
            files: HashMap::from([("sfx/jump.ogg".to_string(), vec![1, 2, 3])]),
        };
        let cfg = HashMap::from([("jump".to_string(), "sfx/jump.ogg".to_string())]);
        e.load_sounds_from_bundle(&cfg, &bundle).unwrap();
        assert!(e.has_sound("jump"));
        assert!(!e.has_sound("coin"));
    }

    #[test]
    fn loading_a_missing_bundle_entry_is_an_error_not_a_panic() {
        let mut e = silent();
        let bundle = vibe_asset::AssetBundle {
            files: HashMap::new(),
        };
        let cfg = HashMap::from([("jump".to_string(), "sfx/jump.ogg".to_string())]);
        assert!(e.load_sounds_from_bundle(&cfg, &bundle).is_err());
    }
}
