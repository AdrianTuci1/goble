use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Axis, Clipped, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, EventContext, Expanded, Fill, Flex, Icon, LayoutContext, MainAxisSize, MenuClose,
    MenuId, MenuPointer, PaintContext, Point, ScrollState, Scrollable, SizeConstraint, Text,
    DEFAULT_MENU_ID,
};
use crate::event::{DispatchedEvent, BUTTON_PRIMARY};
use crate::geometry::{rectf, vec2f, PointF, RectF, Size2F, Vector2F};
use crate::platform::text_atlas::{advance_width, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const POPUP_MAX_WIDTH: f32 = 300.0;
const POPUP_ITEM_HEIGHT: f32 = 32.0;
/// The height of a row that carries a second line: a short description under
/// the label, the way the environment menu names what each medium does.
const POPUP_ITEM_WITH_DESCRIPTION_HEIGHT: f32 = 48.0;
const POPUP_ITEM_SPACING: f32 = 2.0;
/// The label size of a menu row, and of the description line under it: the
/// composer's type scale, one step up from the old 12pt rows.
const POPUP_ITEM_LABEL_SIZE: f32 = 13.0;
const POPUP_ITEM_DESCRIPTION_SIZE: f32 = 13.0;
/// The height a pane separator takes in the panel's flow: the 1px `Border` rule
/// with four points above and below it (the reference's "Work in a New
/// Worktree" divider).
const POPUP_SEPARATOR_HEIGHT: f32 = 9.0;
/// The panel's own inset around its rows. The rows are rounded boxes inside it
/// (the reference menu casing), so the panel needs a gutter to hold them.
const POPUP_PANEL_PADDING: f32 = 6.0;
/// The rounding of a row's hover/highlight fill. It is smaller than the
/// panel's own radius so the two curves read as one casing: the row is a
/// control inside the panel, not a second panel.
const POPUP_ITEM_RADIUS: f32 = 6.0;
/// The panel's default corner radius (§ reference menu casing).
pub const POPUP_PANEL_RADIUS: f32 = 12.0;
/// How many rows a capped panel draws before its list scrolls.
///
/// Rows are the natural unit: a row is a fixed [`POPUP_ITEM_HEIGHT`] with
/// [`POPUP_ITEM_SPACING`] between rows, so eight rows are 270 px — a panel that
/// still fits over the input in a short window, while showing every row of an
/// ordinary directory. Longer lists scroll instead of running off the pane.
pub const MENU_MAX_VISIBLE_ROWS: usize = 8;
const POPUP_GAP: f32 = 6.0;
/// The gap between the hovered row and its tray (warp-new's action sidecar).
const TRAY_GAP: f32 = 4.0;

/// The line counts drawn at the trailing edge of a menu row: a changed file's
/// additions and deletions, in the same green and red as the diff control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PopupMenuDiff {
    pub added: u32,
    pub removed: u32,
}

/// One entry in a [`PopupMenu`].
#[derive(Clone, Debug)]
pub struct PopupMenuItem {
    pub label: String,
    pub icon: Option<&'static str>,
    pub selected: bool,
    pub disabled: bool,
    /// The file's line counts, right-aligned on the row. `None` for a plain
    /// menu item.
    pub diff: Option<PopupMenuDiff>,
    /// A second, muted line under the label saying what the item does — the
    /// environment menu's "Clones this repo in a cloud environment…". `None`
    /// leaves the row one line tall.
    pub description: Option<String>,
    /// Draw the panel's 1px pane separator above this row. The reference's
    /// branch menu separates the branches from "Work in a New Worktree" with
    /// it; `None` keeps the row in the ordinary flow.
    pub separator_before: bool,
}

impl PopupMenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            selected: false,
            disabled: false,
            diff: None,
            description: None,
            separator_before: false,
        }
    }

    /// Right-align a changed file's additions and deletions on the row.
    pub fn with_diff(mut self, added: u32, removed: u32) -> Self {
        self.diff = Some(PopupMenuDiff { added, removed });
        self
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// A muted second line under the label saying what the item does.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Draw the 1px pane separator above this row.
    pub fn with_separator_before(mut self) -> Self {
        self.separator_before = true;
        self
    }

    /// The row's own height: one line, or two when it carries a description.
    pub fn height(&self) -> f32 {
        if self.description.is_some() {
            POPUP_ITEM_WITH_DESCRIPTION_HEIGHT
        } else {
            POPUP_ITEM_HEIGHT
        }
    }
}

/// Where a [`PopupMenu`] panel appears relative to its trigger.
///
/// The `*End` variants align the panel's trailing edge with the trigger's, so a
/// trigger that sits near the right edge of its pane opens the panel leftwards,
/// inside the window, instead of spilling past the edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupMenuPosition {
    Below,
    Above,
    BelowEnd,
    AboveEnd,
}

impl Default for PopupMenuPosition {
    fn default() -> Self {
        Self::Below
    }
}

/// A tray drawn beside the row under the pointer, following warp-new's action
/// sidecar: it appears next to the hovered item (to its right, or to its left
/// when it would leave the window) and carries that item's own actions, so a
/// menu row can offer more than the one thing a click on it does.
struct HoverTray {
    width: f32,
    /// Content for the hovered item's index, or `None` for an item that carries
    /// no tray.
    build: Rc<dyn Fn(usize, &AppContext) -> Option<Box<dyn Element>>>,
}

/// A capped panel's scroll offset and the row the offset was last moved to
/// reveal.
///
/// The tree is rebuilt every frame, so the app owns one of these — like the
/// menu's open flag — and hands it to [`PopupMenu::with_panel_scroll`]. The
/// revealed row is remembered beside the offset, for the same reason and so
/// that a wheel scroll away from the highlighted row is not undone by the next
/// frame's reveal: only a *change* of the highlighted row moves the offset.
#[derive(Clone, Debug, Default)]
pub struct PanelScroll {
    offset: Rc<RefCell<ScrollState>>,
    revealed: Rc<RefCell<Option<usize>>>,
}

impl PanelScroll {
    pub fn new() -> Self {
        Self::default()
    }

    /// The offset the [`Scrollable`] inside the panel reads and writes.
    pub fn state(&self) -> Rc<RefCell<ScrollState>> {
        Rc::clone(&self.offset)
    }

    /// The row the offset was last moved to reveal, if any.
    pub fn revealed(&self) -> Option<usize> {
        *self.revealed.borrow()
    }

    fn mark_revealed(&self, index: usize) {
        *self.revealed.borrow_mut() = Some(index);
    }
}

/// A trigger that opens a floating item menu.
///
/// The open flag is app-owned (`Rc<RefCell<bool>>`) so it survives the
/// per-frame element rebuild. While open the panel is drawn over the trigger's
/// neighbours; clicking outside it (or selecting an item) closes it.
pub struct PopupMenu {
    trigger: Box<dyn Element>,
    items: Vec<PopupMenuItem>,
    open: Rc<RefCell<bool>>,
    position: PopupMenuPosition,
    on_select: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    hover_tray: Option<HoverTray>,
    /// The item the pointer is over, shared with the app. The tree is rebuilt
    /// every frame, so this cannot live in the element: it is written by
    /// `paint` (where hover is decided) and read by the next frame's `layout`,
    /// which is what builds the tray.
    hover_index: Rc<RefCell<Option<usize>>>,
    /// The item the keyboard is on, shared with the app. Like the hover cell
    /// the tree is rebuilt every frame, so the row Up/Down moved to has to live
    /// outside the element. `None` while the menu is closed; an open menu with
    /// no highlight starts on the item marked `.selected()` (or the first row).
    highlight_index: Rc<RefCell<Option<usize>>>,
    /// A row drawn after every item, in the panel's own flow — the tab menu's
    /// colour dots, which are a row of controls rather than a labelled item.
    /// `None` leaves the panel exactly the items it was given.
    footer_row: Option<Box<dyn Element>>,
    /// Told when the menu closes *itself* — a press outside the panel — so the
    /// app's own open state hears about it. Without this the app would not know
    /// the menu had closed and would open it again on the next frame.
    on_close: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// The label this menu registers under in the window's shared open-menu
    /// state ([`crate::elements::OpenMenus`]). Opening it closes whatever menu
    /// was open before, and while it is up the pointer over its panel belongs
    /// to it — nothing painted behind it hovers. The default is
    /// [`DEFAULT_MENU_ID`]; the app names the menus that matter
    /// (`"sidebar.view-options"`, `"composer.dir"`…).
    menu_id: MenuId,
    state: InteractiveState,
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    panel_origin: Vector2F,
    /// A nudge applied to the panel after it is placed: the composer's
    /// environment menu sits 10pt past its trigger symbol's right edge so the
    /// casing ends a few points inside the card, the way the reference's does.
    panel_offset: Vector2F,
    /// How many rows the panel draws before its list scrolls. `None` leaves the
    /// panel at its natural height — every row, the way it has always been — so
    /// a menu that never asked for a cap is drawn exactly as before.
    max_visible_rows: Option<usize>,
    panel_scroll: PanelScroll,
    /// The panel's widest row and the radius of its own box. The app's menus
    /// keep the defaults; the diff menu widens for file paths and squares its
    /// corners.
    max_width: f32,
    corner_radius: f32,
    /// A row height override for this menu (the sidebar's view options are 44pt
    /// tall); `None` keeps the shared casing's 32/48.
    row_height: Option<f32>,
    /// The row glyph size (defaults to the shared casing's 14).
    icon_size: f32,
    /// The ink of the selected row's label. `None` keeps the shared casing's
    /// `Accent`; the Workspaces view-options menu asks for the web's accent
    /// cyan (`Focus`) so its current choice reads as bright as its neighbours.
    selected_color: Option<ColorToken>,
    tray: Option<Box<dyn Element>>,
    tray_size: Option<Vector2F>,
    tray_origin: Vector2F,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PopupMenu {
    pub fn new(trigger: Box<dyn Element>, items: Vec<PopupMenuItem>) -> Self {
        Self {
            trigger,
            items,
            open: Rc::new(RefCell::new(false)),
            position: PopupMenuPosition::default(),
            on_select: None,
            hover_tray: None,
            hover_index: Rc::new(RefCell::new(None)),
            highlight_index: Rc::new(RefCell::new(None)),
            footer_row: None,
            on_close: None,
            menu_id: DEFAULT_MENU_ID,
            state: InteractiveState::default(),
            panel: None,
            panel_size: None,
            panel_origin: Vector2F::zero(),
            panel_offset: Vector2F::zero(),
            max_visible_rows: None,
            panel_scroll: PanelScroll::default(),
            max_width: POPUP_MAX_WIDTH,
            corner_radius: POPUP_PANEL_RADIUS,
            row_height: None,
            icon_size: 14.0,
            selected_color: None,
            tray: None,
            tray_size: None,
            tray_origin: Vector2F::zero(),
            size: None,
            origin: None,
        }
    }

    pub fn with_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.open = open;
        self
    }

    /// Name this menu in the window's shared open-menu state. A named menu is
    /// the one that can be open: opening it closes any other menu first, and
    /// the pointer on its panel belongs to it alone (see
    /// [`crate::elements::OpenMenus`]).
    pub fn with_menu_id(mut self, id: MenuId) -> Self {
        self.menu_id = id;
        self
    }

    /// Cap the panel's row list at `rows` rows ([`MENU_MAX_VISIBLE_ROWS`] is the
    /// value the app's directory menu uses): a longer list scrolls inside that
    /// height instead of growing past the pane it opens into.
    pub fn with_max_visible_rows(mut self, rows: usize) -> Self {
        self.max_visible_rows = Some(rows);
        self
    }

    /// Share the panel's scroll offset with the app, so it survives the
    /// per-frame rebuild (the open flag is app-owned for the same reason, as is
    /// the hovered row). Without one a capped panel still scrolls, but opens at
    /// the top again on the frame after the wheel moved it.
    pub fn with_panel_scroll(mut self, scroll: PanelScroll) -> Self {
        self.panel_scroll = scroll;
        self
    }

    /// The widest the panel's rows may be. A menu of file paths asks for more
    /// room than the default item list.
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = width;
        self
    }

    /// The panel's corner radius, and the source of its rows' radius. A row is
    /// as round as the panel: a full-bleed row sits on the panel's own edges, so
    /// the two must share the corner curve to keep the row inside the panel's
    /// rounded bounds. Zero squares both.
    /// Override the row height for this menu (the Workspaces view options use
    /// the web's 44pt rows and 16pt glyphs).
    pub fn with_row_height(mut self, height: f32) -> Self {
        self.row_height = Some(height);
        self
    }

    /// Set the row glyphs' size for this menu.
    pub fn with_icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }

    /// The ink of the selected row's label. The default is the shared casing's
    /// `Accent` (the composer menus' look, unchanged); a menu whose selected
    /// label should read as bright as the others passes the app accent here.
    pub fn with_selected_color(mut self, token: ColorToken) -> Self {
        self.selected_color = Some(token);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_position(mut self, position: PopupMenuPosition) -> Self {
        self.position = position;
        self
    }

    /// Nudge the panel after it is placed, in window points. Used to align a
    /// trailing panel with the edge of the card it belongs to rather than with
    /// the width of its own trigger.
    pub fn with_panel_offset(mut self, offset: Vector2F) -> Self {
        self.panel_offset = offset;
        self
    }

    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show a tray beside the item under the pointer, built from that item's
    /// index (`None` for items that carry no tray). The tray is laid out at
    /// `width` and hangs from the hovered row's top edge.
    ///
    /// Hover is decided at paint time (see [`PaintContext::hovered`]) because
    /// the tree is rebuilt every frame, so the tray for a row appears on the
    /// frame after the pointer reaches it. Pass the app's own cell to
    /// [`Self::with_hover_index`] when the menu is rebuilt every frame, as the
    /// app's menus are, otherwise that frame-to-frame memory is lost with the
    /// element.
    pub fn with_hover_tray<F>(mut self, width: f32, build: F) -> Self
    where
        F: Fn(usize, &AppContext) -> Option<Box<dyn Element>> + 'static,
    {
        self.hover_tray = Some(HoverTray { width, build: Rc::new(build) });
        self
    }

    /// Share the item-under-the-pointer cell with the app, so the tray survives
    /// the per-frame rebuild (the open flag is app-owned for the same reason).
    pub fn with_hover_index(mut self, hover_index: Rc<RefCell<Option<usize>>>) -> Self {
        self.hover_index = hover_index;
        self
    }

    /// Share the keyboard highlight with the app, so Up/Down/Enter survive the
    /// per-frame rebuild (the open flag is app-owned for the same reason). An
    /// open menu with no highlight starts on its `.selected()` item, or the
    /// first row; a closed menu resets the cell to `None`.
    pub fn with_highlight_index(mut self, highlight_index: Rc<RefCell<Option<usize>>>) -> Self {
        self.highlight_index = highlight_index;
        self
    }

    /// Add a row after the items, in the panel's own column and at its own
    /// height: warp-new's tab menu carries the tab's colours as one such row —
    /// a strip of dots, each its own click target — rather than as a labelled
    /// item (see `app/src/tab.rs::dot_color_option_menu_items`).
    pub fn with_footer_row(mut self, row: Box<dyn Element>) -> Self {
        self.footer_row = Some(row);
        self
    }

    /// Called when the menu closes itself — a press outside the panel — so an
    /// app that rebuilds the element every frame can clear its own open state.
    /// An item's selection is reported through [`Self::with_on_select`] instead,
    /// which the menu calls before it closes.
    pub fn with_on_close<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn is_open(&self) -> bool {
        *self.open.borrow()
    }

    /// The id this menu registers under in the shared open-menu state.
    pub fn menu_id(&self) -> MenuId {
        self.menu_id
    }

    /// How to put this menu away from outside the element: the shared registry
    /// keeps it while the menu is the one open, so opening another menu closes
    /// this one, and a dismiss from the root view clears the app's own flag.
    fn close_handle(&self) -> MenuClose {
        let open = Rc::clone(&self.open);
        let on_close = self.on_close.clone();
        Rc::new(RefCell::new(move || {
            if !*open.borrow() {
                return;
            }
            *open.borrow_mut() = false;
            if let Some(cb) = on_close.as_ref() {
                (cb.borrow_mut())();
            }
        }))
    }

    /// The toggle a press on the trigger runs: opening claims the window's one
    /// open menu (closing whatever was up), closing puts this one away.
    fn trigger_toggle(&self, app: &AppContext) -> Rc<RefCell<dyn FnMut()>> {
        let menus = app.open_menus.clone();
        let id = self.menu_id;
        let open = Rc::clone(&self.open);
        let close = self.close_handle();
        Rc::new(RefCell::new(move || {
            if *open.borrow() {
                *open.borrow_mut() = false;
                return;
            }
            menus.open(id, close.clone());
            *open.borrow_mut() = true;
        }))
    }

    fn panel_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.panel_size?;
        Some(RectF::new(
            PointF::new(origin.x() + self.panel_origin.x, origin.y() + self.panel_origin.y),
            Size2F::new(size.x, size.y),
        ))
    }

    fn tray_bounds(&self) -> Option<RectF> {
        let size = self.tray_size?;
        let origin = self.origin?;
        Some(RectF::new(
            PointF::new(origin.x() + self.tray_origin.x, origin.y() + self.tray_origin.y),
            Size2F::new(size.x, size.y),
        ))
    }

    /// The gap between `row` and its tray, while the tray sits beside it: the
    /// corridor the pointer crosses on its way from the row to the tray. A row
    /// stays hovered across it, so the tray can be reached — without it the
    /// tray would vanish the moment the pointer left the row, and a tray this
    /// far from its row could never be clicked.
    fn tray_corridor(&self, row: RectF) -> Option<RectF> {
        let tray = self.tray_bounds()?;
        let (left, right) = if tray.min_x() >= row.max_x() {
            (row.max_x(), tray.min_x())
        } else if tray.max_x() <= row.min_x() {
            (tray.max_x(), row.min_x())
        } else {
            // The tray overlaps the row horizontally: there is no gap to cross.
            return None;
        };
        // The corridor spans both heights: the tray is taller than a row (a
        // title and its button), and a diagonal path to the button must not
        // leave the row before it is over the tray.
        let top = row.min_y().min(tray.min_y());
        let bottom = row.max_y().max(tray.max_y());
        Some(RectF::new(
            PointF::new(left, top),
            Size2F::new((right - left).max(0.0), (bottom - top).max(0.0)),
        ))
    }

    /// The row's own height: one line, or two when it carries a description.
    fn item_height(&self, index: usize) -> f32 {
        self.items
            .get(index)
            .map(PopupMenuItem::height)
            .unwrap_or(POPUP_ITEM_HEIGHT)
    }

    /// The top of item `index`'s row, relative to the panel's content top: the
    /// rows and the pane separators between them, each on the column's spacing.
    fn item_top(&self, index: usize) -> f32 {
        let mut top = 0.0;
        for (i, item) in self.items.iter().enumerate().take(index + 1) {
            if i > 0 {
                top += POPUP_ITEM_SPACING;
            }
            if item.separator_before {
                top += POPUP_SEPARATOR_HEIGHT + POPUP_ITEM_SPACING;
            }
            if i == index {
                break;
            }
            top += item.height();
        }
        top
    }

    /// The panel's scroll offset: zero for a panel with no cap, which draws
    /// every row from the top.
    fn scroll_offset(&self) -> f32 {
        if self.max_visible_rows.is_some() {
            self.panel_scroll.state().borrow().offset()
        } else {
            0.0
        }
    }

    /// The bounds of item `index`'s row, in window coordinates. Rows are a
    /// fixed height with a fixed spacing between them, so a row's rect follows
    /// from the panel's geometry without asking the row element. A row is inset
    /// from the panel by its casing padding, and a capped panel draws its rows
    /// shifted by the scroll offset and clipped to the window, so a row scrolled
    /// out of sight has no bounds: nothing hangs from it and the pointer cannot
    /// hover it.
    fn row_bounds(&self, index: usize, _app: &AppContext) -> Option<RectF> {
        let origin = self.origin?;
        let panel_size = self.panel_size?;
        let padding = POPUP_PANEL_PADDING;
        let x = origin.x() + self.panel_origin.x + padding;
        let width = (panel_size.x - padding * 2.0).max(0.0);
        let height = self.item_height(index);
        let top = self.item_top(index) - self.scroll_offset();
        if self.max_visible_rows.is_some() {
            let window = (panel_size.y - padding * 2.0).max(0.0);
            if top + height <= 0.0 || top >= window {
                return None;
            }
        }
        let y = origin.y() + self.panel_origin.y + padding + top;
        Some(RectF::new(
            PointF::new(x, y),
            Size2F::new(width, height),
        ))
    }

    /// Build the hovered row's tray and place it beside that row: to its right,
    /// or to its left when it would otherwise leave the window (warp-new's
    /// overflow rule). Called from `layout`, so the tray is in the tree and can
    /// take a click on the frame after the pointer reached the row.
    fn build_tray(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) {
        self.tray = None;
        self.tray_size = None;
        self.tray_origin = Vector2F::zero();

        let (Some(index), Some(hover_tray)) = (*self.hover_index.borrow(), self.hover_tray.as_ref())
        else {
            return;
        };
        let Some(content) = (hover_tray.build)(index, app) else {
            return;
        };
        let Some(panel_size) = self.panel_size else {
            return;
        };
        // Menu-relative, like `panel_origin`: the tray hangs from the hovered
        // row's top edge and sits just past the panel's trailing edge. Rows
        // start one casing padding inside the panel's top edge.
        let row_top =
            self.panel_origin.y + POPUP_PANEL_PADDING + self.item_top(index) - self.scroll_offset();
        let panel_right = self.panel_origin.x + panel_size.x;

        let mut tray = content;
        let max_height = if constraint.max.y.is_finite() {
            constraint.max.y
        } else {
            POPUP_MAX_WIDTH
        };
        let size = tray.layout(
            SizeConstraint::loose(vec2f(hover_tray.width, max_height)),
            ctx,
            app,
        );

        // The overflow test needs the menu's position on screen, which layout
        // does not have: it comes from this element's own last paint, so a menu
        // the app rebuilds every frame knows only the panel's geometry. An
        // unbounded available width (a shrink-to-content row, as the topbar's
        // is) means there is no edge to run into, so the tray opens rightwards.
        let menu_left = self.origin.map(|o| o.x()).unwrap_or(0.0);
        let fits_right = !constraint.max.x.is_finite()
            || menu_left + panel_right + TRAY_GAP + size.x <= constraint.max.x;
        let x = if fits_right {
            panel_right + TRAY_GAP
        } else {
            self.panel_origin.x - TRAY_GAP - size.x
        };
        self.tray_origin = vec2f(x, row_top);
        self.tray_size = Some(size);
        self.tray = Some(tray);
    }

    /// Re-read which row the pointer is over from the paint-time cursor, which
    /// is where hover lives in this framework. The pointer over the tray itself
    /// keeps the tray open, so it can be moved onto and clicked, and so does a
    /// pointer in the gap between the row and the tray (see
    /// [`Self::tray_corridor`]): the two are four points apart, and a pointer
    /// crossing that gap would otherwise drop the row and the tray with it.
    fn track_hover(&mut self, ctx: &PaintContext, app: &AppContext) {
        let over_row = (0..self.items.len())
            .find(|i| self.row_bounds(*i, app).is_some_and(|row| ctx.hovered(row)));
        if let Some(index) = over_row {
            *self.hover_index.borrow_mut() = Some(index);
            return;
        }
        let over_tray = self.tray_bounds().is_some_and(|tray| ctx.hovered(tray));
        let hovered = *self.hover_index.borrow();
        let in_corridor = hovered
            .and_then(|index| self.row_bounds(index, app))
            .and_then(|row| self.tray_corridor(row))
            .is_some_and(|corridor| ctx.hovered(corridor));
        if !over_tray && !in_corridor {
            *self.hover_index.borrow_mut() = None;
        }
    }

    /// The keys an open menu owns: Up/Down move the highlight through the
    /// enabled rows, Enter selects the highlighted row and closes the menu,
    /// Escape closes it through `on_close`. A closed menu consumes nothing and
    /// every other key is left to the surface behind it.
    fn handle_key(&mut self, event: &DispatchedEvent) -> bool {
        if !*self.open.borrow() {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        // A chorded key is a shortcut of the surface behind the menu, not a
        // menu key: Cmd/Ctrl+Enter still belongs to the composer.
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = self.items.len();
                if len == 0 {
                    return true;
                }
                let delta = if key == "ArrowDown" { 1 } else { -1 };
                let current = *self.highlight_index.borrow();
                let Some(current) = current else {
                    // Nothing highlighted yet: the first enabled row is where a
                    // move starts.
                    *self.highlight_index.borrow_mut() =
                        (0..len).find(|index| !self.items[*index].disabled);
                    return true;
                };
                let mut index = current.min(len - 1);
                for _ in 0..len {
                    index = (index as isize + delta).rem_euclid(len as isize) as usize;
                    if !self.items[index].disabled {
                        *self.highlight_index.borrow_mut() = Some(index);
                        break;
                    }
                }
                true
            }
            "Enter" => {
                let Some(index) = *self.highlight_index.borrow() else {
                    return false;
                };
                let Some(item) = self.items.get(index) else {
                    return false;
                };
                if item.disabled {
                    return true;
                }
                if let Some(cb) = self.on_select.as_ref() {
                    (cb.borrow_mut())(index);
                }
                *self.open.borrow_mut() = false;
                true
            }
            "Escape" => {
                *self.open.borrow_mut() = false;
                if let Some(cb) = self.on_close.as_ref() {
                    (cb.borrow_mut())();
                }
                true
            }
            _ => false,
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        if !*self.open.borrow() {
            self.panel = None;
            self.panel_size = None;
            self.tray = None;
            self.tray_size = None;
            *self.hover_index.borrow_mut() = None;
            *self.highlight_index.borrow_mut() = None;
            return;
        }
        // An open menu with no highlight starts on the item in force (or the
        // first row), the way a dropdown opens on the value it holds.
        let mut highlight = self.highlight_index.borrow_mut();
        let stale = matches!(*highlight, Some(index) if index >= self.items.len());
        if highlight.is_none() || stale {
            *highlight = self
                .items
                .iter()
                .position(|item| item.selected)
                .or_else(|| (!self.items.is_empty()).then_some(0));
        }
        drop(highlight);
        if self.panel.is_none() {
            self.panel = Some(self.build_panel(app));
        }
    }

    fn build_panel(&mut self, app: &AppContext) -> Box<dyn Element> {
        let on_select = self.on_select.clone();
        let open = self.open.clone();
        let highlight = self.highlight_index.clone();
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(POPUP_ITEM_SPACING);
        // A row inside a rounded casing is a control of its own: it is as round
        // as the reference's menu rows, never as round as the panel (whose
        // curve holds it), and never square (the diff menu's old look is gone).
        let row_radius = self.corner_radius.min(POPUP_ITEM_RADIUS);
        for (index, item) in self.items.iter().enumerate() {
            // A pane separator is a full-width 1px rule between two groups of
            // rows, with four points of air above and below it.
            if item.separator_before {
                column = column.with_child(
                    Container::new(Divider::horizontal().finish())
                        .with_padding(EdgeInsets::new(0.0, 4.0, 0.0, 4.0))
                        .finish(),
                );
            }
            let label = item.label.clone();
            let icon = item.icon.clone();
            let selected = item.selected;
            let disabled = item.disabled;
            let is_open = open.clone();
            let cb = on_select.clone();
            let diff = item.diff;
            let description = item.description.clone();
            let row = PopupMenuItemView::new(
                index,
                highlight.clone(),
                label,
                icon,
                selected,
                disabled,
                diff,
                description,
                self.selected_color.unwrap_or(ColorToken::Accent),
                self.max_width,
                row_radius,
                self.row_height,
                self.icon_size,
                move || {
                    if !disabled {
                        if let Some(cb) = cb.as_ref() {
                            (cb.borrow_mut())(index);
                        }
                        *is_open.borrow_mut() = false;
                    }
                },
            )
            .finish();
            column = column.with_child(row);
        }
        // A capped panel is a viewport over the rows — the cap, or the whole
        // list when that is shorter, so a three-row directory does not leave
        // five rows of empty panel in the tray.
        let list: Box<dyn Element> = match self.max_visible_rows {
            Some(max_rows) => {
                let visible = max_rows.max(1).min(self.items.len().max(1)) as f32;
                let height = visible * POPUP_ITEM_HEIGHT + (visible - 1.0) * POPUP_ITEM_SPACING;
                ConstrainedBox::new(
                    Scrollable::new(column.finish(), Axis::Vertical)
                        .with_state(self.panel_scroll.state())
                        .finish(),
                )
                .with_max_height(height)
                .finish()
            }
            None => column.finish(),
        };
        // A footer row hangs below the list at its own height and outside the
        // list's viewport: the cap is a window over the *items*, and a row that
        // is not an item is neither scrolled away with them nor counted in.
        let content: Box<dyn Element> = match self.footer_row.take() {
            Some(row) => Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(POPUP_ITEM_SPACING)
                .with_child(list)
                .with_child(row)
                .finish(),
            None => list,
        };
        Container::new(content)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_corner_radius(self.corner_radius)
            .with_padding(EdgeInsets::uniform(POPUP_PANEL_PADDING))
            .finish()
    }

    /// Bring the highlighted row into the panel's window when the panel is
    /// capped and the highlight has moved — the keyboard half of a scrollable
    /// tray. Called from `layout`, after the viewport has been measured.
    ///
    /// Only a change of row moves the offset, so the wheel keeps whatever the
    /// user scrolled to, and an open menu still shows the row it is on.
    fn reveal_highlighted_row(&mut self) {
        if self.max_visible_rows.is_none() {
            return;
        }
        let highlighted = *self.highlight_index.borrow();
        let Some(index) = highlighted.or_else(|| self.items.iter().position(|item| item.selected))
        else {
            return;
        };
        if self.panel_scroll.revealed() == Some(index) {
            return;
        }
        let state = self.panel_scroll.state();
        let (offset, viewport) = {
            let state = state.borrow();
            (state.offset(), state.viewport())
        };
        // Nothing has measured the viewport yet: leave the row for the next
        // layout rather than marking it revealed against a zero-height window.
        if viewport <= 0.0 {
            return;
        }
        let top = self.item_top(index);
        let bottom = top + POPUP_ITEM_HEIGHT;
        let wanted = if top < offset {
            top
        } else if bottom > offset + viewport {
            bottom - viewport
        } else {
            offset
        };
        state.borrow_mut().scroll_by(wanted - offset);
        self.panel_scroll.mark_revealed(index);
    }
}

impl Default for PopupMenu {
    fn default() -> Self {
        Self::new(
            Box::new(crate::elements::Empty::new()),
            Vec::new(),
        )
    }
}

impl Element for PopupMenu {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let trigger_size = self.trigger.layout(constraint, ctx, app);
        self.size = Some(trigger_size);

        self.panel_origin = Vector2F::zero();
        if let Some(panel) = self.panel.as_mut() {
            // Keep the panel within the horizontal space actually available so
            // it re-anchors under its trigger and is never clipped/cut off when
            // the window or composer is resized (narrower than the max width).
            let available_width = if constraint.max.x.is_finite() {
                constraint.max.x
            } else {
                POPUP_MAX_WIDTH
            };
            let panel_size = panel.layout(
                SizeConstraint::loose(vec2f(available_width.min(self.max_width), 400.0)),
                ctx,
                app,
            );
            self.panel_size = Some(panel_size);
            // Default: right-align a panel narrower than its trigger so it sits
            // under the trigger, otherwise align to the trigger's leading edge.
            // A trailing-aligned panel keeps its right edge on the trigger's,
            // which is what lets a control near the window's right edge open to
            // the left rather than off-screen.
            let trailing = matches!(
                self.position,
                PopupMenuPosition::BelowEnd | PopupMenuPosition::AboveEnd
            );
            let mut x = if trailing {
                trigger_size.x - panel_size.x
            } else {
                (trigger_size.x - panel_size.x).max(0.0)
            };
            // Clamp so the whole panel stays within the available width (no
            // spill past the composer/window edge on a narrow or resized layout).
            let max_x = (constraint.max.x - panel_size.x).max(0.0);
            x = x.min(max_x);
            let y = match self.position {
                PopupMenuPosition::Below | PopupMenuPosition::BelowEnd => {
                    trigger_size.y + POPUP_GAP
                }
                PopupMenuPosition::Above | PopupMenuPosition::AboveEnd => {
                    -(panel_size.y + POPUP_GAP)
                }
            };
            self.panel_origin = vec2f(x + self.panel_offset.x, y + self.panel_offset.y);
            self.reveal_highlighted_row();
        }
        self.build_tray(constraint, ctx, app);
        trigger_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.trigger.paint(origin, ctx, app);
        // The menu may have been closed between layout and this paint (another
        // menu claimed the window, or the app cleared the flag): only the
        // trigger draws then.
        if !*self.open.borrow() {
            return;
        }
        if self.panel.is_some() {
            // The pointer on the panel belongs to the menu: behind it, nothing
            // hovers while the menu paints over it (see `PaintContext::hovered`).
            ctx.enter_menu_surface();
            if let Some(panel) = self.panel.as_mut() {
                panel.paint(origin + self.panel_origin, ctx, app);
            }
            if let Some(tray) = self.tray.as_mut() {
                tray.paint(origin + self.tray_origin, ctx, app);
            }
            self.track_hover(ctx, app);
            ctx.leave_menu_surface();
        }
        // Register the surface the menu owns. The root view reads it next frame
        // to gate hover behind it, and to decide that a press is inside the
        // menu rather than an outside dismiss.
        let mut surfaces = Vec::new();
        if let Some(panel) = self.panel_bounds() {
            surfaces.push(panel);
        }
        if let Some(tray) = self.tray_bounds() {
            surfaces.push(tray);
        }
        if let Some(corridor) = (*self.hover_index.borrow())
            .and_then(|index| self.row_bounds(index, app))
            .and_then(|row| self.tray_corridor(row))
        {
            surfaces.push(corridor);
        }
        app.open_menus
            .report(self.menu_id, self.close_handle(), surfaces, MenuPointer::Paint);
        if let Some(anchor) = self.bounds() {
            app.open_menus.report_anchor(self.menu_id, anchor);
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        if *self.open.borrow() {
            // An open menu owns the keys that pick from it; a closed one takes
            // none of them.
            if self.handle_key(event) {
                return true;
            }
            // A wheel carries no position to hit-test with, so it goes to the
            // panel and is taken by whichever region was under the pointer at
            // the last paint — the row list of a capped tray.
            if matches!(event, DispatchedEvent::Scroll { .. }) {
                return self
                    .panel
                    .as_mut()
                    .map(|panel| panel.dispatch_event(event, ctx, app))
                    .unwrap_or(false);
            }
            // The tray is drawn beside the panel, so it sits outside the panel's
            // own bounds: route events there first, and consume them so a click
            // on the tray's controls neither closes the menu nor falls through
            // to the "outside the panel" close below.
            if let Some(position) = event_position(event) {
                if self.tray_bounds().is_some_and(|tray| contains(tray, position)) {
                    let _ = self
                        .tray
                        .as_mut()
                        .map(|t| t.dispatch_event(event, ctx, app));
                    return true;
                }
            }
            if let Some(panel_bounds) = self.panel_bounds() {
                if self.panel_is_inside(event, panel_bounds) {
                    let consumed = self
                        .panel
                        .as_mut()
                        .map(|p| p.dispatch_event(event, ctx, app))
                        .unwrap_or(false);
                    // The panel is a surface, not only its rows: a press in the
                    // gap between two of its controls is still a press on the
                    // open menu. Letting it through would reach whatever the
                    // panel is drawn over — the tab a tab menu hangs on would
                    // take the click as its own.
                    return consumed
                        || matches!(
                            event,
                            DispatchedEvent::MouseDown { .. } | DispatchedEvent::MouseUp { .. }
                        );
                }
            }
            // A press that lands on the trigger itself should toggle the menu
            // closed, not be swallowed by the "outside the panel" close below
            // (which would otherwise let the release re-open it).
            if let Some(position) = event_position(event) {
                if contains(bounds, position) {
                    let cb = self.trigger_toggle(app);
                    let mut toggle = move || (cb.borrow_mut())();
                    return handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle);
                }
            }
            // Clicking outside the panel (but somewhere in the window) closes
            // it. Only the primary button dismisses a menu: a right press is a
            // gesture of its own — it is what opens a tab's menu, and on the tab
            // that menu already belongs to it is what closes it again — so
            // treating it as a dismissal here would have the two cancel out and
            // leave the menu standing.
            if matches!(
                event,
                DispatchedEvent::MouseDown {
                    button: BUTTON_PRIMARY,
                    ..
                }
            ) {
                *self.open.borrow_mut() = false;
                if let Some(cb) = self.on_close.as_ref() {
                    (cb.borrow_mut())();
                }
            }
            return false;
        }

        let cb = self.trigger_toggle(app);
        let mut toggle = move || (cb.borrow_mut())();
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
    }
}

/// The pointer position carried by a mouse event, if any.
fn event_position(event: &DispatchedEvent) -> Option<Vector2F> {
    match event {
        DispatchedEvent::MouseDown { position, .. }
        | DispatchedEvent::MouseUp { position, .. }
        | DispatchedEvent::MouseMove { position } => Some(*position),
        _ => None,
    }
}

trait PanelInside {
    fn panel_is_inside(&self, event: &DispatchedEvent, bounds: RectF) -> bool;
}

impl PanelInside for PopupMenu {
    fn panel_is_inside(&self, event: &DispatchedEvent, bounds: RectF) -> bool {
        match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => {
                bounds.contains(PointF::new(position.x, position.y))
            }
            _ => false,
        }
    }
}

/// A single clickable row inside the popup panel.
struct PopupMenuItemView {
    index: usize,
    /// The app-owned keyboard highlight. The row paints its background from
    /// this, not from `selected`: the current value still marks the active item
    /// in its label colour, while the keyboard highlight moves on its own until
    /// Enter commits it.
    highlight: Rc<RefCell<Option<usize>>>,
    label: String,
    icon: Option<&'static str>,
    selected: bool,
    disabled: bool,
    diff: Option<PopupMenuDiff>,
    /// A muted second line under the label, when the item has one.
    description: Option<String>,
    /// The ink of the selected label (the menu's own choice).
    selected_color: ColorToken,
    max_width: f32,
    corner_radius: f32,
    /// This menu's row height and glyph size overrides, if any.
    row_height: Option<f32>,
    icon_size: f32,
    state: InteractiveState,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    content: Option<Box<dyn Element>>,
    /// The width `content` was built at: a description is ellipsized to the
    /// row's own width, so a resize has to rebuild it.
    content_width: f32,
    content_origin: Vector2F,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PopupMenuItemView {
    #[allow(clippy::too_many_arguments)]
    fn new(
        index: usize,
        highlight: Rc<RefCell<Option<usize>>>,
        label: String,
        icon: Option<&'static str>,
        selected: bool,
        disabled: bool,
        diff: Option<PopupMenuDiff>,
        description: Option<String>,
        selected_color: ColorToken,
        max_width: f32,
        corner_radius: f32,
        row_height: Option<f32>,
        icon_size: f32,
        on_select: impl FnMut() + 'static,
    ) -> Self {
        Self {
            index,
            highlight,
            label,
            icon,
            selected,
            disabled,
            diff,
            description,
            selected_color,
            max_width,
            corner_radius,
            row_height,
            icon_size,
            state: InteractiveState::default(),
            on_select: Some(Rc::new(RefCell::new(on_select))),
            content: None,
            content_width: 0.0,
            content_origin: Vector2F::zero(),
            size: None,
            origin: None,
        }
    }

    fn build_content(&mut self, app: &AppContext, width: f32) {
        if self.content.is_some() && (self.content_width - width).abs() < 0.5 {
            return;
        }
        self.content_width = width;
        let text_color = if self.disabled {
            ColorToken::Muted
        } else if self.selected {
            self.selected_color
        } else {
            ColorToken::Text
        };
        let text = Text::new(self.label.clone())
            .with_theme_color(text_color, app)
            .with_font_size(POPUP_ITEM_LABEL_SIZE)
            .with_max_lines(1);
        // The row's first line: the glyph, the label, and whatever marks the
        // row's value at its trailing edge (a file's counts, the current
        // item's check). The trailing mark is right-aligned, so every row's
        // label starts on the same pen.
        let mut line = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0);
        if let Some(icon_name) = self.icon.clone() {
            line = line.with_child(
                Icon::new(icon_name)
                    .with_size(self.icon_size)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        match self.diff {
            // A changed file's row keeps the path on the left and its counts
            // on the right, the two ends of the row's full width.
            Some(diff) => {
                line = line
                    .with_child(Expanded::new(Clipped::new(text.finish()).finish()).finish())
                    .with_child(diff_counts(diff, app));
            }
            None => {
                line = line.with_child(Expanded::new(text.finish()).finish());
            }
        }
        // The item in force is marked with the reference's trailing check: the
        // label keeps its own colour and the check is the value's stamp.
        if self.selected && !self.disabled && self.diff.is_none() {
            line = line.with_child(
                Icon::new("check")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
        }
        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(line.finish());
        if let Some(description) = self.description.clone() {
            // One line, cut with an ellipsis at the row's own width: the menu
            // casing is a fixed panel, so a long description must not fold into
            // the row above it (the reference menu shows `…`).
            content = content.with_child(
                Text::new(ellipsize(
                    &description,
                    POPUP_ITEM_DESCRIPTION_SIZE,
                    FontWeight::Regular,
                    FontFamily::System,
                    width,
                ))
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(POPUP_ITEM_DESCRIPTION_SIZE)
                .with_max_lines(1)
                .finish(),
            );
        }
        self.content = Some(content.finish());
    }

    /// The row's own height: one line, or two when it carries a description.
    fn height(&self) -> f32 {
        if let Some(height) = self.row_height {
            return height;
        }
        if self.description.is_some() {
            POPUP_ITEM_WITH_DESCRIPTION_HEIGHT
        } else {
            POPUP_ITEM_HEIGHT
        }
    }
}

/// `text` cut to one line that fits `max_width`, with an ellipsis where it was
/// cut. A description has one row and the panel has a fixed width, so this is
/// what keeps it from folding into the label above it.
fn ellipsize(
    text: &str,
    font_size: f32,
    weight: FontWeight,
    family: FontFamily,
    max_width: f32,
) -> String {
    let ellipsis = advance_width("…", font_size, weight, family, false);
    let mut out = String::new();
    let mut width = 0.0;
    for ch in text.chars() {
        let mut buffer = [0u8; 4];
        let advance = advance_width(ch.encode_utf8(&mut buffer), font_size, weight, family, false);
        if width + advance + ellipsis > max_width {
            out.push('…');
            return out;
        }
        out.push(ch);
        width += advance;
    }
    out
}

/// The additions and deletions at a changed file row's trailing edge. An empty
/// category is left out, the way the diff control leaves out a zero count.
fn diff_counts(diff: PopupMenuDiff, app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    if diff.added > 0 {
        row = row.with_child(
            Text::new(format!("+{}", diff.added))
                .with_theme_color(ColorToken::Success, app)
                .with_font_size(11.0)
                .with_weight(FontWeight::SemiBold)
                .finish(),
        );
    }
    if diff.removed > 0 {
        row = row.with_child(
            Text::new(format!("-{}", diff.removed))
                .with_theme_color(ColorToken::Error, app)
                .with_font_size(11.0)
                .with_weight(FontWeight::SemiBold)
                .finish(),
        );
    }
    if diff.added == 0 && diff.removed == 0 {
        row = row.with_child(
            Text::new("0")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        );
    }
    row.finish()
}

impl Element for PopupMenuItemView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(constraint.max.x.min(self.max_width), self.height());
        let content_width = (size.x - 16.0).max(0.0);
        self.build_content(app, content_width);
        self.size = Some(size);
        let content = self.content.as_mut().unwrap();
        let _ = content.layout(
            SizeConstraint::loose(vec2f(content_width, (size.y - 8.0).max(0.0))),
            ctx,
            app,
        );
        let content_size = content.size().unwrap_or(Vector2F::zero());
        self.content_origin = vec2f(8.0, (size.y - content_size.y).max(0.0) / 2.0);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let hit = rectf(origin.x, origin.y, size.x, size.y);
        // The row under the keyboard and the row under the pointer read as the
        // same raised band. It is the selection colour composited half over the
        // panel's own fill — the reference's selected row is only a few luma
        // above the casing, not a full selection step — so the band stays a
        // quiet tint under the label and the trailing check.
        let highlighted = *self.highlight.borrow() == Some(self.index);
        let bg = if (highlighted || ctx.hovered(hit)) && !self.disabled {
            Some(
                app.theme
                    .color(ColorToken::SurfaceRaised)
                    .mix(&app.theme.color(ColorToken::Selected), 0.5),
            )
        } else {
            None
        };
        if let (Some(color), Some(renderer)) = (bg, ctx.renderer.as_mut()) {
            renderer.fill_rounded_rect(hit, color, self.corner_radius);
        }
        if let Some(content) = self.content.as_mut() {
            content.paint(origin + self.content_origin, ctx, app);
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let cb = self.on_select.clone();
        let mut on_select = move || {
            if let Some(cb) = cb.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_select)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Button, Empty, HoverRow, Text};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::render_element;

    /// One frame of the real loop for a menu: lay out, seed the open-menu gate
    /// from the registry (as the root view does), paint, reconcile.
    fn frame(menu: &mut PopupMenu, app: &AppContext, cursor: Option<Vector2F>) {
        menu.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(cursor) = cursor {
            ctx.cursor_position = cursor;
            ctx.cursor_inside = true;
        }
        ctx.open_menu_surfaces = app.open_menus.surfaces();
        app.open_menus.begin_paint();
        menu.paint(vec2f(0.0, 0.0), &mut ctx, app);
        app.open_menus.end_paint();
    }

    fn click_at(menu: &mut PopupMenu, x: f32, y: f32, app: &AppContext) {
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: vec2f(x, y),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: vec2f(x, y),
                button: 0,
            },
        ] {
            menu.dispatch_event(&event, &mut ctx, app);
        }
    }

    fn trigger() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(40.0, 32.0)).finish()
    }

    fn click(menu: &mut PopupMenu, x: f32, y: f32, app: &AppContext) {
        let mut ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(x, y),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(x, y),
            button: 0,
        };
        menu.dispatch_event(&down, &mut ctx, app);
        menu.dispatch_event(&up, &mut ctx, app);
    }

    #[test]
    fn opens_on_trigger_click_and_selects_item() {
        let items = vec![
            PopupMenuItem::new("One").with_icon("cpu"),
            PopupMenuItem::new("Two"),
        ];
        let selected = Rc::new(RefCell::new(None));
        let selected_clone = selected.clone();
        let mut menu = PopupMenu::new(trigger(), items)
            .with_position(PopupMenuPosition::Above)
            .with_on_select(move |index| *selected_clone.borrow_mut() = Some(index));
        let app = AppContext::default();

        menu.layout(
            SizeConstraint::loose(vec2f(200.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        // Click on the trigger opens the menu.
        click(&mut menu, 10.0, 16.0, &app);
        assert!(menu.is_open());

        // The app rebuilds the tree each frame, so layout again to build the panel.
        menu.layout(
            SizeConstraint::loose(vec2f(200.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        // The panel appears above the trigger (negative y). Click the first
        // item, which sits a few pixels into the panel.
        click(&mut menu, 10.0, -64.0, &app);
        assert!(!menu.is_open());
        assert_eq!(*selected.borrow(), Some(0));
    }

    #[test]
    fn a_trailing_panel_aligns_its_right_edge_with_the_trigger() {
        let items = vec![
            PopupMenuItem::new("A long label"),
            PopupMenuItem::new("Another item"),
        ];
        let app = AppContext::default();
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::new(RefCell::new(true)))
            .with_position(PopupMenuPosition::BelowEnd);
        // The trigger is 40 wide and sits at the right end of a 360 wide row,
        // which is the dots button at the pane's right edge.
        menu.layout(
            SizeConstraint::loose(vec2f(360.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let size = menu.panel_size.expect("panel built");
        assert!(size.x > 40.0, "the panel is wider than its trigger");
        assert!(
            (menu.panel_origin.x + size.x - 40.0).abs() < 0.01,
            "the panel's right edge sits on the trigger's: origin {} size {}",
            menu.panel_origin.x,
            size.x
        );
    }

    #[test]
    fn panel_reanchors_and_stays_within_available_width_on_resize() {
        let items = vec![
            PopupMenuItem::new("A long label that wraps"),
            PopupMenuItem::new("Another long item"),
        ];
        let open = Rc::new(RefCell::new(true));
        let mut menu = PopupMenu::new(trigger(), items).with_open(open);
        let app = AppContext::default();

        // Narrow layout: the panel must not overflow the available width.
        menu.layout(
            SizeConstraint::loose(vec2f(120.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let narrow_size = menu.panel_size.expect("panel built");
        assert!(
            narrow_size.x <= 120.0,
            "panel width {} exceeds available width",
            narrow_size.x
        );
        assert!(menu.panel_origin.x >= 0.0);
        assert!(
            menu.panel_origin.x + narrow_size.x <= 120.0,
            "panel overflows available width on narrow layout"
        );

        // Widened layout: the panel re-anchors under the trigger and fits.
        menu.layout(
            SizeConstraint::loose(vec2f(360.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let wide_size = menu.panel_size.expect("panel built");
        assert!(wide_size.x <= POPUP_MAX_WIDTH);
        assert!(
            menu.panel_origin.x + wide_size.x <= 360.0,
            "panel overflows available width after resize"
        );
    }

    /// A menu whose second item carries a tray with a "Make default" button and
    /// whose first item carries none.
    fn menu_with_a_tray(clicked: Rc<RefCell<usize>>) -> (PopupMenu, Rc<RefCell<bool>>) {
        let open = Rc::new(RefCell::new(true));
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::clone(&open))
            .with_hover_tray(180.0, move |index, _app| {
                (index == 1).then(|| {
                    let clicked = Rc::clone(&clicked);
                    Button::new(Text::new("Make default").finish())
                        .with_on_click(move || *clicked.borrow_mut() = index)
                        .finish()
                })
            });
        (menu, open)
    }

    /// Put the pointer over item `index` and lay the menu out again, so the tray
    /// that pointer calls for is in the tree (hover is read at paint time, one
    /// frame before the tray is built).
    fn hover_item(menu: &mut PopupMenu, index: usize, app: &AppContext, width: f32, origin: Vector2F) {
        let cursor = menu
            .row_bounds(index, app)
            .map(|row| vec2f(row.min_x() + 4.0, row.min_y() + 4.0))
            .expect("the panel is laid out, so its rows have bounds");
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = cursor;
        paint_ctx.cursor_inside = true;
        menu.paint(origin, &mut paint_ctx, app);
        menu.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(origin, &mut paint_ctx, app);
    }

    fn lay_out_open(menu: &mut PopupMenu, app: &AppContext, width: f32, origin: Vector2F) {
        menu.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(origin, &mut PaintContext::default(), app);
    }

    /// Move the paint-time cursor to `point` and rebuild the tree, the way a
    /// frame with the pointer there does: what the pointer holds open is what
    /// the next layout builds.
    fn move_pointer(
        menu: &mut PopupMenu,
        point: Vector2F,
        app: &AppContext,
        width: f32,
        origin: Vector2F,
    ) {
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = point;
        paint_ctx.cursor_inside = true;
        menu.paint(origin, &mut paint_ctx, app);
        menu.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(origin, &mut paint_ctx, app);
    }

    #[test]
    fn hovering_a_row_shows_its_tray_beside_it() {
        let clicked = Rc::new(RefCell::new(usize::MAX));
        let (mut menu, _open) = menu_with_a_tray(Rc::clone(&clicked));
        let app = AppContext::default();
        let at = vec2f(0.0, 0.0);
        lay_out_open(&mut menu, &app, 600.0, at);
        assert!(
            menu.tray.is_none(),
            "no tray before the pointer is over a row"
        );

        hover_item(&mut menu, 1, &app, 600.0, at);
        let tray_size = menu
            .tray
            .as_ref()
            .and_then(|tray| tray.size())
            .expect("the hovered row's tray is laid out");
        let panel = menu.panel_bounds().expect("the panel has bounds");
        let row = menu.row_bounds(1, &app).expect("the hovered row");

        // Beside the panel the row belongs to, one `TRAY_GAP` away, hanging from
        // the row's top edge.
        let tray = menu.tray_bounds().expect("the tray has bounds");
        assert!(
            (tray.min_x() - (panel.max_x() + TRAY_GAP)).abs() < 0.01,
            "the tray sits {TRAY_GAP} px right of the panel (tray at {}, panel ends at {})",
            tray.min_x(),
            panel.max_x()
        );
        assert!(
            !tray.intersects(&panel),
            "and does not overlap the panel it belongs to"
        );
        assert!(
            (tray.min_y() - row.min_y()).abs() < 0.01,
            "the tray hangs from the hovered row's top edge (tray {}, row {})",
            tray.min_y(),
            row.min_y()
        );
        assert!(tray_size.x > 0.0 && tray_size.y > 0.0);

        // An item whose builder returns `None` shows no tray.
        hover_item(&mut menu, 0, &app, 600.0, at);
        assert!(
            menu.tray.is_none(),
            "the first item carries no tray, so none is drawn for it"
        );
    }

    /// The tray is four points past the panel, so the pointer crosses a gap to
    /// reach it: the row stays hovered while the pointer is in that gap, and
    /// the tray survives the crossing.
    #[test]
    fn the_pointer_can_cross_from_the_row_to_its_tray() {
        let clicked = Rc::new(RefCell::new(usize::MAX));
        let (mut menu, _open) = menu_with_a_tray(Rc::clone(&clicked));
        let app = AppContext::default();
        let at = vec2f(0.0, 0.0);
        lay_out_open(&mut menu, &app, 600.0, at);
        hover_item(&mut menu, 1, &app, 600.0, at);

        let row = menu.row_bounds(1, &app).expect("the hovered row");
        let tray = menu.tray_bounds().expect("the tray beside it");
        assert!(
            tray.min_x() > row.max_x(),
            "the tray sits to the right of its row: {tray:?} after {row:?}"
        );

        // In the gap, level with the row: still the row's tray.
        let gap = vec2f(
            (row.max_x() + tray.min_x()) / 2.0,
            row.min_y() + POPUP_ITEM_HEIGHT / 2.0,
        );
        move_pointer(&mut menu, gap, &app, 600.0, at);
        assert_eq!(
            *menu.hover_index.borrow(),
            Some(1),
            "the row stays hovered while the pointer crosses the gap"
        );
        assert!(
            menu.tray.is_some(),
            "and its tray is still drawn on the next frame"
        );

        // Onto the tray's own top-left: the pointer over the tray keeps it.
        let on_tray = vec2f(tray.min_x() + 4.0, tray.min_y() + 4.0);
        move_pointer(&mut menu, on_tray, &app, 600.0, at);
        assert!(
            menu.tray.is_some(),
            "the tray stays while the pointer is on it"
        );

        // Away from both: the tray closes.
        move_pointer(&mut menu, vec2f(0.0, 380.0), &app, 600.0, at);
        assert_eq!(*menu.hover_index.borrow(), None, "the row is left");
        assert!(menu.tray.is_none(), "and the tray goes with it");
    }

    #[test]
    fn a_tray_click_fires_its_own_button_and_keeps_the_menu_open() {
        let clicked = Rc::new(RefCell::new(usize::MAX));
        let (mut menu, open) = menu_with_a_tray(Rc::clone(&clicked));
        let app = AppContext::default();
        let at = vec2f(0.0, 0.0);
        lay_out_open(&mut menu, &app, 600.0, at);
        hover_item(&mut menu, 1, &app, 600.0, at);

        // Press on the tray's button, rebuild the tree the way a redraw does
        // (the pointer is still on the tray, which is what keeps it in the tree)
        // and release on it.
        let mut ctx = EventContext::default();
        let mut press_and_release = |menu: &mut PopupMenu| {
            let tray = menu.tray_bounds().expect("the tray has bounds");
            let pos = vec2f(tray.min_x() + 8.0, tray.min_y() + 8.0);
            let down = DispatchedEvent::MouseDown { position: pos, button: 0 };
            let up = DispatchedEvent::MouseUp { position: pos, button: 0 };
            let _ = menu.dispatch_event(&down, &mut ctx, &app);
            let _ = menu.dispatch_event(&up, &mut ctx, &app);
        };
        press_and_release(&mut menu);
        assert_eq!(*clicked.borrow(), 1, "the tray's own button took the click");
        assert!(*open.borrow(), "a click on the tray keeps the menu open");

        // The rows still select, and selecting one still closes the menu.
        let row = menu.row_bounds(0, &app).expect("row zero");
        click(&mut menu, row.min_x() + 4.0, row.min_y() + 4.0, &app);
        assert_eq!(
            *clicked.borrow(),
            1,
            "a click on the row is not the tray's button"
        );
        assert!(!*open.borrow(), "selecting a row closes the menu");
    }

    #[test]
    fn a_tray_that_would_leave_the_window_renders_to_the_left() {
        let app = AppContext::default();
        let width = 600.0;
        // A trailing menu at the window's right edge (the topbar's "▾" control):
        // its panel opens leftwards from the trigger, so a tray to the panel's
        // right would leave the window.
        let at = vec2f(width - 40.0, 0.0);
        let open = Rc::new(RefCell::new(true));
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(open)
            .with_position(PopupMenuPosition::BelowEnd)
            .with_hover_tray(180.0, move |index, _app| {
                (index == 1).then(|| Button::new(Text::new("Make default").finish()).finish())
            });
        lay_out_open(&mut menu, &app, width, at);
        hover_item(&mut menu, 1, &app, width, at);

        let tray = menu.tray_bounds().expect("the tray has bounds");
        let panel = menu.panel_bounds().expect("the panel has bounds");
        assert!(
            tray.max_x() <= panel.min_x(),
            "the tray renders left of the panel when it does not fit on the right: \
             tray {}..{}, panel starts at {}",
            tray.min_x(),
            tray.max_x(),
            panel.min_x()
        );
        assert!(
            tray.min_x() >= 0.0 && tray.max_x() <= width,
            "and stays inside the window: {}..{}",
            tray.min_x(),
            tray.max_x()
        );
    }

    #[test]
    fn the_tray_survives_the_per_frame_rebuild_through_the_shared_hover_cell() {
        // The app builds a new element tree every frame, so the row under the
        // pointer cannot live in the menu element: it is an app-owned cell,
        // written at paint and read by the next frame's layout.
        let hover: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let app = AppContext::default();
        let build = |hover: &Rc<RefCell<Option<usize>>>| {
            PopupMenu::new(
                trigger(),
                vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")],
            )
            .with_open(Rc::new(RefCell::new(true)))
            .with_hover_index(Rc::clone(hover))
            .with_hover_tray(180.0, |index, _app| {
                (index == 1).then(|| Button::new(Text::new("Make default").finish()).finish())
            })
        };

        // This frame: the pointer is over the second row, which painting records.
        let mut frame = build(&hover);
        lay_out_open(&mut frame, &app, 600.0, vec2f(0.0, 0.0));
        let row = frame.row_bounds(1, &app).expect("row one");
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = vec2f(row.min_x() + 4.0, row.min_y() + 4.0);
        paint_ctx.cursor_inside = true;
        frame.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        assert_eq!(*hover.borrow(), Some(1), "the frame records the hovered row");
        drop(frame);

        // The next frame is a brand-new menu; the shared cell carries the hover.
        let mut next_frame = build(&hover);
        next_frame.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            next_frame.tray.is_some(),
            "the rebuilt menu still draws the tray for the row under the pointer"
        );
    }

    /// The height of a tray capped at [`MENU_MAX_VISIBLE_ROWS`] rows.
    fn cap_height() -> f32 {
        let rows = MENU_MAX_VISIBLE_ROWS as f32;
        rows * POPUP_ITEM_HEIGHT + (rows - 1.0) * POPUP_ITEM_SPACING
    }

    /// A capped tray of `count` directory rows, the way the rich input's
    /// working-directory menu asks for one. `highlighted` is the row the app
    /// marks as the one the keyboard is on.
    fn directory_menu(
        count: usize,
        highlighted: Option<usize>,
        scroll: &PanelScroll,
    ) -> PopupMenu {
        let items = (0..count)
            .map(|i| {
                let item = PopupMenuItem::new(format!("dir-{i:02}"));
                if highlighted == Some(i) {
                    item.selected()
                } else {
                    item
                }
            })
            .collect();
        PopupMenu::new(trigger(), items)
            .with_open(Rc::new(RefCell::new(true)))
            .with_max_visible_rows(MENU_MAX_VISIBLE_ROWS)
            .with_panel_scroll(scroll.clone())
    }

    /// A directory with more entries than the cap draws a window of the cap's
    /// height, clutched to its rows, instead of a panel taller than the pane.
    #[test]
    fn a_directory_with_more_rows_than_the_cap_draws_a_window_of_the_cap() {
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = AppContext::default();
        let size = vec2f(600.0, 700.0);
        let mut menu = directory_menu(20, Some(0), &PanelScroll::new());
        let _ = menu.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let panel = menu.panel_size.expect("the panel is built");
        let expected = cap_height() + POPUP_PANEL_PADDING * 2.0;
        assert!(
            (panel.y - expected).abs() < 0.01,
            "twenty rows are drawn in an eight-row window: panel {} px, expected {expected}",
            panel.y
        );
        let shown: Vec<usize> = (0..20).filter(|i| menu.row_bounds(*i, &app).is_some()).collect();
        assert_eq!(
            shown,
            (0..MENU_MAX_VISIBLE_ROWS).collect::<Vec<usize>>(),
            "only the rows inside the window have bounds"
        );

        let mut element = directory_menu(20, Some(0), &PanelScroll::new()).finish();
        let commands = render_element(&mut element, size, &app);
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, RenderCommand::ClipRect { .. })),
            "and the window clips the rows it cannot show: {commands:?}"
        );
    }

    /// A wheel over the tray moves its rows, and the offset is the app's cell,
    /// so the next frame's rebuild does not throw it away.
    #[test]
    fn a_wheel_over_the_tray_scrolls_its_rows() {
        let app = AppContext::default();
        let size = vec2f(600.0, 700.0);
        let scroll = PanelScroll::new();
        let state = scroll.state();
        let mut menu = directory_menu(20, Some(0), &scroll);
        let _ = menu.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);

        // A wheel carries no position to hit-test with, so the region that was
        // under the pointer at the last paint takes it: paint with the pointer
        // over the panel's rows.
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let panel = menu.panel_bounds().expect("the panel has bounds");
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = vec2f(panel.min_x() + 10.0, panel.min_y() + 10.0);
        paint_ctx.cursor_inside = true;
        menu.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);

        let row = menu.row_bounds(3, &app).expect("row three is in the window");
        let mut ctx = EventContext::default();
        assert!(
            menu.dispatch_event(&DispatchedEvent::scroll(vec2f(0.0, -40.0)), &mut ctx, &app),
            "the tray takes the wheel over its rows"
        );
        assert_eq!(state.borrow().offset(), 40.0, "the content moved up by 40 px");

        // The rows are drawn shifted by the offset they moved, and the ones
        // that left the window are no longer in it.
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let moved = menu.row_bounds(3, &app).expect("row three is still in the window");
        assert!(
            (row.min_y() - moved.min_y() - 40.0).abs() < 0.01,
            "the rows shifted by the offset: {} -> {}",
            row.min_y(),
            moved.min_y()
        );
        assert!(
            menu.row_bounds(0, &app).is_none(),
            "and the first row scrolled out of the window"
        );

        // The app rebuilds the tree every frame, and the highlighted row has not
        // moved, so the wheel's offset is kept rather than scrolled back.
        let mut next_frame = directory_menu(20, Some(0), &scroll);
        let _ = next_frame.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);
        assert_eq!(
            state.borrow().offset(),
            40.0,
            "the next frame keeps the wheel's offset"
        );
    }

    /// The keyboard half: a highlight moved below the window is brought into it,
    /// and moving it back up brings the earlier row back.
    #[test]
    fn moving_the_highlight_below_the_window_brings_it_into_view() {
        let app = AppContext::default();
        let size = vec2f(600.0, 700.0);
        let scroll = PanelScroll::new();
        let state = scroll.state();
        let mut menu = directory_menu(20, Some(0), &scroll);
        let _ = menu.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);
        assert_eq!(state.borrow().offset(), 0.0, "the first row opens at the top");

        // The app moves its highlight down the list and rebuilds the tree, the
        // way a host that answered ArrowDown does.
        let mut menu = directory_menu(20, Some(15), &scroll);
        let _ = menu.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);
        let (offset, viewport) = {
            let state = state.borrow();
            (state.offset(), state.viewport())
        };
        assert_eq!(viewport, cap_height(), "the window is the cap's height");
        let top = 15.0 * (POPUP_ITEM_HEIGHT + POPUP_ITEM_SPACING);
        assert!(offset > 0.0, "the window moved down to reach the row");
        assert!(
            offset <= top && offset + viewport >= top + POPUP_ITEM_HEIGHT,
            "and holds the whole highlighted row: offset {offset}, window {viewport}, row at {top}"
        );

        // Moving back up brings the earlier row into the window again.
        let mut menu = directory_menu(20, Some(2), &scroll);
        let _ = menu.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), &app);
        let (offset, viewport) = {
            let state = state.borrow();
            (state.offset(), state.viewport())
        };
        let top = 2.0 * (POPUP_ITEM_HEIGHT + POPUP_ITEM_SPACING);
        assert!(
            offset <= top && offset + viewport >= top + POPUP_ITEM_HEIGHT,
            "row two is inside the window: offset {offset}, window {viewport}"
        );
    }

    /// Send one key to a laid-out menu; the app supplies the default (no)
    /// modifiers.
    fn press(menu: &mut PopupMenu, key: &str, app: &AppContext) -> bool {
        let mut ctx = EventContext::default();
        menu.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            app,
        )
    }

    /// An open menu with no app highlight starts on the item in force, and the
    /// keys move that highlight through enabled rows only.
    #[test]
    fn arrow_keys_move_the_highlight_through_enabled_rows() {
        let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let items = vec![
            PopupMenuItem::new("One"),
            PopupMenuItem::new("Two").disabled(),
            PopupMenuItem::new("Three").selected(),
        ];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::new(RefCell::new(true)))
            .with_highlight_index(Rc::clone(&highlight));
        let app = AppContext::default();
        lay_out_open(&mut menu, &app, 600.0, vec2f(0.0, 0.0));

        assert_eq!(
            *highlight.borrow(),
            Some(2),
            "an open menu starts on the item in force"
        );

        assert!(press(&mut menu, "ArrowDown", &app));
        assert_eq!(
            *highlight.borrow(),
            Some(0),
            "the highlight wraps past the last row and skips the disabled one"
        );
        assert!(press(&mut menu, "ArrowUp", &app));
        assert_eq!(
            *highlight.borrow(),
            Some(2),
            "and wraps back the other way over the disabled row"
        );
    }

    /// Enter selects the highlighted row and closes the menu; a rebuild after
    /// the close resets the app's highlight to `None`.
    #[test]
    fn enter_selects_the_highlighted_row_and_closes_the_menu() {
        let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let open = Rc::new(RefCell::new(true));
        let picked = Rc::new(RefCell::new(None));
        let click = Rc::clone(&picked);
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::clone(&open))
            .with_highlight_index(Rc::clone(&highlight))
            .with_on_select(move |index| *click.borrow_mut() = Some(index));
        let app = AppContext::default();
        lay_out_open(&mut menu, &app, 600.0, vec2f(0.0, 0.0));

        assert!(press(&mut menu, "ArrowDown", &app));
        assert_eq!(*highlight.borrow(), Some(1));
        assert!(press(&mut menu, "Enter", &app));
        assert_eq!(*picked.borrow(), Some(1), "Enter takes the highlighted row");
        assert!(!*open.borrow(), "and closes the menu");

        let _ = menu.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(
            *highlight.borrow(),
            None,
            "the closed menu resets the shared highlight"
        );
    }

    /// Escape closes through `on_close`, and Enter on a disabled row selects
    /// nothing.
    #[test]
    fn escape_closes_through_on_close_and_disabled_rows_are_not_selected() {
        let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let open = Rc::new(RefCell::new(true));
        let closed = Rc::new(RefCell::new(false));
        let on_close = Rc::clone(&closed);
        let picked = Rc::new(RefCell::new(None));
        let click = Rc::clone(&picked);
        let items = vec![PopupMenuItem::new("One").disabled(), PopupMenuItem::new("Two")];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::clone(&open))
            .with_highlight_index(Rc::clone(&highlight))
            .with_on_select(move |index| *click.borrow_mut() = Some(index))
            .with_on_close(move || *on_close.borrow_mut() = true);
        let app = AppContext::default();
        lay_out_open(&mut menu, &app, 600.0, vec2f(0.0, 0.0));

        // The menu opens on row zero even though it is disabled; Enter there
        // neither selects nor closes.
        assert_eq!(*highlight.borrow(), Some(0));
        assert!(press(&mut menu, "Enter", &app));
        assert_eq!(*picked.borrow(), None);
        assert!(*open.borrow());

        assert!(press(&mut menu, "Escape", &app));
        assert!(!*open.borrow(), "Escape closes the menu");
        assert!(*closed.borrow(), "and reports the close through on_close");
    }

    /// A closed menu consumes no keys, so the surface behind it keeps them.
    #[test]
    fn a_closed_menu_consumes_no_keys() {
        let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(Some(1)));
        let closed = Rc::new(RefCell::new(false));
        let on_close = Rc::clone(&closed);
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::new(RefCell::new(false)))
            .with_highlight_index(Rc::clone(&highlight))
            .with_on_close(move || *on_close.borrow_mut() = true);
        let app = AppContext::default();
        lay_out_open(&mut menu, &app, 600.0, vec2f(0.0, 0.0));

        for key in ["ArrowDown", "ArrowUp", "Enter", "Escape"] {
            assert!(
                !press(&mut menu, key, &app),
                "{key} is not the closed menu's to take"
            );
        }
        assert!(!*closed.borrow());
        assert_eq!(*highlight.borrow(), None, "a closed menu holds no highlight");
    }

    /// The panel is a casing: its rows sit one padding inside it, they are
    /// rounded by their own (smaller) radius, and the selected row carries the
    /// trailing check. The environment menu's second line is drawn under the
    /// label.
    #[test]
    fn the_panel_is_a_casing_around_rounded_rows() {
        let items = vec![
            PopupMenuItem::new("This Computer").selected(),
            PopupMenuItem::new("Remote")
                .with_icon("cloud")
                .with_description("Clones this repo in a cloud environment…"),
        ];
        let mut menu = PopupMenu::new(trigger(), items).with_open(Rc::new(RefCell::new(true)));
        let app = AppContext::default();
        let at = vec2f(30.0, 40.0);
        lay_out_open(&mut menu, &app, 600.0, at);

        let panel = menu.panel_bounds().expect("the panel has bounds");
        let row = menu.row_bounds(0, &app).expect("the first row");
        assert!(
            (row.min_x() - panel.min_x() - POPUP_PANEL_PADDING).abs() < 0.01,
            "the row is inset by the casing padding ({row:?} in {panel:?})"
        );
        assert!(
            (panel.max_x() - row.max_x() - POPUP_PANEL_PADDING).abs() < 0.01,
            "on both sides ({row:?} in {panel:?})"
        );

        let mut element = menu.finish();
        let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
        // The casing is the 12pt panel and the rows are 6pt controls inside it.
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { corner_radius, .. }
                    if (*corner_radius - POPUP_PANEL_RADIUS).abs() < 0.01
            )),
            "the panel is drawn at the menu casing radius: {commands:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, size, .. } if name == "check" && (*size - 14.0).abs() < 0.01
            )),
            "the selected row is stamped with the trailing check: {commands:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, font_size, .. }
                    if text.starts_with("Clones this repo")
                        && text.ends_with('…')
                        && (*font_size - POPUP_ITEM_DESCRIPTION_SIZE).abs() < 0.01
            )),
            "the description line is one ellipsized line under the label: {commands:?}"
        );
    }
/// The selected label's ink is the menu's own choice: the shared casing keeps
/// `Accent` (so the composer menus are untouched), while a menu that asks for
/// another token draws the selected label in it — the Workspaces view options
/// pass `Focus`, the web's accent cyan, so the current choice is as bright as
/// its neighbours. The trailing check stays the primary text ink either way.
#[test]
fn the_selected_label_ink_follows_the_menus_choice() {
    let app = AppContext::default();
    let selected_ink = |selected_color: Option<ColorToken>| {
        let items = vec![
            PopupMenuItem::new("Folder").selected(),
            PopupMenuItem::new("Flat list"),
        ];
        let mut menu = PopupMenu::new(trigger(), items).with_open(Rc::new(RefCell::new(true)));
        if let Some(token) = selected_color {
            menu = menu.with_selected_color(token);
        }
        lay_out_open(&mut menu, &app, 600.0, vec2f(30.0, 40.0));
        let mut element = menu.finish();
        let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
        let ink = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == "Folder" => Some(*color),
                _ => None,
            })
            .expect("the selected label is drawn");
        let check_ink = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, color, .. } if name == "check" => Some(*color),
                _ => None,
            })
            .expect("the selected row is stamped with the trailing check");
        assert_eq!(
            check_ink,
            app.theme.color(ColorToken::Text),
            "the trailing check keeps the primary text ink"
        );
        ink
    };

    assert_eq!(
        selected_ink(None),
        app.theme.color(ColorToken::Accent),
        "the shared casing's selected label keeps its Accent ink"
    );
    assert_eq!(
        selected_ink(Some(ColorToken::Focus)),
        app.theme.color(ColorToken::Focus),
        "a menu that asks for Focus draws the selected label in it"
    );
    assert_ne!(
        app.theme.color(ColorToken::Focus),
        app.theme.color(ColorToken::Accent),
        "the two tokens are different inks, so the assertion above means something"
    );
}

/// A pane separator: a row of the second group (the branch menu's "Work in a
/// New Worktree") opens under a full-width 1px `Border` rule, and the row
/// geometry follows the separator so hit-testing and hover stay on the rows.
#[test]
fn a_pane_separator_divides_two_groups_of_rows() {
    let app = AppContext::default();
    let items = vec![
        PopupMenuItem::new("main").selected(),
        PopupMenuItem::new("feature/checkout-flow"),
        PopupMenuItem::new("Work in a New Worktree")
            .with_icon("branch")
            .with_description("Keeps your checkout untouched")
            .with_separator_before(),
    ];
    let open = Rc::new(RefCell::new(true));
    let mut menu = PopupMenu::new(trigger(), items).with_open(open);
    let (width, at) = (600.0, vec2f(20.0, 30.0));
    lay_out_open(&mut menu, &app, width, at);

    let border = app.theme.color(ColorToken::Border);
    let mut element = menu.finish();
    let commands = render_element(&mut element, vec2f(width, 400.0), &app);
    let rules: Vec<crate::geometry::RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == border && (rect.height() - 1.0).abs() < 0.5 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(rules.len(), 1, "one separator rule in the panel: {rules:?}");
    let rule = rules[0];
    assert!(
        rule.min_y() > 30.0 + POPUP_PANEL_PADDING + POPUP_ITEM_HEIGHT + POPUP_ITEM_SPACING,
        "the rule sits between the two groups: {rule:?}"
    );

    // The third row's bounds are its own: below the rule and taller than the
    // plain rows because it carries a description.
    let mut menu = PopupMenu::new(
        trigger(),
        vec![
            PopupMenuItem::new("main").selected(),
            PopupMenuItem::new("feature/checkout-flow"),
            PopupMenuItem::new("Work in a New Worktree")
                .with_description("Keeps your checkout untouched")
                .with_separator_before(),
        ],
    )
    .with_open(Rc::new(RefCell::new(true)));
    lay_out_open(&mut menu, &app, width, at);
    let first = menu.row_bounds(0, &app).expect("the first row");
    let plain = first.height();
    let third = menu.row_bounds(2, &app).expect("the described row");
    assert!(
        third.min_y() > first.max_y() + plain,
        "the described row starts under the separator and the row before it: \
         first {first:?}, third {third:?}"
    );
    assert!(
        third.height() > plain,
        "the described row is taller than a plain row: {third:?}"
    );
}


    // ---- the window's shared open-menu state ------------------------------

    #[test]
    fn opening_a_menu_claims_the_shared_state_and_closes_the_previous_menu() {
        let app = AppContext::default();
        let mut first = PopupMenu::new(trigger(), vec![PopupMenuItem::new("A")])
            .with_menu_id("test.first");
        let mut second = PopupMenu::new(trigger(), vec![PopupMenuItem::new("B")])
            .with_menu_id("test.second");

        frame(&mut first, &app, None);
        frame(&mut second, &app, None);
        click_at(&mut first, 10.0, 16.0, &app);
        assert!(first.is_open());
        assert_eq!(app.open_menus.id(), Some("test.first"));

        click_at(&mut second, 10.0, 16.0, &app);
        assert!(second.is_open(), "the second menu opened");
        assert!(!first.is_open(), "opening another menu closed the first");
        assert_eq!(app.open_menus.id(), Some("test.second"));
    }

    #[test]
    fn hovering_the_open_panel_hovers_the_panel_not_the_element_behind_it() {
        let app = AppContext::default();
        let mut menu = PopupMenu::new(trigger(), vec![PopupMenuItem::new("Row one")])
            .with_menu_id("test.hover")
            .with_position(PopupMenuPosition::Above);
        // Open through the trigger, then paint the opened panel so the registry
        // learns the rectangle it owns.
        frame(&mut menu, &app, None);
        click_at(&mut menu, 10.0, 16.0, &app);
        let panel = {
            menu.layout(
                SizeConstraint::loose(vec2f(400.0, 400.0)),
                &mut LayoutContext::default(),
                &app,
            );
            menu.panel_bounds().expect("the panel was laid out")
        };
        // A point inside the panel's first row (the panel's own padding is 6).
        let point = vec2f(panel.min_x() + 8.0, panel.min_y() + 12.0);
        frame(&mut menu, &app, Some(point));
        assert_eq!(
            *menu.hover_index.borrow(),
            Some(0),
            "the menu's own row hovers under the pointer"
        );

        // Next frame: the registry owns the pointer on that surface. A row
        // painted behind it must not read as hovered.
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_position = point;
        ctx.cursor_inside = true;
        ctx.open_menu_surfaces = app.open_menus.surfaces();
        let hover_cell: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let mut behind = HoverRow::new(Text::new("behind").finish())
            .with_hover_key(Rc::clone(&hover_cell), "behind");
        behind.layout(
            SizeConstraint::loose(vec2f(300.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        behind.paint(vec2f(point.x - 20.0, point.y - 10.0), &mut ctx, &app);
        assert_eq!(
            *hover_cell.borrow(),
            None,
            "the row behind the open menu must not hover while the pointer is on the menu"
        );

        // Outside the panel, the same row hovers normally: the menu does not
        // shadow the whole window, only its own surface.
        let mut outside = PaintContext::new(Renderer::new());
        outside.cursor_position = vec2f(350.0, 350.0);
        outside.cursor_inside = true;
        outside.open_menu_surfaces = app.open_menus.surfaces();
        behind.paint(vec2f(340.0, 340.0), &mut outside, &app);
        assert_eq!(
            *hover_cell.borrow(),
            Some("behind".to_string()),
            "hover outside the open menu still works"
        );
    }

    #[test]
    fn a_press_outside_closes_the_menu_and_clears_the_shared_state() {
        let app = AppContext::default();
        let mut menu = PopupMenu::new(trigger(), vec![PopupMenuItem::new("Row one")])
            .with_menu_id("test.dismiss");
        frame(&mut menu, &app, None);
        click_at(&mut menu, 10.0, 16.0, &app);
        frame(&mut menu, &app, None);
        assert_eq!(app.open_menus.id(), Some("test.dismiss"));

        // A press well outside the trigger and the panel.
        click_at(&mut menu, 300.0, 300.0, &app);
        assert!(!menu.is_open(), "an outside press closed the menu");
        frame(&mut menu, &app, None);
        assert!(!app.open_menus.is_open(), "the shared state let go of it");
    }

    #[test]
    fn escape_closes_the_menu_and_clears_the_shared_state() {
        let app = AppContext::default();
        let mut menu = PopupMenu::new(trigger(), vec![PopupMenuItem::new("Row one")])
            .with_menu_id("test.escape");
        frame(&mut menu, &app, None);
        click_at(&mut menu, 10.0, 16.0, &app);
        frame(&mut menu, &app, None);
        assert_eq!(app.open_menus.id(), Some("test.escape"));

        let mut ctx = EventContext::default();
        menu.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        );
        assert!(!menu.is_open(), "Escape closed the menu");
        frame(&mut menu, &app, None);
        assert!(!app.open_menus.is_open(), "the shared state let go of it");
    }

    #[test]
    fn an_open_menu_is_only_closed_by_a_click_inside_that_never_reaches_the_row_behind() {
        // A press on the panel is the menu's: it must not dismiss the menu, and
        // the shared state must keep it while a frame paints it.
        let app = AppContext::default();
        let picked = Rc::new(RefCell::new(None));
        let picked_clone = Rc::clone(&picked);
        let mut menu = PopupMenu::new(trigger(), vec![PopupMenuItem::new("Row one")])
            .with_menu_id("test.inside")
            .with_on_select(move |index| *picked_clone.borrow_mut() = Some(index));
        frame(&mut menu, &app, None);
        click_at(&mut menu, 10.0, 16.0, &app);
        let panel = {
            menu.layout(
                SizeConstraint::loose(vec2f(400.0, 400.0)),
                &mut LayoutContext::default(),
                &app,
            );
            menu.panel_bounds().expect("the panel was laid out")
        };
        frame(&mut menu, &app, None);
        assert_eq!(app.open_menus.id(), Some("test.inside"));

        let inside = vec2f(panel.min_x() + 8.0, panel.min_y() + 12.0);
        click_at(&mut menu, inside.x, inside.y, &app);
        assert_eq!(*picked.borrow(), Some(0), "the menu ran the row's action");
        assert!(!menu.is_open(), "choosing a row closes the menu");
    }
}
