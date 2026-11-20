//! The settings status surfaces: the one-line notice a page puts above its
//! groups when the file changed on disk, an edit conflicted, or a page has
//! something honest to say about what it cannot do.
//!
//! The notice is one of T3's status rows, drawn as a tinted card: a fill of the
//! status colour at 10% over the page, a 1px border at 32%, a 12px line, and an
//! icon. The row-level equivalent is [`super::row::SettingsRowStatus`], which
//! sits under a row's description.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::ColorToken;

/// What a notice says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsNoticeKind {
    /// Neutral: where the file is, what was loaded.
    Info,
    /// The edit was written.
    Success,
    /// Something the user should read before acting.
    Warning,
    /// The edit failed or the file cannot be parsed.
    Error,
}

impl SettingsNoticeKind {
    fn token(self) -> ColorToken {
        match self {
            SettingsNoticeKind::Info => ColorToken::Focus,
            SettingsNoticeKind::Success => ColorToken::Success,
            SettingsNoticeKind::Warning => ColorToken::Warning,
            SettingsNoticeKind::Error => ColorToken::Error,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            SettingsNoticeKind::Info => "info",
            SettingsNoticeKind::Success => "check",
            SettingsNoticeKind::Warning => "info",
            SettingsNoticeKind::Error => "x-circle",
        }
    }
}

/// A full-width notice line.
pub struct SettingsNotice {
    kind: SettingsNoticeKind,
    text: String,
    action: Option<Box<dyn Element>>,
    built: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsNotice {
    pub fn new(kind: SettingsNoticeKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            action: None,
            built: None,
            size: None,
            origin: None,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(SettingsNoticeKind::Info, text)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(SettingsNoticeKind::Success, text)
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(SettingsNoticeKind::Warning, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(SettingsNoticeKind::Error, text)
    }

    /// A trailing control, e.g. the "Reload" the conflict notice offers.
    pub fn with_action(mut self, action: Box<dyn Element>) -> Self {
        self.action = Some(action);
        self
    }

    fn build(&mut self, app: &AppContext) {
        let color = app.theme.color(self.kind.token());
        let icon = crate::elements::Icon::new(self.kind.icon())
            .with_size(14.0)
            .with_color(color)
            .finish();
        let text = Text::new(self.text.clone())
            .with_font_size(12.0)
            .with_line_height(1.5)
            .with_color(app.theme.color(ColorToken::Text))
            .finish();
        let mut parts: Vec<Box<dyn Element>> = vec![
            crate::elements::Container::new(icon)
                .with_padding(crate::elements::EdgeInsets::new(1.0, 0.0, 0.0, 0.0))
                .finish(),
            crate::elements::Expanded::new(text).finish(),
        ];
        if let Some(action) = self.action.take() {
            parts.push(action);
        }
        let row = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Start)
            .with_spacing(8.0)
            .with_children(parts)
            .finish();
        let fill = app.theme.color(ColorToken::Bg).mix(&color, 0.10);
        let border = app.theme.color(ColorToken::Bg).mix(&color, 0.32);
        self.built = Some(
            crate::elements::Container::new(row)
                .with_padding(crate::elements::EdgeInsets::new(8.0, 10.0, 8.0, 10.0))
                .with_background(crate::elements::Fill::Solid(fill))
                .with_border(crate::elements::Border::all(1.0).with_border_color(border))
                .with_corner_radius(8.0)
                .finish(),
        );
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.built.is_none() {
            self.build(app);
        }
        self.built.as_mut().unwrap()
    }
}

impl Element for SettingsNotice {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root_mut(app).layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut(app).paint(origin, ctx, app);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.root_mut(app).dispatch_event(event, ctx, app)
    }
}

/// The plain status line a row or a section footer draws: no card, no icon, an
/// 11px sentence in the status colour. Used for the honest "Not available in
/// this build" line under a disabled control and for file diagnostics.
pub struct SettingsStatusLine {
    text: String,
    kind: SettingsNoticeKind,
    built: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsStatusLine {
    pub fn new(kind: SettingsNoticeKind, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind,
            built: None,
            size: None,
            origin: None,
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self::new(SettingsNoticeKind::Info, text)
    }

    /// The status line a page draws for a row it cannot perform.
    pub fn unavailable() -> Self {
        Self::muted("Not available in this build.")
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.built.is_none() {
            let color = if self.kind == SettingsNoticeKind::Info {
                app.theme.color(ColorToken::Muted)
            } else {
                app.theme.color(self.kind.token())
            };
            self.built = Some(
                Text::new(self.text.clone())
                    .with_font_size(12.0)
                    .with_line_height(1.5)
                    .with_color(color)
                    .finish(),
            );
        }
        self.built.as_mut().unwrap()
    }
}

impl Element for SettingsStatusLine {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root_mut(app).layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut(app).paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// A 1px rule in the card's own separator colour, for a page that needs a
/// divider outside a group (the diagnostics table's header).
pub struct SettingsRule {
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsRule {
    pub fn new() -> Self {
        Self {
            size: None,
            origin: None,
        }
    }
}

impl Default for SettingsRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for SettingsRule {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = Vector2F::new(constraint.max.x, 1.0);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let (Some(size), Some(renderer)) = (self.size, ctx.renderer.as_mut()) {
            renderer.fill_rect(
                rectf(origin.x, origin.y, size.x, size.y),
                super::row_rule(app),
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
