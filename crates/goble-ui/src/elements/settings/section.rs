//! The section and its group card: the page's second-level structure.
//!
//! T3's `SettingsSection` is a heading plus a `SettingsGroup`, and the group is
//! the rounded card the rows live in (`rounded-xl border border-border/60
//! bg-card/40`, rows divided by `border-border/50`, no rule at the card's own
//! edges). This module draws exactly that, one element for the heading and the
//! card together so a page cannot put a row between the two.
//!
//! The rules are painted by each row *cell* after its row: a cell knows its own
//! laid-out width, so the rule spans the card exactly and the card's first and
//! last rows get no rule.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{
    group_border, group_fill, row_rule, SETTINGS_GROUP_RADIUS, SETTINGS_ROW_H_PADDING,
    SETTINGS_SECTION_HEADER_GAP, SETTINGS_SECTION_HEADER_HEIGHT, SETTINGS_SECTION_HEADER_SIZE,
};

/// A heading and the group card under it.
pub struct SettingsSection {
    title: String,
    description: String,
    header_action: Option<Box<dyn Element>>,
    rows: Vec<Box<dyn Element>>,
    footer: Option<Box<dyn Element>>,
    /// Whether the card draws the surrounding fill and border. A section that
    /// holds a theme library draws its own surface, so it asks for a bare
    /// heading instead (T3's `variant="plain"`).
    plain: bool,
    built: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsSection {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: String::new(),
            header_action: None,
            rows: Vec::new(),
            footer: None,
            plain: false,
            built: None,
            size: None,
            origin: None,
        }
    }

    /// A sentence under the heading, for a section whose title alone does not
    /// say what its rows do.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// A button in the heading line, right-aligned (T3's `Restore device
    /// defaults` header action).
    pub fn with_header_action(mut self, action: Box<dyn Element>) -> Self {
        self.header_action = Some(action);
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

    /// A block under the rows, inside the same card (a diagnostics table, a
    /// trailing note).
    pub fn with_footer(mut self, footer: Box<dyn Element>) -> Self {
        self.footer = Some(footer);
        self
    }

    /// Skip the card: draw the heading and the rows against the page itself.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    pub fn is_plain(&self) -> bool {
        self.plain
    }

    fn build(&mut self, app: &AppContext) {
        let title = Text::new(self.title.clone())
            .with_font_size(SETTINGS_SECTION_HEADER_SIZE)
            .with_line_height(1.3)
            .with_weight(FontWeight::SemiBold)
            .with_color(app.theme.color(ColorToken::Text))
            .finish();
        let mut line: Vec<Box<dyn Element>> = vec![title];
        if let Some(action) = self.header_action.take() {
            line.push(crate::elements::Spacer::new().finish());
            line.push(action);
        }
        let line = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_children(line)
            .finish();
        let line = crate::elements::ConstrainedBox::new(line)
            .with_min_height(SETTINGS_SECTION_HEADER_HEIGHT)
            .finish();
        // The heading is inset like the rows inside the card below it: T3's
        // `px-3 sm:px-4`.
        let mut children: Vec<Box<dyn Element>> = vec![crate::elements::Container::new(line)
            .with_padding(crate::elements::EdgeInsets::new(
                0.0,
                SETTINGS_ROW_H_PADDING,
                0.0,
                SETTINGS_ROW_H_PADDING,
            ))
            .finish()];
        if !self.description.is_empty() {
            let description = Text::new(self.description.clone())
                .with_font_size(12.0)
                .with_line_height(1.5)
                .with_color(app.theme.color(ColorToken::Muted))
                .finish();
            children.push(
                crate::elements::Container::new(description)
                    .with_padding(crate::elements::EdgeInsets::new(
                        4.0,
                        SETTINGS_ROW_H_PADDING,
                        0.0,
                        SETTINGS_ROW_H_PADDING,
                    ))
                    .finish(),
            );
        }
        if !self.rows.is_empty() || self.footer.is_some() {
            let mut cells: Vec<Box<dyn Element>> = Vec::new();
            let mut rows = std::mem::take(&mut self.rows);
            let row_count = rows.len();
            for (index, row) in rows.drain(..).enumerate() {
                // A rule above every row but the first, and above the footer.
                cells.push(Box::new(RowCell {
                    row,
                    rule_above: index > 0,
                    size: None,
                    origin: None,
                }));
            }
            if let Some(footer) = self.footer.take() {
                cells.push(Box::new(RowCell {
                    row: footer,
                    rule_above: row_count > 0,
                    size: None,
                    origin: None,
                }));
            }
            let content = super::stretch_column(cells, 0.0);
            if self.plain {
                children.push(content);
            } else {
                let card = crate::elements::Container::new(content)
                    .with_background(crate::elements::Fill::Solid(group_fill(app)))
                    .with_border(
                        crate::elements::Border::all(1.0).with_border_color(group_border(app)),
                    )
                    .with_corner_radius(SETTINGS_GROUP_RADIUS)
                    .finish();
                children.push(card);
            }
        }
        let gap = if self.plain {
            8.0
        } else {
            SETTINGS_SECTION_HEADER_GAP
        };
        let root = crate::elements::Container::new(super::stretch_column(children, gap)).finish();
        self.built = Some(root);
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.built.is_none() {
            self.build(app);
        }
        self.built.as_mut().unwrap()
    }
}

/// One section child: paints its content and then, when asked, the 1px
/// `border-border/50` rule across its top edge.
struct RowCell {
    row: Box<dyn Element>,
    rule_above: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for RowCell {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.row.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.row.paint(origin, ctx, app);
        if self.rule_above {
            if let (Some(size), Some(renderer)) = (self.size, ctx.renderer.as_mut()) {
                renderer.fill_rect(rectf(origin.x, origin.y, size.x, 1.0), row_rule(app));
            }
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.row.dispatch_event(event, ctx, app)
    }
}

impl Element for SettingsSection {
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
