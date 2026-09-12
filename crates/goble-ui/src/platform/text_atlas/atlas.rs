use std::collections::HashMap;

use crate::render::RenderCommand;
use crate::theme::FontFamily;

use super::fonts::{rasterize_text, FontWeight};

const ATLAS_SIZE: u32 = 2048;
const PADDING: u32 = 4;

/// What one attempt to place a run did.
enum Placement {
    /// The run is in the atlas.
    Done,
    /// The atlas is full: the caller may clear it and start over.
    Full,
    /// The run is larger than the atlas and can never be placed, at any level
    /// of fullness.
    TooLarge,
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
struct TextKey {    text: String,
    font_size: u32,
    weight: FontWeight,
    mono: bool,
    italic: bool,
    max_width: u32,
    /// Line-height multiplier (e.g. 1.2) encoded ×100 so the key stays hashable.
    line_height: u32,
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
        scale: f32,
    ) {
        let keys: Vec<TextKey> = commands
            .iter()
            .filter_map(|command| {
                if let RenderCommand::DrawText {
                    text,
                    font_size,
                    font_weight,
                    font_family,
                    font_italic,
                    max_width,
                    line_height,
                    ..
                } = command
                {
                    Some(TextKey {
                        text: text.clone(),
                        font_size: (*font_size * scale).round() as u32,
                        weight: *font_weight,
                        mono: *font_family == FontFamily::Mono,
                        italic: *font_italic,
                        max_width: (*max_width * scale).round() as u32,
                        line_height: (*line_height * 100.0).round() as u32,
                    })
                } else {
                    None
                }
            })
            .collect();

        // The atlas is a cache, not a ledger: when it runs out of room it is
        // cleared and filled again from this frame's commands. Without that, a
        // long-lived window would eventually fail every placement and draw no
        // text at all (the entries are never evicted on their own).
        let mut restarted = false;
        let mut index = 0;
        while index < keys.len() {
            let key = &keys[index];
            if self.entries.contains_key(key) {
                index += 1;
                continue;
            }
            match self.place(queue, key) {
                Placement::Done | Placement::TooLarge => index += 1,
                Placement::Full if !restarted => {
                    // Everything placed so far in this frame was dropped with
                    // the atlas, so the pass starts over on the empty one.
                    self.reset();
                    restarted = true;
                    index = 0;
                }
                // A key that still does not fit on an empty atlas is one that
                // cannot ever fit (`TooLarge` covers that, so this is the
                // belt-and-braces path): skip it rather than spin.
                Placement::Full => index += 1,
            }
        }
    }

    /// Rasterize and place one run, or report why it could not be placed.
    fn place(&mut self, queue: &wgpu::Queue, key: &TextKey) -> Placement {
        let Some((raster, data, width, height)) = rasterize_text(
            &key.text,
            key.font_size,
            key.weight,
            key.mono,
            key.italic,
            key.max_width as f32,
            key.line_height as f32 / 100.0,
        ) else {
            // Nothing to draw (an empty run): no entry, and no reason to retry.
            return Placement::Done;
        };

        // A run wider or taller than the atlas can never be placed, however
        // empty it is. Callers with an unbounded wrap width (terminal lines,
        // code blocks) produce those; clamping them into the texture would
        // stretch or crop them, so they are skipped instead.
        if width + PADDING > ATLAS_SIZE || height + PADDING > ATLAS_SIZE {
            return Placement::TooLarge;
        }

        if self.cursor_x + width + PADDING > ATLAS_SIZE {
            self.cursor_x = PADDING;
            self.cursor_y += self.row_height + PADDING;
            self.row_height = 0;
        }
        if self.cursor_y + height + PADDING > ATLAS_SIZE {
            return Placement::Full;
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
            offset: raster.offset,
        };
        self.entries.insert(key.clone(), entry);

        self.write_region(queue, x, y, width, height, &data);
        Placement::Done
    }

    /// Empty the atlas and start filling it from the top again. Entries are
    /// dropped with it, so the next pass re-rasterizes what the frame draws;
    /// stale pixels are never sampled, because a lookup without an entry draws
    /// nothing.
    fn reset(&mut self) {
        self.entries.clear();
        self.cursor_x = PADDING;
        self.cursor_y = PADDING;
        self.row_height = 0;
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
        // A single text run can be wider (or taller) than the atlas's usable
        // space when the caller passes an unbounded max_width (e.g. terminal
        // lines). The placement check above only wraps to a new row and does
        // not shrink such a run, so clamp the copy extent to the atlas bounds
        // here — writing past ATLAS_SIZE makes wgpu validate the copy as
        // overrunning the destination texture and panic.
        let write_width = (ATLAS_SIZE.saturating_sub(x)).min(width);
        let write_height = (ATLAS_SIZE.saturating_sub(y)).min(height);
        if write_width == 0 || write_height == 0 {
            return;
        }
        for row in 0..write_height {
            let src_offset = (row * width) as usize;
            let dst_offset = ((y + row) * ATLAS_SIZE + x) as usize;
            self.texture_data[dst_offset..dst_offset + write_width as usize]
                .copy_from_slice(&data[src_offset..src_offset + write_width as usize]);
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
                rows_per_image: Some(write_height),
            },
            wgpu::Extent3d {
                width: write_width,
                height: write_height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn entry(
        &self,
        text: &str,
        font_size: f32,
        weight: FontWeight,
        max_width: f32,
        line_height: f32,
    ) -> Option<&AtlasEntry> {
        self.entry_with_family(
            text,
            font_size,
            weight,
            FontFamily::System,
            false,
            max_width,
            line_height,
        )
    }

    pub fn entry_with_family(
        &self,
        text: &str,
        font_size: f32,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
        max_width: f32,
        line_height: f32,
    ) -> Option<&AtlasEntry> {
        let key = TextKey {
            text: text.to_string(),
            font_size: font_size.round() as u32,
            weight,
            mono: family == FontFamily::Mono,
            italic,
            max_width: max_width.round() as u32,
            line_height: (line_height * 100.0).round() as u32,
        };
        self.entries.get(&key)
    }
}
