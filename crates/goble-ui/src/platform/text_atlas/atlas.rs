use std::collections::HashMap;

use crate::render::RenderCommand;
use crate::theme::FontFamily;

use super::fonts::{rasterize_text_with_emoji, EmojiQuad, FontWeight};

/// A 16 MB single-channel atlas. It packs whole runs, so a screen full of
/// terminal output at 2x display scale is several megabytes of boxes on its
/// own; 2048² left little room for the window's chrome once a scrollback and a
/// transcript were both on screen.
pub(super) const ATLAS_SIZE: u32 = 4096;
/// Empty texels kept between two entries, and between an entry and the edge of
/// the atlas. The atlas is sampled with a linear filter, so the outermost
/// fragment of a glyph's quad can land between its own texels and the ones
/// after them; that gutter is what it lands on, and it is zero, or a
/// neighbouring glyph's ink shows up at the edge of the run as a speckle.
pub(super) const PADDING: u32 = 4;

/// What one attempt to place a run did.
pub(super) enum Placement {
    /// The run is in the atlas's pixels, in this region, from this coverage.
    Written { region: Region, data: Vec<u8> },
    /// Nothing to draw (an empty run).
    Empty,
    /// The atlas is full: the caller may clear it and start over.
    Full,
    /// The run is larger than the atlas and can never be placed, at any level
    /// of fullness.
    TooLarge,
}

/// A rectangle of atlas texels: where a run's pixels live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
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
    store: AtlasStore,
    /// The render scale the last `prepare` ran at. Runs are keyed in physical
    /// pixels, so a change of scale makes every entry unreachable, and the
    /// measurement cache the same tree was measured with is dropped with it.
    scale: f32,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub(super) struct TextKey {
    text: String,
    /// The run's size in physical pixels, as `f32` bits. It is not rounded:
    /// the rasterizer's line breaks are a function of this number, and the
    /// element measured its box with the size the layout actually holds, so a
    /// rounded size can break a run into one line more than the box it was
    /// measured into.
    font_size: u32,
    weight: FontWeight,
    mono: bool,
    italic: bool,
    /// The width the run wraps at, in physical pixels, as `f32` bits (`∞` when
    /// the caller gave no bound). Keyed exactly, for the same reason.
    max_width: u32,
    /// Line-height multiplier, as `f32` bits.
    line_height: u32,
}

/// The key a run is cached under. Callers pass already-scaled sizes, so both
/// the frame's lookup and the placement pass derive the same key from the same
/// numbers.
pub(super) fn text_key(
    text: &str,
    font_size: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
    max_width: f32,
    line_height: f32,
) -> TextKey {
    TextKey {
        text: text.to_string(),
        font_size: font_size.to_bits(),
        weight,
        mono: family == FontFamily::Mono,
        italic,
        max_width: max_width.to_bits(),
        line_height: line_height.to_bits(),
    }
}

/// The atlas without its texture: which run sits where, and the coverage those
/// runs rasterized to. Kept apart from the wgpu objects so the packing and the
/// pixels can be read back in a test, with no device.
pub(super) struct AtlasStore {
    /// The atlas's edge in texels. It is [`ATLAS_SIZE`] outside tests; a
    /// smaller store lets a test fill one in a handful of runs.
    size: u32,
    /// One byte of coverage per texel, row-major, `size` per row: the exact
    /// bytes the texture holds.
    pixels: Vec<u8>,
    entries: HashMap<TextKey, AtlasEntry>,
    /// The colour emoji each placed run draws over its coverage bitmap.
    emoji: HashMap<TextKey, Vec<EmojiQuad>>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl AtlasStore {
    pub(super) fn new() -> Self {
        Self::with_size(ATLAS_SIZE)
    }

    fn with_size(size: u32) -> Self {
        Self {
            size,
            pixels: vec![0u8; (size * size) as usize],
            entries: HashMap::new(),
            emoji: HashMap::new(),
            cursor_x: PADDING,
            cursor_y: PADDING,
            row_height: 0,
        }
    }

    /// A store small enough for a test to fill, while packing exactly like the
    /// real one.
    #[cfg(test)]
    pub(super) fn for_test(size: u32) -> Self {
        Self::with_size(size)
    }

    pub(super) fn contains(&self, key: &TextKey) -> bool {
        self.entries.contains_key(key)
    }

    pub(super) fn entry(&self, key: &TextKey) -> Option<&AtlasEntry> {
        self.entries.get(key)
    }

    pub(super) fn emoji(&self, key: &TextKey) -> Option<&[EmojiQuad]> {
        self.emoji.get(key).map(Vec::as_slice)
    }

    pub(super) fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Rasterize and place one run, or report why it could not be placed.
    pub(super) fn place(&mut self, key: &TextKey) -> Placement {
        let Some((entry, data, width, height, emoji)) = rasterize_text_with_emoji(
            &key.text,
            f32::from_bits(key.font_size),
            key.weight,
            key.mono,
            key.italic,
            f32::from_bits(key.max_width),
            f32::from_bits(key.line_height),
        ) else {
            // Nothing to draw: no entry, and no reason to retry.
            return Placement::Empty;
        };

        // A run wider or taller than the atlas can never be placed, however
        // empty it is. Callers with an unbounded wrap width (terminal lines,
        // code blocks) produce those; clamping them into the texture would
        // stretch or crop them, so they are skipped instead.
        if width + PADDING > self.size || height + PADDING > self.size {
            return Placement::TooLarge;
        }

        let Some(region) = self.next_region(width, height) else {
            return Placement::Full;
        };

        if emoji.is_empty() {
            self.emoji.remove(key);
        } else {
            self.emoji.insert(key.clone(), emoji);
        }
        self.entries.insert(
            key.clone(),
            AtlasEntry {
                uv_origin: [
                    region.x as f32 / self.size as f32,
                    region.y as f32 / self.size as f32,
                ],
                uv_size: [
                    width as f32 / self.size as f32,
                    height as f32 / self.size as f32,
                ],
                size: [width as f32, height as f32],
                offset: entry.offset,
            },
        );
        self.blit(region, width, &data);
        Placement::Written { region, data }
    }

    /// The next free region that fits a run of `width x height`, advancing the
    /// packing cursor past it. Every region keeps [`PADDING`] texels clear on
    /// all four sides, and the cursor only moves once a region is known to fit.
    fn next_region(&mut self, width: u32, height: u32) -> Option<Region> {
        let mut x = self.cursor_x;
        let mut y = self.cursor_y;
        let mut row_height = self.row_height;
        if x + width + PADDING > self.size {
            x = PADDING;
            y += row_height + PADDING;
            row_height = 0;
        }
        if y + height + PADDING > self.size {
            return None;
        }
        self.cursor_x = x + width + PADDING;
        self.cursor_y = y;
        self.row_height = row_height.max(height);
        Some(Region {
            x,
            y,
            width,
            height,
        })
    }

    /// Copy a rasterized run's bytes into the region they were placed in. Every
    /// pixel a placement owns is written, so no part of the atlas keeps the ink
    /// of an earlier pass.
    fn blit(&mut self, region: Region, src_stride: u32, data: &[u8]) {
        for row in 0..region.height {
            let src = (row * src_stride) as usize;
            let dst = ((region.y + row) * self.size + region.x) as usize;
            self.pixels[dst..dst + region.width as usize]
                .copy_from_slice(&data[src..src + region.width as usize]);
        }
    }

    /// Empty the atlas: entries are dropped, the cursor goes back to the top,
    /// and the pixels are wiped. The wipe is what keeps the gutter around a
    /// re-placed run clear — without it the previous pass's ink stays under the
    /// new runs' edges, where the sampler's filter reaches it.
    pub(super) fn reset(&mut self) {
        self.entries.clear();
        self.emoji.clear();
        self.cursor_x = PADDING;
        self.cursor_y = PADDING;
        self.row_height = 0;
        self.pixels.fill(0);
    }
}

/// Place a frame's keys into `store`, clearing it once if it fills.
///
/// Returns whether the atlas was cleared, and the runs to upload. After a
/// clear, placement continues with the keys that had not been placed yet
/// instead of starting over: the runs placed before the clear lost their
/// pixels with it, while the ones later in the frame — the window's chrome,
/// drawn over the panes — are the runs a frame cannot afford to lose. Those
/// first runs draw nothing until the next clear makes room for them again.
pub(super) fn pack(store: &mut AtlasStore, keys: &[TextKey]) -> (bool, Vec<(Region, Vec<u8>)>) {
    let mut placed = Vec::new();
    let mut restarted = false;
    let mut index = 0;
    while index < keys.len() {
        let key = &keys[index];
        if store.contains(key) {
            index += 1;
            continue;
        }
        match store.place(key) {
            Placement::Written { region, data } => {
                placed.push((region, data));
                index += 1;
            }
            Placement::Empty | Placement::TooLarge => index += 1,
            Placement::Full if !restarted => {
                store.reset();
                restarted = true;
                // The regions above were wiped with the reset: the pass on the
                // empty atlas is the one whose runs get uploaded.
                placed.clear();
            }
            // A key that still does not fit on an empty atlas is one that
            // cannot ever fit (`TooLarge` covers that, so this is the
            // belt-and-braces path): skip it rather than spin.
            Placement::Full => index += 1,
        }
    }
    (restarted, placed)
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
            store: AtlasStore::new(),
            scale: 0.0,
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
        // Runs are keyed in physical pixels, so a change of scale makes every
        // entry unreachable: the atlas refills at the new scale, and the
        // measurement cache the tree laid out against is dropped with it.
        // fontdue's metrics are scale-free (elements measure in logical
        // points), so this costs one frame of re-measurement per zoom or
        // display change rather than preventing a wrong answer.
        if self.scale != scale {
            self.scale = scale;
            super::cache::invalidate_text_measure_cache();
        }
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
                    Some(text_key(
                        text,
                        *font_size * scale,
                        *font_weight,
                        *font_family,
                        *font_italic,
                        *max_width * scale,
                        *line_height,
                    ))
                } else {
                    None
                }
            })
            .collect();

        // The atlas is a cache, not a ledger: when it runs out of room it is
        // cleared and filled again from this frame's commands. Without that, a
        // long-lived window would eventually fail every placement and draw no
        // text at all (the entries are never evicted on their own).
        let (restarted, placements) = pack(&mut self.store, &keys);
        if restarted {
            // The cleared store already holds this pass's pixels, so one
            // upload puts both the wipe and the new runs into the texture.
            self.clear_texture(queue);
        } else {
            for (region, data) in placements {
                self.upload(queue, region, &data);
            }
        }
    }

    /// Upload one placed run's coverage into the region it was given. The
    /// region is inside the atlas (the store only hands out regions that fit),
    /// so the copy needs no clamp: writing past `ATLAS_SIZE` would make wgpu
    /// reject the copy as overrunning the texture.
    fn upload(&self, queue: &wgpu::Queue, region: Region, data: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(region.width),
                rows_per_image: Some(region.height),
            },
            wgpu::Extent3d {
                width: region.width,
                height: region.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Write the whole (wiped) atlas back to the texture. A reset only touches
    /// the pixels the next pass places, so the rest of the texture has to be
    /// cleared in one go or the previous pass's ink stays under the new runs.
    fn clear_texture(&self, queue: &wgpu::Queue) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.store.pixels(),
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
        self.store.entry(&text_key(
            text,
            font_size,
            weight,
            family,
            italic,
            max_width,
            line_height,
        ))
    }

    /// The colour emoji a run draws over its coverage bitmap, if it holds any.
    pub(crate) fn emoji_with_family(
        &self,
        text: &str,
        font_size: f32,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
        max_width: f32,
        line_height: f32,
    ) -> Option<&[EmojiQuad]> {
        self.store.emoji(&text_key(
            text,
            font_size,
            weight,
            family,
            italic,
            max_width,
            line_height,
        ))
    }
}
