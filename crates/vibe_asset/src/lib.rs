use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use anyhow::Result;
use vibe_render::{Font, Renderer, Texture, TextureId};

/// Pre-fetched asset bytes for platforms without filesystem access (WASM).
/// Assets are loaded via HTTP before game start and stored in this bundle.
pub struct AssetBundle {
    pub files: HashMap<String, Vec<u8>>,
}

/// Manages loaded game assets (textures, fonts).
///
/// `AssetManager` is intentionally **GPU-backend agnostic**: it owns the
/// already-uploaded [`Texture`] / [`Font`] handles and exposes a name → id
/// lookup, but never touches `wgpu` directly. All GPU operations
/// (texture decode + upload, font glyph rasterization) are funneled
/// through [`Renderer`]'s high-level API, which is the only crate that
/// holds GPU device/queue handles.
#[derive(Default)]
pub struct AssetManager {
    textures: Vec<Texture>,
    texture_names: HashMap<String, TextureId>,
    fonts: HashMap<String, Font>,
}

impl AssetManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load textures defined in the config. Image bytes are read from disk
    /// here, then handed to [`Renderer::load_texture`] for GPU upload.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_textures(
        &mut self,
        renderer: &Renderer,
        base_path: &Path,
        texture_configs: &HashMap<String, String>,
    ) -> Result<()> {
        for (name, rel_path) in texture_configs {
            let full_path = base_path.join(rel_path);
            let bytes = std::fs::read(&full_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to load texture '{}' from {:?}: {}",
                    name,
                    full_path,
                    e
                )
            })?;

            let texture = renderer.load_texture(name, &bytes)?;
            let id = TextureId(self.textures.len());
            self.textures.push(texture);
            self.texture_names.insert(name.clone(), id);
        }
        Ok(())
    }

    /// Get a texture ID by its config name.
    pub fn texture_id(&self, name: &str) -> Option<TextureId> {
        self.texture_names.get(name).copied()
    }

    /// Get a texture reference by ID.
    pub fn texture(&self, id: TextureId) -> &Texture {
        &self.textures[id.0]
    }

    /// Get the (width, height) of a texture in pixels.
    pub fn texture_size(&self, id: TextureId) -> (u32, u32) {
        let t = &self.textures[id.0];
        (t.width, t.height)
    }

    /// Get all textures as a slice (for rendering).
    pub fn all_textures(&self) -> Vec<&Texture> {
        self.textures.iter().collect()
    }

    /// Load fonts defined in the config. Each entry maps name → "path:size".
    ///
    /// Glyphs are **not** rasterized at load time beyond the printable
    /// ASCII warm-up that [`Renderer::load_font`] does internally — atlases
    /// otherwise start empty and fill on demand via
    /// [`AssetManager::prepare_text`]. This lets fonts with huge codepoint
    /// coverage (CJK) load instantly.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_fonts(
        &mut self,
        renderer: &Renderer,
        base_path: &Path,
        font_configs: &HashMap<String, String>,
    ) -> Result<()> {
        for (name, config_str) in font_configs {
            // Format: "path/to/font.ttf:32" (path:size)
            let (rel_path, size_str) = config_str.rsplit_once(':').ok_or_else(|| {
                anyhow::anyhow!("Font config '{}' must be 'path:size'", config_str)
            })?;

            let font_size: f32 = size_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid font size '{}' for '{}'", size_str, name))?;

            let full_path = base_path.join(rel_path);
            let bytes = std::fs::read(&full_path).map_err(|e| {
                anyhow::anyhow!("Failed to load font '{}' from {:?}: {}", name, full_path, e)
            })?;

            let atlas_texture_id = TextureId(self.textures.len());
            let (font, atlas_texture) = renderer.load_font(&bytes, font_size, atlas_texture_id)?;

            self.textures.push(atlas_texture);
            self.fonts.insert(name.clone(), font);
            tracing::info!("Loaded font '{}' (lazy atlas, {}px)", name, font_size);
        }
        Ok(())
    }

    /// Load textures from pre-fetched bytes (WASM path where filesystem
    /// is unavailable and assets have been fetched via HTTP).
    pub fn load_textures_from_bundle(
        &mut self,
        renderer: &Renderer,
        texture_configs: &HashMap<String, String>,
        bundle: &AssetBundle,
    ) -> Result<()> {
        for (name, rel_path) in texture_configs {
            let bytes = bundle.files.get(rel_path).ok_or_else(|| {
                anyhow::anyhow!(
                    "Asset bundle missing texture '{}' (path: {})",
                    name,
                    rel_path
                )
            })?;

            let texture = renderer.load_texture(name, bytes)?;
            let id = TextureId(self.textures.len());
            self.textures.push(texture);
            self.texture_names.insert(name.clone(), id);
        }
        Ok(())
    }

    /// Load fonts from pre-fetched bytes (WASM path).
    pub fn load_fonts_from_bundle(
        &mut self,
        renderer: &Renderer,
        font_configs: &HashMap<String, String>,
        bundle: &AssetBundle,
    ) -> Result<()> {
        for (name, config_str) in font_configs {
            let (rel_path, size_str) = config_str.rsplit_once(':').ok_or_else(|| {
                anyhow::anyhow!("Font config '{}' must be 'path:size'", config_str)
            })?;

            let font_size: f32 = size_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid font size '{}' for '{}'", size_str, name))?;

            let bytes = bundle.files.get(rel_path).ok_or_else(|| {
                anyhow::anyhow!("Asset bundle missing font '{}' (path: {})", name, rel_path)
            })?;

            let atlas_texture_id = TextureId(self.textures.len());
            let (font, atlas_texture) = renderer.load_font(bytes, font_size, atlas_texture_id)?;

            self.textures.push(atlas_texture);
            self.fonts.insert(name.clone(), font);
            tracing::info!("Loaded font '{}' (lazy atlas, {}px)", name, font_size);
        }
        Ok(())
    }

    /// Get a font by name.
    pub fn font(&self, name: &str) -> Option<&Font> {
        self.fonts.get(name)
    }

    /// Ensure that every character in `text` has been rasterized into the
    /// font's glyph atlas, allocating and uploading new pixels as needed.
    ///
    /// **Call this in `update()` / `update_ui()` for any text you plan to
    /// draw later in the frame.** The render path itself only consumes
    /// already-prepared glyphs; characters not prepared in time will render
    /// as blank space.
    ///
    /// All GPU work is delegated to [`Renderer::prepare_text`]; this method
    /// only performs the name → font / atlas-slot lookup, so `AssetManager`
    /// itself stays decoupled from `wgpu`.
    pub fn prepare_text(&mut self, renderer: &Renderer, font_name: &str, text: &str) -> Result<()> {
        let font = self
            .fonts
            .get_mut(font_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown font: {}", font_name))?;

        let atlas_slot_idx = font.atlas_texture_id.0;
        // Borrow the atlas slot mutably; `Renderer::prepare_text` may
        // overwrite it in place if the atlas has to grow. The `TextureId`
        // we handed out at load time stays valid because it indexes into
        // the same slot in `self.textures`.
        let atlas_slot = &mut self.textures[atlas_slot_idx];
        renderer.prepare_text(font, atlas_slot, text)
    }

    /// Register a runtime-created texture under the given name and
    /// return the assigned [`TextureId`]. Use this to register textures
    /// you built via [`Renderer::create_white_pixel_texture`],
    /// [`Renderer::create_filled_circle_texture`],
    /// [`Renderer::create_ring_texture`], or
    /// [`Renderer::create_rgba_texture`] so that subsequent code can
    /// look them up by name with [`Self::texture_id`].
    ///
    /// Names live in the same flat namespace as `game.yaml`-loaded
    /// textures, so pick names that won't collide with your asset
    /// config. Re-registering a name overwrites only the lookup
    /// pointer; the previously-registered `Texture` slot remains
    /// reachable via its old `TextureId` and continues to render
    /// correctly.
    pub fn register_texture(&mut self, name: &str, texture: Texture) -> TextureId {
        let id = TextureId(self.textures.len());
        self.textures.push(texture);
        self.texture_names.insert(name.to_string(), id);
        id
    }
}
