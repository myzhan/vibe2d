// The default sprite fragment shader.
//
// The vertex stage, texture/sampler bindings and the per-shader uniform block
// live in `shader_common.wgsl`; the engine concatenates that preamble with this
// file to build the built-in pipeline's full module. Custom game shaders supply
// their own fragment source in place of this file (same preamble), which is why
// the vertex layout and binding groups must stay identical across every
// pipeline.
//
// Games do *not* have to use `uniforms` here — an unused binding is valid as
// long as it is declared in the (shared) bind group layout.

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_texture, t_sampler, in.tex_coords);
    return tex_color * in.color;
}
