use crate::color::ColorU;
use crate::geometry::Vector2F;
use crate::platform::text_atlas::FontWeight;
use crate::theme::FontFamily;

/// A 2D renderer command emitted by elements during paint.
#[derive(Clone, Debug)]
pub enum RenderCommand {
    FillRect {
        rect: crate::geometry::RectF,
        color: ColorU,
        corner_radius: f32,
    },
    /// A fill whose alpha ramps 0 -> 1 from left to right (used to fade text
    /// out near an edge; `color` is the opaque right-side color).
    FillRectFadeRight {
        rect: crate::geometry::RectF,
        color: ColorU,
        corner_radius: f32,
    },
    StrokeRect {
        rect: crate::geometry::RectF,
        color: ColorU,
        width: f32,
        corner_radius: f32,
    },
    DrawText {
        origin: Vector2F,
        text: String,
        font_size: f32,
        color: ColorU,
        max_width: f32,
        line_height: f32,
        font_weight: FontWeight,
        font_family: FontFamily,
    },
    DrawIcon {
        origin: Vector2F,
        name: String,
        size: f32,
        color: ColorU,
    },
    ClipRect(crate::geometry::RectF),
    PopClip,
}

/// A 2D renderer that accumulates render commands.
///
/// In a real windowing backend these commands are translated into a wgpu
/// render pass (solid quads, rounded corners via SDF, text textures, icon
/// textures). Offline tests can inspect the command list.
#[derive(Default)]
pub struct Renderer {
    commands: Vec<RenderCommand>,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn fill_rect(&mut self, rect: crate::geometry::RectF, color: ColorU) {
        self.commands.push(RenderCommand::FillRect {
            rect,
            color,
            corner_radius: 0.0,
        });
    }

    pub fn fill_rounded_rect(
        &mut self,
        rect: crate::geometry::RectF,
        color: ColorU,
        corner_radius: f32,
    ) {
        self.commands.push(RenderCommand::FillRect {
            rect,
            color,
            corner_radius,
        });
    }

    /// Fills a rect whose alpha ramps from fully transparent (left) to fully
    /// opaque (right). Used to fade content out near the right edge of a row.
    pub fn fill_rect_fade_right(
        &mut self,
        rect: crate::geometry::RectF,
        color: ColorU,
        corner_radius: f32,
    ) {
        self.commands.push(RenderCommand::FillRectFadeRight {
            rect,
            color,
            corner_radius,
        });
    }

    pub fn stroke_rect(
        &mut self,
        rect: crate::geometry::RectF,
        color: ColorU,
        width: f32,
        corner_radius: f32,
    ) {
        self.commands.push(RenderCommand::StrokeRect {
            rect,
            color,
            width,
            corner_radius,
        });
    }

    pub fn draw_text(
        &mut self,
        origin: Vector2F,
        text: impl Into<String>,
        font_size: f32,
        color: ColorU,
        max_width: f32,
        line_height: f32,
    ) {
        self.commands.push(RenderCommand::DrawText {
            origin,
            text: text.into(),
            font_size,
            color,
            max_width,
            line_height,
            font_weight: FontWeight::Regular,
            font_family: FontFamily::System,
        });
    }

    pub fn draw_text_weighted(
        &mut self,
        origin: Vector2F,
        text: impl Into<String>,
        font_size: f32,
        color: ColorU,
        max_width: f32,
        font_weight: FontWeight,
    ) {
        self.draw_text_with_font(
            origin,
            text,
            font_size,
            color,
            max_width,
            1.2,
            font_weight,
            FontFamily::System,
        )
    }

    /// Draw text with an explicit font family (e.g. `FontFamily::Mono` for terminal output).
    pub fn draw_text_with_font(
        &mut self,
        origin: Vector2F,
        text: impl Into<String>,
        font_size: f32,
        color: ColorU,
        max_width: f32,
        line_height: f32,
        font_weight: FontWeight,
        font_family: FontFamily,
    ) {
        self.commands.push(RenderCommand::DrawText {
            origin,
            text: text.into(),
            font_size,
            color,
            max_width,
            line_height,
            font_weight,
            font_family,
        });
    }

    pub fn draw_icon(
        &mut self,
        origin: Vector2F,
        name: impl Into<String>,
        size: f32,
        color: ColorU,
    ) {
        self.commands.push(RenderCommand::DrawIcon {
            origin,
            name: name.into(),
            size,
            color,
        });
    }

    pub fn clip_rect(&mut self, rect: crate::geometry::RectF) {
        self.commands.push(RenderCommand::ClipRect(rect));
    }

    pub fn pop_clip(&mut self) {
        self.commands.push(RenderCommand::PopClip);
    }

    pub fn commands(&self) -> &[RenderCommand] {
        &self.commands
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}
