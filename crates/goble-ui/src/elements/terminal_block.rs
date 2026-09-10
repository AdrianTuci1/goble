use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Fill,
    Flex, Icon, LayoutContext, MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, SizeConstraint, Spacer, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

/// Blend two colours in sRGB space, weight `t` toward `b`.
fn mix(a: ColorU, b: ColorU, t: f32) -> ColorU {
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    ColorU::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b), lerp(a.a, b.a))
}

const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 1.4;
const HEADER_FONT_SIZE: f32 = 12.0;
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 10.0;
const HEADER_SPACING: f32 = 8.0;
const BUTTON_SIZE: f32 = 24.0;
const PROMPT_PREFIX: &str = "❯ ";

/// App-owned, per-block filter state for a terminal block. Both cells live in
/// app state (`UiState.terminal_filters`) so the tray's open flag and the
/// selected filter survive the per-frame element rebuild; each terminal block
/// resolves its own entry by content key.
#[derive(Clone, Debug)]
pub struct TerminalFilter {
    pub open: Rc<RefCell<bool>>,
    pub selected: Rc<RefCell<usize>>,
}

impl Default for TerminalFilter {
    fn default() -> Self {
        Self {
            open: Rc::new(RefCell::new(false)),
            selected: Rc::new(RefCell::new(0)),
        }
    }
}

const FILTER_LABELS: &[&str] = &["All", "Commands", "Output", "Notes", "Success", "Errors"];

const FILTERS: &[( &str, fn(TerminalLineKind) -> bool)] = &[
    (FILTER_LABELS[0], |_| true),
    (FILTER_LABELS[1], |k| k == TerminalLineKind::Command),
    (FILTER_LABELS[2], |k| k == TerminalLineKind::Output),
    (FILTER_LABELS[3], |k| k == TerminalLineKind::Info),
    (FILTER_LABELS[4], |k| k == TerminalLineKind::Success),
    (FILTER_LABELS[5], |k| k == TerminalLineKind::Error),
];

/// The filter option labels, in order, shared by the per-block tray and the
/// whole-transcript filter bar.
pub fn filter_option_labels() -> &'static [&'static str] {
    FILTER_LABELS
}

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

    /// A stable per-block key (title + line text) used to look up the block's
    /// app-owned filter state in [`crate::elements::ChatView`]'s filter map.
    /// Two terminal blocks with identical content share a filter state, which
    /// is acceptable — content-derived keys reset as streaming output changes.
    pub fn filter_key(&self) -> String {
        let mut key = self.title.clone();
        for line in &self.lines {
            key.push('\n');
            key.push_str(&line.text);
        }
        key
    }

    /// The whole block as display text (the ❯ prompt is included for commands),
    /// used by the copy button.
    pub fn full_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| TerminalBlock::line_text(line))
            .collect::<Vec<_>>()
            .join("\n")
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

/// A terminal-style command block matching warp's layout: a single header row
/// with the command identity on the left and copy + filter controls on the
/// right (no icon, no literal status label — state is shown by colour), plus
/// monospaced command/output lines.
pub struct TerminalBlock {
    title: String,
    lines: Vec<TerminalLine>,
    status: Option<TerminalStatus>,
    filter: TerminalFilter,
    global_filter: Option<TerminalFilter>,
    on_copy: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
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
            filter: TerminalFilter::default(),
            global_filter: None,
            on_copy: None,
            root: None,
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

    /// Attach the app-owned per-block filter state (open flag + selected index).
    pub fn with_filter(mut self, filter: TerminalFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Attach an optional whole-transcript filter. A line is shown only if it
    /// matches this global filter *and* the block's own filter.
    pub fn with_global_filter(mut self, filter: Option<TerminalFilter>) -> Self {
        self.global_filter = filter;
        self
    }

    /// Set the copy callback; it receives the block's full display text.
    pub fn with_on_copy<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_copy = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn line_text(line: &TerminalLine) -> String {
        match line.kind {
            TerminalLineKind::Command => format!("{PROMPT_PREFIX}{}", line.text),
            _ => line.text.clone(),
        }
    }

    fn color_token(line: &TerminalLine) -> ColorToken {
        match line.kind {
            TerminalLineKind::Command | TerminalLineKind::Output => ColorToken::Text,
            TerminalLineKind::Info => ColorToken::Muted,
            TerminalLineKind::Success => ColorToken::Success,
            TerminalLineKind::Error => ColorToken::Error,
        }
    }

    fn filter_matches(selected: usize, kind: TerminalLineKind) -> bool {
        FILTERS
            .get(selected)
            .map(|(_, pred)| pred(kind))
            .unwrap_or(true)
    }

    /// Whether a line passes both the block's own filter and, if present, the
    /// whole-transcript filter.
    fn line_shown(
        block_selected: usize,
        global_selected: Option<usize>,
        kind: TerminalLineKind,
    ) -> bool {
        let block_ok = Self::filter_matches(block_selected, kind);
        let global_ok = global_selected.map_or(true, |g| Self::filter_matches(g, kind));
        block_ok && global_ok
    }

    /// Colour for the block identity in the header, conveying state without a
    /// literal label: accent while running, error colour on failure, muted
    /// otherwise.
    const fn title_color(status: Option<TerminalStatus>) -> ColorToken {
        match status {
            Some(TerminalStatus::Running) => ColorToken::Accent,
            Some(TerminalStatus::Error) => ColorToken::Error,
            _ => ColorToken::Muted,
        }
    }

    fn content_width(&self, max_text_width: f32) -> f32 {
        self.lines
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
            .fold(0.0, f32::max)
    }

    fn full_text(&self) -> String {
        self.lines
            .iter()
            .map(Self::line_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(crate::theme::SpacingToken::Sm);
        let radius = app.theme.radius_px();
        let muted = ColorToken::Muted;

        // Header: warp-style command-block header. The title area is
        // content-sized but the row is flex-grown so the Spacer pushes the
        // copy/filter controls to the right edge. There is no leading icon and
        // no literal status label — the block identity carries the state colour
        // and the card is tinted on failure.
        let mut header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(HEADER_SPACING)
            .with_child(
                Text::new(self.title.clone())
                    .with_theme_color(Self::title_color(self.status), app)
                    .with_font_size(HEADER_FONT_SIZE)
                    .with_font_family(FontFamily::Mono)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(Spacer::new().finish());

        // Copy button: passes the block's display text to the app's copy handler.
        let on_copy = self.on_copy.clone();
        let copy_text = self.full_text();
        let copy_button = TopbarButton::new(
            Icon::new("copy")
                .with_size(14.0)
                .with_theme_color(muted, app)
                .finish(),
        )
        .with_size(BUTTON_SIZE)
        .with_on_click(move || {
            if let Some(cb) = on_copy.as_ref() {
                (cb.borrow_mut())(copy_text.clone());
            }
        })
        .finish();
        header = header.with_child(copy_button);

        // Filter button: opens the filter tray selecting which line kinds show.
        let filter = self.filter.clone();
        let selected = *filter.selected.borrow();
        let filter_items = FILTERS
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                let mut item = PopupMenuItem::new(*label);
                if i == selected {
                    item = item.selected();
                }
                item
            })
            .collect::<Vec<_>>();
        let filter_trigger = TopbarButton::new(
            Icon::new("sliders")
                .with_size(14.0)
                .with_theme_color(muted, app)
                .finish(),
        )
        .with_size(BUTTON_SIZE)
        .finish();
        let filter_for_select = filter.clone();
        let filter_menu = PopupMenu::new(filter_trigger, filter_items)
            .with_open(filter.open.clone())
            .with_position(PopupMenuPosition::Below)
            .with_on_select(move |idx| {
                *filter_for_select.selected.borrow_mut() = idx;
            })
            .finish();
        header = header.with_child(filter_menu);

        // Body: mono lines filtered by the selected filter. The lines are
        // stretched to the resolved block width so a too-long line wraps at the
        // block edge rather than overflowing the card.
        let global_selected = self.global_filter.as_ref().map(|g| *g.selected.borrow());
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm / 2.0);
        for line in self
            .lines
            .iter()
            .filter(|l| Self::line_shown(selected, global_selected, l.kind))
        {
            let color = Self::color_token(line);
            if line.kind == TerminalLineKind::Command {
                let row = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(PROMPT_PREFIX.to_string())
                            .with_theme_color(ColorToken::Accent, app)
                            .with_font_size(FONT_SIZE)
                            .with_font_family(FontFamily::Mono)
                            .finish(),
                    )
                    .with_child(
                        Text::new(line.text.clone())
                            .with_theme_color(color, app)
                            .with_font_size(FONT_SIZE)
                            .with_font_family(FontFamily::Mono)
                            .finish(),
                    )
                    .finish();
                body = body.with_child(row);
            } else {
                body = body.with_child(
                    Text::new(line.text.clone())
                        .with_theme_color(color, app)
                        .with_font_size(FONT_SIZE)
                        .with_font_family(FontFamily::Mono)
                        .finish(),
                );
            }
        }

        // The card is tinted toward the error colour on failure (warp paints a
        // red-tinted block background) and stays default otherwise.
        let base_bg = app.theme.color(ColorToken::SurfaceRaised);
        let bg = match self.status {
            Some(TerminalStatus::Error) => mix(base_bg, app.theme.color(ColorToken::Error), 0.10),
            _ => base_bg,
        };
        let card = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm)
                .with_child(header.finish())
                .with_child(body.finish())
                .finish(),
        )
        .with_background(Fill::Solid(bg))
        .with_border(Border::all(1.0).with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))))
        .with_padding(EdgeInsets::new(PADDING_Y, PADDING_X, PADDING_Y, PADDING_X))
        .with_corner_radius(radius)
        .finish();

        // Width is offered below (in layout) via `self.constraint_width`; the
        // block stays content-adaptive. Store a placeholder width of 0 here and
        // let `layout` rebuild with the resolved width.
        self.root = Some(card);
    }

    fn resolve_width(&self, max_text_width: f32) -> f32 {
        let content_w = self.content_width(max_text_width);
        let title_w = measure_text_family(
            &self.title,
            HEADER_FONT_SIZE,
            1.2,
            max_text_width,
            FontWeight::Regular,
            FontFamily::Mono,
        )
        .x;
        // Conservative minimum for the header (title + copy + filter and their
        // spacing) so the right-aligned buttons never clip. The Spacer is
        // flexible (min 0), so we count three inter-child gaps.
        let header_min = title_w + 3.0 * HEADER_SPACING + 2.0 * BUTTON_SIZE;
        content_w.max(header_min).max(0.0)
    }
}

impl Element for TerminalBlock {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let max_text_width = (constraint.max.x - 2.0 * PADDING_X).max(0.0);
        let block_width = (self.resolve_width(max_text_width) + 2.0 * PADDING_X)
            .min(constraint.max.x);
        self.rebuild(app);
        let mut root = ConstrainedBox::new(self.root.take().expect("rebuild set root"))
            .with_width(block_width);
        let size = root.layout(constraint, ctx, app);
        self.root = Some(Box::new(root));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::AppContext;
    use crate::geometry::vec2f;
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
        // The header shows the block identity (no icon, no status label).
        let text: Vec<&String> = commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            text.iter().any(|t| t.as_str() == "zsh"),
            "the header should draw the block title, got {text:?}"
        );
        assert!(
            text.iter().any(|t| t.as_str() == "cargo test"),
            "the command line should be drawn, got {text:?}"
        );
        assert!(
            text.iter().any(|t| t.as_str() == "warning: unused variable"),
            "the error line should be drawn, got {text:?}"
        );
    }

    #[test]
    fn terminal_copy_fires_with_block_text() {
        let app = app();
        let copied = Rc::new(RefCell::new(String::new()));
        let copied_clone = copied.clone();
        let mut block = TerminalBlock::new()
            .with_title("zsh")
            .with_line(TerminalLine::command("ls"))
            .with_line(TerminalLine::output("file.txt"))
            .with_on_copy(move |text| *copied_clone.borrow_mut() = text);
        block.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        block.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        // Compute the copy button's center (right-aligned, left of the filter
        // button) and click it.
        let size = block.size.unwrap();
        let copy_center = vec2f(size.x - PADDING_X - BUTTON_SIZE - HEADER_SPACING - BUTTON_SIZE / 2.0, PADDING_Y + BUTTON_SIZE / 2.0);
        let mut event_ctx = crate::elements::EventContext::default();
        let down = DispatchedEvent::MouseDown { position: copy_center, button: 0 };
        let up = DispatchedEvent::MouseUp { position: copy_center, button: 0 };
        assert!(block.dispatch_event(&down, &mut event_ctx, &app));
        assert!(block.dispatch_event(&up, &mut event_ctx, &app));
        let text = copied.borrow();
        assert!(text.contains("file.txt"), "copy should pass the block text, got {text:?}");
    }

    #[test]
    fn terminal_filter_hides_mismatched_lines() {
        let app = app();
        let filter = TerminalFilter::default();
        *filter.selected.borrow_mut() = 5; // Errors only
        let mut block = TerminalBlock::new()
            .with_title("zsh")
            .with_line(TerminalLine::command("cargo test"))
            .with_line(TerminalLine::success("ok"))
            .with_line(TerminalLine::error("boom"))
            .with_filter(filter);
        block.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .unwrap()
            .commands()
            .to_vec();
        let text: Vec<&String> = commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            !text.iter().any(|t| t.as_str() == "cargo test"),
            "filtered-out command line should not be drawn"
        );
        assert!(!text.iter().any(|t| t.as_str() == "ok"));
        assert!(
            text.iter().any(|t| t.as_str() == "boom"),
            "the matching error line should still be drawn"
        );
    }

    #[test]
    fn terminal_global_filter_composes_with_block_filter() {
        let app = app();
        let global = TerminalFilter::default();
        *global.selected.borrow_mut() = 5; // Errors only
        let mut block = TerminalBlock::new()
            .with_title("zsh")
            .with_line(TerminalLine::command("cargo test"))
            .with_line(TerminalLine::output("compiling"))
            .with_line(TerminalLine::error("boom"))
            .with_global_filter(Some(global));
        block.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .unwrap()
            .commands()
            .to_vec();
        let text: Vec<&String> = commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            !text.iter().any(|t| t.as_str() == "cargo test"),
            "global Errors filter should hide the command line"
        );
        assert!(!text.iter().any(|t| t.as_str() == "compiling"));
        assert!(
            text.iter().any(|t| t.as_str() == "boom"),
            "the error line should still be drawn under the global filter"
        );
    }
}
