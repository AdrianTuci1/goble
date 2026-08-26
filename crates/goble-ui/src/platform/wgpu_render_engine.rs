use crate::color::ColorU;
use crate::platform::icon_atlas::IconAtlas;
use crate::platform::text_atlas::TextAtlas;
use crate::render::{RenderCommand, Renderer};
use wgpu::util::DeviceExt;

const MAX_RECTS: usize = 4096;
const MAX_TEXT_VERTICES: usize = 8192;

/// A run of geometry produced between two clip boundaries, drawn with one
/// scissor rect. Geometry is accumulated across the whole frame into the three
/// vertex buffers, then each batch is drawn as a slice of those buffers so that
/// `queue.write_buffer` ordering stays correct (all writes happen once, before
/// the single submit).
struct Batch {
    scissor: (u32, u32, u32, u32),
    rect_start: usize,
    rect_end: usize,
    text_start: usize,
    text_end: usize,
    icon_start: usize,
    icon_end: usize,
}

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
    /// 1 = fade alpha 0 at the left edge -> 1 at the right edge.
    gradient: u32,
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
            gradient: 0,
        }
    }

    fn new_fade_right(rect: crate::geometry::RectF, color: ColorU, radius: f32) -> Self {
        Self {
            origin: [rect.origin.x, rect.origin.y],
            size: [rect.size.width, rect.size.height],
            color: color.to_linear_f32(),
            stroke_color: [0.0; 4],
            radius,
            stroke_width: 0.0,
            is_stroke: 0,
            gradient: 1,
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
            gradient: 0,
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
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
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

        let rect_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let text_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

    /// Render the command list, scaling from logical points to physical device
    /// pixels by `scale` (the window's device pixel ratio). Layout runs in
    /// logical points so UI sizes (topbar height, sidebar width, font sizes)
    /// stay consistent across HiDPI displays.
    #[allow(unused_assignments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        viewport_size: (u32, u32),
        renderer: &Renderer,
        scale: f32,
    ) {
        self.text_atlas.prepare(device, queue, renderer.commands(), scale);

        let viewport = [viewport_size.0 as f32, viewport_size.1 as f32];
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&viewport));

        let mut rect_instances: Vec<RectInstance> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut icon_vertices: Vec<TextVertex> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        let mut clip_stack: Vec<crate::geometry::RectF> = Vec::new();
        let mut prev_rect = 0usize;
        let mut prev_text = 0usize;
        let mut prev_icon = 0usize;

        // Record a batch for the geometry accumulated since the previous clip
        // boundary, drawing it with the scissor currently active.
        macro_rules! record_batch {
            () => {{
                batches.push(Batch {
                    scissor: clip_scissor(&clip_stack, scale, viewport_size),
                    rect_start: prev_rect,
                    rect_end: rect_instances.len(),
                    text_start: prev_text,
                    text_end: text_vertices.len(),
                    icon_start: prev_icon,
                    icon_end: icon_vertices.len(),
                });
                prev_rect = rect_instances.len();
                prev_text = text_vertices.len();
                prev_icon = icon_vertices.len();
            }};
        }

        for command in renderer.commands() {
            match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } => {
                    rect_instances.push(RectInstance::new_fill(
                        rect.scale(scale, scale),
                        *color,
                        *corner_radius * scale,
                    ));
                }
                RenderCommand::FillRectFadeRight {
                    rect,
                    color,
                    corner_radius,
                } => {
                    rect_instances.push(RectInstance::new_fade_right(
                        rect.scale(scale, scale),
                        *color,
                        *corner_radius * scale,
                    ));
                }
                RenderCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    corner_radius,
                } => {
                    rect_instances.push(RectInstance::new_stroke(
                        rect.scale(scale, scale),
                        *color,
                        *width * scale,
                        *corner_radius * scale,
                    ));
                }
                RenderCommand::DrawText {
                    origin,
                    text,
                    font_size,
                    color,
                    font_weight,
                    font_family,
                    max_width,
                    line_height,
                } => {
                    if let Some(entry) = self.text_atlas.entry_with_family(
                        text,
                        *font_size * scale,
                        *font_weight,
                        *font_family,
                        *max_width * scale,
                        *line_height,
                    ) {
                        let left = origin.x * scale + entry.offset[0];
                        let top = origin.y * scale + entry.offset[1];
                        let right = left + entry.size[0];
                        let bottom = top + entry.size[1];
                        let u0 = entry.uv_origin[0];
                        let v0 = entry.uv_origin[1];
                        let u1 = u0 + entry.uv_size[0];
                        let v1 = v0 + entry.uv_size[1];
                        let color = color.to_linear_f32();

                        text_vertices.extend_from_slice(&[
                            TextVertex {
                                position: [left, top],
                                uv: [u0, v0],
                                color,
                            },
                            TextVertex {
                                position: [right, top],
                                uv: [u1, v0],
                                color,
                            },
                            TextVertex {
                                position: [left, bottom],
                                uv: [u0, v1],
                                color,
                            },
                            TextVertex {
                                position: [right, bottom],
                                uv: [u1, v1],
                                color,
                            },
                        ]);
                    }
                }
                RenderCommand::DrawIcon {
                    origin,
                    name,
                    size,
                    color,
                } => {
                    const ICON_CELL: f32 = 64.0;
                    if let Some(entry) = self.icon_atlas.entry(name) {
                        let icon_scale = (*size * scale) / ICON_CELL;
                        let left = origin.x * scale + entry.offset[0] * icon_scale;
                        let top = origin.y * scale + entry.offset[1] * icon_scale;
                        let right = left + entry.size[0] * icon_scale;
                        let bottom = top + entry.size[1] * icon_scale;
                        let u0 = entry.uv_origin[0];
                        let v0 = entry.uv_origin[1];
                        let u1 = u0 + entry.uv_size[0];
                        let v1 = v0 + entry.uv_size[1];
                        let color = color.to_linear_f32();

                        icon_vertices.extend_from_slice(&[
                            TextVertex {
                                position: [left, top],
                                uv: [u0, v0],
                                color,
                            },
                            TextVertex {
                                position: [right, top],
                                uv: [u1, v0],
                                color,
                            },
                            TextVertex {
                                position: [left, bottom],
                                uv: [u0, v1],
                                color,
                            },
                            TextVertex {
                                position: [right, bottom],
                                uv: [u1, v1],
                                color,
                            },
                        ]);
                    }
                }
                RenderCommand::ClipRect(rect) => {
                    record_batch!();
                    clip_stack.push(*rect);
                }
                RenderCommand::PopClip => {
                    record_batch!();
                    clip_stack.pop();
                }
            }
        }
        record_batch!();

        let rect_total = rect_instances.len().min(MAX_RECTS);
        if rect_total > 0 {
            queue.write_buffer(
                &self.rect_instance_buffer,
                0,
                bytemuck::cast_slice(&rect_instances[..rect_total]),
            );
        }
        let text_total = text_vertices.len().min(MAX_TEXT_VERTICES);
        if text_total > 0 {
            queue.write_buffer(
                &self.text_vertex_buffer,
                0,
                bytemuck::cast_slice(&text_vertices[..text_total]),
            );
        }
        let icon_total = icon_vertices.len().min(MAX_TEXT_VERTICES);
        if icon_total > 0 {
            queue.write_buffer(
                &self.icon_vertex_buffer,
                0,
                bytemuck::cast_slice(&icon_vertices[..icon_total]),
            );
        }

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
                        // Neutral gray, matching the dark theme background (0x0e0e0e).
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.055,
                            b: 0.055,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            for batch in &batches {
                let rect_start = batch.rect_start.min(rect_total);
                let rect_end = batch.rect_end.min(rect_total).max(rect_start);
                if rect_end > rect_start {
                    let count = rect_end - rect_start;
                    let bytes = std::mem::size_of::<RectInstance>();
                    pass.set_pipeline(&self.rect_pipeline);
                    pass.set_bind_group(0, &self.rect_bind_group, &[]);
                    pass.set_index_buffer(self.rect_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    pass.set_vertex_buffer(
                        0,
                        self.rect_instance_buffer
                            .slice(((rect_start * bytes) as u64)..((rect_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(batch.scissor.0, batch.scissor.1, batch.scissor.2, batch.scissor.3);
                    pass.draw_indexed(0..6, 0, 0..count as u32);
                }

                let text_start = batch.text_start.min(text_total);
                let text_end = batch.text_end.min(text_total).max(text_start);
                if text_end > text_start {
                    let count = text_end - text_start;
                    let index_count = (count / 4) * 6;
                    let bytes = std::mem::size_of::<TextVertex>();
                    pass.set_pipeline(&self.text_pipeline);
                    pass.set_bind_group(0, &self.text_bind_group, &[]);
                    pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    pass.set_vertex_buffer(
                        0,
                        self.text_vertex_buffer
                            .slice(((text_start * bytes) as u64)..((text_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(batch.scissor.0, batch.scissor.1, batch.scissor.2, batch.scissor.3);
                    pass.draw_indexed(0..index_count as u32, 0, 0..1);
                }

                let icon_start = batch.icon_start.min(icon_total);
                let icon_end = batch.icon_end.min(icon_total).max(icon_start);
                if icon_end > icon_start {
                    let count = icon_end - icon_start;
                    let index_count = (count / 4) * 6;
                    let bytes = std::mem::size_of::<TextVertex>();
                    pass.set_pipeline(&self.text_pipeline);
                    pass.set_bind_group(0, self.icon_atlas.bind_group(), &[]);
                    pass.set_index_buffer(self.text_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    pass.set_vertex_buffer(
                        0,
                        self.icon_vertex_buffer
                            .slice(((icon_start * bytes) as u64)..((icon_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(batch.scissor.0, batch.scissor.1, batch.scissor.2, batch.scissor.3);
                    pass.draw_indexed(0..index_count as u32, 0, 0..1);
                }
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Compute the scissor rect (in physical pixels) from the active clip stack.
/// An empty stack clips to the full viewport.
fn clip_scissor(
    clip_stack: &[crate::geometry::RectF],
    scale: f32,
    viewport: (u32, u32),
) -> (u32, u32, u32, u32) {
    let vw = viewport.0 as f32;
    let vh = viewport.1 as f32;
    let mut acc: Option<crate::geometry::RectF> = None;
    for r in clip_stack {
        acc = Some(match acc {
            None => *r,
            Some(a) => intersect(a, *r),
        });
    }
    let (x0, y0, x1, y1) = match acc {
        Some(r) => (
            r.origin.x * scale,
            r.origin.y * scale,
            (r.origin.x + r.size.width) * scale,
            (r.origin.y + r.size.height) * scale,
        ),
        None => (0.0, 0.0, vw, vh),
    };
    let sx = x0.max(0.0).min(vw);
    let sy = y0.max(0.0).min(vh);
    let ex = x1.max(x0).min(vw);
    let ey = y1.max(y0).min(vh);
    (
        sx as u32,
        sy as u32,
        (ex - sx).max(0.0) as u32,
        (ey - sy).max(0.0) as u32,
    )
}

fn intersect(a: crate::geometry::RectF, b: crate::geometry::RectF) -> crate::geometry::RectF {
    let x0 = a.origin.x.max(b.origin.x);
    let y0 = a.origin.y.max(b.origin.y);
    let x1 = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let y1 = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    crate::geometry::rectf(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
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
