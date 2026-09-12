use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::platform::icon_atlas::IconAtlas;
use crate::platform::text_atlas::TextAtlas;

use super::{
    ImageTexture, RectInstance, TextVertex, WgpuRenderEngine, MAX_ICON_VERTICES, MAX_IMAGES,
    MAX_RECTS, MAX_TEXT_VERTICES,
};

use super::shaders::{IMAGE_SHADER, RECT_SHADER, TEXT_SHADER};

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
            size: (std::mem::size_of::<TextVertex>() * MAX_ICON_VERTICES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Image pipeline: a textured quad rendered from an RGBA8 source. The
        // bind group layout mirrors the text layout (uniform + texture +
        // sampler), but the image fragment shader samples the full RGBA and
        // does not multiply by an alpha coverage channel.
        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("goble-ui image shader"),
            source: wgpu::ShaderSource::Wgsl(IMAGE_SHADER.into()),
        });

        let image_bind_group_layout = text_bind_group_layout.clone();

        let image_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("goble-ui image pipeline layout"),
                bind_group_layouts: &[&image_bind_group_layout],
                push_constant_ranges: &[],
            });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("goble-ui image pipeline"),
            layout: Some(&image_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: Some("vs_image"),
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
                module: &image_shader,
                entry_point: Some("fs_image"),
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

        let image_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("goble-ui image index buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let image_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui image vertex buffer"),
            size: (std::mem::size_of::<TextVertex>() * MAX_IMAGES * 4) as u64,
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
            image_pipeline,
            image_index_buffer,
            image_vertex_buffer,
            image_bind_group_layout,
            image_textures: HashMap::new(),
        }
    }

    /// Return (creating or updating on demand) the bind group for `source`'s
    /// texture. A new texture is created when the source's dimensions change;
    /// the pixels are re-uploaded only when `frame_seq` differs from what is
    /// cached, so an unchanged live frame costs one bind-group lookup.
    pub(super) fn ensure_image_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &str,
        width: u32,
        height: u32,
        frame_seq: u64,
        data: std::sync::Arc<[u8]>,
    ) -> wgpu::BindGroup {
        if let Some(entry) = self.image_textures.get(source) {
            if entry.frame_seq == frame_seq && entry.width == width && entry.height == height {
                return entry.bind_group.clone();
            }
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("goble-ui image: {source}")),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((width.max(1) * 4).min(usize::MAX as u32)),
                rows_per_image: Some(height.max(1)),
            },
            wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("goble-ui image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("goble-ui image bind group: {source}")),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        self.image_textures.insert(
            source.to_string(),
            ImageTexture {
                frame_seq,
                width,
                height,
                texture,
                bind_group: bind_group.clone(),
            },
        );
        bind_group
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
