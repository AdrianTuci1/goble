use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use fontdue::{Font, FontSettings, Metrics};
use glam::Mat4;
use wgpu::util::DeviceExt;
use wgpu::{include_wgsl, Adapter, Device, Queue, Surface, SurfaceConfiguration};
use winit::window::Window;

use crate::scene::{Color, Fill, Glyph, Layer, Rect, RectF, Scene};

const ATLAS_SIZE: u32 = 1024;

pub struct Renderer {
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub adapter: Adapter,
    pub window: std::sync::Arc<Window>,
    pub render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    font_atlas: Mutex<FontAtlas>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    transform: [[f32; 4]; 4],
}

impl Globals {
    fn new(width: f32, height: f32) -> Self {
        Self {
            transform: Mat4::orthographic_lh(0.0, width, height, 0.0, -1.0, 1.0).to_cols_array_2d(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    uv: [f32; 2],
    kind: f32,
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
        2 => Float32x2,
        3 => Float32,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphKey {
    index: u16,
    size_px: u16,
}

#[derive(Clone, Copy, Debug)]
struct AtlasGlyph {
    uv: RectF,
    metrics: Metrics,
}

struct ShelfPacker {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    row_height: u32,
}

impl ShelfPacker {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            x: 0,
            y: 0,
            row_height: 0,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.width || h > self.height {
            return None;
        }
        if self.x + w > self.width {
            self.x = 0;
            self.y += self.row_height;
            self.row_height = 0;
        }
        if self.y + h > self.height {
            return None;
        }
        let pos = (self.x, self.y);
        self.x += w;
        self.row_height = self.row_height.max(h);
        Some(pos)
    }
}

struct FontAtlas {
    font: Font,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    glyphs: HashMap<GlyphKey, AtlasGlyph>,
    packer: ShelfPacker,
    size: u32,
}

impl FontAtlas {
    fn new(device: &Device, font: Font) -> Self {
        let size = ATLAS_SIZE;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font-atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("font-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("font-atlas-bind-group-layout"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        Self {
            font,
            texture,
            texture_view,
            sampler,
            bind_group_layout,
            glyphs: HashMap::new(),
            packer: ShelfPacker::new(size, size),
            size,
        }
    }

    fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    fn prepare(&mut self, queue: &Queue, index: u16, size_px: u16) -> Option<AtlasGlyph> {
        let key = GlyphKey { index, size_px };
        if let Some(g) = self.glyphs.get(&key) {
            return Some(*g);
        }

        let (metrics, bitmap) = self.font.rasterize_indexed(index, size_px as f32);
        if metrics.width == 0 || metrics.height == 0 {
            // Cache a zero-size glyph so we do not re-rasterize it.
            let glyph = AtlasGlyph {
                uv: RectF::new(0.0, 0.0, 0.0, 0.0),
                metrics,
            };
            self.glyphs.insert(key, glyph);
            return Some(glyph);
        }

        let (x, y) = self
            .packer
            .allocate(metrics.width as u32, metrics.height as u32)?;

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(metrics.width as u32),
                rows_per_image: Some(metrics.height as u32),
            },
            wgpu::Extent3d {
                width: metrics.width as u32,
                height: metrics.height as u32,
                depth_or_array_layers: 1,
            },
        );

        let uv = RectF::new(
            x as f32 / self.size as f32,
            y as f32 / self.size as f32,
            metrics.width as f32 / self.size as f32,
            metrics.height as f32 / self.size as f32,
        );
        let glyph = AtlasGlyph { uv, metrics };
        self.glyphs.insert(key, glyph);
        Some(glyph)
    }
}

#[derive(Clone, Debug)]
enum CmdKind {
    Rect(Rect),
    Glyph(Glyph),
}

#[derive(Clone, Debug)]
struct DrawCmd {
    z_index: i32,
    clip: Option<RectF>,
    kind: CmdKind,
}

impl Renderer {
    pub fn new(window: std::sync::Arc<Window>) -> Result<Self> {
        pollster::block_on(Self::new_async(window))
    }

    pub async fn new_async(window: std::sync::Arc<Window>) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("failed to find adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: Some("goble-device"),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("failed to request device")?;

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let font_bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/DejaVuSans.ttf"
        ))
        .to_vec();
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(|e| anyhow::anyhow!("failed to load font: {}", e))?;
        let font_atlas = FontAtlas::new(&device, font);
        let bind_group_layout = font_atlas.layout().clone();

        let shader = device.create_shader_module(include_wgsl!("shader.wgsl"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("goble-pipeline-layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("goble-render-pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter,
            window,
            render_pipeline,
            bind_group_layout,
            font_atlas: Mutex::new(font_atlas),
        })
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&self, scene: &Scene) -> Result<()> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render-encoder"),
            });

        let globals = Globals::new(self.config.width as f32, self.config.height as f32);
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("globals-uniform"),
                contents: bytemuck::cast_slice(&[globals]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let mut atlas = self.font_atlas.lock().unwrap();
        let mut cmds = Vec::new();
        collect_layer(&scene.layers, None, &mut cmds);
        cmds.sort_by_key(|c| c.z_index);

        // Ensure all referenced glyphs are rasterized before building geometry.
        for cmd in &cmds {
            if let CmdKind::Glyph(g) = &cmd.kind {
                let size_px = (g.size * scene.scale_factor).round().max(1.0) as u16;
                atlas.prepare(&self.queue, g.glyph_index, size_px);
            }
        }

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas.texture_view),
                },
            ],
        });

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        let mut ranges: Vec<(Option<RectF>, u32, u32)> = Vec::new();

        for cmd in &cmds {
            let start = indices.len() as u32;
            match &cmd.kind {
                CmdKind::Rect(r) => {
                    push_rect_vertices(r, scene.scale_factor, &mut vertices, &mut indices);
                }
                CmdKind::Glyph(g) => {
                    let size_px = (g.size * scene.scale_factor).round().max(1.0) as u16;
                    if let Some(atlas_glyph) = atlas.prepare(&self.queue, g.glyph_index, size_px) {
                        push_glyph_vertices(
                            g,
                            atlas_glyph,
                            scene.scale_factor,
                            &mut vertices,
                            &mut indices,
                        );
                    }
                }
            }
            let count = indices.len() as u32 - start;
            if count > 0 {
                ranges.push((cmd.clip, start, count));
            }
        }

        drop(atlas);

        if !vertices.is_empty() {
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("vertex-buffer"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("index-buffer"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("render-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.04,
                                g: 0.04,
                                b: 0.05,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);

                for (clip, start, count) in ranges {
                    if let Some(clip) = clip {
                        if let Some(scissor) = rect_to_scissor(
                            clip,
                            scene.scale_factor,
                            self.config.width,
                            self.config.height,
                        ) {
                            render_pass
                                .set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                        } else {
                            continue;
                        }
                    } else {
                        render_pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                    }
                    render_pass.draw_indexed(start..start + count, 0, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}

fn collect_layer(layers: &[Layer], parent_clip: Option<RectF>, out: &mut Vec<DrawCmd>) {
    for layer in layers {
        let clip = match (layer.clip_bounds, parent_clip) {
            (Some(a), Some(b)) => a.intersection(b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        for rect in &layer.rects {
            out.push(DrawCmd {
                z_index: 0,
                clip,
                kind: CmdKind::Rect(rect.clone()),
            });
        }
        for glyph in &layer.glyphs {
            out.push(DrawCmd {
                z_index: glyph.position.z_index,
                clip,
                kind: CmdKind::Glyph(glyph.clone()),
            });
        }

        collect_layer(&layer.children, clip, out);
    }
}

fn push_rect_vertices(
    rect: &Rect,
    scale_factor: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
) {
    let Fill::Solid(color) = rect.background else {
        return;
    };

    let b = &rect.bounds;
    let x0 = b.x * scale_factor;
    let y0 = b.y * scale_factor;
    let x1 = x0 + b.width * scale_factor;
    let y1 = y0 + b.height * scale_factor;

    let c = color_to_array(color);
    let base = vertices.len() as u16;
    vertices.extend_from_slice(&[
        Vertex {
            position: [x0, y0],
            color: c,
            uv: [0.0, 0.0],
            kind: 0.0,
        },
        Vertex {
            position: [x1, y0],
            color: c,
            uv: [1.0, 0.0],
            kind: 0.0,
        },
        Vertex {
            position: [x0, y1],
            color: c,
            uv: [0.0, 1.0],
            kind: 0.0,
        },
        Vertex {
            position: [x1, y1],
            color: c,
            uv: [1.0, 1.0],
            kind: 0.0,
        },
    ]);
    indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
}

fn push_glyph_vertices(
    glyph: &Glyph,
    atlas: AtlasGlyph,
    scale_factor: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
) {
    if atlas.uv.width == 0.0 || atlas.uv.height == 0.0 {
        return;
    }

    let pos = glyph.position;
    let metrics = atlas.metrics;
    let x0 = pos.x * scale_factor + metrics.xmin as f32;
    let y0 = pos.y * scale_factor - metrics.ymin as f32 - metrics.height as f32;
    let x1 = x0 + metrics.width as f32;
    let y1 = y0 + metrics.height as f32;

    let c = color_to_array(glyph.color);
    let uv = atlas.uv;
    let base = vertices.len() as u16;
    vertices.extend_from_slice(&[
        Vertex {
            position: [x0, y0],
            color: c,
            uv: [uv.x, uv.y],
            kind: 1.0,
        },
        Vertex {
            position: [x1, y0],
            color: c,
            uv: [uv.x + uv.width, uv.y],
            kind: 1.0,
        },
        Vertex {
            position: [x0, y1],
            color: c,
            uv: [uv.x, uv.y + uv.height],
            kind: 1.0,
        },
        Vertex {
            position: [x1, y1],
            color: c,
            uv: [uv.x + uv.width, uv.y + uv.height],
            kind: 1.0,
        },
    ]);
    indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
}

fn color_to_array(color: Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

fn rect_to_scissor(
    rect: RectF,
    scale_factor: f32,
    surface_width: u32,
    surface_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let x0 = (rect.x * scale_factor).floor().max(0.0) as u32;
    let y0 = (rect.y * scale_factor).floor().max(0.0) as u32;
    let x1 = ((rect.x + rect.width) * scale_factor).ceil() as u32;
    let y1 = ((rect.y + rect.height) * scale_factor).ceil() as u32;

    let x1 = x1.min(surface_width);
    let y1 = y1.min(surface_height);

    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);

    if width == 0 || height == 0 {
        None
    } else {
        Some((x0, y0, width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Fill, FontId, Point, RectF};

    fn test_metrics(w: usize, h: usize) -> Metrics {
        Metrics {
            xmin: 0,
            ymin: 0,
            width: w,
            height: h,
            advance_width: w as f32,
            advance_height: 0.0,
            bounds: fontdue::OutlineBounds {
                xmin: 0.0,
                ymin: 0.0,
                width: w as f32,
                height: h as f32,
            },
        }
    }

    #[test]
    fn test_rect_vertices_solid_fill() {
        let rect = Rect::new(RectF::new(10.0, 20.0, 30.0, 40.0))
            .with_background(Fill::Solid(Color::new(1.0, 0.0, 0.0, 1.0)));
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_rect_vertices(&rect, 1.0, &mut vertices, &mut indices);

        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);

        assert_eq!(vertices[0].position, [10.0, 20.0]);
        assert_eq!(vertices[3].position, [40.0, 60.0]);
        assert_eq!(vertices[0].kind, 0.0);
    }

    #[test]
    fn test_rect_vertices_no_fill_skipped() {
        let rect = Rect::new(RectF::new(0.0, 0.0, 10.0, 10.0));
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_rect_vertices(&rect, 1.0, &mut vertices, &mut indices);
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn test_glyph_vertices_uvs() {
        let glyph = Glyph {
            font: FontId::default(),
            glyph_index: 0,
            position: Point::new(5.0, 10.0, 1),
            size: 16.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        };
        let atlas = AtlasGlyph {
            uv: RectF::new(0.25, 0.25, 0.5, 0.5),
            metrics: test_metrics(10, 12),
        };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_glyph_vertices(&glyph, atlas, 1.0, &mut vertices, &mut indices);

        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
        assert_eq!(vertices[0].kind, 1.0);
        assert_eq!(vertices[0].uv, [0.25, 0.25]);
        assert_eq!(vertices[3].uv, [0.75, 0.75]);
    }

    #[test]
    fn test_collect_scene_orders_by_z_index() {
        let mut scene = Scene::new(1.0);
        scene.push_glyph(Glyph {
            font: FontId::default(),
            glyph_index: 0,
            position: Point::new(0.0, 0.0, 5),
            size: 16.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        });
        scene.push_glyph(Glyph {
            font: FontId::default(),
            glyph_index: 0,
            position: Point::new(0.0, 0.0, 1),
            size: 16.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        });

        let mut cmds = Vec::new();
        collect_layer(&scene.layers, None, &mut cmds);
        cmds.sort_by_key(|c| c.z_index);
        assert_eq!(cmds[0].z_index, 1);
        assert_eq!(cmds[1].z_index, 5);
    }

    #[test]
    fn test_scissor_clip() {
        let r = RectF::new(10.0, 20.0, 30.0, 40.0);
        let scissor = rect_to_scissor(r, 1.0, 100, 100).unwrap();
        assert_eq!(scissor, (10, 20, 30, 40));

        let off = RectF::new(200.0, 200.0, 10.0, 10.0);
        assert!(rect_to_scissor(off, 1.0, 100, 100).is_none());
    }

    #[test]
    fn test_shelf_packer_allocates() {
        let mut packer = ShelfPacker::new(64, 64);
        assert_eq!(packer.allocate(10, 10), Some((0, 0)));
        assert_eq!(packer.allocate(10, 10), Some((10, 0)));
    }

    #[test]
    fn test_globals_ortho_matrix() {
        let g = Globals::new(800.0, 600.0);
        assert_eq!(g.transform[0][0], 2.0 / 800.0);
        assert_eq!(g.transform[1][1], -2.0 / 600.0);
        assert_eq!(g.transform[3][0], -1.0);
        assert_eq!(g.transform[3][1], 1.0);
    }
}
