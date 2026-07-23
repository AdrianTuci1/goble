@group(0) @binding(0)
var<uniform> globals: Globals;

struct Globals {
    transform: mat4x4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) kind: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) kind: f32,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = globals.transform * vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    out.kind = kind;
    return out;
}

@group(0) @binding(1)
var atlas_sampler: sampler;

@group(0) @binding(2)
var atlas_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.kind < 0.5) {
        return in.color;
    }
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
