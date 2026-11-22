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

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{
    group_border, group_fill, row_rule, SETTINGS_FOLD_HEADER_HEIGHT, SETTINGS_GROUP_RADIUS,
    SETTINGS_ROW_H_PADDING, SETTINGS_SECTION_HEADER_GAP, SETTINGS_SECTION_HEADER_HEIGHT,
    SETTINGS_SECTION_HEADER_SIZE,
};

type ToggleCallback = Rc<RefCell<dyn FnMut()>>;

/// A section heading that folds its rows away: DSH/T3's `FoldedSettingsSection`.
///
/// The whole heading line is the trigger — a chevron, the title and a muted
/// summary, on the same hover band the fold element uses — and the group card is
/// drawn only while the section is open. A folded section keeps its heading in
/// the page's flow, so a list of per-item groups reads as a list of headers until
/// one is opened.
#[derive(Clone)]
pub struct FoldHeader {
    pub open: bool,
    pub on_toggle: Option<ToggleCallback>,
}

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
    /// The fold's open state and toggle callback; `None` is an ordinary,
    /// always-open section.
    fold: Option<FoldHeader>,
    /// The DSH fold header's compact type (14/500) instead of the page
    /// section's 16/600.
    compact: bool,
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
            fold: None,
            compact: false,
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

    /// Draw the section as a fold: the heading becomes the trigger and the rows
    /// are drawn only while `open`. The section's description moves into the
    /// heading line as the fold's muted summary.
    pub fn with_fold(mut self, open: bool) -> Self {
        self.fold = Some(FoldHeader {
            open,
            on_toggle: None,
        });
        self
    }

    pub fn with_on_fold_toggle<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        if let Some(fold) = self.fold.as_mut() {
            fold.on_toggle = Some(Rc::new(RefCell::new(callback)));
        }
        self
    }

    /// The fold header's compact type: 14/500 rather than the page's 16/600.
    pub fn compact_header(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Whether the fold is open (always `true` for an ordinary section).
    fn is_open(&self) -> bool {
        self.fold.as_ref().map(|fold| fold.open).unwrap_or(true)
    }

    /// The heading line: optional fold chevron, the title, a fold's summary and
    /// the section's right-aligned header action.
    fn heading(&mut self, app: &AppContext) -> Box<dyn Element> {
        let fold_open = self.fold.as_ref().map(|fold| fold.open);
        let (size, weight) = if self.compact || fold_open.is_some() {
            (14.0, FontWeight::Medium)
        } else {
            (SETTINGS_SECTION_HEADER_SIZE, FontWeight::SemiBold)
        };
        let mut line: Vec<Box<dyn Element>> = Vec::new();
        if let Some(open) = fold_open {
            line.push(
                crate::elements::Icon::new(if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                })
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            );
        }
        line.push(
            Text::new(self.title.clone())
                .with_font_size(size)
                .with_line_height(1.3)
                .with_weight(weight)
                .with_color(app.theme.color(ColorToken::Text))
                .finish(),
        );
        // A fold's description is its summary in the heading line; an ordinary
        // section's description sits under the heading.
        if fold_open.is_some() && !self.description.is_empty() {
            line.push(
                Text::new(self.description.clone())
                    .with_font_size(12.0)
                    .with_line_height(1.4)
                    .with_color(app.theme.color(ColorToken::Muted))
                    .finish(),
            );
        }
        if let Some(action) = self.header_action.take() {
            line.push(crate::elements::Spacer::new().finish());
            line.push(action);
        }
        let line = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_children(line)
            .finish();
        let min_height = if fold_open.is_some() {
            SETTINGS_FOLD_HEADER_HEIGHT
        } else {
            SETTINGS_SECTION_HEADER_HEIGHT
        };
        crate::elements::ConstrainedBox::new(line)
            .with_min_height(min_height)
            .finish()
    }

    fn build(&mut self, app: &AppContext) {
        let fold_open = self.is_open();
        let heading = self.heading(app);

        // The heading is inset like the rows inside the card below it: T3's
        // `px-3 sm:px-4`.
        let heading = crate::elements::Container::new(heading)
            .with_padding(crate::elements::EdgeInsets::new(
                0.0,
                SETTINGS_ROW_H_PADDING,
                0.0,
                SETTINGS_ROW_H_PADDING,
            ))
            .finish();
        let heading: Box<dyn Element> = if let Some(fold) = self.fold.as_ref() {
            // The whole heading is the fold's trigger, on T3's hover band.
            let on_toggle = fold.on_toggle.clone();
            crate::elements::HoverRow::new(heading)
                .with_padding(crate::elements::EdgeInsets::new(0.0, 0.0, 0.0, 0.0))
                .with_corner_radius(8.0)
                .with_on_click(move || {
                    if let Some(callback) = on_toggle.as_ref() {
                        (callback.borrow_mut())();
                    }
                })
                .finish()
        } else {
            heading
        };
        let mut children: Vec<Box<dyn Element>> = vec![heading];
        if self.fold.is_none() && !self.description.is_empty() {
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
        if fold_open && (!self.rows.is_empty() || self.footer.is_some()) {
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
        } else if self.fold.is_some() {
            6.0
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
        // L: settings carry no row separator rules; the card casing and the
        // row spacing do the grouping.
        let _ = (self.rule_above, self.size, app);
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
