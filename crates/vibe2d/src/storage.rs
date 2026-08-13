//! Persistent key-value storage for save data (level progress, high scores,
//! options).
//!
//! One API, two backends:
//! - **Native**: a JSON file under the OS config directory, or `./<name>.json`
//!   when no config dir is available.
//! - **Web**: `window.localStorage`, keyed by the same store name.
//!
//! Values are JSON, so anything `serde`-serializable round-trips. The whole
//! store is read and written as a unit — this is for a handful of small keys, not
//! a database.
//!
//! ## Why this lives in the engine
//!
//! A game can't reasonably do this itself: `std::fs` doesn't exist on wasm, and
//! `web_sys` isn't a dependency games should need. Without an engine-level API,
//! every game either loses its saves on the web build or grows its own `cfg`
//! forest.

use std::collections::BTreeMap;

use anyhow::Result;

/// Where a store's bytes live.
///
/// Native carries an optional directory override so callers (and tests) can point
/// a store somewhere specific instead of the OS config dir; on web the location is
/// always `localStorage`, so there is nothing to carry. Aliasing it keeps
/// `backend::read`/`write` a single shape on both platforms.
#[cfg(not(target_arch = "wasm32"))]
type Location = Option<std::path::PathBuf>;
#[cfg(target_arch = "wasm32")]
type Location = ();

/// A named persistent store of JSON values.
///
/// Load it once (usually in `Game::new`), mutate it during play, and call
/// [`Storage::save`] at natural checkpoints. Nothing is written implicitly —
/// an autosave on every mutation would hammer the disk during gameplay.
#[derive(Debug, Clone)]
pub struct Storage {
    name: String,
    /// `BTreeMap` so the serialized file has a stable key order and diffs
    /// cleanly, which matters when a save file ends up in version control or a
    /// bug report.
    values: BTreeMap<String, serde_json::Value>,
    dirty: bool,
    location: Location,
}

impl Storage {
    /// Load the named store, or start empty if it doesn't exist yet.
    ///
    /// A corrupt or unreadable store is reported and treated as empty rather than
    /// failing: losing progress is bad, but refusing to launch is worse.
    pub fn load(name: &str) -> Self {
        Self::load_at(name, Location::default())
    }

    /// Load from an explicit directory instead of the OS config dir.
    ///
    /// Useful for portable installs, dedicated save folders, and tests — which
    /// need it to stay parallel-safe, since the alternative (overriding
    /// `XDG_CONFIG_HOME`) mutates process-global state.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_in(dir: impl Into<std::path::PathBuf>, name: &str) -> Self {
        Self::load_at(name, Some(dir.into()))
    }

    fn load_at(name: &str, location: Location) -> Self {
        let values = match backend::read(name, &location) {
            Ok(Some(raw)) => {
                match serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&raw) {
                    Ok(map) => map,
                    Err(e) => {
                        tracing::warn!("Save store '{}' is corrupt, starting fresh: {}", name, e);
                        BTreeMap::new()
                    }
                }
            }
            Ok(None) => BTreeMap::new(),
            Err(e) => {
                tracing::warn!("Could not read save store '{}': {}", name, e);
                BTreeMap::new()
            }
        };
        Self {
            name: name.to_string(),
            values,
            dirty: false,
            location,
        }
    }

    /// Read and deserialize a key, or `None` if absent or the wrong shape.
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let raw = self.values.get(key)?;
        match serde_json::from_value(raw.clone()) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("Save key '{}' has unexpected shape: {}", key, e);
                None
            }
        }
    }

    /// Read a key, falling back to `default` when absent or malformed.
    pub fn get_or<T: serde::de::DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    /// Set a key. Nothing hits the disk until [`Storage::save`].
    pub fn set<T: serde::Serialize>(&mut self, key: &str, value: T) {
        match serde_json::to_value(value) {
            Ok(v) => {
                self.values.insert(key.to_string(), v);
                self.dirty = true;
            }
            Err(e) => tracing::warn!("Could not serialize save key '{}': {}", key, e),
        }
    }

    pub fn remove(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.dirty = true;
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(|k| k.as_str())
    }

    /// Have there been changes since the last successful save?
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Drop every key. Still requires a [`Storage::save`] to persist.
    pub fn clear(&mut self) {
        if !self.values.is_empty() {
            self.values.clear();
            self.dirty = true;
        }
    }

    /// Write the store out. A no-op when nothing has changed.
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let raw = serde_json::to_string_pretty(&self.values)?;
        backend::write(&self.name, &self.location, &raw)?;
        self.dirty = false;
        Ok(())
    }

    /// Where the store lives, for diagnostics.
    pub fn location(&self) -> String {
        backend::location(&self.name, &self.location)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use anyhow::{Context, Result};
    use std::path::PathBuf;

    /// `$XDG_CONFIG_HOME/vibe2d/<name>.json` (or the platform equivalent),
    /// falling back to the working directory when no config dir is available.
    fn path(name: &str, override_dir: &super::Location) -> PathBuf {
        let file = format!("{name}.json");
        if let Some(dir) = override_dir {
            return dir.join(file);
        }
        match config_dir() {
            Some(dir) => dir.join("vibe2d").join(file),
            None => PathBuf::from(file),
        }
    }

    /// Resolved from environment variables rather than a `dirs`-style dependency —
    /// this is the only place the engine needs a config path, and it isn't worth
    /// a crate.
    fn config_dir() -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.is_empty()
        {
            return Some(PathBuf::from(xdg));
        }
        // macOS keeps app data under Library/Application Support; everything else
        // POSIX-ish uses ~/.config.
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            let home = PathBuf::from(home);
            return Some(if cfg!(target_os = "macos") {
                home.join("Library").join("Application Support")
            } else {
                home.join(".config")
            });
        }
        // Windows.
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }

    pub fn read(name: &str, loc: &super::Location) -> Result<Option<String>> {
        let p = path(name, loc);
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(&p).with_context(|| {
            format!("reading save store {}", p.display())
        })?))
    }

    pub fn write(name: &str, loc: &super::Location, contents: &str) -> Result<()> {
        let p = path(name, loc);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        // Write-then-rename so a crash mid-write can't truncate an existing save.
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &p).with_context(|| format!("replacing {}", p.display()))?;
        Ok(())
    }

    pub fn location(name: &str, loc: &super::Location) -> String {
        path(name, loc).display().to_string()
    }
}

#[cfg(target_arch = "wasm32")]
mod backend {
    use anyhow::{Result, anyhow};

    fn storage() -> Result<web_sys::Storage> {
        web_sys::window()
            .ok_or_else(|| anyhow!("no window"))?
            .local_storage()
            .map_err(|e| anyhow!("localStorage unavailable: {:?}", e))?
            .ok_or_else(|| anyhow!("localStorage is null (private browsing?)"))
    }

    fn key(name: &str) -> String {
        format!("vibe2d:{name}")
    }

    pub fn read(name: &str, _loc: &super::Location) -> Result<Option<String>> {
        storage()?
            .get_item(&key(name))
            .map_err(|e| anyhow!("localStorage read failed: {:?}", e))
    }

    pub fn write(name: &str, _loc: &super::Location, contents: &str) -> Result<()> {
        storage()?
            .set_item(&key(name), contents)
            .map_err(|e| anyhow!("localStorage write failed: {:?}", e))
    }

    pub fn location(name: &str, _loc: &super::Location) -> String {
        format!("localStorage[{}]", key(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own directory, passed explicitly — no process-global
    /// env mutation, so these stay parallel-safe.
    fn isolated(test: &str) -> (Storage, tempdir::TempDir) {
        let dir = tempdir::TempDir::new(test);
        let s = Storage::load_in(dir.path(), test);
        (s, dir)
    }

    fn reload(dir: &tempdir::TempDir, name: &str) -> Storage {
        Storage::load_in(dir.path(), name)
    }

    /// Minimal scratch directory — avoids a dev-dependency for four tests.
    mod tempdir {
        use std::path::{Path, PathBuf};
        pub struct TempDir(PathBuf);
        impl TempDir {
            pub fn new(tag: &str) -> Self {
                // Process id + tag is enough uniqueness; tests in one binary
                // share a process but not a tag.
                let p = std::env::temp_dir().join(format!(
                    "vibe2d_storage_{}_{}",
                    tag,
                    std::process::id()
                ));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).expect("create temp dir");
                Self(p)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn missing_store_loads_empty() {
        let (s, _d) = isolated("missing");
        assert!(!s.is_dirty());
        assert_eq!(s.get::<u32>("nope"), None);
        assert_eq!(s.get_or("nope", 7u32), 7);
    }

    #[test]
    fn values_round_trip_through_a_save_and_reload() {
        let (mut s, _d) = isolated("roundtrip");
        let dir = &_d;
        s.set("world", 3u32);
        s.set("level", 2u32);
        s.set("name", "mario");
        s.set("unlocked", vec![1u32, 2, 3]);
        assert!(s.is_dirty());
        s.save().expect("save");
        assert!(!s.is_dirty(), "save should clear the dirty flag");

        let reloaded = reload(dir, "roundtrip");
        assert_eq!(reloaded.get::<u32>("world"), Some(3));
        assert_eq!(reloaded.get::<u32>("level"), Some(2));
        assert_eq!(reloaded.get::<String>("name").as_deref(), Some("mario"));
        assert_eq!(reloaded.get::<Vec<u32>>("unlocked"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn wrong_type_reads_as_none_rather_than_panicking() {
        let (mut s, _d) = isolated("wrongtype");
        s.set("world", "not a number");
        // A save file edited by hand (or written by an older build) must not take
        // the game down.
        assert_eq!(s.get::<u32>("world"), None);
        assert_eq!(s.get_or("world", 1u32), 1);
    }

    #[test]
    fn corrupt_store_is_reported_and_treated_as_empty() {
        let dir = tempdir::TempDir::new("corrupt");
        std::fs::write(dir.path().join("corrupt.json"), "{ this is not json").unwrap();

        let s = Storage::load_in(dir.path(), "corrupt");
        assert_eq!(
            s.keys().count(),
            0,
            "should start fresh, not fail to launch"
        );
    }

    #[test]
    fn remove_and_clear_track_dirtiness() {
        let (mut s, _d) = isolated("dirty");
        s.set("a", 1u32);
        s.save().unwrap();

        s.remove("missing");
        assert!(!s.is_dirty(), "removing an absent key is not a change");
        s.remove("a");
        assert!(s.is_dirty());
        s.save().unwrap();

        s.clear();
        assert!(!s.is_dirty(), "clearing an empty store is not a change");
        s.set("b", 2u32);
        s.clear();
        assert!(s.is_dirty());
    }

    #[test]
    fn save_is_a_noop_when_nothing_changed() {
        let (mut s, _d) = isolated("noop");
        s.set("x", 1u32);
        s.save().unwrap();
        // Second save writes nothing; the point is that it also can't fail.
        assert!(s.save().is_ok());
        assert!(!s.is_dirty());
    }

    #[test]
    fn keys_are_ordered_for_stable_diffs() {
        let (mut s, _d) = isolated("ordered");
        s.set("zebra", 1u32);
        s.set("apple", 2u32);
        s.set("mango", 3u32);
        assert_eq!(
            s.keys().collect::<Vec<_>>(),
            vec!["apple", "mango", "zebra"]
        );
    }
}
