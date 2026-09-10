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
    AppContext, Axis, Container, ContextPill, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Expanded, Fill, Flex, HoverButton, Icon, LayoutContext, MainAxisSize,
    PaintContext, PillTraySide, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, SizeConstraint,
    Spacer, SplitNode, Text, Tooltip, TooltipPosition, TopbarButton, CONTEXT_PILL_HEIGHT,
};
use crate::terminal::TuiAgent;
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::chat;
use super::shell::TOPBAR_HEIGHT;
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
            let chat = chat::build_agent_chat(app, state, actions, id, true);
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
        PaneKind::Chat => chat::build_agent_chat(app, state, actions, id, active),
        PaneKind::Terminal => {
            // The terminal spawns in this pane's own cwd (the same per-session
            // path used by the composer for chat panes), so a terminal and a
            // chat pane never share a working directory.
            let cwd = state
                .pane_chat
                .get(&id)
                .map(|c| c.composer_path.clone())
                .unwrap_or_default();
            // Harness mode is per pane: Cmd+Enter at the rich input activates it
            // and Esc returns this pane to the plain shell.
            let harness_mode = state
                .pane_controls
                .get(&id)
                .map(|c| c.harness_mode)
                .unwrap_or(false);
            terminal::build_terminal(app, state, actions, id, cwd, active, harness_mode)
        }
    };
    // Clicking anywhere in the pane body focuses the pane (mouse-driven pane
    // navigation), while child interactions (composer, buttons, terminal keys)
    // still get the event forwarded to them.
    let on_activate = actions.on_pane_activate.clone();
    let content = Box::new(PaneBody::new(id, content, on_activate));
    // A chat pane already carries its own agent header (conversation name +
    // actions + 3-dots + close), so adding the generic pane header here would
    // stack a second topbar. Only a terminal pane uses the generic header.
    let column = if matches!(kind, PaneKind::Terminal) {
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(build_pane_header(app, state, actions, id, kind))
            .with_child(Expanded::new(content).finish())
    } else {
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(content).finish())
    };

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

fn build_pane_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
    kind: PaneKind,
) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let label = match kind {
        PaneKind::Chat => "Chat",
        PaneKind::Terminal => "Terminal",
    };

    // A plain pty carries its tray buttons in the topbar (the working directory
    // and the git branch), while the agent keeps its own pills at the bottom of
    // the pane. The agent additionally offers the model switcher there; the pty
    // deliberately does not.
    let pills = build_context_pills(app, state, actions, id);
    let has_pills = !pills.is_empty();
    // The header is padded out to the shared topbar height around its tallest
    // child, so the bar stays exactly `TOPBAR_HEIGHT` with or without pills.
    let content_height = if has_pills {
        CONTEXT_PILL_HEIGHT
    } else {
        20.0
    };
    let v_pad = ((TOPBAR_HEIGHT - content_height).max(0.0)) / 2.0;

    let on_close = actions.on_close_pane.clone();
    let close = TopbarButton::new(
        Icon::new("x")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(20.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(label)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    for pill in pills {
        row = row.with_child(pill);
    }
    row = row.with_child(Spacer::new().finish());

    // A terminal pane is the unified surface for shell *and* a real TUI agent:
    // offer a compact "Run agent" menu to launch codex/claude/... native-first.
    if matches!(kind, PaneKind::Terminal) {
        // While the harness owns this pane's input, spell out the way back to
        // the plain shell at the top of the pane.
        let harness_mode = state
            .pane_controls
            .get(&id)
            .map(|c| c.harness_mode)
            .unwrap_or(false);
        if harness_mode {
            row = row.with_child(build_esc_hint(app));
        }
        row = row.with_child(build_run_agent(app, state, actions, id));
    }
    let row = row.with_child(close).finish();

    let hover = state
        .pane_hover
        .get(&id)
        .cloned()
        .unwrap_or_else(|| Rc::new(RefCell::new(false)));
    let on_activate = actions.on_pane_activate.clone();
    let header = HoverButton::new(row, hover)
        .with_padding(EdgeInsets::new(xs, v_pad, xs, v_pad))
        .with_on_click(move || (on_activate.borrow_mut())(id))
        .finish();

    // Unobtrusive shortcut hint: hover the pane header to see the pane split /
    // close keybindings. The tooltip paints above the header and never affects
    // layout.
    Tooltip::new(
        Container::new(header)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .finish(),
        "Ctrl/Cmd+Space: split right · Cmd+Shift+D: split down · Ctrl/Cmd+W: close pane · Cmd+Shift+T: new terminal",
    )
    .with_position(TooltipPosition::Above)
    .finish()
}

/// The pty's topbar tray buttons: the working directory and, when the pane's
/// checkout has one, the git branch. Both are the same [`ContextPill`] the
/// agent's composer footer uses, with their trays opening downwards.
fn build_context_pills(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
) -> Vec<Box<dyn Element>> {
    let Some(context) = state.composer_context.get(&id) else {
        return Vec::new();
    };
    let mut pills: Vec<Box<dyn Element>> = Vec::new();
    if !context.dir_label.is_empty() {
        let dir_ids = context.dir_ids.clone();
        let on_select_dir = actions.on_select_dir.clone();
        let mut pill = ContextPill::new("folder", context.dir_label.clone())
            .with_tooltip("Select working directory")
            .with_tray_side(PillTraySide::Below)
            .with_max_label_width(240.0);
        if !context.dir_items.is_empty() {
            pill = pill.with_menu(
                context.dir_items.clone(),
                context.dir_menu_open.clone(),
                move |idx| {
                    if let Some(session_id) = dir_ids.get(idx) {
                        (on_select_dir.borrow_mut())(id, session_id.clone());
                    }
                },
            );
        }
        pills.push(pill.finish(app));
    }
    if !context.branch_label.is_empty() {
        let branch_ids = context.branch_ids.clone();
        let on_select_branch = actions.on_select_branch.clone();
        let mut pill = ContextPill::new("git-branch", context.branch_label.clone())
            .with_tooltip("Select branch")
            .with_tray_side(PillTraySide::Below)
            .with_max_label_width(160.0);
        if !context.branch_items.is_empty() {
            pill = pill.with_menu(
                context.branch_items.clone(),
                context.branch_menu_open.clone(),
                move |idx| {
                    if let Some(branch) = branch_ids.get(idx) {
                        (on_select_branch.borrow_mut())(id, branch.clone());
                    }
                },
            );
        }
        pills.push(pill.finish(app));
    }
    pills
}

/// While the harness is active on a pane, its topbar shows `esc for terminal`
/// so the way back to the plain pty is always visible.
fn build_esc_hint(app: &AppContext) -> Box<dyn Element> {
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
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(keycap)
        .with_child(
            Text::new("for terminal")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

/// A compact "Run agent" control for a terminal pane header: a button that opens
/// a menu of real TUI agents. Choosing one launches it into the pane's PTY and
/// switches the pane to native-first agent mode.
fn build_run_agent(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
) -> Box<dyn Element> {
    let menu_open = state.terminal_run_agent_menu_open.clone();
    let on_launch = actions.on_launch_tui_agent.clone();
    let launcher_id = id;

    let trigger = TopbarButton::new(
        Icon::new("cpu")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(20.0)
    .finish();

    let items: Vec<PopupMenuItem> = TuiAgent::ALL
        .iter()
        .map(|a| PopupMenuItem::new(a.label()).with_icon("terminal"))
        .collect();

    Box::new(
        PopupMenu::new(trigger, items)
            .with_open(menu_open)
            .with_position(PopupMenuPosition::Below)
            .with_on_select(move |index| {
                if let Some(agent) = TuiAgent::ALL.get(index) {
                    (on_launch.borrow_mut())(launcher_id, agent.command().to_string());
                }
            }),
    )
}


