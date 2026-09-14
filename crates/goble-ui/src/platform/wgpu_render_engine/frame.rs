use crate::render::{RenderCommand, Renderer};

use super::batch::{clip_scissor, starts_new_batch, Primitive};
use super::{
    RectInstance, TextVertex, WgpuRenderEngine, MAX_ICON_VERTICES, MAX_IMAGES, MAX_RECTS,
    MAX_TEXT_VERTICES,
};

/// One image quad to draw: the start vertex into the image vertex buffer plus
/// the bind group for its source texture. Images are drawn individually because
/// each source has its own texture (so they cannot share one bind group).
struct ImageDraw {
    vertex_index: usize,
    bind_group: wgpu::BindGroup,
}

/// A run of geometry produced between two clip boundaries or two primitive
/// kinds, drawn with one scissor rect. Geometry is accumulated across the whole
/// frame into the four vertex buffers, then each batch is drawn as a slice of
/// those buffers so that `queue.write_buffer` ordering stays correct (all writes
/// happen once, before the single submit).
struct Batch {
    scissor: (u32, u32, u32, u32),
    rect_start: usize,
    rect_end: usize,
    text_start: usize,
    text_end: usize,
    icon_start: usize,
    icon_end: usize,
    image_start: usize,
    image_end: usize,
}

impl WgpuRenderEngine {
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
        self.text_atlas
            .prepare(device, queue, renderer.commands(), scale);

        let viewport = [viewport_size.0 as f32, viewport_size.1 as f32];
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&viewport));

        let mut rect_instances: Vec<RectInstance> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut icon_vertices: Vec<TextVertex> = Vec::new();
        let mut image_vertices: Vec<TextVertex> = Vec::new();
        let mut image_draws: Vec<ImageDraw> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        let mut clip_stack: Vec<crate::geometry::RectF> = Vec::new();
        let mut prev_rect = 0usize;
        let mut prev_text = 0usize;
        let mut prev_icon = 0usize;
        let mut prev_image = 0usize;

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
                    image_start: prev_image,
                    image_end: image_draws.len(),
                });
                prev_rect = rect_instances.len();
                prev_text = text_vertices.len();
                prev_icon = icon_vertices.len();
                prev_image = image_draws.len();
            }};
        }

        // A batch draws its rects, then its text, then its icons and images, so
        // geometry of different kinds must not share one batch: a panel painted
        // after a label has to cover it (a tray over the surface below it).
        // Close the batch whenever the incoming command changes kind, which
        // makes the batches — and therefore the draw calls — follow the command
        // order exactly.
        let mut current: Option<Primitive> = None;

        for command in renderer.commands() {
            if let Some(kind) = starts_new_batch(current, command) {
                record_batch!();
                current = Some(kind);
            }
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
                    font_italic,
                    max_width,
                    line_height,
                } => {
                    if let Some(entry) = self.text_atlas.entry_with_family(
                        text,
                        *font_size * scale,
                        *font_weight,
                        *font_family,
                        *font_italic,
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
                RenderCommand::DrawImage {
                    rect,
                    source,
                    width,
                    height,
                    frame_seq,
                    data,
                } => {
                    if *width == 0 || *height == 0 {
                        continue;
                    }
                    let scaled = rect.scale(scale, scale);
                    let left = scaled.origin.x;
                    let top = scaled.origin.y;
                    let right = left + scaled.size.width;
                    let bottom = top + scaled.size.height;
                    let white = [1.0, 1.0, 1.0, 1.0];
                    let vertex_index = image_vertices.len();
                    image_vertices.extend_from_slice(&[
                        TextVertex {
                            position: [left, top],
                            uv: [0.0, 0.0],
                            color: white,
                        },
                        TextVertex {
                            position: [right, top],
                            uv: [1.0, 0.0],
                            color: white,
                        },
                        TextVertex {
                            position: [left, bottom],
                            uv: [0.0, 1.0],
                            color: white,
                        },
                        TextVertex {
                            position: [right, bottom],
                            uv: [1.0, 1.0],
                            color: white,
                        },
                    ]);
                    let bind_group = self.ensure_image_texture(
                        device,
                        queue,
                        source,
                        *width,
                        *height,
                        *frame_seq,
                        std::sync::Arc::clone(data),
                    );
                    image_draws.push(ImageDraw {
                        vertex_index,
                        bind_group,
                    });
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
        if rect_instances.len() > MAX_RECTS {
            log::warn!(
                "frame drew {} rects, the budget is {MAX_RECTS}; the rest were dropped",
                rect_instances.len()
            );
        }
        if rect_total > 0 {
            queue.write_buffer(
                &self.rect_instance_buffer,
                0,
                bytemuck::cast_slice(&rect_instances[..rect_total]),
            );
        }
        let text_total = text_vertices.len().min(MAX_TEXT_VERTICES);
        if text_vertices.len() > MAX_TEXT_VERTICES {
            log::warn!(
                "frame drew {} text vertices, the budget is {MAX_TEXT_VERTICES}; the rest were dropped",
                text_vertices.len()
            );
        }
        if text_total > 0 {
            queue.write_buffer(
                &self.text_vertex_buffer,
                0,
                bytemuck::cast_slice(&text_vertices[..text_total]),
            );
        }
        let icon_total = icon_vertices.len().min(MAX_ICON_VERTICES);
        if icon_total > 0 {
            queue.write_buffer(
                &self.icon_vertex_buffer,
                0,
                bytemuck::cast_slice(&icon_vertices[..icon_total]),
            );
        }
        let image_vertex_total = image_vertices.len().min(MAX_IMAGES * 4);
        if image_vertex_total > 0 {
            queue.write_buffer(
                &self.image_vertex_buffer,
                0,
                bytemuck::cast_slice(&image_vertices[..image_vertex_total]),
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
                    pass.set_index_buffer(
                        self.rect_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.set_vertex_buffer(
                        0,
                        self.rect_instance_buffer
                            .slice(((rect_start * bytes) as u64)..((rect_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(
                        batch.scissor.0,
                        batch.scissor.1,
                        batch.scissor.2,
                        batch.scissor.3,
                    );
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
                    pass.set_index_buffer(
                        self.text_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.set_vertex_buffer(
                        0,
                        self.text_vertex_buffer
                            .slice(((text_start * bytes) as u64)..((text_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(
                        batch.scissor.0,
                        batch.scissor.1,
                        batch.scissor.2,
                        batch.scissor.3,
                    );
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
                    pass.set_index_buffer(
                        self.text_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.set_vertex_buffer(
                        0,
                        self.icon_vertex_buffer
                            .slice(((icon_start * bytes) as u64)..((icon_end * bytes) as u64)),
                    );
                    pass.set_scissor_rect(
                        batch.scissor.0,
                        batch.scissor.1,
                        batch.scissor.2,
                        batch.scissor.3,
                    );
                    pass.draw_indexed(0..index_count as u32, 0, 0..1);
                }

                // Images are drawn one quad at a time because every source has
                // its own texture (and thus its own bind group). The vertex
                // index is an absolute offset into the image vertex buffer.
                let image_start = batch.image_start.min(image_draws.len());
                let image_end = batch.image_end.min(image_draws.len()).max(image_start);
                if image_end > image_start {
                    let bytes = std::mem::size_of::<TextVertex>();
                    pass.set_pipeline(&self.image_pipeline);
                    pass.set_index_buffer(
                        self.image_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    for draw in &image_draws[image_start..image_end] {
                        let v = draw.vertex_index;
                        if v + 4 > image_vertex_total {
                            continue;
                        }
                        pass.set_bind_group(0, &draw.bind_group, &[]);
                        pass.set_vertex_buffer(
                            0,
                            self.image_vertex_buffer
                                .slice(((v * bytes) as u64)..(((v + 4) * bytes) as u64)),
                        );
                        pass.set_scissor_rect(
                            batch.scissor.0,
                            batch.scissor.1,
                            batch.scissor.2,
                            batch.scissor.3,
                        );
                        pass.draw_indexed(0..6, 0, 0..1);
                    }
                }
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}
