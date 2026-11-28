use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    harness_bar_mark, AppContext, ConstrainedBox, Container, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Empty, EventContext, Expanded, Flex, Icon, LayoutContext,
    MainAxisAlignment, MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, SizeConstraint, Text, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ColorU;

use super::super::panes::PANE_INSET;
use super::super::{UiActions, UiSnapshot};
use crate::state::PaneWorkItem;

/// Height of the agent header's tallest control (`TopbarButton`'s default
/// size); the header is padded out to `shell::TOPBAR_HEIGHT` around it.
pub(crate) const HEADER_CONTROL_HEIGHT: f32 = 32.0;

/// The harness mark the bar leads its title with.
const HARNESS_MARK_SIZE: f32 = 14.0;

/// How far the bar's band paints past the pane card's own top, left and right
/// edges: exactly the pane frame's inset, so the topbar runs flush with the
/// pane's slot while the pane body below keeps its inset card and rounded
/// corners.
const BAND_BLEED: f32 = PANE_INSET;

/// The pane's topbar band: a square `Surface` band painted flush with the
/// pane's slot, around one row of chrome.
///
/// The band paints `bleed` points past its own top, left and right layout
/// edges, covering the pane frame's inset gutter and the shadow under it, so
/// the topbar has no margin against the pane's edge while the pane's card below
/// stays inset and rounded. The row inside it always spans the whole band: the
/// pane's adjustable content-lane width never reaches the topbar, it only
/// meters the conversation column under it.
struct HeaderBand {
    row: Box<dyn Element>,
    color: ColorU,
    bleed: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HeaderBand {
    fn new(row: Box<dyn Element>, color: ColorU) -> Self {
        Self {
            row,
            color,
            bleed: BAND_BLEED,
            size: None,
            origin: None,
        }
    }
}

impl Element for HeaderBand {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let row_size = self.row.layout(constraint, ctx, app);
        let size = vec2f(
            if constraint.max.x.is_finite() {
                constraint.max.x.max(0.0)
            } else {
                row_size.x
            },
            row_size.y,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            // Flush with the pane's slot: past the card's own inset on the top,
            // left and right, and square at every corner — the band is the
            // pane's chrome, not a card.
            renderer.fill_rounded_rect(
                rectf(
                    origin.x - self.bleed,
                    origin.y - self.bleed,
                    size.x + self.bleed * 2.0,
                    size.y + self.bleed,
                ),
                self.color,
                0.0,
            );
        }
        self.row.paint(origin, ctx, app);
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

/// What a pane prints in the middle of its own bar. A shell names the
/// directory it runs in; an agent surface names the conversation it holds.
pub(crate) enum PaneHeading {
    /// The pane's working directory, shortened the way the tab strip reads it
    /// (`~`, `~/Projects/goble`), with the same form on hover.
    Path(String),
    /// The agent conversation's subject, or its new-conversation placeholder.
    Agent(String),
}

impl PaneHeading {
    /// The title cluster's current crumb. DSH draws the current crumb at
    /// 14/20, weight 500, label-primary, with its ancestors muted before a
    /// `/` separator; a shell path is the whole cluster.
    fn element(self, app: &AppContext, crumb: Option<String>) -> Box<dyn Element> {
        let (text, tooltip) = match self {
            PaneHeading::Path(path) => {
                let display = crate::state::display_path(&path);
                (display.clone(), Some(display))
            }
            PaneHeading::Agent(subject) => (subject, None),
        };
        let current = Text::new(text)
            .with_font_size(14.0)
            .with_line_height(20.0 / 14.0)
            .with_weight(goble_ui::platform::text_atlas::FontWeight::Medium)
            .with_theme_color(ColorToken::Text, app)
            .with_max_lines(1)
            .finish();
        let current = match tooltip {
            Some(tooltip) => Tooltip::new(current, tooltip)
                .with_position(TooltipPosition::Below)
                .finish(),
            None => current,
        };
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0);
        if let Some(crumb) = crumb.filter(|crumb| !crumb.trim().is_empty()) {
            row = row
                .with_child(
                    Text::new(crumb)
                        .with_font_size(14.0)
                        .with_line_height(20.0 / 14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(
                    Text::new("/")
                        .with_font_size(14.0)
                        .with_line_height(20.0 / 14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
        }
        row = row.with_child(ConstrainedBox::new(current).with_max_width(340.0).finish());
        row.finish()
    }
}

/// Agent header row: a right-aligned work chip, the pane's expand control and
/// a close X.
///
/// The pane's topbar — the one bar a pane has, shared by both of its surfaces.
///
/// The agent view and the terminal pane's shell view draw this same bar (see
/// `terminal::build_terminal`): a draft's directory and branch are not repeated
/// here, because the rich input at the bottom of the pane already carries them.
/// A terminal pane adds the "Run agent" menu, since that pane can host a real
/// TUI agent, and while its harness is open the bar leads with `esc` — the way
/// back to the plain shell, spelled out at the front. A chat pane has no shell
/// behind it, so the same hint stands there for the terminal Esc opens in its
/// place.
///
/// The fullscreen control is the bar's only action besides closing the pane: it
/// draws this pane over the whole panes space and then draws itself as the way
/// back. A one-pane space has nothing to expand over, so the bar draws no
/// control there at all. What used to be folded into a 3-dots tray here now
/// lives where it can be reached from any pane: `/clear` and `/fullscreen` in
/// the ⌘K palette (`ui::palette`), and Copy in the macOS Edit menu
/// (`platform::mac::menus`).
pub(crate) fn build_agent_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    // Retired: the bar no longer draws `esc for terminal`. Kept in the signature
    // while the terminal-pane legacy callers are reworked.
    _esc: Option<Rc<RefCell<dyn FnMut()>>>,
    // The harness the pane's surface works with when no CLI agent is running in
    // its pty: the conversation's own harness on the agent view, `None` on a
    // plain shell.
    surface_harness: Option<&'static goble_core::harness::HarnessSpec>,
    heading: Option<PaneHeading>,
    // Whether this pane offers the header's bottom-terminal-dock icon.
    terminal_dock: bool,
) -> Box<dyn Element> {
    // DSH's title cluster: the harness mark, the parent crumb (the harness the
    // surface works with), a `/` and the current conversation title. A shell
    // path has no parent crumb; the path is the current crumb alone.
    // The panel topbar names ONLY the conversation: the harness mark and the
    // parent crumb belonged to the retired general bar's title language.
    let mark = None;
    let crumb = None;
    let mut title = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    if let Some(mark) = mark {
        title = title.with_child(mark);
    }
    match heading {
        Some(heading) => title = title.with_child(heading.element(app, crumb)),
        None => {
            if let Some(crumb) = crumb {
                title = title.with_child(
                    Text::new(crumb)
                        .with_font_size(14.0)
                        .with_line_height(20.0 / 14.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
            }
        }
    }
    build_agent_header_with(
        app,
        state,
        actions,
        pane_id,
        None,
        HeaderSlots {
            leading: Some(title.finish()),
            terminal_dock,
            ..HeaderSlots::default()
        },
    )
}

/// The harness this pane is working with, drawn at the bar's leading edge: the
/// catalog mark and its name. The CLI agent running in the pty outranks the
/// surface's own harness; a shell that has neither draws nothing, so its bar
/// stays as empty as it was.
fn harness_chip(
    app: &AppContext,
    state: &UiSnapshot,
    pane_id: u64,
    surface_harness: Option<&'static goble_core::harness::HarnessSpec>,
) -> Option<Box<dyn Element>> {
    let harness = state
        .terminal
        .borrow()
        .pane_harness(pane_id)
        .or(surface_harness)?;
    // The mark alone, in the harness family's own accent: the bar names the
    // agent it is working with without spending the room a name would take. The
    // tooltip is where the name is written out.
    let mark = harness_bar_mark(harness.id, HARNESS_MARK_SIZE, app);
    Some(
        Tooltip::new(mark, format!("Working with {}", harness.name))
            .with_position(TooltipPosition::Below)
            .finish(),
    )
}

/// Content a pane adds to its own bar around the controls every pane wears.
///
/// The bar keeps one row — the pane's own title and totals at the leading edge,
/// its own actions, when it has any, before the expand and close controls. A
/// pane whose actions do not fit that row draws them as a second bar under this
/// one and leaves `trailing` empty (the review pane does).
#[derive(Default)]
pub(crate) struct HeaderSlots {
    /// Drawn at the bar's leading edge: what the pane is showing, in its own
    /// words. It takes the space left over after the bar's controls, so a long
    /// title cannot push them off a narrow pane.
    pub leading: Option<Box<dyn Element>>,
    /// Drawn in the middle of the bar, between two equal flexible spaces: the
    /// pane's identity (a shell's path, an agent's subject). The bar stays
    /// balanced whatever the leading or trailing slots hold.
    pub center: Option<Box<dyn Element>>,
    /// Drawn at the bar's trailing edge, before the expand and close controls:
    /// the pane's own controls, to the left of the ones every pane shares.
    pub trailing: Option<Box<dyn Element>>,
    /// Whether the bar draws the terminal-dock icon in its utilities. Agent
    /// panes with a local shell offer it; shell/settings/file panes do not.
    pub terminal_dock: bool,
}

/// The same bar, with the pane's own content in its leading and trailing slots.
pub(crate) fn build_agent_header_with(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    // Retired: the `esc for terminal` hint is gone with the dual surface.
    _esc: Option<Rc<RefCell<dyn FnMut()>>>,
    slots: HeaderSlots,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    // DSH's header metrics: actions gap 10, utilities gap 8, a 28x28 More
    // surface and 28x28 corner controls.
    let actions_gap = 10.0;
    let utilities_gap = 8.0;
    // DSH's header controls are 28x28; pad the row out to the pane's topbar
    // height around that control height.
    let dsh_control = HEADER_CONTROL_HEIGHT;
    let v_pad = ((super::super::shell::TOPBAR_HEIGHT - dsh_control) / 2.0).max(0.0);
    let marker_inset = if state.active_pane_id == pane_id {
        super::super::panes::FOCUS_MARKER_SIZE
    } else {
        0.0
    };

    let maximized = state.maximized_pane == Some(pane_id);
    let expandable = state
        .spaces
        .get(state.active_space)
        .is_some_and(|space| space.root.leaf_count() > 1);
    let on_toggle_maximized = actions.on_toggle_pane_maximized.clone();
    let expand_button: Option<Box<dyn Element>> = (expandable || maximized).then(|| {
        let glyph = if maximized {
            "fullscreen-exit"
        } else {
            "fullscreen-enter"
        };
        let tooltip = if maximized {
            "Exit fullscreen"
        } else {
            "Fullscreen"
        };
        Tooltip::new(
            TopbarButton::new(
                Icon::new(glyph)
                    .with_size(15.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(28.0)
            .with_corner_radius(app.theme.radius_px())
            .with_on_click(move || (on_toggle_maximized.borrow_mut())(pane_id))
            .finish(),
            tooltip,
        )
        .finish()
    });

    let on_close = actions.on_close_pane.clone();
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(15.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let controls = state.pane_controls.get(&pane_id).cloned();
    let dock_open = controls.as_ref().is_some_and(|c| c.terminal_dock_open);
    let on_toggle_dock = actions.on_toggle_terminal_dock.clone();
    let dock_button: Option<Box<dyn Element>> = slots.terminal_dock.then(|| {
        Tooltip::new(
            TopbarButton::new(
                Icon::new("terminal")
                    .with_size(15.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(28.0)
            .with_corner_radius(app.theme.radius_px())
            .with_active(dock_open)
            .with_on_click(move || (on_toggle_dock.borrow_mut())(pane_id))
            .finish(),
            "Toggle terminal drawer",
        )
        .with_position(TooltipPosition::Below)
        .finish()
    });

    // DSH's header More: a 28x28 ellipsis that opens the panel's actions menu.
    // The menu is the header's own seat for the controls that used to be
    // scattered (and for the dock, so the icon also has a keyboard path).
    let menu_open = controls
        .as_ref()
        .map(|c| c.header_menu_open.clone())
        .unwrap_or_else(|| Rc::new(RefCell::new(false)));
    let on_menu_close = actions.on_close_pane.clone();
    let on_menu_max = actions.on_toggle_pane_maximized.clone();
    let on_menu_dock = actions.on_toggle_terminal_dock.clone();
    let mut dock_item = PopupMenuItem::new("Toggle terminal drawer").with_icon("terminal");
    if !slots.terminal_dock {
        dock_item = dock_item.disabled();
    }
    let mut max_item = PopupMenuItem::new(if maximized {
        "Exit fullscreen"
    } else {
        "Fullscreen"
    })
    .with_icon("fullscreen-enter");
    if !expandable && !maximized {
        max_item = max_item.disabled();
    }
    let on_split_right = actions.on_split_right.clone();
    let on_split_down = actions.on_split_down.clone();
    let on_replace = actions.on_create_submit.clone();
    let menu_items = vec![
        PopupMenuItem::new("Split right").with_shortcut("⌘ Space"),
        PopupMenuItem::new("Split down").with_shortcut("⌘ ⇧ D"),
        PopupMenuItem::new("Replace with new").with_separator_before(),
        dock_item,
        max_item,
        PopupMenuItem::new("Close pane").with_icon("x"),
    ];
    let menu = PopupMenu::new(
        Tooltip::new(
            TopbarButton::new(
                Icon::new("ellipsis")
                    .with_size(15.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(28.0)
            .with_corner_radius(app.theme.radius_px())
            .finish(),
            "More",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
        menu_items,
    )
    .with_open(menu_open)
    .with_position(PopupMenuPosition::AboveEnd)
    .with_max_width(220.0)
    .with_on_select(move |index| match index {
        0 => (on_split_right.borrow_mut())(),
        1 => (on_split_down.borrow_mut())(),
        2 => (on_replace.borrow_mut())(),
        3 => (on_menu_dock.borrow_mut())(pane_id),
        4 => (on_menu_max.borrow_mut())(pane_id),
        _ => (on_menu_close.borrow_mut())(),
    })
    .finish();

    let work_chip = build_work_chip(
        app,
        state
            .pane_live_work
            .get(&pane_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    let HeaderSlots {
        leading,
        center: _center,
        trailing,
        terminal_dock: _,
    } = slots;

    // The title cluster is the bar's leading group; everything else is the DSH
    // utilities/actions tail on the right. The cluster takes the leftover room
    // and clips a long title, so the controls never leave the bar.
    let mut title = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    if let Some(leading) = leading {
        title = title.with_child(leading);
    }
    let title = Container::new(title.finish())
        .with_padding(EdgeInsets::new(marker_inset, 0.0, 0.0, 0.0))
        .finish();

    let mut utilities = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(utilities_gap)
        .with_child(work_chip);
    if let Some(dock_button) = dock_button {
        utilities = utilities.with_child(dock_button);
    }
    let on_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_open = state.sidebar_visible;
    let sidebar_button = Tooltip::new(
        TopbarButton::new(
            Icon::new(if sidebar_open {
                "left-panel-close"
            } else {
                "left-panel-open"
            })
            .with_size(15.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_size(28.0)
        .with_corner_radius(app.theme.radius_px())
        .with_on_click(move || (on_sidebar.borrow_mut())())
        .finish(),
        if sidebar_open {
            "Hide sidebar"
        } else {
            "Show sidebar"
        },
    )
    .with_position(TooltipPosition::Below)
    .finish();
    utilities = utilities.with_child(sidebar_button);
    let on_split = actions.on_split_right.clone();
    let split_button = Tooltip::new(
        TopbarButton::new(
            Icon::new("split-right")
                .with_size(15.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(28.0)
        .with_corner_radius(app.theme.radius_px())
        .with_on_click(move || (on_split.borrow_mut())())
        .finish(),
        "Split right",
    )
    .with_position(TooltipPosition::Below)
    .finish();
    utilities = utilities.with_child(split_button);
    utilities = utilities.with_child(menu);
    if let Some(expand_button) = expand_button {
        utilities = utilities.with_child(expand_button);
    }
    let utilities = utilities.with_child(close_button).finish();

    let mut right = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_main_axis_alignment(MainAxisAlignment::End)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(actions_gap);
    if let Some(trailing) = trailing {
        right = right.with_child(trailing);
    }
    let right = right.with_child(utilities).finish();

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(
            // The bar's controls are 28x28, but the band's control height is
            // 32: a zero-width strut keeps the row at that height even when a
            // pane has no work chip to hold it open.
            Empty::new().with_size(vec2f(0.0, dsh_control)).finish(),
        )
        .with_child(Expanded::new(title).finish())
        .with_child(right)
        .finish();

    // The bar is the pane's chrome: its band paints flush with the pane's slot
    // (past the pane card's own inset, square) while only the row inside it is
    // bounded to the tab's content width and centred. The band's height stays
    // the row's own `TOPBAR_HEIGHT`, so the pane below starts exactly under it.
    // DSH's header controls are 28x28, while the band's own control height is
    // 32: reserve the bar's control height so the band stays exactly
    // `TOPBAR_HEIGHT` tall (the pane below starts on that edge).
    let row = ConstrainedBox::new(row)
        .with_min_height(dsh_control)
        .finish();
    let row = Container::new(row)
        .with_padding(EdgeInsets::new(0.0, v_pad, 0.0, v_pad))
        .finish();
    HeaderBand::new(row, app.theme.color(ColorToken::ContentBg)).finish()
}

/// The pane's own work cue, drawn between the header's empty space and the
/// expand control: a rule, then `◆ N` in the accent colour, the same shape and
/// spacing the topbar's live cue uses (`shell::build_live_indicator`).
///
/// `items` is what this pane alone is charged with ([`crate::state::UiState::pane_live_work`]);
/// the count is its length and the tooltip names every item, one line of `kind ·
/// description` each, so the number is never a mystery. At zero items it draws
/// nothing at all — rule included — so a pane with no work keeps the header it
/// had and there is no hit target. Information only: it carries no handler, so
/// a click passes through to whatever is under it.
fn build_work_chip(app: &AppContext, items: &[PaneWorkItem]) -> Box<dyn Element> {
    if items.is_empty() {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    }
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let message = items
        .iter()
        .map(|item| format!("{} · {}", item.kind.label(), item.description))
        .collect::<Vec<String>>()
        .join("\n");
    let chip = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5)
        .with_child(
            Icon::new("diamond")
                .with_size(8.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new(items.len().to_string())
                .with_theme_color(ColorToken::Accent, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    let chip: Box<dyn Element> = Tooltip::new(chip, message).finish();
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            // The rule runs the height of the header's controls. The inner row
            // is `TOPBAR_HEIGHT` minus the container's own padding, so a rule
            // the full height of the bar would push the header past it.
            ConstrainedBox::new(Divider::vertical().finish())
                .with_height(HEADER_CONTROL_HEIGHT)
                .finish(),
        )
        .with_child(chip)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    use goble_core::harness::ToolCallStatus;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{RectF, Vector2F};
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::{SubAgentRow, SubAgentRowStatus, ToolCall};

    use crate::root_view::RootView;
    use crate::state::{PaneSession, SubAgentRecord, UiState};
    use crate::ui::{Pane, PaneKind, Space, SplitDir};

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// The real window over an in-memory store, with the overlays a fresh start
    /// can carry switched off, so the pane's own chrome is what is on screen.
    fn pane_root() -> (
        RootView,
        Rc<RefCell<UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        let view = RootView::new(&AppContext::default(), &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
        }
        (view, state, desktop, dir)
    }

    /// A horizontal split of two chat panes (left 7, right 8) as the only
    /// space, active on the left one — the shape the pane-space tests measure
    /// against.
    fn split_root() -> (
        RootView,
        Rc<RefCell<UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
    ) {
        let (root, state, desktop, dir) = pane_root();
        {
            let mut s = state.borrow_mut();
            s.spaces = vec![Space::unnamed(Pane::Split {
                id: 9,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: Box::new(Pane::Leaf {
                    id: 7,
                    kind: PaneKind::Chat,
                }),
                second: Box::new(Pane::Leaf {
                    id: 8,
                    kind: PaneKind::Chat,
                }),
            })];
            s.active_space = 0;
            s.active_pane_id = 7;
            for pane_id in [7, 8] {
                s.pane_sessions.insert(
                    pane_id,
                    PaneSession {
                        conversation_id: String::new(),
                        draft: String::new(),
                        path: String::new(),
                        daemon_session_id: String::new(),
                    },
                );
            }
        }
        (root, state, desktop, dir)
    }

    /// Where each pane bar's close X was drawn, one per pane on screen. The
    /// pane-header band is what scopes them: the app topbar's own tab closes
    /// draw above it.
    fn pane_bars(cmds: &[RenderCommand], bar_y: f32) -> Vec<Vector2F> {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. }
                    if name == "x-close"
                        && origin.x >= crate::ui::SIDEBAR_WIDTH
                        && in_header_band(origin.y, bar_y) =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .collect()
    }

    /// The y the pane bars' close X are drawn on — the pane-header band. Read
    /// from the close X rather than from the expand control, because the control
    /// the pane draws depends on whether it is the expanded one. The app
    /// topbar's own tab closes draw above it, and the sidebar's close control
    /// sits left of the panes; both are excluded here.
    fn pane_bar_y(cmds: &[RenderCommand]) -> f32 {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. }
                    if name == "x-close"
                        && origin.x >= crate::ui::SIDEBAR_WIDTH
                        && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(origin.y)
                }
                _ => None,
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .expect("a pane draws its bar's close X")
    }

    /// One frame of the real tree: layout and paint, with the pointer at
    /// `cursor` when one is given (hover is read at paint).
    fn frame(
        root: &mut RootView,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<RenderCommand> {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let renderer = Renderer::new();
        let mut paint_ctx = PaintContext::new(renderer);
        if let Some(position) = cursor {
            paint_ctx.cursor_position = position;
            paint_ctx.cursor_inside = true;
        }
        root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
        paint_ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// A real click: press, one redraw, release — the frames the app draws
    /// between down and up.
    fn click(root: &mut RootView, app: &AppContext, pos: (f32, f32)) {
        let mut ctx = EventContext;
        let down = DispatchedEvent::MouseDown {
            position: vec2f(pos.0, pos.1),
            button: 0,
        };
        let _ = root.dispatch_event(&down, &mut ctx, app);
        let _ = frame(root, app, None);
        let up = DispatchedEvent::MouseUp {
            position: vec2f(pos.0, pos.1),
            button: 0,
        };
        let _ = root.dispatch_event(&up, &mut ctx, app);
    }

    /// Center of the top-most `name` icon drawn. The pane header's expand
    /// control is the top-most one in the window, and it is what the band of
    /// the pane's own bar is measured from: the app's topbar sits above it and
    /// the transcript below it.
    fn topmost_icon(cmds: &[RenderCommand], name: &str) -> (f32, f32) {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name: drawn,
                    size,
                    ..
                } if drawn == name => Some((origin.x + size / 2.0, origin.y + size / 2.0)),
                _ => None,
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|| panic!("the window draws a {name} icon"))
    }

    /// Whether something drawn at `y` is in the pane header's band: on the
    /// bar's own line, not in the app's topbar above it nor in the pane below it.
    fn in_header_band(y: f32, bar_y: f32) -> bool {
        (y - bar_y).abs() < 24.0
    }

    /// Center of the right-most `name` icon drawn: the pane on the right of a
    /// horizontal split, found by where it is rather than by draw order (both
    /// bars are on the same line, so "top-most" cannot tell them apart).
    fn rightmost_icon(cmds: &[RenderCommand], name: &str) -> (f32, f32) {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name: drawn,
                    size,
                    ..
                } if drawn == name => Some((origin.x + size / 2.0, origin.y + size / 2.0)),
                _ => None,
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|| panic!("the window draws a {name} icon"))
    }

    /// The focused pane's corner marks, with where each starts. Drawn in the
    /// accent colour, so an inactive pane's bar cannot contribute one.
    fn focus_markers(cmds: &[RenderCommand], app: &AppContext) -> Vec<Vector2F> {
        let accent = app.theme.color(ColorToken::Accent);
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name,
                    color,
                    ..
                } if name == "upper-left-triangle" && *color == accent => Some(*origin),
                _ => None,
            })
            .collect()
    }

    /// Every text the header band draws, with where it starts.
    fn header_texts(cmds: &[RenderCommand], bar_y: f32) -> Vec<(String, Vector2F)> {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if in_header_band(origin.y, bar_y) => {
                    Some((text.clone(), *origin))
                }
                _ => None,
            })
            .collect()
    }

    /// The pane header's accent diamonds: where each was drawn and how big.
    fn header_diamonds(
        cmds: &[RenderCommand],
        app: &AppContext,
        bar_y: f32,
    ) -> Vec<(Vector2F, f32)> {
        let accent = app.theme.color(ColorToken::Accent);
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name,
                    size,
                    color,
                } if name == "diamond"
                    && *size == 8.0
                    && *color == accent
                    && in_header_band(origin.y, bar_y) =>
                {
                    Some((*origin, *size))
                }
                _ => None,
            })
            .collect()
    }

    /// The pane header's separators: the 1 pt rules on the header's own line.
    fn header_rules(cmds: &[RenderCommand], app: &AppContext, bar_y: f32) -> Vec<RectF> {
        let border = app.theme.color(ColorToken::Border);
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == border
                    && *corner_radius == 0.0
                    && rect.width() == 1.0
                    && rect.height() == HEADER_CONTROL_HEIGHT
                    && in_header_band(rect.min_y(), bar_y) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect()
    }

    fn tool_call(id: &str, name: &str) -> ToolCall {
        tool_call_with(id, name, "{}")
    }

    fn tool_call_with(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            status: ToolCallStatus::Running,
            result: None,
            diff: None,
        }
    }

    fn sub_agent(child_id: &str, kind: &str, description: &str) -> SubAgentRecord {
        SubAgentRecord {
            // A spawn call of its own: the fixture's in-flight `call-1` is a
            // different piece of work, not the call that spawned these.
            parent_call_id: "call-spawn".to_string(),
            row: SubAgentRow {
                child_id: child_id.to_string(),
                subagent_type: kind.to_string(),
                description: description.to_string(),
                status: SubAgentRowStatus::Running,
                activity: String::new(),
                elapsed: Duration::ZERO,
                turns: 0,
                tool_calls: 0,
                tokens: 0,
                outcome: None,
                background: false,
            },
        }
    }

    /// A child that has reported its outcome: its record stays in the pane —
    /// the transcript's row draws it — but it is no longer work in flight.
    fn finished_sub_agent(child_id: &str, kind: &str, description: &str) -> SubAgentRecord {
        let mut record = sub_agent(child_id, kind, description);
        record.row.status = SubAgentRowStatus::Completed;
        record
    }

    /// A chat pane is one surface: Escape no longer turns it into a terminal,
    /// and no harness-mode flag is touched.
    #[test]
    fn escape_does_not_switch_a_chat_pane_to_a_terminal() {
        use goble_ui::event::DispatchedEvent;

        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let pane_id = state.borrow().active_pane_id;
        let _ = frame(&mut root, &app, None);
        let handled = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: goble_ui::event::ModifiersState::none(),
            },
            &mut goble_ui::elements::EventContext::default(),
            &app,
        );
        let _ = handled;
        let s = state.borrow();
        assert!(
            !s.pane_controls(pane_id).harness_mode,
            "Escape never turns a chat pane into a harness terminal"
        );
        assert_eq!(
            s.spaces[0].leaf_kind(pane_id),
            Some(&crate::ui::PaneKind::Chat),
            "the pane is still the agent chat"
        );
        drop(s);
        let cmds = frame(&mut root, &app, None);
        assert!(
            !cmds.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "Run any command"
            )),
            "no terminal surface replaced the chat"
        );
    }

    /// The expand command is reachable from the pane's own input too, listed
    /// with the other pane gestures, so a user who works from the keyboard does
    /// not have to find the bar's control.
    #[test]
    fn the_panes_input_lists_the_expand_command() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = split_root();
        {
            // `/max` narrows the menu to the expand command, which is what the
            // menu draws.
            let mut s = state.borrow_mut();
            s.set_active_pane_draft("/max".to_string());
        }
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            drawn.iter().any(|text| text == "/maximize"),
            "the input's slash menu lists the expand command: {drawn:?}"
        );
        assert!(
            drawn
                .iter()
                .any(|text| text == "Expand this pane over the panes space"),
            "with what running it does: {drawn:?}"
        );
    }

    /// The middle of a pane, from the bounds the pane recorded while painting.
    fn pane_center(state: &Rc<RefCell<UiState>>, pane_id: u64) -> f32 {
        let bounds = state
            .borrow()
            .pane_bounds
            .borrow()
            .get(&pane_id)
            .copied()
            .expect("the pane recorded its bounds");
        (bounds.min_x() + bounds.max_x()) / 2.0
    }

    /// The horizontal middle of the one run `needle` the pane bar draws.
    fn heading_center(cmds: &[RenderCommand], bar_y: f32, needle: &str) -> f32 {
        use goble_ui::elements::text::measure_text;
        let mut drawn = header_texts(cmds, bar_y)
            .into_iter()
            .filter(|(text, _)| text == needle);
        let (_, origin) = drawn
            .next()
            .unwrap_or_else(|| panic!("{needle:?} is drawn in the bar"));
        assert!(drawn.next().is_none(), "{needle:?} is drawn once");
        origin.x + measure_text(needle, 12.0, 1.2, f32::INFINITY).x / 2.0
    }

    /// One frame of the real tree at an explicit window size, so the wide
    /// (lane-active) and narrow (lane-inactive) pane shapes can both be read.
    fn frame_sized(root: &mut RootView, app: &AppContext, size: Vector2F) -> Vec<RenderCommand> {
        let _ = root.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
        let mut ctx = PaintContext::new(Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The pane's topbar band: the square `Surface` fill above the pane body
    /// whose height is the bar plus the pane frame's inset. Excludes the app
    /// topbar's lip/tab fills (rounded) and the pane cards (rounded, taller).
    fn header_band(cmds: &[RenderCommand], app: &AppContext) -> RectF {
        let band_height = crate::ui::shell::TOPBAR_HEIGHT + PANE_INSET;
        let mut bands: Vec<RectF> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && *corner_radius == 0.0
                    && (rect.height() - band_height).abs() < 0.01
                    && rect.min_y() > crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(bands.len(), 1, "one pane, one header band: {bands:?}");
        bands.remove(0)
    }

    fn pane_rect(state: &Rc<RefCell<UiState>>) -> RectF {
        let s = state.borrow();
        let bounds = s
            .pane_bounds
            .borrow()
            .get(&s.active_pane_id)
            .copied()
            .expect("the pane recorded its bounds");
        bounds
    }

    /// The panel itself is ONE darker surface: the card's whole background is
    /// the same `Bg` token the adjustable content column paints, so the area
    /// outside the text column reads exactly like the column — the lane is a
    /// width, not a different material — while the panel surface around the
    /// card is the sidebar's `Surface`. Checked in both themes with theme
    /// tokens only.
    #[test]
    fn the_agent_pane_surface_matches_the_lane_column() {
        fn topmost_fill(cmds: &[RenderCommand], point: Vector2F) -> Option<goble_ui::ColorU> {
            cmds.iter().rev().find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if point.x >= rect.min_x()
                        && point.x <= rect.max_x()
                        && point.y >= rect.min_y()
                        && point.y <= rect.max_y() =>
                {
                    Some(*color)
                }
                _ => None,
            })
        }

        fn assert_uniform(app: &AppContext) {
            use goble_ui::elements::resolve_content_width;

            let (mut root, state, _desktop, _dir) = pane_root();
            let size = vec2f(1600.0, 900.0);
            let _ = frame_sized(&mut root, app, size);
            let cmds = frame_sized(&mut root, app, size);
            let pane = pane_rect(&state);
            let bg = app.theme.color(ColorToken::ContentBg);

            // The pane card: the tall fill spanning the pane. An agent pane's
            // card is the darker `Bg`, not the lighter `Surface`.
            let (card, card_color) = cmds
                .iter()
                .find_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. }
                        if (rect.width() - pane.width()).abs() < 0.5
                            && rect.height() > 300.0
                            && (rect.min_x() - pane.min_x()).abs() < 0.5 =>
                    {
                        Some((*rect, *color))
                    }
                    _ => None,
                })
                .expect("the pane card paints its surface");
            assert_eq!(
                card_color, bg,
                "the whole agent pane card is the ContentBg token, not a hardcoded colour"
            );

            // The lane column: the tall fill under the pane's chrome, narrower
            // than the card, centred.
            let (column, column_color) = cmds
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. }
                        if rect.width() < pane.width() - 1.0
                            && rect.height() > 300.0
                            && rect.min_x() >= pane.min_x()
                            && rect.max_x() <= pane.max_x() =>
                    {
                        Some((*rect, *color))
                    }
                    _ => None,
                })
                .max_by(|a, b| {
                    (a.0.width() * a.0.height()).total_cmp(&(b.0.width() * b.0.height()))
                })
                .expect("the lane paints its content column");

            let lane = resolve_content_width(pane.width(), None);
            let gutter = (pane.width() - lane) / 2.0;
            assert!(
                (column.width() - lane).abs() < 0.5
                    && (column.min_x() - (pane.min_x() + gutter)).abs() < 0.5,
                "the column is the centred lane: {column:?} vs {lane} @ {gutter}"
            );
            assert_eq!(
                column_color, bg,
                "the lane column is the darker `Bg` token, not a hardcoded colour"
            );
            assert_eq!(
                column_color, card_color,
                "outside the text column is the same background as inside it"
            );

            // And what the frame actually leaves visible in the gutter beside
            // the column, under the chrome band, is the same `Bg` — the
            // outside matches the inside on screen, not only in the fill list.
            for x in [
                pane.min_x() + gutter / 2.0,
                pane.min_x() + gutter + lane + gutter / 2.0,
            ] {
                let point = vec2f(x, pane.min_y() + pane.height() / 2.0);
                assert_eq!(
                    topmost_fill(&cmds, point),
                    Some(bg),
                    "the gutter beside the content column is the panel background at {point:?}: {card:?}"
                );
            }

            // The panel surface AROUND the card is the content material; the
            // shell/topbar keeps the grey `Surface`.
            let outside_card = vec2f(
                pane.min_x() - PANE_INSET / 2.0,
                pane.min_y() + pane.height() / 2.0,
            );
            assert_eq!(
                topmost_fill(&cmds, outside_card),
                Some(app.theme.color(ColorToken::ContentBg)),
                "the panel surface around the card is ContentBg: {outside_card:?}"
            );
        }

        // Dark: the window's gray `Bg` across the whole pane.
        assert_uniform(&AppContext::default());
        // Light: the same token, no hardcoded colours.
        let mut light = AppContext::default();
        light.theme = goble_ui::theme::Theme::light();
        assert_uniform(&light);
    }
}
