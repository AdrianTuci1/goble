//! The rich input's full-width bands: the commands the draft can run, and the
//! models a pane's turns can run, listed directly above the input the way
//! grok-build lists them.
//!
//! They are not overlays and they do not take the keyboard: the editor keeps the
//! caret and the draft stays the query, so the command list narrows as the user
//! types after the `/`. The composer routes the keys that take a row — Up and
//! Down to move, Enter or Tab to take the selection, Escape to put the list away
//! — and a band draws what it was given and reports what the pointer chose.
//!
//! A band is square and spans the input's whole width: it reads as the top of
//! the input block, not as a floating card over it. Both menus are the same band
//! ([`MenuBand`]), so the two can never drift apart in their geometry or in how
//! a row answers the pointer.
//!
//! Self-drawn rather than composed from children: every row has the same fixed
//! height, so the panel's own geometry is the hit-test geometry, and the row
//! bounds `layout` computes (which has no cursor) are the ones `paint` and
//! `dispatch_event` (which have no layout pass) use.

use std::cell::RefCell;
use std::rc::Rc;

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

const ROW_HEIGHT: f32 = 24.0;
/// The row a pointer is in is the row it hit; the band is dense, the way the
/// command list it mirrors is.
const ROW_INSET: f32 = 12.0;
/// The gap between a command and the description that follows it.
const DESCRIPTION_GAP: f32 = 14.0;
/// The description column never takes more than this share of the width, so a
/// long command name does not squeeze what it does off the row.
const NAME_COLUMN_SHARE: f32 = 0.45;
const NAME_FONT_SIZE: f32 = 12.0;
const DESCRIPTION_FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.2;

/// Whether a draft opens the menu: it is a command (it starts with `/`) and
/// Escape has not put this draft's list away. The host and the editor both read
/// it, so what the editor routes and what is drawn can never disagree.
pub fn slash_menu_open(draft: &str, dismissed: bool) -> bool {
    !dismissed && draft.trim_start().starts_with('/')
}

/// One command the menu offers: the name as it is typed after the `/`, and what
/// running it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashMenuItem {
    pub name: String,
    pub description: String,
}

impl SlashMenuItem {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }

    /// The command as the row draws it: the name with the prefix it is typed
    /// with.
    pub fn typed(&self) -> String {
        format!("/{}", self.name)
    }
}

/// One row of a band: what it reads, the muted second column beside it (empty
/// draws none), and whether it is the value in force — which reads in the
/// accent colour, the way a popup menu marks the item it is on.
struct BandRow {
    label: String,
    description: String,
    current: bool,
}

/// One drawn row: the row it shows, where it sits relative to the band's
/// origin, and its own pointer state.
struct DrawnRow {
    row: usize,
    rect: RectF,
    state: InteractiveState,
}

/// A band: the rows it draws, the window around the selection, and the
/// callbacks a row reports to.
///
/// Both menus are this element — see the module docs — so the geometry, the hit
/// testing and the paint live here once.
struct MenuBand {
    rows: Vec<BandRow>,
    /// What a band with no rows at all says.
    empty_text: &'static str,
    /// The chosen row, host-owned: the element is rebuilt every frame, so an
    /// index kept inside it would reset on every one of them.
    index: Rc<RefCell<usize>>,
    on_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    drawn: Vec<DrawnRow>,
    /// The width the row labels occupy, so the second column lines up.
    name_column: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl MenuBand {
    fn new(rows: Vec<BandRow>, empty_text: &'static str, index: Rc<RefCell<usize>>) -> Self {
        Self {
            rows,
            empty_text,
            index,
            on_move: None,
            on_accept: None,
            drawn: Vec::new(),
            name_column: 0.0,
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

    fn panel_height(&self) -> f32 {
        self.visible_rows().max(1) as f32 * ROW_HEIGHT
    }

    /// The drawn bounds of one visible row, relative to the band's origin. The
    /// rows run edge to edge: the band is the top of the input, not a card
    /// floating over it.
    fn row_rect(position: usize, width: f32) -> RectF {
        rectf(0.0, position as f32 * ROW_HEIGHT, width.max(0.0), ROW_HEIGHT)
    }

    /// Where one line of `font_size` sits in a row: vertically centred.
    fn line_offset(font_size: f32) -> f32 {
        (ROW_HEIGHT - font_size * LINE_HEIGHT) / 2.0
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
        let first = self.window_start();
        self.drawn = (0..self.visible_rows())
            .map(|position| DrawnRow {
                row: first + position,
                rect: Self::row_rect(position, width),
                state: InteractiveState::default(),
            })
            .collect();

        // The second column starts in one place, so the list reads down the page
        // rather than stepping right with every row's label.
        let widest = self.drawn.iter().fold(0.0f32, |widest, row| {
            let label = &self.rows[row.row].label;
            widest.max(
                measure_text_family(
                    label,
                    NAME_FONT_SIZE,
                    LINE_HEIGHT,
                    f32::INFINITY,
                    FontWeight::Regular,
                    FontFamily::System,
                    false,
                )
                .x,
            )
        });
        self.name_column = ROW_INSET + widest + DESCRIPTION_GAP;
        self.name_column = self
            .name_column
            .min(width * NAME_COLUMN_SHARE + ROW_INSET + DESCRIPTION_GAP);

        let size = Vector2F::new(width, self.panel_height());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // The hover query reads the paint context, and taking the renderer
        // borrows it, so the pointer is resolved first.
        self.track_hover(ctx);

        let size = self.size.unwrap_or(Vector2F::zero());
        let panel = rectf(origin.x, origin.y, size.x, size.y);
        let renderer = match ctx.renderer.as_mut() {
            Some(renderer) => renderer,
            None => return,
        };
        // Square and full width: a band over the input, not a rounded card.
        renderer.fill_rect(panel, app.theme.color(ColorToken::SurfaceRaised));
        renderer.stroke_rect(panel, app.theme.color(ColorToken::Border), 1.0, 0.0);

        if self.rows.is_empty() {
            let row = rectf(panel.min_x(), panel.min_y(), panel.width(), ROW_HEIGHT);
            renderer.draw_text(
                Vector2F::new(
                    row.min_x() + ROW_INSET,
                    row.min_y() + Self::line_offset(NAME_FONT_SIZE),
                ),
                self.empty_text,
                NAME_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (row.width() - ROW_INSET * 2.0).max(0.0),
                LINE_HEIGHT,
            );
            return;
        }

        let selected = self.selection();
        for drawn in &self.drawn {
            let bounds = self.absolute(drawn.rect);
            if selected == Some(drawn.row) {
                renderer.fill_rect(bounds, app.theme.color(ColorToken::Selected));
            }
            let row = &self.rows[drawn.row];
            // The value in force keeps its own colour, so the list says which
            // row the input already holds as well as which one is chosen.
            renderer.draw_text(
                Vector2F::new(
                    bounds.min_x() + ROW_INSET,
                    bounds.min_y() + Self::line_offset(NAME_FONT_SIZE),
                ),
                row.label.clone(),
                NAME_FONT_SIZE,
                app.theme.color(if row.current {
                    ColorToken::Accent
                } else {
                    ColorToken::Text
                }),
                // The label keeps its own column: a label too long for it is
                // truncated rather than run over the second column.
                (self.name_column - ROW_INSET - DESCRIPTION_GAP).max(0.0),
                LINE_HEIGHT,
            );
            if !row.description.is_empty() && self.name_column < bounds.width() {
                renderer.draw_text(
                    Vector2F::new(
                        bounds.min_x() + self.name_column,
                        bounds.min_y() + Self::line_offset(DESCRIPTION_FONT_SIZE),
                    ),
                    row.description.clone(),
                    DESCRIPTION_FONT_SIZE,
                    app.theme.color(ColorToken::Muted),
                    (bounds.width() - self.name_column - ROW_INSET).max(0.0),
                    LINE_HEIGHT,
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

    /// The band this menu draws: the commands as they are typed, and not one of
    /// them in force — a draft is not one of the commands until it is run.
    fn band(&self) -> MenuBand {
        MenuBand::new(
            self.items
                .iter()
                .map(|item| BandRow {
                    label: item.typed(),
                    description: item.description.clone(),
                    current: false,
                })
                .collect(),
            "No matching commands",
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

/// The models a pane's turns can run, drawn as the same band the slash commands
/// use: what the model control opens, over the input's own width.
///
/// A row is the model's own name — the id the turn is run with — and the model
/// the pane runs now reads in the accent colour, so the list says where the
/// selection already is. The row of the model in force is where the composer
/// opens the selection, so Enter takes the model the pane already has.
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
                    // A model is a name and nothing else: the second column
                    // belongs to the command list, which has to say what
                    // running a command does.
                    description: String::new(),
                    current: item.selected,
                })
                .collect(),
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
            SlashMenuItem::new("new", "Start a fresh conversation"),
            SlashMenuItem::new("clear", "Clear this conversation's transcript"),
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

    fn drawn_texts(menu: &mut SlashMenu, app: &AppContext) -> Vec<String> {
        drawn(menu, app)
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
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

    /// The band spans the whole width it is given: it is the top of the input,
    /// not a panel inset into it.
    #[test]
    fn the_band_spans_the_width_it_is_given() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(items(), Rc::new(RefCell::new(0)));
        let size = menu.layout(
            SizeConstraint::loose(vec2f(640.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 640.0);
        assert!(size.y > 0.0);
    }

    /// A row is the command as it is typed and what it does — the chord that
    /// runs it is not repeated on the row.
    #[test]
    fn a_row_is_the_command_and_what_it_does() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(items(), Rc::new(RefCell::new(0)));
        let texts = drawn_texts(&mut menu, &app);
        assert!(texts.iter().any(|text| text == "/new"), "got {texts:?}");
        assert!(
            texts.iter().any(|text| text == "Start a fresh conversation"),
            "got {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains('⌘') || text.contains('↵')),
            "no chord is drawn in the list: {texts:?}"
        );
    }

    /// A query that matches nothing says so instead of drawing an empty band.
    #[test]
    fn an_empty_list_says_so() {
        let app = AppContext::default();
        let mut menu = SlashMenu::new(Vec::new(), Rc::new(RefCell::new(0)));
        let texts = drawn_texts(&mut menu, &app);
        assert!(
            texts.iter().any(|text| text == "No matching commands"),
            "got {texts:?}"
        );
    }

    /// The model menu is the same band over the same width, one row per model,
    /// with the model the pane runs now reading in the accent colour.
    #[test]
    fn the_model_menu_lists_the_models_in_the_same_band() {
        let app = AppContext::default();
        let mut menu = ModelMenu::new(models(), Rc::new(RefCell::new(0)));
        let size = menu.layout(
            SizeConstraint::loose(vec2f(640.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 640.0, "the band spans the input's own width");

        let mut ctx = PaintContext::new(Renderer::new());
        menu.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        let commands = ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default();
        let labels: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|text| text == "gpt-4o"), "got {labels:?}");
        assert!(labels.iter().any(|text| text == "o3-mini"), "got {labels:?}");
        let accent = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, color, .. }
                    if text == "o3-mini" && *color == app.theme.color(ColorToken::Accent)
            )
        });
        assert!(accent, "the model in force reads in the accent colour");
    }

    /// A model menu with nothing to offer says so, the way the command list
    /// does.
    #[test]
    fn a_model_menu_without_models_says_so() {
        let app = AppContext::default();
        let mut menu = ModelMenu::new(Vec::new(), Rc::new(RefCell::new(0)));
        let commands = drawn(&mut menu, &app);
        let labels: Vec<String> = commands
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            labels.iter().any(|text| text == "No models configured"),
            "got {labels:?}"
        );
    }
}
