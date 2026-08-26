use std::collections::HashMap;

use crate::platform::text_atlas::AtlasEntry;

const ATLAS_SIZE: u32 = 512;
const CELL_SIZE: u32 = 64;
const COLUMNS: u32 = ATLAS_SIZE / CELL_SIZE;

macro_rules! icon_bytes {
    ($name:literal, $file:literal) => {
        (
            $name,
            include_bytes!(concat!("../../assets/icons/", $file)) as &[u8],
        )
    };
}

const ICON_FILES: &[(&str, &[u8])] = &[
    icon_bytes!("close", "close.svg"),
    icon_bytes!("minimize-01", "minimize-01.svg"),
    icon_bytes!("maximize-01", "maximize-01.svg"),
    icon_bytes!("menu-01", "menu-01.svg"),
    icon_bytes!("search", "search.svg"),
    icon_bytes!("bell", "bell.svg"),
    icon_bytes!("user", "user.svg"),
    icon_bytes!("user-02", "user-02.svg"),
    icon_bytes!("settings", "settings.svg"),
    icon_bytes!("gear", "gear.svg"),
    icon_bytes!("chat-dashed", "chat-dashed.svg"),
    icon_bytes!("message-chat-square", "message-chat-square.svg"),
    icon_bytes!("message-plus-square", "message-plus-square.svg"),
    icon_bytes!("new-conversation", "new-conversation.svg"),
    icon_bytes!("layers-three-01", "layers-three-01.svg"),
    icon_bytes!("users-02", "users-02.svg"),
    icon_bytes!("chevron-down", "chevron-down.svg"),
    icon_bytes!("chevron-left", "chevron-left.svg"),
    icon_bytes!("chevron-right", "chevron-right.svg"),
    icon_bytes!("plus", "plus.svg"),
    icon_bytes!("check", "check.svg"),
    icon_bytes!("x-circle", "x-circle.svg"),
    icon_bytes!("x-close", "x-close.svg"),
    icon_bytes!("cancelled", "cancelled.svg"),
    icon_bytes!("left-panel-close", "left-panel-close.svg"),
    icon_bytes!("left-panel-open", "left-panel-open.svg"),
    icon_bytes!("dots-horizontal", "dots-horizontal.svg"),
    icon_bytes!("trash-02", "trash-02.svg"),
    icon_bytes!("paperclip", "paperclip.svg"),
    icon_bytes!("send", "send.svg"),
    icon_bytes!("inbox-01", "inbox-01.svg"),
    icon_bytes!("agentmode", "agentmode.svg"),
    icon_bytes!("mic", "mic.svg"),
    icon_bytes!("key", "key.svg"),
    icon_bytes!("sliders", "sliders.svg"),
    icon_bytes!("sparkle", "sparkle.svg"),
    icon_bytes!("ai-assistant", "ai-assistant.svg"),
    icon_bytes!("copy", "copy.svg"),
    icon_bytes!("refresh", "refresh.svg"),
    icon_bytes!("terminal", "terminal.svg"),
    icon_bytes!("terminal-input", "terminal-input.svg"),
    icon_bytes!("prompt", "prompt.svg"),
    icon_bytes!("image", "image.svg"),
    icon_bytes!("code", "code.svg"),
    icon_bytes!("link", "link.svg"),
    icon_bytes!("stop", "stop.svg"),
    icon_bytes!("arrow-up", "arrow-up.svg"),
    icon_bytes!("info", "info.svg"),
    icon_bytes!("cloud-off", "cloud-off.svg"),
];

pub struct IconAtlas {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    entries: HashMap<String, AtlasEntry>,
}

impl IconAtlas {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("goble-ui icon atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
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
            label: Some("goble-ui icon sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("goble-ui icon bind group layout"),
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

        // Placeholder bind group; will be recreated after rasterization once the
        // uniform buffer handle is available from the render engine. We keep the
        // layout and texture/sampler here; the renderer supplies the uniform.
        let placeholder_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("goble-ui icon atlas placeholder uniform"),
            size: std::mem::size_of::<[f32; 2]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("goble-ui icon bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: placeholder_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let mut entries = HashMap::new();
        let mut cell_index = 0usize;
        let mut texture_data = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];

        for (name, bytes) in ICON_FILES {
            if cell_index >= (COLUMNS * COLUMNS) as usize {
                log::warn!("icon atlas full; skipping {}", name);
                break;
            }
            if let Some((image, width, height)) = rasterize_icon(bytes) {
                let col = cell_index % COLUMNS as usize;
                let row = cell_index / COLUMNS as usize;
                let base_x = col as u32 * CELL_SIZE;
                let base_y = row as u32 * CELL_SIZE;
                let offset_x = ((CELL_SIZE.saturating_sub(width)) / 2) as f32;
                let offset_y = ((CELL_SIZE.saturating_sub(height)) / 2) as f32;
                let x = base_x + offset_x as u32;
                let y = base_y + offset_y as u32;

                for row_y in 0..height {
                    let src_offset = (row_y * width) as usize;
                    let dst_offset = ((y + row_y) * ATLAS_SIZE + x) as usize;
                    texture_data[dst_offset..dst_offset + width as usize]
                        .copy_from_slice(&image[src_offset..src_offset + width as usize]);
                }

                let uv_origin = [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32];
                let uv_size = [
                    width as f32 / ATLAS_SIZE as f32,
                    height as f32 / ATLAS_SIZE as f32,
                ];
                let entry = AtlasEntry {
                    uv_origin,
                    uv_size,
                    size: [width as f32, height as f32],
                    offset: [offset_x, offset_y],
                };
                entries.insert(name.to_string(), entry);
                cell_index += 1;
            } else {
                log::warn!("failed to rasterize icon {}", name);
            }
        }

        queue.write_texture(
            texture.as_image_copy(),
            &texture_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_SIZE),
                rows_per_image: Some(ATLAS_SIZE),
            },
            wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );

        Self {
            texture,
            texture_view,
            sampler,
            bind_group,
            bind_group_layout,
            entries,
        }
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn set_uniform_bind_group(&mut self, device: &wgpu::Device, uniform_buffer: &wgpu::Buffer) {
        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("goble-ui icon bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
    }

    pub fn entry(&self, name: &str) -> Option<&AtlasEntry> {
        self.entries.get(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }
}

fn rasterize_icon(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(data, &options).ok()?;

    let size = tree.size();
    let scale = CELL_SIZE as f32 / size.width().max(size.height());
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let alpha: Vec<u8> = pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect();
    Some((alpha, width, height))
}
