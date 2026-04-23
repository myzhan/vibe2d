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
    pub fn load(path: &str) -> Result<Self> {
        let resolved = Self::resolve_config_path(path);
        Self::load_from_path(&resolved)
    }

    /// Load config from an already-resolved path.
    pub fn load_from_path(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Resolve the config file path. If `path` doesn't exist in the current
    /// directory, fall back to `CARGO_MANIFEST_DIR` (set by `cargo run`) so
    /// that games work when launched from the workspace root.
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
