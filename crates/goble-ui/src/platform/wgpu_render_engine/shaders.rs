pub(super) const RECT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) stroke_color: vec4<f32>,
    @location(4) @interpolate(flat) radius: f32,
    @location(5) @interpolate(flat) stroke_width: f32,
    @location(6) @interpolate(flat) is_stroke: u32,
    @location(7) @interpolate(flat) gradient: u32,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) origin: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) stroke_color: vec4<f32>,
    @location(4) radius: f32,
    @location(5) stroke_width: f32,
    @location(6) is_stroke: u32,
    @location(7) gradient: u32,
) -> VertexOutput {
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0)
    );
    let p = corners[vertex_index];
    let world = origin + p * size;

    var out: VertexOutput;
    out.position = vec4<f32>(
        world / viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0),
        0.0,
        1.0
    );
    out.local = p * size;
    out.size = size;
    out.color = color;
    out.stroke_color = stroke_color;
    out.radius = radius;
    out.stroke_width = stroke_width;
    out.is_stroke = is_stroke;
    out.gradient = gradient;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = clamp(in.radius, 0.0, min(in.size.x, in.size.y) * 0.5);
    let half_size = in.size * 0.5;
    let q = abs(in.local - half_size) - half_size + vec2<f32>(r);
    var dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;

    let softness = 0.7;
    if in.is_stroke != 0u {
        let inner = dist + in.stroke_width;
        let stroke_alpha = (1.0 - smoothstep(0.0, softness, dist)) * smoothstep(0.0, softness, inner);
        return vec4<f32>(in.stroke_color.rgb, in.stroke_color.a * stroke_alpha);
    } else {
        let alpha = 1.0 - smoothstep(0.0, softness, dist);
        var a = in.color.a * alpha;
        if in.gradient != 0u {
            // Fade alpha 0 at the left edge -> full at the right edge.
            let t = clamp(in.local.x / max(in.size.x, 0.001), 0.0, 1.0);
            a *= t;
        }
        return vec4<f32>(in.color.rgb, a);
    }
}
"#;

pub(super) const TEXT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@group(0) @binding(1)
var atlas_texture: texture_2d<f32>;

@group(0) @binding(2)
var atlas_sampler: sampler;

@vertex
fn vs_text(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(
        position / viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0),
        0.0,
        1.0
    );
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_text(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

pub(super) const IMAGE_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@group(0) @binding(1)
var image_texture: texture_2d<f32>;

@group(0) @binding(2)
var image_sampler: sampler;

@vertex
fn vs_image(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(
        position / viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0),
        0.0,
        1.0
    );
    out.uv = uv;
    return out;
}

@fragment
fn fs_image(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(image_texture, image_sampler, in.uv);
}
"#;
