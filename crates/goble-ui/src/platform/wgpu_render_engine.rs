use crate::color::ColorU;
use crate::platform::icon_atlas::IconAtlas;
use crate::platform::text_atlas::TextAtlas;
use crate::render::{RenderCommand, Renderer};
use wgpu::util::DeviceExt;

const MAX_RECTS: usize = 4096;
const MAX_TEXT_VERTICES: usize = 8192;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RectInstance {
    origin: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    stroke_color: [f32; 4],
    radius: f32,
    stroke_width: f32,
    is_stroke: u32,
    _pad: u32,
}

impl RectInstance {
    fn new_fill(rect: crate::geometry::RectF, color: ColorU, radius: f32) -> Self {
        Self {
            origin: [rect.origin.x, rect.origin.y],
            size: [rect.size.width, rect.size.height],
            color: color.to_linear_f32(),
            stroke_color: [0.0; 4],
            radius,
            stroke_width: 0.0,
            is_stroke: 0,
            _pad: 0,
        }
    }

    fn new_stroke(rect: crate::geometry::RectF, color: ColorU, width: f32, radius: f32) -> Self {
        Self {
            origin: [rect.origin.x, rect.origin.y],
            size: [rect.size.width, rect.size.height],
            color: [0.0; 4],
            stroke_color: color.to_linear_f32(),
            radius,
            stroke_width: width,
            is_stroke: 1,
            _pad: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TextVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub struct WgpuRenderEngine {
    rect_pipeline: wgpu::RenderPipeline,
    rect_bind_group: wgpu::BindGroup,
    rect_index_buffer: wgpu::Buffer,
    rect_instance_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    text_atlas: TextAtlas,
    text_pipeline: wgpu::RenderPipeline,
    text_bind_group: wgpu::BindGroup,
    text_index_buffer: wgpu::Buffer,
    text_vertex_buffer: wgpu::Buffer,
    icon_atlas: IconAtlas,
    icon_vertex_buffer: wgpu::Buffer,
}

impl WgpuRenderEngine {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        let rect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("goble-ui rect shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui viewport uniform"),
            size: std::mem::size_of::<[f32; 2]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let rect_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("goble-ui rect bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let rect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("goble-ui rect bind group"),
            layout: &rect_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let rect_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("goble-ui rect pipeline layout"),
            bind_group_layouts: &[&rect_bind_group_layout],
            push_constant_ranges: &[],
        });

        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("goble-ui rect pipeline"),
            layout: Some(&rect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &rect_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 32,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 48,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32,
                        },
                        wgpu::VertexAttribute {
                            offset: 52,
                            shader_location: 5,
                            format: wgpu::VertexFormat::Float32,
                        },
                        wgpu::VertexAttribute {
                            offset: 56,
                            shader_location: 6,
                            format: wgpu::VertexFormat::Uint32,
                        },
                        wgpu::VertexAttribute {
                            offset: 60,
                            shader_location: 7,
                            format: wgpu::VertexFormat::Uint32,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &rect_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let indices: [u16; 6] = [0, 1, 2, 1, 3, 2];
        let rect_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("goble-ui rect index buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let rect_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui rect instance buffer"),
            size: (std::mem::size_of::<RectInstance>() * MAX_RECTS) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let text_atlas = TextAtlas::new(device, queue);
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("goble-ui text shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER.into()),
        });

        let text_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("goble-ui text bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let text_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("goble-ui text bind group"),
            layout: &text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(text_atlas.texture_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(text_atlas.sampler()),
                },
            ],
        });

        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("goble-ui text pipeline layout"),
            bind_group_layouts: &[&text_bind_group_layout],
            push_constant_ranges: &[],
        });

        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("goble-ui text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_text"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_text"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let text_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("goble-ui text index buffer"),
            contents: bytemuck::cast_slice(&text_indices(MAX_TEXT_VERTICES / 4)),
            usage: wgpu::BufferUsages::INDEX,
        });

        let text_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui text vertex buffer"),
            size: (std::mem::size_of::<TextVertex>() * MAX_TEXT_VERTICES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut icon_atlas = IconAtlas::new(device, queue);
        icon_atlas.set_uniform_bind_group(device, &uniform_buffer);

        let icon_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui icon vertex buffer"),
            size: (std::mem::size_of::<TextVertex>() * MAX_TEXT_VERTICES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            rect_pipeline,
            rect_bind_group,
            rect_index_buffer,
            rect_instance_buffer,
            uniform_buffer,
            text_atlas,
            text_pipeline,
            text_bind_group,
            text_index_buffer,
            text_vertex_buffer,
            icon_atlas,
            icon_vertex_buffer,
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        viewport_size: (u32, u32),
        renderer: &Renderer,
    ) {
        self.text_atlas.prepare(device, queue, renderer.commands());

        let mut rect_instances: Vec<RectInstance> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut icon_vertices: Vec<TextVertex> = Vec::new();

        for command in renderer.commands() {
            match command {
                RenderCommand::FillRect { rect, color, corner_radius } => {
                    rect_instances.push(RectInstance::new_fill(*rect, *color, *corner_radius));
                }
                RenderCommand::StrokeRect { rect, color, width, corner_radius } => {
                    rect_instances.push(RectInstance::new_stroke(*rect, *color, *width, *corner_radius));
                }
                RenderCommand::DrawText { origin, text, font_size, color, font_weight, .. } => {
                    if let Some(entry) = self.text_atlas.entry(text, *font_size, *font_weight) {
                        let left = origin.x + entry.offset[0];
                        let top = origin.y + entry.offset[1];
                        let right = left + entry.size[0];
                        let bottom = top + entry.size[1];
                        let u0 = entry.uv_origin[0];
                        let v0 = entry.uv_origin[1];
                        let u1 = u0 + entry.uv_size[0];
                        let v1 = v0 + entry.uv_size[1];
                        let color = color.to_linear_f32();

                        text_vertices.extend_from_slice(&[
                            TextVertex { position: [left, top], uv: [u0, v0], color },
                            TextVertex { position: [right, top], uv: [u1, v0], color },
                            TextVertex { position: [left, bottom], uv: [u0, v1], color },
                            TextVertex { position: [right, bottom], uv: [u1, v1], color },
                        ]);
                    }
                }
                RenderCommand::DrawIcon { origin, name, size, color } => {
                    const ICON_CELL: f32 = 64.0;
                    if let Some(entry) = self.icon_atlas.entry(name) {
                        let scale = *size / ICON_CELL;
                        let left = origin.x + entry.offset[0] * scale;
                        let top = origin.y + entry.offset[1] * scale;
                        let right = left + entry.size[0] * scale;
                        let bottom = top + entry.size[1] * scale;
                        let u0 = entry.uv_origin[0];
                        let v0 = entry.uv_origin[1];
                        let u1 = u0 + entry.uv_size[0];
                        let v1 = v0 + entry.uv_size[1];
                        let color = color.to_linear_f32();

                        icon_vertices.extend_from_slice(&[
                            TextVertex { position: [left, top], uv: [u0, v0], color },
                            TextVertex { position: [right, top], uv: [u1, v0], color },
                            TextVertex { position: [left, bottom], uv: [u0, v1], color },
                            TextVertex { position: [right, bottom], uv: [u1, v1], color },
                        ]);
                    }
                }
                RenderCommand::ClipRect(_) | RenderCommand::PopClip => {
                    // TODO: implement clipping
                }
            }
        }

        let rect_count = rect_instances.len().min(MAX_RECTS);
        if rect_count > 0 {
            queue.write_buffer(
                &self.rect_instance_buffer,
                0,
                bytemuck::cast_slice(&rect_instances[..rect_count]),
            );
        }

        let text_vertex_count = text_vertices.len().min(MAX_TEXT_VERTICES);
        if text_vertex_count > 0 {
            queue.write_buffer(
                &self.text_vertex_buffer,
                0,
                bytemuck::cast_slice(&text_vertices[..text_vertex_count]),
            );
        }

        let icon_vertex_count = icon_vertices.len().min(MAX_TEXT_VERTICES);
        if icon_vertex_count > 0 {
            queue.write_buffer(
                &self.icon_vertex_buffer,
                0,
                bytemuck::cast_slice(&icon_vertices[..icon_vertex_count]),
            );
        }

        let viewport = [viewport_size.0 as f32, viewport_size.1 as f32];
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&viewport));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("goble-ui render encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("goble-ui render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.07,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if rect_count > 0 {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_bind_group(0, &self.rect_bind_group, &[]);
                pass.set_index_buffer(self.rect_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.set_vertex_buffer(0, self.rect_instance_buffer.slice(..));
                pass.draw_indexed(0..6, 0, 0..rect_count as u32);
            }

            if text_vertex_count > 0 {
                let index_count = (text_vertex_count / 4) * 6;
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &self.text_bind_group, &[]);
                pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.set_vertex_buffer(0, self.text_vertex_buffer.slice(..));
                pass.draw_indexed(0..index_count as u32, 0, 0..1);
            }

            if icon_vertex_count > 0 {
                let index_count = (icon_vertex_count / 4) * 6;
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, self.icon_atlas.bind_group(), &[]);
                pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.set_vertex_buffer(0, self.icon_vertex_buffer.slice(..));
                pass.draw_indexed(0..index_count as u32, 0, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}

fn text_indices(max_quads: usize) -> Vec<u16> {
    let mut indices = Vec::with_capacity(max_quads * 6);
    for quad in 0..max_quads {
        let base = (quad * 4) as u16;
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
    indices
}

const RECT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) stroke_color: vec4<f32>,
    @location(4) @interpolate(flat) radius: f32,
    @location(5) @interpolate(flat) stroke_width: f32,
    @location(6) @interpolate(flat) is_stroke: u32,
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
    @location(7) _pad: u32,
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
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }
}
"#;

const TEXT_SHADER: &str = r#"
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
