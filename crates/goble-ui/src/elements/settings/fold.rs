//! The fold: T3's `FoldedSettingsSection` idiom — a clickable header with a
//! chevron, a title and a muted summary, and a body of rows under it when open.
//!
//! A page uses it where a group is secondary (the Connections page's saved
//! servers, an advanced diagnostics block) so the row count does not hide the
//! primary settings. The chevron points right when closed and down when open,
//! the header takes a `Hover` band, and the body is a group card with the same
//! row rules as a [`super::section::SettingsSection`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{group_border, group_fill, row_rule, SETTINGS_GROUP_RADIUS, SETTINGS_ROW_H_PADDING};

type ToggleCallback = Rc<RefCell<dyn FnMut()>>;

/// A collapsible group.
pub struct SettingsFold {
    title: String,
    summary: String,
    open: bool,
    rows: Vec<Box<dyn Element>>,
    on_toggle: Option<ToggleCallback>,
    built: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsFold {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            summary: String::new(),
            open: false,
            rows: Vec::new(),
            on_toggle: None,
            built: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn with_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn with_row(mut self, row: Box<dyn Element>) -> Self {
        self.rows.push(row);
        self
    }

    pub fn with_rows(mut self, rows: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        self.rows.extend(rows);
        self
    }

    pub fn with_on_toggle<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn header(&self, app: &AppContext) -> Box<dyn Element> {
        let chevron = crate::elements::Icon::new(if self.open {
            "chevron-down"
        } else {
            "chevron-right"
        })
        .with_size(14.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish();
        let title = Text::new(self.title.clone())
            .with_font_size(14.0)
            .with_line_height(1.4)
            .with_weight(FontWeight::Medium)
            .with_color(app.theme.color(ColorToken::Text))
            .finish();
        let mut line: Vec<Box<dyn Element>> = vec![chevron, title];
        if !self.summary.is_empty() {
            line.push(
                Text::new(self.summary.clone())
                    .with_font_size(12.0)
                    .with_line_height(1.4)
                    .with_color(app.theme.color(ColorToken::Muted))
                    .finish(),
            );
        }
        let content = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_children(line)
            .finish();
        let on_toggle = self.on_toggle.clone();
        let row = crate::elements::HoverRow::new(content)
            .with_padding(crate::elements::EdgeInsets::new(
                11.0,
                SETTINGS_ROW_H_PADDING,
                11.0,
                SETTINGS_ROW_H_PADDING,
            ))
            .with_corner_radius(8.0)
            .with_on_click(move || {
                if let Some(callback) = on_toggle.as_ref() {
                    (callback.borrow_mut())();
                }
            })
            .finish();
        let row = crate::elements::ConstrainedBox::new(row)
            .with_min_height(44.0)
            .finish();
        crate::elements::Container::new(row)
            .with_padding(crate::elements::EdgeInsets::new(0.0, 4.0, 0.0, 4.0))
            .finish()
    }

    fn build(&mut self, app: &AppContext) {
        let mut children: Vec<Box<dyn Element>> = vec![self.header(app)];
        if self.open && !self.rows.is_empty() {
            children.push(
                crate::elements::Empty::new()
                    .with_size(Vector2F::new(0.0, 6.0))
                    .finish(),
            );
            let mut body: Vec<Box<dyn Element>> = Vec::new();
            let rows = std::mem::take(&mut self.rows);
            let mut first = true;
            for row in rows {
                if !first {
                    body.push(Box::new(FoldRule {
                        size: None,
                        origin: None,
                    }));
                }
                first = false;
                body.push(row);
            }
            let card = crate::elements::Container::new(super::column(body, 0.0))
                .with_background(crate::elements::Fill::Solid(group_fill(app)))
                .with_border(crate::elements::Border::all(1.0).with_border_color(group_border(app)))
                .with_corner_radius(SETTINGS_GROUP_RADIUS)
                .finish();
            children.push(card);
        }
        let root = crate::elements::Container::new(super::column(children, 0.0)).finish();
        self.built = Some(root);
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.built.is_none() {
            self.build(app);
        }
        self.built.as_mut().unwrap()
    }
}

/// A 1px rule between two rows of a fold's body.
struct FoldRule {
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for FoldRule {
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
            renderer.fill_rect(rectf(origin.x, origin.y, size.x, 1.0), row_rule(app));
        }
    }
    fn size(&self) -> Option<Vector2F> {
        self.size
    }
    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

impl Element for SettingsFold {
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
