use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GameConfig {
    pub meta: Option<MetaConfig>,
    pub window: WindowConfig,
    pub virtual_resolution: Option<VirtualResolutionConfig>,
    pub assets: Option<AssetsConfig>,
    pub physics: Option<PhysicsConfig>,
    pub input: Option<InputConfig>,
    pub debug: Option<DebugConfig>,
    pub constants: Option<HashMap<String, serde_yaml::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct MetaConfig {
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub resizable: Option<bool>,
    pub vsync: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VirtualResolutionConfig {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
pub struct AssetsConfig {
    pub textures: Option<HashMap<String, String>>,
    pub fonts: Option<HashMap<String, String>>,
    pub audio: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct PhysicsConfig {
    pub gravity: Option<f32>,
    pub iterations: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct InputConfig {
    pub actions: HashMap<String, vibe_input::ActionConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DebugConfig {
    pub vdp: Option<VdpConfig>,
    pub physics_overlay: Option<bool>,
    pub fps_counter: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VdpConfig {
    pub enabled: Option<bool>,
    pub port: Option<u16>,
}

impl GameConfig {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &str) -> Result<Self> {
        let resolved = Self::resolve_config_path(path);
        Self::load_from_path(&resolved)
    }

    /// Load config from an already-resolved path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_path(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from raw YAML bytes (used on WASM where there's no filesystem).
    pub fn load_from_bytes(bytes: &[u8]) -> Result<Self> {
        let content = std::str::from_utf8(bytes)
            .map_err(|e| anyhow::anyhow!("Config is not valid UTF-8: {}", e))?;
        let config: Self = serde_yaml::from_str(content)?;
        Ok(config)
    }

    /// Resolve the config file path. If `path` doesn't exist in the current
    /// directory, fall back to `CARGO_MANIFEST_DIR` (set by `cargo run`) so
    /// that games work when launched from the workspace root.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_config_path(path: &str) -> std::path::PathBuf {
        let direct = std::path::Path::new(path);
        if direct.exists() {
            return direct.to_path_buf();
        }
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let candidate = std::path::Path::new(&manifest_dir).join(path);
            if candidate.exists() {
                return candidate;
            }
        }
        // Return original path so the caller gets a clear "not found" error
        direct.to_path_buf()
    }

    pub fn get_constant_f32(&self, key: &str) -> Option<f32> {
        self.constants
            .as_ref()?
            .get(key)?
            .as_f64()
            .map(|v| v as f32)
    }
}

// ─────────────────────────────────────────────────────────────────────
// Unit tests — YAML parsing and path resolution
// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> GameConfig {
        serde_yaml::from_str(yaml).expect("parse yaml")
    }

    #[test]
    fn minimal_config_only_window() {
        let cfg = parse(
            r#"
window:
  width: 800
  height: 600
  title: "Test"
"#,
        );
        assert_eq!(cfg.window.width, 800);
        assert_eq!(cfg.window.height, 600);
        assert_eq!(cfg.window.title, "Test");
        assert!(cfg.window.vsync.is_none());
        assert!(cfg.virtual_resolution.is_none());
        assert!(cfg.assets.is_none());
        assert!(cfg.input.is_none());
        assert!(cfg.debug.is_none());
    }

    #[test]
    fn full_config_parses_all_sections() {
        let cfg = parse(
            r#"
meta:
  name: "Demo"
  version: "0.1.0"
window:
  width: 1280
  height: 720
  title: "Demo"
  vsync: true
virtual_resolution:
  width: 512
  height: 288
assets:
  textures:
    bird: "assets/bird.png"
  fonts:
    score: "assets/font.ttf:32"
  audio:
    flap: "assets/flap.wav"
input:
  actions:
    jump:
      keys: ["Space"]
      mouse_buttons: ["Left"]
debug:
  vdp:
    enabled: true
    port: 9229
constants:
  GRAVITY: 500.0
  PIPE_GAP: 70
"#,
        );

        assert_eq!(cfg.meta.as_ref().unwrap().name.as_deref(), Some("Demo"));
        assert_eq!(cfg.window.vsync, Some(true));

        let vr = cfg.virtual_resolution.as_ref().unwrap();
        assert_eq!((vr.width, vr.height), (512, 288));

        let assets = cfg.assets.as_ref().unwrap();
        assert_eq!(
            assets.textures.as_ref().unwrap().get("bird"),
            Some(&"assets/bird.png".to_string())
        );
        assert_eq!(
            assets.fonts.as_ref().unwrap().get("score"),
            Some(&"assets/font.ttf:32".to_string())
        );
        assert_eq!(
            assets.audio.as_ref().unwrap().get("flap"),
            Some(&"assets/flap.wav".to_string())
        );

        let actions = &cfg.input.as_ref().unwrap().actions;
        let jump = actions.get("jump").unwrap();
        assert_eq!(jump.keys, vec!["Space".to_string()]);
        assert_eq!(jump.mouse_buttons, vec!["Left".to_string()]);

        let vdp = cfg.debug.as_ref().unwrap().vdp.as_ref().unwrap();
        assert_eq!(vdp.enabled, Some(true));
        assert_eq!(vdp.port, Some(9229));

        assert_eq!(cfg.get_constant_f32("GRAVITY"), Some(500.0));
        assert_eq!(cfg.get_constant_f32("PIPE_GAP"), Some(70.0));
        assert_eq!(cfg.get_constant_f32("MISSING"), None);
    }

    #[test]
    fn input_action_keys_default_to_empty_when_omitted() {
        let cfg = parse(
            r#"
window: { width: 1, height: 1, title: "x" }
input:
  actions:
    only_mouse:
      mouse_buttons: ["Left"]
    only_keys:
      keys: ["Space"]
"#,
        );
        let actions = &cfg.input.as_ref().unwrap().actions;
        assert!(actions.get("only_mouse").unwrap().keys.is_empty());
        assert!(actions.get("only_keys").unwrap().mouse_buttons.is_empty());
    }

    #[test]
    fn resolve_config_path_returns_existing_direct_path() {
        let tmp = std::env::temp_dir().join("vibe2d_resolve_direct.yaml");
        std::fs::write(&tmp, "window: { width: 1, height: 1, title: t }").unwrap();
        let resolved = GameConfig::resolve_config_path(tmp.to_str().unwrap());
        assert_eq!(resolved, tmp);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn resolve_config_path_returns_original_when_missing() {
        // When neither the direct path nor MANIFEST_DIR/path exists, the
        // function returns the original path so the caller can produce a
        // clear "not found" error.
        let resolved = GameConfig::resolve_config_path("definitely_does_not_exist_42.yaml");
        assert_eq!(
            resolved,
            std::path::PathBuf::from("definitely_does_not_exist_42.yaml")
        );
    }

    #[test]
    fn load_from_path_reads_yaml_file() {
        let tmp = std::env::temp_dir().join("vibe2d_load_test.yaml");
        std::fs::write(
            &tmp,
            "window:\n  width: 320\n  height: 240\n  title: \"T\"\n",
        )
        .unwrap();
        let cfg = GameConfig::load_from_path(&tmp).unwrap();
        assert_eq!(cfg.window.width, 320);
        let _ = std::fs::remove_file(&tmp);
    }
}
