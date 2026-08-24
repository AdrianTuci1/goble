use std::collections::HashMap;
use std::sync::OnceLock;

use crate::render::RenderCommand;
use crate::theme::FontFamily;

const ATLAS_SIZE: u32 = 2048;
const PADDING: u32 = 4;

/// Bundled Roboto font weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Bold,
    SemiBold,
}

/// Real text metrics returned by [`measure_text`].
#[derive(Clone, Copy, Debug)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

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
    weight: FontWeight,
    mono: bool,
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
                if let RenderCommand::DrawText {
                    text,
                    font_size,
                    font_weight,
                    font_family,
                    ..
                } = command
                {
                    Some(TextKey {
                        text: text.clone(),
                        font_size: (*font_size).round() as u32,
                        weight: *font_weight,
                        mono: *font_family == FontFamily::Mono,
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
            if let Some((entry, data, width, height)) =
                rasterize_text(&key.text, key.font_size, key.weight, key.mono)
            {
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
                let uv_size = [
                    width as f32 / ATLAS_SIZE as f32,
                    height as f32 / ATLAS_SIZE as f32,
                ];
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

    pub fn entry(&self, text: &str, font_size: f32, weight: FontWeight) -> Option<&AtlasEntry> {
        self.entry_with_family(text, font_size, weight, FontFamily::System)
    }

    pub fn entry_with_family(
        &self,
        text: &str,
        font_size: f32,
        weight: FontWeight,
        family: FontFamily,
    ) -> Option<&AtlasEntry> {
        let key = TextKey {
            text: text.to_string(),
            font_size: font_size.round() as u32,
            weight,
            mono: family == FontFamily::Mono,
        };
        self.entries.get(&key)
    }
}

/// Measures a single-line or wrapped text block using the bundled Roboto fonts.
///
/// This is the source of truth for layout in elements such as [`crate::elements::Text`].
/// If the bundled fonts cannot be loaded it returns a conservative heuristic so that
/// layout never panics.
pub fn measure_text(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
) -> crate::geometry::Vector2F {
    measure_text_family(
        text,
        font_size,
        line_height,
        max_width,
        weight,
        FontFamily::System,
    )
}

/// Like [`measure_text`] but using an explicit font family (e.g. mono for terminals).
pub fn measure_text_family(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
    family: FontFamily,
) -> crate::geometry::Vector2F {
    let Some(font_set) = font_set() else {
        return estimate_text_size(text, font_size, line_height, max_width);
    };
    let font = font_set.select(weight, family);
    let fonts = &[font.clone()];
    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    let settings = if max_width.is_finite() && max_width > 0.0 {
        fontdue::layout::LayoutSettings {
            max_width: Some(max_width),
            max_height: None,
            ..Default::default()
        }
    } else {
        fontdue::layout::LayoutSettings::default()
    };
    layout.reset(&settings);
    layout.append(fonts, &fontdue::layout::TextStyle::new(text, font_size, 0));

    if layout.glyphs().is_empty() {
        return crate::geometry::vec2f(0.0, font_size * line_height);
    }

    let width = layout
        .glyphs()
        .iter()
        .map(|g| g.x + g.width as f32)
        .fold(0.0, f32::max)
        .ceil();
    let height = layout.height().ceil().max(font_size * line_height);
    crate::geometry::vec2f(width, height)
}

fn estimate_text_size(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> crate::geometry::Vector2F {
    const APPROX_CHAR_WIDTH_RATIO: f32 = 0.55;
    if text.is_empty() {
        return crate::geometry::vec2f(0.0, font_size * line_height);
    }
    let char_width = font_size * APPROX_CHAR_WIDTH_RATIO;
    let full_width = text.chars().count() as f32 * char_width;
    if full_width <= max_width || max_width.is_infinite() || max_width <= 0.0 {
        return crate::geometry::vec2f(full_width, font_size * line_height);
    }
    let chars_per_line = (max_width / char_width).max(1.0) as usize;
    let total_chars = text.chars().count();
    let raw_lines = (total_chars + chars_per_line - 1) / chars_per_line.max(1);
    let line_count = raw_lines.max(1);
    let width = (chars_per_line as f32 * char_width).min(full_width);
    crate::geometry::vec2f(width, font_size * line_height * line_count as f32)
}

struct FontSet {
    regular: fontdue::Font,
    medium: fontdue::Font,
    bold: fontdue::Font,
    mono: fontdue::Font,
    mono_bold: fontdue::Font,
}

impl FontSet {
    fn select(&self, weight: FontWeight, family: FontFamily) -> &fontdue::Font {
        match family {
            FontFamily::Mono => match weight {
                FontWeight::Regular | FontWeight::Medium => &self.mono,
                FontWeight::Bold | FontWeight::SemiBold => &self.mono_bold,
            },
            FontFamily::System | FontFamily::Serif => match weight {
                FontWeight::Regular => &self.regular,
                FontWeight::Medium => &self.medium,
                FontWeight::Bold | FontWeight::SemiBold => &self.bold,
            },
        }
    }
}

fn font_set() -> Option<&'static FontSet> {
    static FONTS: OnceLock<Option<FontSet>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let regular = load_bundled_font(FontWeight::Regular, FontFamily::System)?;
            let medium = load_bundled_font(FontWeight::Medium, FontFamily::System)
                .unwrap_or_else(|| regular.clone());
            let bold = load_bundled_font(FontWeight::Bold, FontFamily::System)
                .unwrap_or_else(|| regular.clone());
            let mono = load_bundled_font(FontWeight::Regular, FontFamily::Mono)
                .unwrap_or_else(|| regular.clone());
            let mono_bold = load_bundled_font(FontWeight::Bold, FontFamily::Mono)
                .unwrap_or_else(|| mono.clone());
            Some(FontSet {
                regular,
                medium,
                bold,
                mono,
                mono_bold,
            })
        })
        .as_ref()
}

fn load_bundled_font(weight: FontWeight, family: FontFamily) -> Option<fontdue::Font> {
    let bytes: &[u8] = match (family, weight) {
        (FontFamily::Mono, FontWeight::Bold | FontWeight::SemiBold) => {
            include_bytes!("../../assets/fonts/hack/Hack-Bold.ttf")
        }
        (FontFamily::Mono, _) => include_bytes!("../../assets/fonts/hack/Hack-Regular.ttf"),
        (_, FontWeight::Regular) => include_bytes!("../../assets/fonts/roboto/Roboto-Regular.ttf"),
        (_, FontWeight::Medium) => include_bytes!("../../assets/fonts/roboto/Roboto-Medium.ttf"),
        (_, FontWeight::Bold) => include_bytes!("../../assets/fonts/roboto/Roboto-Bold.ttf"),
        (_, FontWeight::SemiBold) => {
            include_bytes!("../../assets/fonts/roboto/RobotoFlex-Semibold.ttf")
        }
    };
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
}

fn rasterize_text(
    text: &str,
    font_size: u32,
    weight: FontWeight,
    mono: bool,
) -> Option<(AtlasEntry, Vec<u8>, u32, u32)> {
    let font_set = font_set()?;
    let family = if mono {
        FontFamily::Mono
    } else {
        FontFamily::System
    };
    let font = font_set.select(weight, family);
    let fonts = &[font.clone()];
    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    layout.reset(&fontdue::layout::LayoutSettings {
        max_width: None,
        max_height: None,
        ..Default::default()
    });
    layout.append(
        fonts,
        &fontdue::layout::TextStyle::new(text, font_size as f32, 0),
    );

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
