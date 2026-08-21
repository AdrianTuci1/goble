use std::collections::HashMap;
use std::sync::OnceLock;

use crate::render::RenderCommand;

const ATLAS_SIZE: u32 = 2048;
const PADDING: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct AtlasEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub size: [f32; 2],
    pub offset: [f32; 2],
}

pub struct TextAtlas {
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    texture_data: Vec<u8>,
    entries: HashMap<TextKey, AtlasEntry>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
struct TextKey {
    text: String,
    font_size: u32,
}

impl TextAtlas {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("goble-ui text atlas"),
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
            label: Some("goble-ui text sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("goble-ui text bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("goble-ui text bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let texture_data = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];
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
            texture_data,
            entries: HashMap::new(),
            cursor_x: PADDING,
            cursor_y: PADDING,
            row_height: 0,
        }
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn texture_view(&self) -> &wgpu::TextureView {
        &self.texture_view
    }

    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    pub fn prepare(
        &mut self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[RenderCommand],
    ) {
        let keys: Vec<TextKey> = commands
            .iter()
            .filter_map(|command| {
                if let RenderCommand::DrawText { text, font_size, .. } = command {
                    Some(TextKey {
                        text: text.clone(),
                        font_size: (*font_size).round() as u32,
                    })
                } else {
                    None
                }
            })
            .collect();

        for key in keys {
            if self.entries.contains_key(&key) {
                continue;
            }
            if let Some((entry, data, width, height)) = rasterize_text(&key.text, key.font_size) {
                if self.cursor_x + width + PADDING > ATLAS_SIZE {
                    self.cursor_x = PADDING;
                    self.cursor_y += self.row_height + PADDING;
                    self.row_height = 0;
                }
                if self.cursor_y + height + PADDING > ATLAS_SIZE {
                    log::warn!("text atlas exhausted");
                    continue;
                }

                let x = self.cursor_x;
                let y = self.cursor_y;
                self.cursor_x += width + PADDING;
                self.row_height = self.row_height.max(height);

                let uv_origin = [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32];
                let uv_size = [width as f32 / ATLAS_SIZE as f32, height as f32 / ATLAS_SIZE as f32];
                let entry = AtlasEntry {
                    uv_origin,
                    uv_size,
                    size: [width as f32, height as f32],
                    offset: entry.offset,
                };
                self.entries.insert(key, entry);

                self.write_region(queue, x, y, width, height, &data);
            }
        }
    }

    fn write_region(
        &mut self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    ) {
        for row in 0..height {
            let src_offset = (row * width) as usize;
            let dst_offset = ((y + row) * ATLAS_SIZE + x) as usize;
            self.texture_data[dst_offset..dst_offset + width as usize]
                .copy_from_slice(&data[src_offset..src_offset + width as usize]);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn entry(&self, text: &str, font_size: f32) -> Option<&AtlasEntry> {
        let key = TextKey {
            text: text.to_string(),
            font_size: font_size.round() as u32,
        };
        self.entries.get(&key)
    }
}

fn system_font() -> Option<&'static fontdue::Font> {
    static FONT: OnceLock<Option<fontdue::Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        let source = font_kit::source::SystemSource::new();
        let family = source.select_family_by_name("Helvetica").ok()?;
        let font = family.fonts().first()?.load().ok()?;
        let data = font.copy_font_data()?;
        let bytes: &[u8] = data.as_ref().as_slice();
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
    })
    .as_ref()
}

fn rasterize_text(text: &str, font_size: u32) -> Option<(AtlasEntry, Vec<u8>, u32, u32)> {
    let font = system_font()?;
    let fonts = &[font.clone()];
    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    layout.reset(&fontdue::layout::LayoutSettings {
        max_width: None,
        max_height: None,
        ..Default::default()
    });
    layout.append(fonts, &fontdue::layout::TextStyle::new(text, font_size as f32, 0));

    let glyphs = layout.glyphs();
    if glyphs.is_empty() {
        let entry = AtlasEntry {
            uv_origin: [0.0; 2],
            uv_size: [0.0; 2],
            size: [1.0, font_size as f32],
            offset: [0.0; 2],
        };
        return Some((entry, vec![0u8; font_size as usize], 1, font_size));
    }

    // Compute the bounding box that contains every glyph's rasterized bitmap.
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for glyph in glyphs {
        let (metrics, _) = font.rasterize_config(glyph.key);
        let glyph_left = glyph.x + metrics.xmin as f32;
        let glyph_top = glyph.y + metrics.ymin as f32;
        let glyph_right = glyph_left + metrics.width as f32;
        let glyph_bottom = glyph_top + metrics.height as f32;

        min_x = min_x.min(glyph_left);
        min_y = min_y.min(glyph_top);
        max_x = max_x.max(glyph_right);
        max_y = max_y.max(glyph_bottom);
    }

    let width = ((max_x - min_x).ceil() as u32).max(1);
    let height = ((max_y - min_y).ceil() as u32).max(1);

    let mut atlas = vec![0u8; (width * height) as usize];
    for glyph in glyphs {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let x_offset = (glyph.x + metrics.xmin as f32 - min_x).floor() as i32;
        let y_offset = (glyph.y + metrics.ymin as f32 - min_y).floor() as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let src = row * metrics.width + col;
                let dst_x = x_offset + col as i32;
                let dst_y = y_offset + row as i32;
                if dst_x < 0 || dst_y < 0 || dst_x >= width as i32 || dst_y >= height as i32 {
                    continue;
                }
                let dst = (dst_y * width as i32 + dst_x) as usize;
                atlas[dst] = atlas[dst].saturating_add(bitmap[src]);
            }
        }
    }

    let entry = AtlasEntry {
        uv_origin: [0.0; 2],
        uv_size: [0.0; 2],
        size: [width as f32, height as f32],
        offset: [-min_x, -min_y],
    };

    Some((entry, atlas, width, height))
}
