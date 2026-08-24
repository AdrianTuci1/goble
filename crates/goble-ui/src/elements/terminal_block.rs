use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 1.4;
const HEADER_FONT_SIZE: f32 = 12.0;
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 10.0;
const HEADER_HEIGHT: f32 = 20.0;
const ICON_SIZE: f32 = 14.0;
const PROMPT_PREFIX: &str = "❯ ";

/// How a terminal line should be styled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalLineKind {
    Command,
    Output,
    Info,
    Success,
    Error,
}

/// A single line inside a [`TerminalBlock`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalLine {
    pub text: String,
    pub kind: TerminalLineKind,
}

impl TerminalLine {
    pub fn command(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Command,
        }
    }

    pub fn output(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Output,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Info,
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Success,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Error,
        }
    }
}

/// Serializable data for embedding a terminal block inside a chat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalData {
    pub title: String,
    pub lines: Vec<TerminalLine>,
    pub status: Option<TerminalStatus>,
}

impl TerminalData {
    pub fn new(title: impl Into<String>, lines: Vec<TerminalLine>) -> Self {
        Self {
            title: title.into(),
            lines,
            status: None,
        }
    }

    pub fn with_status(mut self, status: TerminalStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// Optional status shown in the top-right corner of the terminal header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TerminalStatus {
    #[default]
    Idle,
    Running,
    Success,
    Error,
}

impl TerminalStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Running => "running",
            Self::Success => "done",
            Self::Error => "error",
        }
    }

    pub fn color(self, app: &AppContext) -> ColorU {
        match self {
            Self::Idle => app.theme.color(ColorToken::Muted),
            Self::Running => app.theme.color(ColorToken::Accent),
            Self::Success => app.theme.color(ColorToken::Success),
            Self::Error => app.theme.color(ColorToken::Error),
        }
    }
}

/// A terminal-style block: dark rounded surface, a header row with the
/// terminal icon and title, an optional status label, and monospaced lines.
pub struct TerminalBlock {
    title: String,
    lines: Vec<TerminalLine>,
    status: Option<TerminalStatus>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Default for TerminalBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalBlock {
    pub fn new() -> Self {
        Self {
            title: "Terminal".to_string(),
            lines: Vec::new(),
            status: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_lines(mut self, lines: impl IntoIterator<Item = TerminalLine>) -> Self {
        self.lines.extend(lines);
        self
    }

    pub fn with_line(mut self, line: TerminalLine) -> Self {
        self.lines.push(line);
        self
    }

    pub fn with_status(mut self, status: TerminalStatus) -> Self {
        self.status = Some(status);
        self
    }

    fn line_text(line: &TerminalLine) -> String {
        match line.kind {
            TerminalLineKind::Command => format!("{PROMPT_PREFIX}{}", line.text),
            _ => line.text.clone(),
        }
    }

    fn line_color(line: &TerminalLine, app: &AppContext) -> ColorU {
        match line.kind {
            TerminalLineKind::Command | TerminalLineKind::Output => {
                app.theme.color(ColorToken::Text)
            }
            TerminalLineKind::Info => app.theme.color(ColorToken::Muted),
            TerminalLineKind::Success => app.theme.color(ColorToken::Success),
            TerminalLineKind::Error => app.theme.color(ColorToken::Error),
        }
    }
}

impl Element for TerminalBlock {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let max_text_width = (constraint.max.x - 2.0 * PADDING_X).max(0.0);
        let content_width = self
            .lines
            .iter()
            .map(|line| {
                measure_text_family(
                    &Self::line_text(line),
                    FONT_SIZE,
                    LINE_HEIGHT,
                    max_text_width,
                    FontWeight::Regular,
                    FontFamily::Mono,
                )
                .x
            })
            .fold(0.0, f32::max);

        let header_width = ICON_SIZE
            + 6.0
            + measure_text_family(
                &self.title,
                HEADER_FONT_SIZE,
                1.2,
                max_text_width,
                FontWeight::Regular,
                FontFamily::System,
            )
            .x;

        let width = (content_width.max(header_width) + 2.0 * PADDING_X).min(constraint.max.x);
        let lines_height = self
            .lines
            .iter()
            .map(|line| {
                measure_text_family(
                    &Self::line_text(line),
                    FONT_SIZE,
                    LINE_HEIGHT,
                    (width - 2.0 * PADDING_X).max(0.0),
                    FontWeight::Regular,
                    FontFamily::Mono,
                )
                .y
            })
            .sum::<f32>();
        let height = (PADDING_Y + HEADER_HEIGHT + lines_height + PADDING_Y).min(constraint.max.y);

        let size = vec2f(width, height.max(0.0));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = RectF::new(PointF::new(origin.x, origin.y), Size2F::new(size.x, size.y));

        let surface = app.theme.color(ColorToken::Surface);
        let border = app.theme.color(ColorToken::Border);
        let muted = app.theme.color(ColorToken::Muted);

        renderer.fill_rounded_rect(rect, surface, app.theme.radius_px());
        renderer.stroke_rect(rect, border, 1.0, app.theme.radius_px());

        // Header: terminal icon + title (+ status on the right).
        let header_y = origin.y + PADDING_Y;
        renderer.draw_icon(
            vec2f(origin.x + PADDING_X, header_y),
            "terminal",
            ICON_SIZE,
            muted,
        );
        renderer.draw_text_with_font(
            vec2f(origin.x + PADDING_X + ICON_SIZE + 6.0, header_y),
            self.title.clone(),
            HEADER_FONT_SIZE,
            muted,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
        );

        if let Some(status) = self.status {
            let label = status.label();
            if !label.is_empty() {
                let status_width = measure_text_family(
                    label,
                    HEADER_FONT_SIZE,
                    1.2,
                    f32::INFINITY,
                    FontWeight::Regular,
                    FontFamily::System,
                )
                .x;
                renderer.draw_text_with_font(
                    vec2f(origin.x + size.x - PADDING_X - status_width, header_y),
                    label.to_string(),
                    HEADER_FONT_SIZE,
                    status.color(app),
                    f32::INFINITY,
                    FontWeight::Regular,
                    FontFamily::System,
                );
            }
        }

        // Body lines in mono.
        let mut y = header_y + HEADER_HEIGHT;
        let max_text_width = (size.x - 2.0 * PADDING_X).max(0.0);
        for line in &self.lines {
            let color = Self::line_color(line, app);
            match line.kind {
                TerminalLineKind::Command => {
                    let prefix_width = measure_text_family(
                        PROMPT_PREFIX,
                        FONT_SIZE,
                        LINE_HEIGHT,
                        f32::INFINITY,
                        FontWeight::Regular,
                        FontFamily::Mono,
                    )
                    .x;
                    renderer.draw_text_with_font(
                        vec2f(origin.x + PADDING_X, y),
                        PROMPT_PREFIX.to_string(),
                        FONT_SIZE,
                        app.theme.color(ColorToken::Accent),
                        f32::INFINITY,
                        FontWeight::Regular,
                        FontFamily::Mono,
                    );
                    renderer.draw_text_with_font(
                        vec2f(origin.x + PADDING_X + prefix_width, y),
                        line.text.clone(),
                        FONT_SIZE,
                        color,
                        (max_text_width - prefix_width).max(0.0),
                        FontWeight::Regular,
                        FontFamily::Mono,
                    );
                }
                _ => {
                    renderer.draw_text_with_font(
                        vec2f(origin.x + PADDING_X, y),
                        line.text.clone(),
                        FONT_SIZE,
                        color,
                        max_text_width,
                        FontWeight::Regular,
                        FontFamily::Mono,
                    );
                }
            }
            let line_height = measure_text_family(
                &Self::line_text(line),
                FONT_SIZE,
                LINE_HEIGHT,
                max_text_width,
                FontWeight::Regular,
                FontFamily::Mono,
            )
            .y;
            y += line_height;
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::AppContext;
    use crate::render::Renderer;

    fn app() -> AppContext {
        AppContext::default()
    }

    #[test]
    fn terminal_block_measures_non_zero() {
        let app = app();
        let mut block = TerminalBlock::new()
            .with_title("npm run build")
            .with_line(TerminalLine::command("npm run build"))
            .with_line(TerminalLine::output("Compiled successfully in 1.2s"))
            .with_status(TerminalStatus::Success);
        let size = block.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
        assert!(size.x <= 600.0);
    }

    #[test]
    fn terminal_block_paints_background_header_and_lines() {
        let app = app();
        let mut block = TerminalBlock::new()
            .with_title("zsh")
            .with_line(TerminalLine::command("cargo test"))
            .with_line(TerminalLine::success("test result: ok. 42 passed"))
            .with_line(TerminalLine::error("warning: unused variable"))
            .with_status(TerminalStatus::Running);
        block.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        block.paint(vec2f(10.0, 20.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, crate::render::RenderCommand::FillRect { .. })),
            "terminal block should paint a background"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "terminal")),
            "terminal block should draw the terminal icon"
        );
        let text_count = commands
            .iter()
            .filter(|c| matches!(c, crate::render::RenderCommand::DrawText { .. }))
            .count();
        assert!(
            text_count >= 4,
            "title, status and lines should be drawn, got {text_count}"
        );
    }
}
