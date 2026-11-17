//! The rich input's full-width menu card: the commands the draft can run, and
//! the models a pane's turns can run, drawn in the slot above the input.
//!
//! The design follows DeepSeek Harness's composer menus
//! (`ui-input-trigger`'s `MenuView` for the slash list, `ui-model-selection`'s
//! `ModelSelect` for the model card): a rounded menu surface with a 4pt casing,
//! one source/section heading, and 34pt rows that read
//! `glyph · title · right-aligned description`, with a single shared
//! highlight that both the pointer and the keyboard move.
//!
//! The bands are not overlays and they do not take the keyboard: the editor
//! keeps the caret and the draft stays the query, so the command list narrows
//! as the user types after the `/`. The composer routes the keys that take a
//! row — Up and Down to move, Enter or Tab to take the selection, Escape to put
//! the list away — and a band draws what it was given and reports what the
//! pointer chose.
//!
//! A band is a DSH card, not a slab: it is inset to line up with the composer
//! card under it, rounded, padded and lifted by 4pt of air, so the two read as
//! one composite input. Both menus are the same band (`MenuBand`), so the two
//! can never drift apart in their geometry or in how a row answers the pointer.
//!
//! Self-drawn rather than composed from children: every row has the same fixed
//! height, so the panel's own geometry is the hit-test geometry, and the row
//! bounds `layout` computes (which has no cursor) are the ones `paint` and
//! `dispatch_event` (which have no layout pass) use.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::icon::icon_atlas_name;
use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, PopupMenuItem,
    SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, RectF, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

/// How many rows are drawn at once. A longer list is windowed around the
/// selection instead of growing up over the transcript.
pub const SLASH_MENU_ROWS: usize = 8;

/// The width the band falls back to when its constraint is unbounded. The host
/// hands it the input's own width, so this only covers a caller that measures it
/// with no width at all.
const FALLBACK_WIDTH: f32 = 360.0;

/// The card's side inset: the composer card keeps this gutter from the surface
/// it is drawn on, so the menu card lines up with it exactly.
const CARD_INSET: f32 = 12.0;
/// The casing's padding, the same 4pt the DSH menu surface keeps.
const CARD_PADDING: f32 = 4.0;
/// The air between the menu card and the input block under it (DSH's
/// `bottom: calc(100% + 4px)`).
const CARD_GAP: f32 = 4.0;
/// The radius of the menu card and of the row highlight inside it.
const CARD_RADIUS: f32 = 8.0;
const ROW_RADIUS: f32 = 6.0;

/// One row: DSH's 34pt minimum cell, its 8pt side inset and the 6pt gap between
/// the glyph, the title and the description.
const ROW_HEIGHT: f32 = 34.0;
const ROW_PAD_X: f32 = 8.0;
const ROW_GAP: f32 = 6.0;
const ICON_SIZE: f32 = 14.0;
const CHECK_SIZE: f32 = 14.0;

/// A source/section heading row: 4pt above, the 16pt line, 2pt below.
const HEADING_HEIGHT: f32 = 22.0;
const HEADING_PAD_X: f32 = 8.0;
const HEADING_PAD_TOP: f32 = 4.0;

/// The status row a list with nothing to say draws inside its card.
const STATUS_HEIGHT: f32 = 28.0;

const NAME_FONT_SIZE: f32 = 13.0;
const NAME_LINE_HEIGHT: f32 = 20.0;
const DESCRIPTION_FONT_SIZE: f32 = 12.0;
const DESCRIPTION_LINE_HEIGHT: f32 = 18.0;
const HEADING_FONT_SIZE: f32 = 11.0;
const HEADING_LINE_HEIGHT: f32 = 16.0;

/// The title keeps its own column: a long command name never runs into what the
/// description says about it.
const NAME_COLUMN_SHARE: f32 = 0.4;

/// What a draft's first character asks the input to list: the harness's
/// commands, the skills it can load, or the workspace's paths.
///
/// The reference client reads all three from the session it is attached to, and
/// opens the same band over the prompt for each. Here the prefix is the whole
/// protocol: what the host serves for it is the harness's own list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftTrigger {
    Command,
    Skill,
    Mention,
}

impl DraftTrigger {
    /// The character that opens this band.
    pub fn prefix(self) -> char {
        match self {
            DraftTrigger::Command => '/',
            DraftTrigger::Skill => '$',
            DraftTrigger::Mention => '@',
        }
    }

    /// The source heading the band draws over its rows: DSH names the command
    /// source `Commands` and the skill source `Skills`; the path source is
    /// Goble's own and reads `Paths`.
    pub fn heading(self) -> &'static str {
        match self {
            DraftTrigger::Command => "Commands",
            DraftTrigger::Skill => "Skills",
            DraftTrigger::Mention => "Paths",
        }
    }

    /// The band a draft asks for, when it starts with one of the prefixes.
    pub fn from_draft(draft: &str) -> Option<Self> {
        let trimmed = draft.trim_start();
        let mut chars = trimmed.chars();
        let first = chars.next()?;
        // What follows the prefix. A skill and a path are one word each, so a
        // space after the token means it is complete and the band belongs to
        // the prompt again — which is what keeps an inserted `$skill ` from
        // reopening its own list.
        let rest = chars.as_str();
        let one_word = rest.is_empty() || !rest.contains(char::is_whitespace);
        match first {
            // A command keeps its band open while its arguments are typed.
            '/' => Some(DraftTrigger::Command),
            '$' if one_word => Some(DraftTrigger::Skill),
            '@' if one_word => Some(DraftTrigger::Mention),
            _ => None,
        }
    }

    /// What was typed after the prefix, up to the first word: the band's query.
    pub fn query(self, draft: &str) -> String {
        let trimmed = draft.trim_start();
        let rest = trimmed.strip_prefix(self.prefix()).unwrap_or(trimmed);
        rest.split_whitespace().next().unwrap_or("").to_string()
    }
}

/// Whether a draft opens a band: it starts with one of the trigger prefixes and
/// Escape has not put this draft's list away. The host and the editor both read
/// it, so what the editor routes and what is drawn can never disagree.
pub fn slash_menu_open(draft: &str, dismissed: bool) -> bool {
    !dismissed && DraftTrigger::from_draft(draft).is_some()
}

/// One command the menu offers: the name as it is typed after the prefix, what
/// running it does, and the glyph that names its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashMenuItem {
    pub name: String,
    pub description: String,
    /// The character the row is typed with: `/` for a command, `$` for a
    /// skill, `@` for a path.
    pub prefix: char,
    /// The glyph drawn at the row's leading edge, as an icon-atlas name. `None`
    /// draws a text-only row.
    pub icon: Option<&'static str>,
}

impl SlashMenuItem {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            prefix: '/',
            icon: None,
        }
    }

    /// Draw this row under another prefix: the same list element serves the
    /// skills and the paths.
    pub fn with_prefix(mut self, prefix: char) -> Self {
        self.prefix = prefix;
        self
    }

    /// Draw this row with a glyph at its leading edge.
    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The command as the row draws it: the name with the prefix it is typed
    /// with.
    pub fn typed(&self) -> String {
        format!("{}{}", self.prefix, self.name)
    }
}

/// One row of a band: what it reads, the muted description that ends the row on
/// the right, whether it is the value in force (which draws a trailing check,
/// the DSH selection marker), and its glyph.
struct BandRow {
    label: String,
    description: String,
    current: bool,
    icon: Option<&'static str>,
}

/// One drawn row: the row it shows, where it sits relative to the band's
/// origin, and its own pointer state.
struct DrawnRow {
    row: usize,
    rect: RectF,
    state: InteractiveState,
}

/// A band: the card's rows, the window around the selection, and the callbacks
/// a row reports to.
///
/// Both menus are this element — see the module docs — so the geometry, the hit
/// testing and the paint live here once.
struct MenuBand {
    rows: Vec<BandRow>,
    /// The source/section heading over the rows. `None` draws no heading. The
    /// two bands that share this element must both carry one (or neither): the
    /// host draws them in one slot and their first rows must line up.
    heading: Option<&'static str>,
    /// What a band with no rows at all says.
    empty_text: &'static str,
    /// The chosen row, host-owned: the element is rebuilt every frame, so an
    /// index kept inside it would reset on every one of them.
    index: Rc<RefCell<usize>>,
    on_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    drawn: Vec<DrawnRow>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl MenuBand {
    fn new(
        rows: Vec<BandRow>,
        heading: Option<&'static str>,
        empty_text: &'static str,
        index: Rc<RefCell<usize>>,
    ) -> Self {
        Self {
            rows,
            heading,
            empty_text,
            index,
            on_move: None,
            on_accept: None,
            drawn: Vec::new(),
            size: None,
            origin: None,
        }
    }

    /// Fired when the selection moves (arrow keys, or the pointer onto a row).
    fn with_on_move(mut self, callback: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>) -> Self {
        self.on_move = callback;
        self
    }

    fn with_on_accept(mut self, callback: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>) -> Self {
        self.on_accept = callback;
        self
    }

    /// The selection, clamped to the rows this band was given.
    fn selection(&self) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        Some((*self.index.borrow()).min(self.rows.len() - 1))
    }

    /// How many rows are drawn: the rows, capped at [`SLASH_MENU_ROWS`].
    fn visible_rows(&self) -> usize {
        self.rows.len().min(SLASH_MENU_ROWS)
    }

    /// The first row drawn: the window that keeps the selection visible.
    fn window_start(&self) -> usize {
        let visible = self.visible_rows();
        if self.rows.len() <= visible {
            return 0;
        }
        match self.selection() {
            Some(selected) if selected >= visible => {
                (selected + 1 - visible).min(self.rows.len() - visible)
            }
            _ => 0,
        }
    }

    /// The heading row's height, when the band draws one.
    fn heading_height(&self) -> f32 {
        if self.heading.is_some() && !self.rows.is_empty() {
            HEADING_HEIGHT
        } else {
            0.0
        }
    }

    /// The card's height, excluding the 4pt of air under it.
    fn card_height(&self) -> f32 {
        let body = if self.rows.is_empty() {
            STATUS_HEIGHT
        } else {
            self.visible_rows().max(1) as f32 * ROW_HEIGHT
        };
        CARD_PADDING * 2.0 + self.heading_height() + body
    }

    /// The drawn bounds of one visible row, relative to the band's origin.
    fn row_rect(&self, position: usize, width: f32) -> RectF {
        rectf(
            CARD_INSET + CARD_PADDING,
            CARD_PADDING + self.heading_height() + position as f32 * ROW_HEIGHT,
            width.max(0.0),
            ROW_HEIGHT,
        )
    }

    /// A row's bounds in the frame's coordinates.
    fn absolute(&self, rect: RectF) -> RectF {
        match self.origin {
            Some(origin) => rectf(
                origin.x() + rect.min_x(),
                origin.y() + rect.min_y(),
                rect.width(),
                rect.height(),
            ),
            None => rect,
        }
    }

    /// The drawn row a pointer position falls in.
    fn row_at(&self, position: Vector2F) -> Option<usize> {
        self.drawn
            .iter()
            .position(|row| contains(self.absolute(row.rect), position))
    }

    /// Track the pointer: the highlight follows the row under it. Written once
    /// per change, never per frame.
    fn track_hover(&self, ctx: &PaintContext) {
        let Some(on_move) = self.on_move.clone() else {
            return;
        };
        let hovered = self
            .drawn
            .iter()
            .find(|row| ctx.hovered(self.absolute(row.rect)))
            .map(|row| row.row);
        if let Some(row) = hovered {
            if self.selection() != Some(row) {
                (on_move.borrow_mut())(row);
            }
        }
    }

    /// Draw one line of `font_size` at the row's vertical centre.
    fn line_y(bounds: RectF, line_height: f32) -> f32 {
        bounds.min_y() + (ROW_HEIGHT - line_height).max(0.0) / 2.0
    }
}

impl Element for MenuBand {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() && constraint.max.x > 0.0 {
            constraint.max.x
        } else {
            FALLBACK_WIDTH
        };
        let card_width = (width - CARD_INSET * 2.0).max(0.0);
        let row_width = (card_width - CARD_PADDING * 2.0).max(0.0);
        let first = self.window_start();
        self.drawn = (0..self.visible_rows())
            .map(|position| DrawnRow {
                row: first + position,
                rect: self.row_rect(position, row_width),
                state: InteractiveState::default(),
            })
            .collect();

        let size = Vector2F::new(width, self.card_height() + CARD_GAP);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // The hover query reads the paint context, and taking the renderer
        // borrows it, so the pointer is resolved first.
        self.track_hover(ctx);

        let size = self.size.unwrap_or(Vector2F::zero());
        let card = rectf(
            origin.x + CARD_INSET,
            origin.y,
            (size.x - CARD_INSET * 2.0).max(0.0),
            (size.y - CARD_GAP).max(0.0),
        );
        let radius = CARD_RADIUS;
        let height = self.heading_height();
        let selected = self.selection();
        let drawn = self.drawn.clone();
        let rows = &self.rows;
        let heading = self.heading;
        let empty_text = self.empty_text;
        let renderer = match ctx.renderer.as_mut() {
            Some(renderer) => renderer,
            None => return,
        };
        // The DSH menu surface: one rounded card over the input block, with the
        // surface's own fill and a hairline so it separates from the card under
        // it. Not a slab: the card is the menu, and the 4pt under it is air.
        renderer.fill_rounded_rect(card, app.theme.color(ColorToken::SurfaceRaised), radius);
        renderer.stroke_rect(card, app.theme.color(ColorToken::Border), 1.0, radius);

        if rows.is_empty() {
            renderer.draw_text(
                Vector2F::new(
                    card.min_x() + CARD_PADDING + HEADING_PAD_X,
                    card.min_y() + CARD_PADDING + (STATUS_HEIGHT - DESCRIPTION_LINE_HEIGHT) / 2.0,
                ),
                empty_text.to_string(),
                DESCRIPTION_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (card.width() - CARD_PADDING * 2.0 - HEADING_PAD_X * 2.0).max(0.0),
                DESCRIPTION_LINE_HEIGHT / DESCRIPTION_FONT_SIZE,
            );
            return;
        }

        // The source/section heading, DSH's small grey row over the list.
        if let Some(heading) = heading.filter(|_| height > 0.0) {
            renderer.draw_text(
                Vector2F::new(
                    card.min_x() + CARD_PADDING + HEADING_PAD_X,
                    card.min_y() + CARD_PADDING + HEADING_PAD_TOP,
                ),
                heading.to_string(),
                HEADING_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (card.width() - CARD_PADDING * 2.0 - HEADING_PAD_X * 2.0).max(0.0),
                HEADING_LINE_HEIGHT / HEADING_FONT_SIZE,
            );
        }

        for drawn in &drawn {
            let bounds = self.absolute(drawn.rect);
            if selected == Some(drawn.row) {
                renderer.fill_rounded_rect(
                    bounds,
                    app.theme.color(ColorToken::Selected),
                    ROW_RADIUS,
                );
            }
            let row = &rows[drawn.row];
            let content_left = bounds.min_x() + ROW_PAD_X;
            let mut text_left = content_left;
            if let Some(icon) = row.icon {
                renderer.draw_icon(
                    Vector2F::new(
                        content_left,
                        bounds.min_y() + (ROW_HEIGHT - ICON_SIZE).max(0.0) / 2.0,
                    ),
                    icon_atlas_name(icon),
                    ICON_SIZE,
                    app.theme.color(ColorToken::Muted),
                );
                text_left += ICON_SIZE + ROW_GAP;
            }
            let check_width = if row.current { CHECK_SIZE + ROW_GAP } else { 0.0 };
            let available = (bounds.max_x() - ROW_PAD_X - text_left - check_width).max(0.0);
            let measured_name = measure_text_family(
                &row.label,
                NAME_FONT_SIZE,
                NAME_LINE_HEIGHT / NAME_FONT_SIZE,
                f32::INFINITY,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .x;
            let name_area = measured_name.min(available * NAME_COLUMN_SHARE);
            let name_y = Self::line_y(bounds, NAME_LINE_HEIGHT);
            renderer.draw_text(
                Vector2F::new(text_left, name_y),
                row.label.clone(),
                NAME_FONT_SIZE,
                app.theme.color(ColorToken::Text),
                name_area,
                NAME_LINE_HEIGHT / NAME_FONT_SIZE,
            );
            if !row.description.is_empty() {
                let desc_area = (available - name_area - ROW_GAP).max(0.0);
                if desc_area > 0.0 {
                    let measured = measure_text_family(
                        &row.description,
                        DESCRIPTION_FONT_SIZE,
                        DESCRIPTION_LINE_HEIGHT / DESCRIPTION_FONT_SIZE,
                        f32::INFINITY,
                        FontWeight::Regular,
                        FontFamily::System,
                        false,
                    )
                    .x;
                    // DSH right-aligns the description so every row ends on the
                    // same edge; a description too long for the column starts
                    // at the column's left edge and ellipsizes there.
                    let desc_x = if measured <= desc_area {
                        bounds.max_x() - ROW_PAD_X - check_width - measured
                    } else {
                        text_left + name_area + ROW_GAP
                    };
                    renderer.draw_text(
                        Vector2F::new(desc_x, Self::line_y(bounds, DESCRIPTION_LINE_HEIGHT)),
                        row.description.clone(),
                        DESCRIPTION_FONT_SIZE,
                        app.theme.color(ColorToken::Muted),
                        desc_area,
                        DESCRIPTION_LINE_HEIGHT / DESCRIPTION_FONT_SIZE,
                    );
                }
            }
            if row.current {
                renderer.draw_icon(
                    Vector2F::new(
                        bounds.max_x() - ROW_PAD_X - CHECK_SIZE,
                        bounds.min_y() + (ROW_HEIGHT - CHECK_SIZE).max(0.0) / 2.0,
                    ),
                    icon_atlas_name("check"),
                    CHECK_SIZE,
                    app.theme.color(ColorToken::Text),
                );
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
        _app: &AppContext,
    ) -> bool {
        let position = match event {
            DispatchedEvent::MouseMove { position } => *position,
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. } => *position,
            _ => return false,
        };
        let Some(row_index) = self.row_at(position) else {
            return false;
        };
        let row = self.drawn[row_index].row;
        let bounds = self.absolute(self.drawn[row_index].rect);

        if matches!(event, DispatchedEvent::MouseMove { .. }) {
            if let Some(on_move) = self.on_move.clone() {
                if self.selection() != Some(row) {
                    (on_move.borrow_mut())(row);
                }
            }
            return false;
        }

        let on_accept = self.on_accept.clone();
        let mut accept = move || {
            if let Some(cb) = on_accept.as_ref() {
                (cb.borrow_mut())(row);
            }
        };
        handle_mouse_event(
            &mut self.drawn[row_index].state,
            event,
            bounds,
            ctx,
            &mut accept,
        )
    }
}

/// The slash-command list: the commands the draft can run, drawn over the input
/// block the host owns, above the editor.
pub struct SlashMenu {
    items: Vec<SlashMenuItem>,
    index: Rc<RefCell<usize>>,
    on_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    band: Option<MenuBand>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SlashMenu {
    pub fn new(items: Vec<SlashMenuItem>, index: Rc<RefCell<usize>>) -> Self {
        Self {
            items,
            index,
            on_move: None,
            on_accept: None,
            band: None,
            size: None,
            origin: None,
        }
    }

    /// Fired when the selection moves (arrow keys, or the pointer onto a row).
    pub fn with_on_move<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when a row is clicked: the host runs that command.
    pub fn with_on_accept<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_accept = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The source heading over the rows: DSH names the command source
    /// `Commands`, the skill source `Skills`; the path source reads `Paths`.
    fn heading(&self) -> Option<&'static str> {
        match self.items.first().map(|item| item.prefix) {
            Some('/') => Some(DraftTrigger::Command.heading()),
            Some('$') => Some(DraftTrigger::Skill.heading()),
            Some('@') => Some(DraftTrigger::Mention.heading()),
            _ => None,
        }
    }

    /// What the band says when the query matches nothing. The three lists empty
    /// out for different reasons — a command list has no name match, a skill
    /// list may have nothing installed — so the row says which it is.
    fn empty_label(&self) -> &'static str {
        match self.items.first().map(|item| item.prefix) {
            Some('$') => "No skills installed",
            Some('@') => "No matching paths",
            _ => "No matching commands",
        }
    }

    /// The band this menu draws: the commands as they are typed, each with its
    /// glyph and its description, and not one of them in force — a draft is not
    /// one of the commands until it is run.
    fn band(&self) -> MenuBand {
        MenuBand::new(
            self.items
                .iter()
                .map(|item| BandRow {
                    label: item.typed(),
                    description: item.description.clone(),
                    current: false,
                    icon: item.icon,
                })
                .collect(),
            self.heading(),
            self.empty_label(),
            self.index.clone(),
        )
        .with_on_move(self.on_move.clone())
        .with_on_accept(self.on_accept.clone())
    }
}

impl Element for SlashMenu {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut band = self.band();
        let size = band.layout(constraint, ctx, app);
        self.band = Some(band);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(band) = self.band.as_mut() {
            band.paint(origin, ctx, app);
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
        self.band
            .as_mut()
            .map(|band| band.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

/// The models a pane's turns can run, drawn as the same DSH card the slash
/// commands use. This is the legacy flat list the host draws in the band slot
/// (`ChatView::model_menu`) when it does not wire the composer's own model
/// popup; the composer's DSH model menu is [`super::model_tray`].
///
/// A row is the model's own name — the id the turn is run with — and the model
/// the pane runs now draws the trailing check, DSH's selection marker, so the
/// list says where the selection already is. The row of the model in force is
/// where the composer opens the selection, so Enter takes the model the pane
/// already has.
pub struct ModelMenu {
    items: Vec<PopupMenuItem>,
    index: Rc<RefCell<usize>>,
    on_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    band: Option<MenuBand>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ModelMenu {
    pub fn new(items: Vec<PopupMenuItem>, index: Rc<RefCell<usize>>) -> Self {
        Self {
            items,
            index,
            on_move: None,
            on_accept: None,
            band: None,
            size: None,
            origin: None,
        }
    }

    /// Fired when the selection moves (arrow keys, or the pointer onto a row).
    pub fn with_on_move<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when a row is taken: the host runs the pane's turns on that model.
    pub fn with_on_accept<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_accept = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn band(&self) -> MenuBand {
        MenuBand::new(
            self.items
                .iter()
                .map(|item| BandRow {
                    label: item.label.clone(),
                    // A model is a name and nothing else: the description
                    // column belongs to the command list, which has to say
                    // what running a command does.
                    description: String::new(),
                    current: item.selected,
                    icon: item.icon,
                })
                .collect(),
            Some("Models"),
            "No models configured",
            self.index.clone(),
        )
        .with_on_move(self.on_move.clone())
        .with_on_accept(self.on_accept.clone())
    }
}

impl Element for ModelMenu {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut band = self.band();
        let size = band.layout(constraint, ctx, app);
        self.band = Some(band);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(band) = self.band.as_mut() {
            band.paint(origin, ctx, app);
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
        self.band
            .as_mut()
            .map(|band| band.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    fn items() -> Vec<SlashMenuItem> {
        vec![
            SlashMenuItem::new("new", "Start a fresh conversation").with_icon("new-chat"),
            SlashMenuItem::new("clear", "Clear this conversation's transcript")
                .with_icon("trash-02"),
        ]
    }

    fn models() -> Vec<PopupMenuItem> {
        vec![
            PopupMenuItem::new("gpt-4o"),
            PopupMenuItem::new("o3-mini").selected(),
        ]
    }

    fn drawn(menu: &mut dyn Element, app: &AppContext) -> Vec<RenderCommand> {
        menu.layout(
            SizeConstraint::loose(vec2f(640.0, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        menu.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A draft is a command while it starts with the slash; Escape puts the
    /// list away without touching what was typed.
    #[test]
    fn a_draft_is_a_command_until_escape_puts_it_away() {
        assert!(slash_menu_open("/new", false));
        assert!(slash_menu_open("   /", false));
        assert!(!slash_menu_open("/new", true));
        assert!(!slash_menu_open("hello", false));
    }

    /// The same band opens for the harness's skills and the workspace's paths,
    /// each under its own prefix, and each carries its own query and heading.
    #[test]
    fn a_skill_and_a_path_open_the_same_band_under_their_own_prefix() {
        assert_eq!(
            DraftTrigger::from_draft("$penpot"),
            Some(DraftTrigger::Skill)
        );
        assert_eq!(
            DraftTrigger::from_draft("  @src/ui"),
            Some(DraftTrigger::Mention)
        );
        assert_eq!(
            DraftTrigger::from_draft("/new"),
            Some(DraftTrigger::Command)
        );
        assert_eq!(DraftTrigger::from_draft("hello"), None);
        assert_eq!(DraftTrigger::Skill.query("$penpot"), "penpot");
        assert_eq!(DraftTrigger::Mention.query("@src/ui"), "src/ui");
        assert!(slash_menu_open("$penpot", false));
        assert!(slash_menu_open("@src", false));
        assert!(!slash_menu_open("@src", true), "escape dismisses the band");
        assert!(
            !slash_menu_open("$penpot explain the sinks", false),
            "a completed token leaves the band to the prompt"
        );
        assert_eq!(
            SlashMenuItem::new("penpot", "Design files")
                .with_prefix('$')
                .typed(),
            "$penpot"
        );
        assert_eq!(DraftTrigger::Command.heading(), "Commands");
        assert_eq!(DraftTrigger::Skill.heading(), "Skills");
        assert_eq!(DraftTrigger::Mention.heading(), "Paths");
    }

    /// The band spans the whole slot it is given; its card is inset to line up
    /// with the composer card under it and keeps the 4pt of air beneath it.
    #[test]
    fn the_band_is_the_dsh_card_over_the_composer() {
        use crate::elements::COMPOSER_CARD_GUTTER;

        let app = AppContext::default();
        let mut menu = SlashMenu::new(items(), Rc::new(RefCell::new(0)));
        let size = menu.layout(
            SizeConstraint::loose(vec2f(640.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 640.0);
        let commands = drawn(&mut menu, &app);
        let card = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised) => {
                    Some((*rect, *corner_radius))
                }
                _ => None,
            })
            .expect("the menu card is a raised fill");
        assert!(
            (card.0.min_x() - COMPOSER_CARD_GUTTER).abs() < 0.5,
            "the card lines up with the composer card: {card:?}"
        );
        assert!(
            (card.0.width() - (640.0 - COMPOSER_CARD_GUTTER * 2.0)).abs() < 0.5,
            "and ends on the card's other edge: {card:?}"
        );
        assert!(
            (*card.1 - CARD_RADIUS).abs() < 0.01,
            "the card is rounded, not a slab"
        );
        assert!(
            (size.y - (card.0.height() + CARD_GAP)).abs() < 0.5,
            "the band keeps the DSH air between the card and the input"
        );
    }

    /// The list opens under a source heading (DSH's `Commands` group title) and
    /// every row is `icon · title · right-aligned description`: no chord and no
    /// second column of its own.
    #[test]
    fn a_row_is_the_glyph_the_command_and_what_it_does() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(items(), Rc::new(RefCell::new(0)));
        let commands = drawn(&mut menu, &app);
        let labels = texts(&commands);
        assert!(labels.iter().any(|text| text == "Commands"), "got {labels:?}");
        assert!(labels.iter().any(|text| text == "/new"), "got {labels:?}");
        assert!(
            labels
                .iter()
                .any(|text| text == "Start a fresh conversation"),
            "got {labels:?}"
        );
        assert!(
            !labels.iter().any(|text| text.contains('⌘') || text.contains('↵')),
            "no chord is drawn in the list: {labels:?}"
        );
        let icons: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            icons.iter().any(|name| name.contains("new-chat")),
            "the row's glyph is drawn: {icons:?}"
        );
        // The highlighted first row carries the shared fill; the rows below it
        // are cards drawn straight on the menu surface.
        let selected = commands.iter().any(|command| {
            matches!(command, RenderCommand::FillRect { color, .. }
                if *color == app.theme.color(ColorToken::Selected))
        });
        assert!(selected, "the highlighted row is the shared highlight");
    }

    /// The description is right-aligned: a short one ends on the card's right
    /// edge, which is what makes the titles read as one left column.
    #[test]
    fn the_description_is_right_aligned_on_the_row() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(items(), Rc::new(RefCell::new(0)));
        let commands = drawn(&mut menu, &app);
        let first = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Start a fresh conversation" =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the description is drawn");
        let last = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "/new" => Some(*origin),
                _ => None,
            })
            .expect("the title is drawn");
        assert!(first.x > last.x, "the description follows the title");
        assert!(first.y > 0.0);
    }

    /// A query that matches nothing says so inside the card instead of drawing
    /// an empty band.
    #[test]
    fn an_empty_list_says_so() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(Vec::new(), Rc::new(RefCell::new(0)));
        let commands = drawn(&mut menu, &app);
        let labels = texts(&commands);
        assert!(
            labels.iter().any(|text| text == "No matching commands"),
            "got {labels:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. }
                    if *color == app.theme.color(ColorToken::SurfaceRaised)
            )),
            "the empty state lives in the menu card: {commands:?}"
        );
    }

    /// The model menu is the same card in the same slot, one row per model, with
    /// the model the pane runs now drawing DSH's trailing check.
    #[test]
    fn the_model_menu_is_the_same_card_with_a_check_marker() {
        let app = AppContext::default();
        let mut menu = ModelMenu::new(models(), Rc::new(RefCell::new(0)));
        let size = menu.layout(
            SizeConstraint::loose(vec2f(640.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 640.0, "the card spans the slot it is given");

        let commands = drawn(&mut menu, &app);
        let labels = texts(&commands);
        assert!(labels.iter().any(|text| text == "Models"), "got {labels:?}");
        assert!(labels.iter().any(|text| text == "gpt-4o"), "got {labels:?}");
        assert!(labels.iter().any(|text| text == "o3-mini"), "got {labels:?}");
        let checks = commands
            .iter()
            .filter(|command| {
                matches!(command, RenderCommand::DrawIcon { name, .. } if name.contains("check"))
            })
            .count();
        assert_eq!(checks, 1, "exactly the model in force is checked");
    }

    /// A model menu with nothing to offer says so, the way the command list
    /// does.
    #[test]
    fn a_model_menu_without_models_says_so() {
        let app = AppContext::default();
        let mut menu = ModelMenu::new(Vec::new(), Rc::new(RefCell::new(0)));
        let commands = drawn(&mut menu, &app);
        let labels = texts(&commands);
        assert!(
            labels.iter().any(|text| text == "No models configured"),
            "got {labels:?}"
        );
    }
}
