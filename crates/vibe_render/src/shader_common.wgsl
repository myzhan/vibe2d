// Shared shader preamble for every render pipeline in the engine.
//
// This is concatenated with a fragment-only source to form a complete WGSL
// module:
//   - the built-in sprite pipeline  -> preamble + sprite.wgsl
//   - a game's custom shader         -> preamble + user fragment source
//
// Keeping the vertex stage, the texture/sampler bindings and the per-shader
// uniform block in one place means custom shaders only have to provide their
// own fragment entry point (`fs_main`), exactly like Love2D's pixel shaders.
// The vertex layout (position + tex_coords + color) is fixed and shared, which
// is what lets the renderer keep batching every draw — custom or not — through
// a single vertex/index buffer.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// group(0): orthographic projection (virtual pixels -> clip space).
@group(0) @binding(0)
var<uniform> projection: mat4x4<f32>;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = projection * vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    out.color = in.color;
    return out;
}

// group(1): the sprite's texture + sampler (changed per batch).
@group(1) @binding(0)
var t_texture: texture_2d<f32>;
@group(1) @binding(1)
var t_sampler: sampler;

// group(2): per-shader uniforms (changed per shader).
//
// `time` is advanced automatically by the engine every frame (seconds since
// the renderer was created — the analogue of Love2D's `love_Time`).
// `params` / `params2` are free vec4 slots the game fills via
// `Screen::set_shader_uniforms`. The three `_pad*` floats bring the struct to a
// 16-byte multiple so the two vec4s stay aligned — the Rust `ShaderUniforms`
// mirrors this layout byte-for-byte.
struct ShaderUniforms {
    time: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    params: vec4<f32>,
    params2: vec4<f32>,
};

@group(2) @binding(0)
var<uniform> uniforms: ShaderUniforms;
