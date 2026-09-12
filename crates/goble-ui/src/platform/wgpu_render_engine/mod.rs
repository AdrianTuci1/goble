//! The wgpu renderer: one pass over the frame's render commands, with a
//! pipeline and vertex buffer per primitive kind.
//!
//! One module per surface of the engine: the batches the command list is cut
//! into ([`batch`]), the pipeline and buffer setup ([`engine`]), the per-frame
//! draw ([`frame`]) and the WGSL shaders they run ([`shaders`]). What the
//! surfaces share — the vertex formats and the engine's own state — lives here.

use std::collections::HashMap;

use crate::color::ColorU;
use crate::platform::icon_atlas::IconAtlas;
use crate::platform::text_atlas::TextAtlas;

mod batch;
mod engine;
mod frame;
mod shaders;

#[cfg(test)]
mod tests;

// Per-frame geometry budgets. These size the vertex and instance buffers, and
// anything past them is dropped, so they have to hold the largest frame the app
// can produce. A terminal pane is the reason they are this large: it paints one
// quad per cell, and a full-screen TUI on a big display is tens of thousands of
// cells. A run-length-encoded background keeps the rect count far below the
// text count.
const MAX_RECTS: usize = 16384;
/// Four vertices per text quad, so this is 32768 glyphs per frame.
const MAX_TEXT_VERTICES: usize = 131_072;
/// Icons are one quad each and far fewer than glyphs; they get their own
/// budget so the text budget does not allocate a buffer they cannot fill.
const MAX_ICON_VERTICES: usize = 16_384;
const MAX_IMAGES: usize = 256;

/// A cached per-source texture for [`RenderCommand::DrawImage`]. Keyed by the
/// command's `source`; only re-uploaded when `frame_seq` changes.
struct ImageTexture {
    frame_seq: u64,
    width: u32,
    height: u32,
    /// Owned so the GPU texture outlives the bind group handed to the render
    /// pass; without this the texture would be dropped (and freed) while the
    /// cached bind group still references it.
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
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
    image_pipeline: wgpu::RenderPipeline,
    image_index_buffer: wgpu::Buffer,
    image_vertex_buffer: wgpu::Buffer,
    image_bind_group_layout: wgpu::BindGroupLayout,
    image_textures: HashMap<String, ImageTexture>,
}
