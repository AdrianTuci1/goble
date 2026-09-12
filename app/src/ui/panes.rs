//! Pane-tree builder for a "space" (warp-new style).
//!
//! Recursively renders a [`Space`]'s [`Pane`] tree: leaves host content (the
//! agent chat or a PTY terminal), splits are resizable [`SplitNode`]s with a
//! draggable divider. The active pane is outlined; clicking anywhere in a pane
//! (its header or body) focuses it, and its header has a close button.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Expanded,
    Fill, Flex, HoverButton, LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint,
    SplitNode, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::chat;
use super::terminal;
use super::{Pane, PaneKind, SplitDir, UiActions, UiSnapshot};

/// Build the active space's pane tree as the main chat content.
///
/// Guards against an empty space list or an out-of-range active index (which
/// could arise from a corrupt persisted pane layout): the caller falls back to
/// an empty chat pane rather than panicking on user-controlled state.
pub fn build_active_space(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    match state.spaces.get(state.active_space) {
        Some(space) => build_pane(app, state, actions, &space.root),
        None => {
            let id = 0u64;
            let chat = chat::build_agent_chat(app, state, actions, id, true, None);
            // The chat's own agent header is the single topbar; do not stack the
            // generic pane header above it.
            let column = Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Expanded::new(chat).finish());
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .finish()
        }
    }
}

fn build_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane: &Pane,
) -> Box<dyn Element> {
    match pane {
        Pane::Leaf { id, kind } => build_leaf(app, state, actions, *id, *kind),
        Pane::Split {
            id,
            dir,
            ratio,
            first,
            second,
        } => {
            let axis = match dir {
                SplitDir::Horizontal => Axis::Horizontal,
                SplitDir::Vertical => Axis::Vertical,
            };
            let dragging = state.dragging_pane_id == Some(*id);
            let on_start = actions.on_pane_drag_start.clone();
            let on_move = actions.on_pane_drag_move.clone();
            let on_end = actions.on_pane_drag_end.clone();
            let split_id = *id;
            let node = SplitNode::new(
                axis,
                *ratio,
                build_pane(app, state, actions, first),
                build_pane(app, state, actions, second),
            )
            .with_dragging(dragging)
            .with_on_drag_start(move || (on_start.borrow_mut())(split_id))
            .with_on_drag_move(move |r| (on_move.borrow_mut())(split_id, r))
            .with_on_drag_end(move || (on_end.borrow_mut())());
            Box::new(node)
        }
    }
}

/// Wraps a pane leaf's content so a mouse-down anywhere inside the pane focuses
/// it, while still forwarding the event to the content (so a composer, button,
/// or terminal keystroke keeps working). This gives mouse-driven pane
/// navigation: clicking a pane makes it the active split target/highlight.
struct PaneBody {
    pane_id: u64,
    content: Box<dyn Element>,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PaneBody {
    fn new(
        pane_id: u64,
        content: Box<dyn Element>,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    ) -> Self {
        Self {
            pane_id,
            content,
            on_activate,
            size: None,
            origin: None,
        }
    }

    fn bounds(&self) -> Option<goble_ui::geometry::RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for PaneBody {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.content.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.content.paint(origin, ctx, app);
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
        let handled = self.content.dispatch_event(event, ctx, app);
        // A mouse-down anywhere in the pane focuses it (pane navigation by
        // click). The event is still forwarded to the content above, so child
        // interactions are unaffected.
        let activated = matches!(event, DispatchedEvent::MouseDown { position, .. }
            if self.bounds().map(|b| contains(b, *position)).unwrap_or(false));
        if activated {
            (self.on_activate.borrow_mut())(self.pane_id);
        }
        handled || activated
    }
}

fn build_leaf(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
    kind: PaneKind,
) -> Box<dyn Element> {
    let active = state.active_pane_id == id;
    let content = match kind {
        PaneKind::Chat => chat::build_agent_chat(app, state, actions, id, active, None),
        PaneKind::Terminal => {
            // The terminal spawns in this pane's own cwd (the same per-session
            // path used by the composer for chat panes), so a terminal and a
            // chat pane never share a working directory.
            let cwd = state
                .pane_chat
                .get(&id)
                .map(|c| c.composer_path.clone())
                .unwrap_or_default();
            // The pane's own builder picks the surface: the shell's grid with
            // its rich input bar, or — while this pane's harness is open — the
            // very agent view a chat pane mounts.
            terminal::build_terminal(app, state, actions, id, cwd, active)
        }
    };
    // Clicking anywhere in the pane body focuses the pane (mouse-driven pane
    // navigation), while child interactions (composer, buttons, terminal keys)
    // still get the event forwarded to them.
    let on_activate = actions.on_pane_activate.clone();
    let content = Box::new(PaneBody::new(id, content, on_activate));
    // Every surface draws the one topbar a pane has, so nothing is stacked
    // above the content here: the agent header is the pane's topbar, and the
    // terminal pane draws it for its shell view too (see `build_terminal`).
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(content).finish());

    let active = state.active_pane_id == id;
    let border_color = if active {
        ColorToken::Accent
    } else {
        ColorToken::Border
    };
    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(border_color).into())
        .finish()
}

/// While the harness is active on a pane, its topbar shows `esc for terminal`:
/// the way back to the plain pty, spelled out and clickable, so the surface can
/// be switched with the pointer as well as with the key.
pub(crate) fn build_esc_hint<F: FnMut() + 'static>(
    app: &AppContext,
    hover: Rc<RefCell<bool>>,
    on_click: F,
) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let keycap = Container::new(
        Text::new("esc")
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(xs, 1.0, xs, 1.0))
    .with_border(app.theme.color(ColorToken::Border).into())
    .with_corner_radius(4.0)
    .finish();
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(keycap)
        .with_child(
            Text::new("for terminal")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    HoverButton::new(row, hover)
        .with_padding(EdgeInsets::new(xs, 2.0, xs, 2.0))
        .with_on_click(on_click)
        .finish()
}
