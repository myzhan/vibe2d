//! Custom fragment shaders (Love2D-style "pixel shaders").
//!
//! The engine renders every draw through a single fixed vertex stage and a
//! shared set of bind groups (see `shader_common.wgsl`). A *shader* here is
//! nothing more than an alternative `@fragment fn fs_main` compiled against that
//! same preamble and wired into its own [`wgpu::RenderPipeline`] plus per-shader
//! uniform buffer.
//!
//! Design constraints this satisfies (see AGENTS.md):
//! - **Batching survives.** Because the vertex layout and bind group layouts are
//!   identical for every pipeline, `DrawCommand`s still queue as plain data and
//!   the sprite batcher only gains one more break key (`shader_id`). Switching
//!   shaders costs a `set_pipeline`, exactly like switching texture.
//! - **VDP determinism survives.** The main pass and the offscreen screenshot
//!   pass both run through the *same* `Renderer::execute_draw_commands`, so a
//!   custom shader renders identically in both. Uniforms are driven purely from
//!   `ShaderUniforms` (auto `time` + two `vec4` slots the game sets), never from
//!   ambient GPU state that could diverge between passes.
//! - **Minimal API.** A game just calls [`Renderer::create_fragment_shader`] with
//!   a WGSL fragment source (in `Game::new`, which receives `&Renderer`), then
//!   selects it while drawing via `Screen::set_shader`.

use std::cell::RefCell;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::Renderer;

/// The shared preamble every pipeline (built-in and custom) is built from:
/// vertex stage, texture/sampler bindings and the uniform block declaration.
/// A custom shader's source is concatenated after this.
pub const SHADER_PREAMBLE: &str = include_str!("shader_common.wgsl");

/// Opaque handle to a render pipeline.
///
/// `ShaderId(0)` is the built-in sprite pipeline (texture × color) that every
/// draw uses unless a game selects another one. Custom shaders returned by
/// [`Renderer::create_fragment_shader`] start at `1`. `Default` is `0`, so
/// `DrawCommand` can derive `Default` and untouched draws keep the built-in
/// pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ShaderId(pub usize);

impl ShaderId {
    /// The built-in sprite pipeline. Draws with this id use the default
    /// `texture * color` fragment shader and require no uniform bookkeeping.
    pub const DEFAULT: ShaderId = ShaderId(0);

    /// `true` for the built-in pipeline (id 0).
    pub fn is_default(self) -> bool {
        self.0 == 0
    }
}

/// Per-shader uniform block, mirrored byte-for-byte by the WGSL `ShaderUniforms`
/// struct in `shader_common.wgsl` (48 bytes, 16-byte aligned).
///
/// The engine rewrites [`ShaderUniforms::time`] every frame; `params` /
/// `params2` are free slots the game fills via
/// [`Screen::set_shader_uniforms`](../vibe2d/struct.Screen.html#method.set_shader_uniforms).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable, Default)]
pub struct ShaderUniforms {
    /// Seconds since the renderer was created, advanced automatically. Analogous
    /// to Love2D's `love_Time`. Games rarely need to touch it.
    pub time: f32,
    /// Explicit padding so the two `vec4`s below sit on 16-byte boundaries,
    /// matching the WGSL struct. Kept in sync with `shader_common.wgsl`.
    _pad: [f32; 3],
    /// Generic vec4 slot, read in WGSL as `uniforms.params`.
    pub params: [f32; 4],
    /// Second generic vec4 slot, read in WGSL as `uniforms.params2`.
    pub params2: [f32; 4],
}

impl ShaderUniforms {
    /// A zeroed uniform block. `time` gets filled in by the renderer each frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience: set the first `vec4` slot (e.g. a color / strength).
    pub fn with_params(mut self, params: [f32; 4]) -> Self {
        self.params = params;
        self
    }

    /// Convenience: set the second `vec4` slot (e.g. a resolution / offset).
    pub fn with_params2(mut self, params2: [f32; 4]) -> Self {
        self.params2 = params2;
        self
    }
}

/// A compiled custom shader: its own pipeline + uniform buffer + bind group,
/// plus the latest CPU-side uniforms so `time` can be merged in on write.
pub(crate) struct ShaderEntry {
    pub pipeline: wgpu::RenderPipeline,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub uniforms: ShaderUniforms,
}

/// The registry of custom shaders. Kept behind a [`RefCell`] so
/// [`Renderer::create_fragment_shader`] and
/// [`Renderer::set_shader_uniforms`] can take `&self` — `create_fragment_shader`
/// is normally called from `Game::new`, which only hands the game an immutable
/// `&Renderer`.
pub(crate) type ShaderRegistry = RefCell<Vec<ShaderEntry>>;

impl Renderer {
    /// Compile a custom fragment shader and register a pipeline for it.
    ///
    /// `fragment_source` is the **fragment-side** WGSL only — a complete
    /// `@fragment fn fs_main` (and any helper functions / `const`s it needs).
    /// It is concatenated onto the engine's shared preamble, so the following are
    /// already in scope and must NOT be redeclared:
    /// - `VertexInput` / `VertexOutput` structs and `vs_main`
    /// - `projection` (group 0), `t_texture` / `t_sampler` (group 1)
    /// - `uniforms: ShaderUniforms` (group 2), with fields `time`, `params`,
    ///   `params2`
    ///
    /// Returns a [`ShaderId`] to pass to `Screen::set_shader`. The built-in
    /// pipeline is [`ShaderId::DEFAULT`] and is always id 0.
    ///
    /// On native builds, invalid WGSL is returned as `Err` (via a wgpu
    /// validation error scope). On wasm, compilation errors instead fall through
    /// to the browser's error handler (the pop future can't be blocked there).
    pub fn create_fragment_shader(&self, label: &str, fragment_source: &str) -> Result<ShaderId> {
        let source = format!("{SHADER_PREAMBLE}\n{fragment_source}");

        // On native, capture WGSL validation errors and surface them as `Err`
        // rather than letting an invalid shader panic through wgpu's default
        // error handler. On wasm the pop future can't be blocked, so there we
        // rely on the browser/console error handler.
        #[cfg(not(target_arch = "wasm32"))]
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);

        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self.build_pipeline(label, &module);

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.device.poll(wgpu::Maintain::Wait);
            if let Some(err) = pollster::block_on(self.device.pop_error_scope()) {
                return Err(anyhow::anyhow!(
                    "fragment shader `{label}` failed to compile: {err}"
                ));
            }
        }

        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{label}_uniforms")),
                contents: bytemuck::bytes_of(&ShaderUniforms::default()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
            label: Some(&format!("{label}_uniform_bind_group")),
        });

        let mut registry = self.custom_shaders.borrow_mut();
        registry.push(ShaderEntry {
            pipeline,
            uniform_buffer,
            bind_group,
            uniforms: ShaderUniforms::default(),
        });
        // Custom shaders start at id 1 (0 is the built-in pipeline).
        Ok(ShaderId(registry.len()))
    }

    /// Update the `params` / `params2` slots of a custom shader.
    ///
    /// `time` is owned by the renderer (advanced every frame) and is preserved
    /// here. Passing [`ShaderId::DEFAULT`] is a no-op — the built-in pipeline
    /// ignores its uniform block.
    pub fn set_shader_uniforms(&self, id: ShaderId, uniforms: ShaderUniforms) {
        if id.is_default() {
            return;
        }
        let mut registry = self.custom_shaders.borrow_mut();
        if let Some(entry) = registry.get_mut(id.0 - 1) {
            // Keep the renderer-owned time; only adopt the game-supplied slots.
            entry.uniforms.params = uniforms.params;
            entry.uniforms.params2 = uniforms.params2;
            self.queue.write_buffer(
                &entry.uniform_buffer,
                0,
                bytemuck::bytes_of(&entry.uniforms),
            );
        }
    }

    /// Advance `time` on every custom shader's uniform buffer to the current
    /// wall-clock second. Called once per frame from `Renderer::render` so the
    /// main and screenshot passes observe identical uniform state.
    pub(crate) fn advance_shader_time(&self) {
        let time = self.frame_start.elapsed().as_secs_f32();
        let mut registry = self.custom_shaders.borrow_mut();
        for entry in registry.iter_mut() {
            entry.uniforms.time = time;
            self.queue.write_buffer(
                &entry.uniform_buffer,
                0,
                bytemuck::bytes_of(&entry.uniforms),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_uniforms_mirrors_wgsl_layout() {
        // Must match `struct ShaderUniforms` in shader_common.wgsl byte-for-byte:
        //   time(4) + pad(12) + params(16) + params2(16) = 48 bytes.
        // The three pad floats push `params`/`params2` onto 16-byte offsets so
        // the WGSL `vec4` members stay uniform-buffer-aligned. A mismatch here
        // silently corrupts every custom shader's uniforms, so pin it.
        assert_eq!(std::mem::size_of::<ShaderUniforms>(), 48);

        let u = ShaderUniforms::default();
        let base = &u as *const _ as usize;
        let off_params = &u.params as *const _ as usize - base;
        let off_params2 = &u.params2 as *const _ as usize - base;
        assert_eq!(off_params, 16, "params must sit at byte offset 16");
        assert_eq!(off_params2, 32, "params2 must sit at byte offset 32");
    }

    #[test]
    fn default_shader_id_is_the_builtin_pipeline() {
        assert_eq!(ShaderId::DEFAULT, ShaderId(0));
        assert_eq!(ShaderId::default(), ShaderId::DEFAULT);
        assert!(ShaderId::DEFAULT.is_default());
        assert!(ShaderId(0).is_default());
        assert!(!ShaderId(1).is_default());
    }

    #[test]
    fn uniform_builders_set_the_named_slots() {
        let u = ShaderUniforms::new()
            .with_params([1.0, 2.0, 3.0, 4.0])
            .with_params2([5.0, 6.0, 7.0, 8.0]);
        assert_eq!(u.params, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(u.params2, [5.0, 6.0, 7.0, 8.0]);
        assert_eq!(u.time, 0.0, "builder must not touch renderer-owned time");
    }

    #[test]
    fn preamble_exposes_the_shared_contract_and_no_fragment() {
        // The contract custom shaders depend on: the preamble provides the vertex
        // entry, both texture bindings and the uniform block, but must NOT define
        // `fs_main` (the game supplies exactly one — a duplicate would be a
        // redefinition error).
        assert!(SHADER_PREAMBLE.contains("fn vs_main"));
        assert!(SHADER_PREAMBLE.contains("@group(1) @binding(0)"));
        assert!(SHADER_PREAMBLE.contains("@group(2) @binding(0)"));
        assert!(SHADER_PREAMBLE.contains("struct ShaderUniforms"));
        assert!(
            !SHADER_PREAMBLE.contains("@fragment"),
            "preamble must leave the fragment entry point to the game"
        );
    }
}
