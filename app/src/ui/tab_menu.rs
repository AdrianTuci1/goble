//! The workspace tab's menu, opened by a right click at the pointer.
//!
//! The rows are warp-new's own tab menu (`app/src/tab.rs::modify_tab_menu_items`
//! and `close_tab_menu_items`), cut to the ones goble has something to do with:
//! rename, reset the name, move the tab left, close it, and close every other
//! tab. The tab's colours are its last row, drawn as warp-new draws them
//! (`dot_color_option_menu_items`): a strip of dots — one per preset plus the
//! "no colour" slash — each its own click target, with the tab's own colour
//! ringed.
//!
//! What is *not* here, and why: warp-new's menu also carries "Move Tab Right",
//! "Close Tabs to the Right", the tab-group rows, session sharing, "Save as new
//! config" and the pane-name rows. goble has no tab groups, no shared sessions
//! and no tab configs, and the remaining move/close directions were not asked
//! for — this menu is the list that was.
//!
//! The panel is the app's own [`PopupMenu`] — the same surface, rows, hover and
//! close-on-a-press-outside that every other menu of the shell uses — hung from
//! the pointer by a [`PointerMenu`] instead of from a control.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::{handle_mouse_event, InteractiveState};
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, PointerMenu, PopupMenu,
    PopupMenuItem, SizeConstraint, Tooltip, TooltipPosition,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, TabColor};

/// The side of a colour dot's circle, and the ring a selected one carries:
/// warp-new's own 16 pt dot in a 2 pt border. The ring is drawn *inside* the
/// dot's box, so a ringed dot and a plain one are the same size, and the box is
/// what the row is spaced by.
const DOT_SIZE: f32 = 16.0;
const DOT_RING: f32 = 2.0;
const DOT_BOX: f32 = DOT_SIZE + DOT_RING * 2.0;
/// The gap two dots are never drawn closer than, however narrow the panel is.
const DOT_GAP: f32 = 10.0;
/// The room the colour row keeps above itself, so it reads as a row of its own
/// rather than as one more item of the list.
const DOTS_TOP_GAP: f32 = 6.0;

/// One thing the tab menu can do. Reported together with the tab's index, so the
/// menu itself never has to know where in the strip it was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabMenuAction {
    /// Open the inline rename field on the tab.
    Rename,
    /// Drop the typed name and go back to the label derived from what the tab
    /// holds.
    ResetName,
    /// Move the tab one slot towards the front of the strip.
    MoveLeft,
    /// Close the tab.
    Close,
    /// Close every tab but this one.
    CloseOthers,
    /// Tint the tab, or clear the tint with `None`.
    SetColor(Option<TabColor>),
}

/// The tab a menu is opened on: what its rows are decided from.
#[derive(Clone, Copy, Debug)]
pub struct TabMenuTab {
    pub index: usize,
    /// How many tabs the strip holds. "Close other tabs" needs a second tab to
    /// have anything to close and "Move Tab Left" a slot to move into, gated
    /// exactly as warp-new gates them.
    pub tabs: usize,
    /// The colour the tab carries now, so the matching dot is the ringed one.
    pub color: Option<TabColor>,
    /// Whether the tab carries a name the user typed — the only thing "Reset tab
    /// name" has to undo (warp-new draws that row only for a custom title).
    pub named: bool,
}

/// The rows of the menu, in the order they are drawn, each with the label it
/// reads and what choosing it does. One list, so a row and its action cannot
/// drift apart (warp-new's own sections do the same).
fn rows(tab: TabMenuTab) -> Vec<(&'static str, TabMenuAction)> {
    let mut rows = vec![("Rename tab", TabMenuAction::Rename)];
    if tab.named {
        rows.push(("Reset tab name", TabMenuAction::ResetName));
    }
    if tab.index > 0 {
        rows.push(("Move Tab Left", TabMenuAction::MoveLeft));
    }
    rows.push(("Close tab", TabMenuAction::Close));
    if tab.tabs > 1 {
        rows.push(("Close other tabs", TabMenuAction::CloseOthers));
    }
    rows
}

/// Build the tab menu's panel and hang it from `at` — the point the right click
/// landed on.
///
/// `on_action` is told what was chosen and `on_close` when the panel closed
/// itself on a press outside it. The app owns both, because the tree is rebuilt
/// every frame: an element cannot remember that it was open.
pub fn build_tab_menu(
    tab: TabMenuTab,
    at: Vector2F,
    on_action: Rc<RefCell<dyn FnMut(TabMenuAction)>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let rows = rows(tab);
    let items = rows
        .iter()
        .map(|(label, _)| PopupMenuItem::new(*label))
        .collect();
    let chosen = Rc::clone(&on_action);
    let menu = PopupMenu::new(Box::new(goble_ui::elements::Empty::new()), items)
        // The panel is built and the press-outside close is decided inside the
        // menu, so it is open for the one frame this element lives in; the app's
        // own open state is what the next frame reads.
        .with_open(Rc::new(RefCell::new(true)))
        .with_on_select(move |row| {
            if let Some((_, action)) = rows.get(row) {
                (chosen.borrow_mut())(*action);
            }
        })
        .with_footer_row(colour_row(tab, Rc::clone(&on_action)))
        .with_on_close(move || (on_close.borrow_mut())())
        .finish();
    Box::new(PointerMenu::new(menu, at))
}

/// The colours' row: one dot per preset plus the "no colour" slash, each
/// carrying its own name on the hover the way warp-new's dots do.
fn colour_row(
    tab: TabMenuTab,
    on_action: Rc<RefCell<dyn FnMut(TabMenuAction)>>,
) -> Box<dyn Element> {
    let dots = std::iter::once(None)
        .chain(TabColor::ALL.iter().copied().map(Some))
        .map(|color| {
            let dot: Box<dyn Element> = Box::new(ColorDot::new(
                color,
                color == tab.color,
                Rc::clone(&on_action),
            ));
            let label = color.map(TabColor::label).unwrap_or("Default (no color)");
            Tooltip::new(dot, label)
                .with_position(TooltipPosition::Above)
                .finish()
        })
        .collect();
    Box::new(ColorDots::new(dots))
}

/// The row the dots are laid out in: an equal share of the panel's width each,
/// with a gap at both ends (warp-new's `MainAxisAlignment::SpaceEvenly`), never
/// closer together than [`DOT_GAP`].
struct ColorDots {
    dots: Vec<(f32, Box<dyn Element>)>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ColorDots {
    fn new(dots: Vec<Box<dyn Element>>) -> Self {
        Self {
            dots: dots.into_iter().map(|dot| (0.0, dot)).collect(),
            size: None,
            origin: None,
        }
    }
}

impl Element for ColorDots {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let count = self.dots.len().max(1) as f32;
        let room = constraint.max.x.max(DOT_BOX * count);
        let gap = ((room - DOT_BOX * count) / (count + 1.0)).max(DOT_GAP);
        let mut x = gap;
        for (offset, dot) in &mut self.dots {
            dot.layout(SizeConstraint::loose(vec2f(DOT_BOX, DOT_BOX)), ctx, app);
            *offset = x;
            x += DOT_BOX + gap;
        }
        let size = vec2f(
            (x - gap).max(DOT_BOX),
            DOTS_TOP_GAP + DOT_BOX,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        for (offset, dot) in &mut self.dots {
            dot.paint(origin + vec2f(*offset, DOTS_TOP_GAP), ctx, app);
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
        for (_, dot) in &mut self.dots {
            if dot.dispatch_event(event, ctx, _app) {
                return true;
            }
        }
        false
    }
}

/// One dot: a circle in its preset's colour, or the theme's own slashed circle
/// for "no colour", ringed in the accent colour while it is the tab's.
struct ColorDot {
    color: Option<TabColor>,
    selected: bool,
    on_action: Rc<RefCell<dyn FnMut(TabMenuAction)>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ColorDot {
    fn new(
        color: Option<TabColor>,
        selected: bool,
        on_action: Rc<RefCell<dyn FnMut(TabMenuAction)>>,
    ) -> Self {
        Self {
            color,
            selected,
            on_action,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    fn bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for ColorDot {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(DOT_BOX, DOT_BOX);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let circle = rectf(origin.x + DOT_RING, origin.y + DOT_RING, DOT_SIZE, DOT_SIZE);
        match self.color {
            Some(color) => renderer.fill_rounded_rect(circle, color.color(), DOT_SIZE / 2.0),
            // "No colour" is the theme's slashed circle, drawn the size of a dot
            // over the tab surface the tab would keep.
            None => renderer.draw_icon(
                circle.min().to_vector(),
                "cancelled",
                DOT_SIZE,
                app.theme.color(ColorToken::Text),
            ),
        }
        if self.selected {
            // The ring lies inside the dot's box — an inset of half the stroke,
            // which is what a 2 pt border on a 20 pt box draws — so the row
            // keeps its rhythm whichever dot is the tab's.
            renderer.stroke_rect(
                rectf(
                    origin.x + DOT_RING / 2.0,
                    origin.y + DOT_RING / 2.0,
                    DOT_BOX - DOT_RING,
                    DOT_BOX - DOT_RING,
                ),
                app.theme.color(ColorToken::Accent),
                DOT_RING,
                DOT_BOX / 2.0,
            );
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
            Some(bounds) => bounds,
            None => return false,
        };
        let cb = Rc::clone(&self.on_action);
        // The dots toggle, the way warp-new's do: the dot of the colour the tab
        // already carries clears it again, and the "no colour" dot that is
        // already the tab's leaves it as it is.
        let next = if self.selected { None } else { self.color };
        let mut on_click = move || (cb.borrow_mut())(TabMenuAction::SetColor(next));
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_click)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tab of a two-tab strip that the user has named, not the first one: the
    /// shape every row is drawn for.
    fn tab() -> TabMenuTab {
        TabMenuTab {
            index: 1,
            tabs: 2,
            color: None,
            named: true,
        }
    }

    fn labels(tab: TabMenuTab) -> Vec<&'static str> {
        rows(tab).into_iter().map(|(label, _)| label).collect()
    }

    /// Every row that applies is drawn, in warp-new's own order, and a row is
    /// dropped exactly when it has nothing to do — the same gates warp-new puts
    /// on "Reset tab name" (`custom_title.is_some()`), "Move Tab Left"
    /// (`index != 0`) and "Close other tabs" (`tabs_len > 1`).
    #[test]
    fn the_rows_are_the_ones_that_apply() {
        assert_eq!(
            labels(tab()),
            vec![
                "Rename tab",
                "Reset tab name",
                "Move Tab Left",
                "Close tab",
                "Close other tabs",
            ],
            "a named tab in the middle of the strip"
        );
        assert_eq!(
            labels(TabMenuTab { index: 0, ..tab() }),
            vec!["Rename tab", "Reset tab name", "Close tab", "Close other tabs"],
            "the first tab has nowhere to move left"
        );
        assert_eq!(
            labels(TabMenuTab { tabs: 1, index: 0, ..tab() }),
            vec!["Rename tab", "Reset tab name", "Close tab"],
            "a lone tab has no other tabs to close"
        );
        assert_eq!(
            labels(TabMenuTab { named: false, ..tab() }),
            vec!["Rename tab", "Move Tab Left", "Close tab", "Close other tabs"],
            "a tab that derived its own label has no typed name to reset"
        );
    }

    /// A row and what it does cannot come apart: the list is one ordered list of
    /// pairs, and each label is read back with its own action.
    #[test]
    fn each_row_carries_its_own_action() {
        let rows = rows(tab());
        assert_eq!(rows[0], ("Rename tab", TabMenuAction::Rename));
        assert_eq!(rows[1], ("Reset tab name", TabMenuAction::ResetName));
        assert_eq!(rows[2], ("Move Tab Left", TabMenuAction::MoveLeft));
        assert_eq!(rows[3], ("Close tab", TabMenuAction::Close));
        assert_eq!(rows[4], ("Close other tabs", TabMenuAction::CloseOthers));
    }

    /// Every preset is offered, behind the "no colour" dot, in warp-new's order.
    #[test]
    fn the_colour_row_is_the_no_colour_dot_then_every_preset() {
        let built = colour_row(tab(), Rc::new(RefCell::new(|_: TabMenuAction| {})));
        let mut built = built;
        let commands = goble_ui::test_util::render_element(
            &mut built,
            vec2f(220.0, 40.0),
            &AppContext::default(),
        );
        let dots = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    goble_ui::render::RenderCommand::FillRect { .. }
                        | goble_ui::render::RenderCommand::DrawIcon { .. }
                )
            })
            .count();
        assert_eq!(
            dots,
            TabColor::ALL.len() + 1,
            "one dot per preset, plus the slashed one: {commands:?}"
        );
    }
}
