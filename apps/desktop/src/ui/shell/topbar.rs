use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Button, ComposerButton, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, Empty, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext, Point, PopupMenu, PopupMenuItem, RunningIndicator, SizeConstraint, Stack, Text,
    TextInput, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY, BUTTON_SECONDARY};
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, SpacingToken, TabColor, TabTint};

use crate::media::medium_icon;
use crate::state::PaneDrag;
use crate::ui::{PaneKind, Space};

use super::super::tab_menu::{build_tab_menu, TabMenuAction, TabMenuTab};
use super::super::{
    AgentDashboardAction, DiffPanelAction, MediaActions, MediaSnapshot, PaneDropTarget, UiActions,
    UiSnapshot,
};

/// The width of the environment menu's hover tray.
const ENV_TRAY_WIDTH: f32 = 220.0;

/// The rows the environment menu puts above its environment list: "New
/// terminal", "New Agent", "Cloud agent". The environment rows start here, and
/// the trailing "New environment…" row follows the last of them.
const ENV_MENU_LEADING_ROWS: usize = 3;

/// The widest the ghost card of a lifted pane grows before its title is
/// squeezed, and how far its corner sits below/right of the pointer so it never
/// covers the point the drop is resolved at.
const GHOST_MAX_WIDTH: f32 = 200.0;
const GHOST_OFFSET_X: f32 = 12.0;
const GHOST_OFFSET_Y: f32 = 6.0;

// The general toolbar doubles as the OS titlebar on macOS, so it is a touch
// shorter and leaves room for the traffic lights on the left. Shared with the
// pane headers (terminal/agent) so the general toolbar and the per-pane topbar
// render at the same height.
#[cfg(target_os = "macos")]
pub const TOPBAR_HEIGHT: f32 = 36.0;
#[cfg(not(target_os = "macos"))]
pub const TOPBAR_HEIGHT: f32 = 40.0;

/// The height of the full-window-width foot of the topbar: the band under the
/// bar's own row. The bar block is `TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND` tall and
/// is all the window's `Bg` — the band is the extra room the floating tabs need
/// under them and the dark surface that reaches the window's outline at the
/// topbar's own edges. The content sheet (the sidebar's `Surface` and the panes'
/// header band) begins exactly at the band's bottom, so the window's darker
/// background fills every pixel up to the outline above it and there is no
/// lighter strip between the bar and the content. The selected tab is the same
/// `Surface` as that sheet and continues through the band into it, while its
/// inverted corners are cut out of its flare in the bar's `Bg`, which is what
/// makes the merge read. 4 pt: inside a 3–5 pt window, thin enough not to read
/// as its own band.
pub const TOPBAR_BOTTOM_BAND: f32 = 4.0;

/// The height of the whole bar block: the bar's own row plus the
/// [`TOPBAR_BOTTOM_BAND`] under it. The body reserves this below the bar, and
/// the bar's own layout uses it too, so the controls and the tab labels centre on
/// the block the window actually shows — not on the shorter row, which would
/// leave them a couple of points high with the band's room unused underneath.
pub(super) fn bar_height() -> f32 {
    TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND
}

/// The room the macOS traffic lights take at the bar's leading edge, plus the
/// gap that sets the Tools control apart from them. They are real OS controls
/// overlaying the content (the bar doubles as the titlebar), so the bar's
/// controls start past them.
#[cfg(target_os = "macos")]
pub(super) const TOPBAR_TRAFFIC_INSET: f32 = 94.0;
#[cfg(not(target_os = "macos"))]
pub(super) const TOPBAR_TRAFFIC_INSET: f32 = 0.0;

/// The bar's leading padding while the window is full screen: macOS hides the
/// traffic lights then, so their room goes to the tabs and the bar only keeps
/// enough to hold its first icon off the window's edge. warp-new does the same,
/// falling back to its plain `TAB_BAR_PADDING_LEFT`
/// (`app/src/workspace/view.rs:19649-19670`).
#[cfg(target_os = "macos")]
pub(super) const TOPBAR_FULLSCREEN_PADDING: f32 = 10.0;
#[cfg(not(target_os = "macos"))]
pub(super) const TOPBAR_FULLSCREEN_PADDING: f32 = 0.0;

/// The bar's leading padding: the traffic lights' room while the window is
/// windowed, and [`TOPBAR_FULLSCREEN_PADDING`] while it is full screen.
pub(super) fn left_padding(fullscreen: bool) -> f32 {
    if fullscreen {
        TOPBAR_FULLSCREEN_PADDING
    } else {
        TOPBAR_TRAFFIC_INSET
    }
}

/// The bar's trailing padding: the margin its content keeps from the window's
/// own trailing edge — the same one the leading control keeps past the traffic
/// lights ([`TOPBAR_FULLSCREEN_PADDING`]), so the Settings icon ends as far
/// from the right edge as the first control starts from the bar's content
/// edge. The traffic lights only buy room on the leading side, so this is the
/// same in both window states and the right-hand group does not move when the
/// window goes full screen.
///
/// The bar lays its content out between the two paddings, so the window's own
/// 1 px rule (drawn at the content edge, see [`crate::ui::build_ui`]) sits in
/// the margin rather than on top of the last control.
pub(super) const TOPBAR_RIGHT_PADDING: f32 = TOPBAR_FULLSCREEN_PADDING;

/// The box a topbar icon sits in: a 16 pt glyph with 3 pt of padding on every
/// side, so the icon reads tight against the bar's edge.
pub(super) const TOPBAR_ICON_BUTTON_SIZE: f32 = 22.0;

/// The gap between the bar's own controls, and between the last of them and the
/// tab strip's first tab (the tab's own fill insets a further
/// [`TAB_EDGE_INSET`], so the visible gap is 6).
const TOPBAR_CONTROL_GAP: f32 = 3.0;

/// The width a tab never grows past, however wide the window is: warp-new caps
/// a tab at 200 (`app/src/tab.rs:1841`, `.with_max_width(200.)`). The visible
/// fill is [`TAB_EDGE_INSET`] narrower on each side, so it tops out at 194.
const MAX_TAB_WIDTH: f32 = 200.0;

/// The flex the empty spacer after the last tab of warp-new's tab bar takes,
/// relative to a tab's own 1.0 (`app/src/workspace/view.rs:19393`,
/// `Shrinkable::new(0.5, Empty::new().finish())`). The spacer lays out to the
/// max a flex child is offered, so it *is* the room the tabs leave over, and
/// the bar's own right-hand controls stay pinned to the right edge because of
/// it.
const TAB_STRIP_SPACER_FLEX: f32 = 0.5;

/// The room between the bar's top edge and the top of a tab's fill. Chrome's
/// tab strip pads the tab 6 pt inside a 41 pt strip
/// (`browser/ui/layout_constants.cc:121-122`); our bar is 36 pt on macOS and
/// 40 pt elsewhere, so the fill floats the same distance from the top.
#[cfg(target_os = "macos")]
pub(super) const TAB_TOP_INSET: f32 = 5.0;
#[cfg(not(target_os = "macos"))]
pub(super) const TAB_TOP_INSET: f32 = 6.0;

/// The top corner radius of a tab's fill: Chrome's
/// `TabStyle::GetTopCornerRadius` (10, `chrome/browser/ui/tabs/tab_style.cc`).
pub(super) const TAB_TOP_RADIUS: f32 = 10.0;

/// The radius of the flare — the inverse corner where the selected tab meets
/// the content: Chrome's `TabStyle::GetBottomCornerRadius` (12, same file).
pub(super) const TAB_FLARE_RADIUS: f32 = 12.0;

/// How far a tab's fill is inset from its slot on each side. Two neighbours
/// therefore show a 6 pt gap between their fills, Chrome's own gap
/// (`tab_style.cc:109-113`: a 238 pt step between 232 pt contents).
pub(super) const TAB_EDGE_INSET: f32 = 3.0;

/// The gap between a floating pill's bottom edge and the merge line, where the
/// selected tab continues into the content below. It is the
/// [`TOPBAR_BOTTOM_BAND`] the selected tab has to cross plus the bar's own 1 pt
/// foot and Chrome's 6 pt pill-to-strip-bottom gap, so a pill ends 1 pt above the
/// bar row's own bottom edge (`y = TOPBAR_HEIGHT − 1`) and the whole band stays
/// visible, dark, underneath it. The selected tab's feet therefore land on the
/// pane card's top border below the band, and its stem crosses the pane gutter
/// to the card's `Surface` (the merge).
pub(super) const TAB_PILL_BOTTOM_GAP: f32 = TOPBAR_BOTTOM_BAND + 6.0;

/// How far the selected tab's fill runs past the bar's bottom edge so that it
/// meets the pane surface below. The bar block — bar plus [`TOPBAR_BOTTOM_BAND`]
/// — ends at [`tab_fillet_bottom`], the top of the content sheet (the sidebar and
/// the panes' full-bleed header band). Below the sheet's first rows over the pane
/// area lie the pane area's 4 pt gutter and the pane card's 1 pt border
/// ([`crate::ui::panes::PANE_INSET`]): the fill crosses all of them to reach the
/// card's `Surface` — Chrome's "the active tab meets the toolbar", one level
/// down — so it merges with the content whatever the pane's own header draws.
pub(super) const TAB_MERGE_DROP: f32 = TOPBAR_BOTTOM_BAND + crate::ui::panes::PANE_INSET + 1.0;

/// The narrowest a tab's contents can get and still carry its close control:
/// Chrome's `kMinWidthToShowCloseButton` (68,
/// `chrome/browser/ui/views/tabs/common/tab_view_horizontal_layout.cc:28-29`).
pub(super) const MIN_CONTENTS_FOR_CLOSE: f32 = 68.0;

/// The width one tab of the bar is drawn at, given the room the strip has for
/// tabs (`room`: the window's width less everything the bar draws itself) and
/// how many tabs are on it.
///
/// warp-new's tab bar is a flex row of one `Shrinkable::new(1.0, tab)` per tab
/// with a `Shrinkable::new(0.5, Empty)` spacer after the last of them
/// (`app/src/workspace/view.rs:19348-19395`), so the room is divided by the sum
/// of the flex factors — `tabs + TAB_STRIP_SPACER_FLEX` — and every tab takes
/// an equal part of it, capped at [`MAX_TAB_WIDTH`]. The spacer's half share is
/// the room the tabs leave over: it stays at the end of the strip, between the
/// last tab and the bar's own right-hand controls, and grows once the tabs
/// reach the cap and stop taking their share.
fn tab_slot(room: f32, tabs: usize) -> f32 {
    let flex = tabs.max(1) as f32 + TAB_STRIP_SPACER_FLEX;
    (room.max(0.0) / flex).min(MAX_TAB_WIDTH)
}

/// The top corner radius of a tab whose visible fill is `content_width` wide.
/// Chrome clamps it so at least a third of the top edge stays flat
/// (`chrome/browser/ui/views/tabs/common/horizontal_tab_style_views.cc:900-910`:
/// `clamp((width − 2·10)/3, 0, 10)`), which keeps a crowded strip's fills
/// rounded shapes rather than lozenges.
pub(super) fn tab_top_radius(content_width: f32) -> f32 {
    ((content_width - 2.0 * TAB_TOP_RADIUS) / 3.0).clamp(0.0, TAB_TOP_RADIUS)
}

/// The y the selected tab's fill reaches: past the bar's bottom edge, into the
/// surface below (see [`TAB_MERGE_DROP`]).
pub(super) fn tab_merge_bottom() -> f32 {
    TOPBAR_HEIGHT + TAB_MERGE_DROP
}

/// The y a floating tab's fill ends at: [`TAB_PILL_BOTTOM_GAP`] above the merge
/// line, so the pill floats clear of the content.
pub(super) fn tab_pill_bottom() -> f32 {
    tab_merge_bottom() - TAB_PILL_BOTTOM_GAP
}

/// The y the selected tab's inverted corners bottom out: the bottom edge of the
/// [`TOPBAR_BOTTOM_BAND`], which is the top edge of the content sheet. The flare
/// and its concave fillets therefore bottom out on the sheet's edge instead of
/// hanging over it, and the tab's own width continues from here across the pane
/// gutter and card border to the pane's surface (the merge).
pub(super) fn tab_fillet_bottom() -> f32 {
    TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND
}

/// The rectangle a tab's visible fill occupies inside the slot it was given:
/// the slot inset by [`TAB_EDGE_INSET`] on each side, with its top at
/// [`TAB_TOP_INSET`] and the floating pill's height.
pub(super) fn tab_content_rect(slot: RectF) -> RectF {
    rectf(
        slot.min_x() + TAB_EDGE_INSET,
        TAB_TOP_INSET,
        (slot.width() - 2.0 * TAB_EDGE_INSET).max(0.0),
        tab_pill_bottom() - TAB_TOP_INSET,
    )
}

/// Paint a tab's fill: the selected tab's body merged into the surface below, or
/// a floating pill. `slot` is the tab's whole slot in window coordinates, `fill`
/// the colour the tab's state gives it and `background` the bar's tone the
/// selected tab's inverted corners are cut out with.
///
/// The selected tab is composed of a rounded-top body — which carries the fill
/// down through the bar's foot into the content sheet, so the merge has no
/// seam — plus a foot band along the bottom of the band and a concave fillet at
/// each end of it. The fillets bottom out at [`tab_fillet_bottom`], the content
/// sheet's top edge, so the tab flares into the sheet exactly where the sheet
/// begins; only the tab's own width continues below that line. Chromium's
/// active-tab path has the same inverse corners
/// (`chrome/browser/ui/views/tabs/common/horizontal_tab_style_views.cc:188-340`):
/// each is a quarter circle tangent to the tab's vertical side and to the
/// horizontal bottom side of the foot. Our renderer has no paths, so a
/// background-coloured disc bites the foot's outer top corner under a clip,
/// leaving exactly that tangent quarter circle — the bite is the bar's own `Bg`,
/// and the tab flows into the sheet below.
pub(super) fn paint_tab_surface(
    renderer: &mut goble_ui::render::Renderer,
    slot: RectF,
    selected: bool,
    fill: goble_ui::ColorU,
    background: goble_ui::ColorU,
) {
    let content = tab_content_rect(slot);
    if !selected {
        renderer.fill_rounded_rect(content, fill, tab_top_radius(content.width()));
        return;
    }
    let merge_bottom = tab_merge_bottom();
    let radius = TAB_FLARE_RADIUS;
    // A fill narrower than the two fillets is too narrow to notch with them, so
    // it keeps its own rounded bottom corners instead. (Chromium does not
    // special-case this: its path self-intersects at the minimum width.)
    let feet = content.width() >= 2.0 * radius + 4.0;
    if feet {
        // The fillets cut `radius` past the fill and must not bleed into the
        // content below the merge line: clip to the tab's own merge rectangle.
        renderer.clip_rect(rectf(
            content.min_x() - radius,
            TAB_TOP_INSET,
            content.width() + 2.0 * radius,
            merge_bottom - TAB_TOP_INSET,
        ));
    }
    // The body carries the fill from the bar down to the surface, so the merge
    // is continuous under the tab's own width.
    renderer.fill_rounded_rect(
        rectf(
            content.min_x(),
            TAB_TOP_INSET,
            content.width(),
            merge_bottom - TAB_TOP_INSET,
        ),
        fill,
        tab_top_radius(content.width()),
    );
    if feet {
        // The foot band: the `radius` rows above the fillet line, widened by
        // `radius` on each side, so the tab flares out into the band and its
        // bottom side sits on the band's bottom edge.
        let fillet_bottom = tab_fillet_bottom();
        let band_top = fillet_bottom - radius;
        renderer.fill_rect(
            rectf(
                content.min_x() - radius,
                band_top,
                content.width() + 2.0 * radius,
                radius,
            ),
            fill,
        );
        // The concave fillets: a quarter of a background disc at each outer top
        // corner of the band, clipped to that corner so it bites only there. The
        // arc is tangent to the tab's vertical side and to the band's bottom
        // edge, so the side bends smoothly into the content plane.
        let disc = radius * 2.0;
        renderer.clip_rect(rectf(content.min_x() - radius, band_top, radius, radius));
        renderer.fill_rounded_rect(
            rectf(content.min_x() - disc, band_top - radius, disc, disc),
            background,
            radius,
        );
        renderer.pop_clip();
        renderer.clip_rect(rectf(content.max_x(), band_top, radius, radius));
        renderer.fill_rounded_rect(
            rectf(content.max_x(), band_top - radius, disc, disc),
            background,
            radius,
        );
        renderer.pop_clip();
        renderer.pop_clip();
        // Below the fillet line only the tab's own width continues: square off
        // the body's rounded bottom corners there, so the merge into the surface
        // is the tab's straight column rather than a pinch.
        renderer.fill_rect(
            rectf(
                content.min_x(),
                fillet_bottom,
                content.width(),
                merge_bottom - fillet_bottom,
            ),
            fill,
        );
    }
}

/// Single general toolbar, reduced to the essentials: a sidebar toggle, the
/// agent management control, the workspace tabs (one per space, the active one
/// editable inline), a "+ ▾"
/// control that opens a new workspace / picks the environment it runs in, and
/// the inbox beside the Settings icon pinned to the far right.
///
/// The tabs sit next to the toggle as Chrome-style tabs: the selected one is
/// filled with the pane surface and continues into the content below, the
/// others float as rounded shapes over the bar's `Bg` with small gaps and no
/// rules between them, so a new workspace shows up *next to* the current one
/// instead of replacing it.
///
/// The "+" opens a new workspace in the active environment; the "▾" lists the
/// two kinds of workspace it can start — a terminal and an agent view holding a
/// fresh conversation — then the environments (the VMs the user added appear by
/// name) — choosing one opens a new workspace that runs on it, and hovering one
/// shows a tray whose "Make default" makes it the environment new workspaces
/// start in — plus a trailing "New environment…" entry.
pub fn build_topbar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    // Sidebar toggle (retract/expand the left conversation list). Its hover
    // tag names the panel it toggles, like the other icon-only controls.
    let on_toggle_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_button = Tooltip::new(
        TopbarButton::new(
            Icon::new("menu-01")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        // The tools panel is a toggle like the dashboard: lit while the
        // sidebar is out (including a manual re-expand below the responsive
        // breakpoint), dark while it is retracted or auto-collapsed.
        .with_active(state.responsive.sidebar_shown(state.sidebar_visible))
        .with_size(TOPBAR_ICON_BUTTON_SIZE)
        .with_on_click(move || (on_toggle_sidebar.borrow_mut())())
        .finish(),
        "Tools",
    )
    .with_position(TooltipPosition::Below)
    .finish();

    // Agent management: grok-build's `[Dashboard]` as a panel beside the tools
    // control. It lists the roster, peeks and replies, and dispatches.
    let on_agent_dashboard = actions.on_agent_dashboard.clone();
    let agent_dashboard_button = Tooltip::new(
        TopbarButton::new(
            Icon::new("robot")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_active(state.agent_dashboard.open)
        .with_size(TOPBAR_ICON_BUTTON_SIZE)
        .with_on_click(move || {
            (on_agent_dashboard.borrow_mut())(AgentDashboardAction::Toggle);
        })
        .finish(),
        "Agent management",
    )
    .with_position(TooltipPosition::Below)
    .finish();

    // One tab per workspace, active one first-class (double-click renames it).
    // A file open in a pane is not a workspace: it is named in its pane's own
    // topbar and adds nothing to this bar, so browsing files leaves the strip
    // as it was.
    //
    // The bar records its own rectangle while it paints (see [`TopbarRow`]): a
    // lifted pane treats the whole bar as its tab strip, so a release on the
    // empty room beside the tabs still names an insertion point.
    let bar_bounds: Rc<RefCell<Option<RectF>>> = Rc::new(RefCell::new(None));
    let workspaces = TabStrip::new(vec![build_workspace_strip(
        app,
        state,
        actions,
        Rc::clone(&bar_bounds),
    )])
    .finish();

    // "+ ▾": open a new workspace, or pick the environment it runs in.
    let add_workspace = build_add_workspace_control(app, state, media, media_actions, actions);

    // The live cue: a spinner plus the count of work in flight (C1's state),
    // between the workspace controls and the settings icon. It renders nothing
    // at zero, so the bar keeps its current look when nothing is running. It is
    // information only — no click opens anything from it.
    let live_indicator = build_live_indicator(app, state.live_work_count, state.live_work_phase);

    // The global inbox: an icon with the unread count, at the right end of the
    // bar next to Settings. Pressing it toggles the overlay
    // (Cmd/Ctrl+Shift+I does the same), and the count is the snapshot's live
    // unread total.
    let inbox_button = build_inbox_control(
        app,
        state.inbox_unread_count,
        state.inbox_open,
        actions,
        Rc::clone(&state.inbox_anchor),
    );

    // Settings stays reachable as an icon pinned to the far right.
    let on_settings = actions.on_settings.clone();
    let settings_button = Tooltip::new(
        TopbarButton::new(
            Icon::new("settings")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(TOPBAR_ICON_BUTTON_SIZE)
        .with_on_click(move || (on_settings.borrow_mut())())
        .finish(),
        "Settings",
    )
    .with_position(TooltipPosition::Below)
    .finish();

    // The strip's tabs share the room the bar has left for them, so the row is
    // laid out by the bar itself rather than by a plain flex row: the strip is
    // given what the toggle, the "+ ▾" control, the live cue, the inbox and
    // Settings leave behind, and the controls keep the positions they had (see
    // [`TopbarRow`]).
    // The selected pane's changes, immediately left of the inbox: the same
    // control the rich input used to carry, now a general-topbar item.
    let diff_control = build_diff_control(app, state, actions);
    let row = TopbarRow::new(
        vec![sidebar_button, agent_dashboard_button],
        workspaces,
        vec![add_workspace],
        vec![live_indicator, diff_control, inbox_button, settings_button],
    )
    .with_left_spacing(TOPBAR_CONTROL_GAP)
    .with_group_spacing(sm)
    .with_bar_bounds(bar_bounds)
    .with_gesture(
        state.pane_drag.is_some() || state.space_drag.is_some() || state.space_press.is_some(),
    )
    .finish();

    // The workspace strip is drawn at the full height of the bar block — the
    // bar row plus its band ([`bar_height`]) — and each tab spans it and floats
    // its fill inside it (see [`WorkspaceChip`]), so the bar needs no vertical
    // padding of its own: the shorter controls centre in the same block, its
    // bottom edge is the content sheet's top, and the macOS traffic lights sit
    // vertically centred inside it.
    //
    // The bar's own fill is the window frame's `Bg`, one tone away from the
    // pane surface the selected tab is filled with. That two-tone is what makes
    // the selected tab legible: with the bar left on `Surface` the selected
    // fill would be invisible against it and there would be nothing for the tab
    // to continue into. (Light-theme hover is darker than the strip and
    // dark-theme hover lighter — Chrome behaves the same way in each theme.)
    //
    // The leading padding is the room those traffic lights need — but only while
    // they are on screen: a full-screen window hides them, and the bar gives
    // their room back to the tabs. The window's own state answers first (the
    // event loop records it on every frame, the green button included); the app
    // flag covers a tree built without a live window, where nothing has
    // reported yet. The trailing padding is the bar's own content margin, so
    // the right-hand group ends that far from the window's edge instead of
    // sitting on it.
    let fullscreen = app
        .window_control
        .window_fullscreen()
        .unwrap_or(state.fullscreen);
    let bar: Box<dyn Element> = Container::new(row)
        .with_padding(EdgeInsets::new(
            left_padding(fullscreen),
            0.0,
            TOPBAR_RIGHT_PADDING,
            0.0,
        ))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish();
    // The card a pane being dragged onto the strip carries floats over the whole
    // bar — tabs, "+ ▾" and the settings icon alike — so it is a second,
    // zero-sized child of a stack rather than part of the row. The tab menu is
    // the third: hung from the pointer it was opened at, it floats over the bar
    // and over the pane below it, and it is the first child dispatched, so the
    // press that lands on it never reaches the tab under it.
    let menu = build_space_menu(state, actions);
    goble_ui::elements::Stack::new()
        .with_children(vec![
            bar,
            build_pane_drag_ghost(app, state.pane_drag.as_ref()),
            menu,
        ])
        .finish()
}

/// The menu of the tab a right click opened, or nothing at all while none is
/// open. The panel hangs from the point the click landed on, over the window —
/// see [`crate::ui::tab_menu`].
fn build_space_menu(state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let Some(menu) = *state.space_menu.borrow() else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let Some(space) = state.spaces.get(menu.index) else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let tab = TabMenuTab {
        index: menu.index,
        tabs: state.spaces.len(),
        color: space.color,
        named: space.named,
    };
    let on_action = actions.on_space_menu_action.clone();
    let on_close = actions.on_space_menu_close.clone();
    let index = menu.index;
    build_tab_menu(
        tab,
        menu.at,
        Rc::new(RefCell::new(move |action: TabMenuAction| {
            (on_action.borrow_mut())(index, action)
        })),
        Rc::new(RefCell::new(move || (on_close.borrow_mut())())),
    )
}

/// The topbar's inbox control: the inbox glyph, with a small unread badge when
/// the count is non-zero. The badge is an overlay so it does not make the
/// button wider than the other topbar controls.
///
/// The control reports its own rect while it paints (`anchor`): the inbox panel
/// is a window-level popover hung from this button's trailing edge, so it needs
/// the button's bounds and cannot be the button's child.
fn build_inbox_control(
    app: &AppContext,
    unread: usize,
    open: bool,
    actions: &UiActions,
    anchor: Rc<RefCell<Option<RectF>>>,
) -> Box<dyn Element> {
    let on_inbox = actions.on_inbox.clone();
    let button = TopbarButton::new(
        Icon::new("inbox-01")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_active(open)
    .with_size(TOPBAR_ICON_BUTTON_SIZE)
    .with_on_click(move || (on_inbox.borrow_mut())())
    .finish();
    let button = Tooltip::new(button, "Inbox")
        .with_position(TooltipPosition::Below)
        .finish();
    let control: Box<dyn Element> = if unread == 0 {
        button
    } else {
        let count = if unread > 99 {
            "99+".to_string()
        } else {
            unread.to_string()
        };
        let badge = Container::new(
            Text::new(count)
                .with_font_size(9.0)
                .with_theme_color(ColorToken::Bg, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_padding(EdgeInsets::new(3.0, 0.0, 3.0, 0.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Badge)))
        .with_corner_radius(6.0)
        .finish();
        Stack::new()
            .with_children(vec![button])
            .with_overlay(badge, vec2f(16.0, 0.0))
            .finish()
    };
    crate::ui::inbox::AnchorProbe::new(control, anchor).finish()
}

/// The topbar's live cue: a spinner and `◆ N`, where N is C1's count of work in
/// flight (`LiveWork::work_count`). At zero it renders nothing at all, so the
/// bar keeps its current look and there is no hit target to click.
///
/// It is information only: it carries no handler, so a click passes through.
pub(super) fn build_live_indicator(
    app: &AppContext,
    work_count: usize,
    phase: f32,
) -> Box<dyn Element> {
    if work_count == 0 {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    }
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5)
        .with_child(
            RunningIndicator::new()
                .with_size(12.0)
                .with_phase(phase)
                .finish(),
        )
        .with_child(
            Icon::new("diamond")
                .with_size(8.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new(work_count.to_string())
                .with_theme_color(ColorToken::Accent, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    // The same padding the indicator had as a hit target, so removing the click
    // leaves the bar's layout unchanged.
    Container::new(row)
        .with_padding(EdgeInsets::new(5.0, 4.0, 5.0, 4.0))
        .finish()
}

/// The "Open changes" control: the selected pane's working tree as one topbar
/// item, immediately left of the inbox.
///
/// It reads the same per-pane summary the rich input's diff control used to
/// show — the changed-file count and the lines added and removed — and it is
/// drawn only while the selected pane has changes: a clean tree, a pane outside
/// a repository, and a worker/SSH pane (whose work is not on this machine) all
/// hide it, the way the composer's control was hidden. Clicking it opens the
/// review pane on every change (`DiffPanelAction::Open(None)`), the same action
/// the composer's control sent.
fn build_diff_control(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let pane_id = state.active_pane_id;
    let Some(diff) = state
        .composer_context
        .get(&pane_id)
        .and_then(|context| context.diff.as_ref())
        .filter(|diff| !diff.is_empty())
    else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let added = diff.added();
    let removed = diff.removed();
    let mut content = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Icon::new("file")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(diff.files_changed().to_string())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(DIFF_CONTROL_FONT_SIZE)
                .with_weight(FontWeight::SemiBold)
                .with_max_lines(1)
                .finish(),
        );
    if added > 0 || removed > 0 {
        content = content.with_child(
            Text::new("•")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(DIFF_CONTROL_FONT_SIZE)
                .with_weight(FontWeight::SemiBold)
                .with_max_lines(1)
                .finish(),
        );
    }
    if added > 0 {
        content = content.with_child(
            Text::new(format!("+{added}"))
                .with_theme_color(ColorToken::Success, app)
                .with_font_size(DIFF_CONTROL_FONT_SIZE)
                .with_weight(FontWeight::SemiBold)
                .with_max_lines(1)
                .finish(),
        );
    }
    if removed > 0 {
        content = content.with_child(
            Text::new(format!("-{removed}"))
                .with_theme_color(ColorToken::Error, app)
                .with_font_size(DIFF_CONTROL_FONT_SIZE)
                .with_weight(FontWeight::SemiBold)
                .with_max_lines(1)
                .finish(),
        );
    }
    let on_diff = actions.on_diff_panel.clone();
    let button = ComposerButton::new(content.finish())
        .with_height(TOPBAR_ICON_BUTTON_SIZE)
        .with_h_pad(6.0)
        .with_radius(6.0)
        .with_hover_fill(ColorToken::Hover)
        .with_on_click(move || {
            (on_diff.borrow_mut())(pane_id, DiffPanelAction::Open(None));
        })
        .finish();
    Tooltip::new(button, "Open changes")
        .with_position(TooltipPosition::Below)
        .finish()
}

/// The font size every tab of the bar reads at: 12 pt, the toolkit's own UI text
/// size (`Text`'s default and `LabelSize::Xs`), which is what warp-new's
/// `appearance.ui_font_size()` is to its tabs (`TabStyles::default`,
/// `app/src/tab.rs:800-812`). `goble-ui`'s `Theme` carries no UI font-size token
/// — only the density factor — so there is nothing for this to be read from.
const TAB_FONT_SIZE: f32 = 12.0;

/// The font size the "Open changes" control reads at: the bar's own UI size.
const DIFF_CONTROL_FONT_SIZE: f32 = 12.0;

/// The font size a tab's shortcut hint reads at, a step under the name's so
/// the hint reads as the tab's secondary line of information.
const TAB_HINT_FONT_SIZE: f32 = 11.0;

/// The chord's modifier, as the tab hints spell it: the Command symbol on
/// macOS, the word on every other platform.
#[cfg(target_os = "macos")]
const CHORD: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const CHORD: &str = "Ctrl+";

/// The shortcut a tab advertises while the chord modifier is held: `⌘1` for the
/// first tab on macOS, `Ctrl+1` for it elsewhere.
pub(super) fn tab_hint(index: usize) -> String {
    format!("{CHORD}{index}")
}

/// What a name cut down to its tab is marked with, at the end it lost.
const ELLIPSIS: &str = "…";

/// What a name too long for its tab keeps: warp-new's `ClipConfig`
/// (`app/src/tab.rs:1101-1152`). A title derived from where the tab runs is cut
/// at its **start**, so the last path component stays readable; a name the user
/// typed, or an agent conversation's subject, is cut at its **end**, because
/// the words that tell one such tab from another sit at the front
/// (`should_clip_text_start`, `app/src/tab.rs:1363`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum LabelClip {
    Start,
    End,
}

/// One tab's name, drawn on a single line: the name whole while the tab is wide
/// enough to draw it, and otherwise cut with an ellipsis at the end
/// [`LabelClip`] names — the way warp-new truncates a tab title, so a bar that
/// has to share its room shortens the names instead of wrapping them out of the
/// bar.
///
/// The run is drawn at the weight the tab's state gives it: warp-new draws the
/// active tab's title in `Weight::Medium` over the inactive tabs' regular one
/// (`TabStyles::default`, `app/src/tab.rs:800-812`).
struct TabLabel {
    name: String,
    color: ColorToken,
    weight: FontWeight,
    clip: LabelClip,
    /// The run drawn this frame: the name whole, or as much of it as fits.
    run: Option<Text>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabLabel {
    fn new(
        name: impl Into<String>,
        color: ColorToken,
        weight: FontWeight,
        clip: LabelClip,
    ) -> Self {
        Self {
            name: name.into(),
            color,
            weight,
            clip,
            run: None,
            size: None,
            origin: None,
        }
    }

    /// The one-line run that draws `name` at a tab's font size and weight.
    fn run(&self, name: &str, app: &AppContext) -> Text {
        Text::new(name)
            .with_theme_color(self.color, app)
            .with_font_size(TAB_FONT_SIZE)
            .with_weight(self.weight)
            .with_max_lines(1)
    }

    /// How wide `name` is drawn on one line.
    fn width_of(&self, name: &str, app: &AppContext) -> f32 {
        let mut run = self.run(name, app);
        run.layout(
            SizeConstraint::new(Vector2F::zero(), vec2f(f32::INFINITY, f32::INFINITY)),
            &mut LayoutContext,
            app,
        )
        .x
    }

    /// The name as the run that fits `room` draws it: whole when it fits, and
    /// otherwise as much of it as leaves room for the ellipsis that replaces the
    /// rest — the tail of the name for a start-clipped label, the head of it for
    /// an end-clipped one. Nothing at all when there is no room.
    fn fit(&self, room: f32, app: &AppContext) -> String {
        let width = self.width_of(&self.name, app);
        if width <= room {
            return self.name.clone();
        }
        if room <= 0.0 {
            return String::new();
        }
        let chars: Vec<char> = self.name.chars().collect();
        // The first guess comes from the run's own width per character; the walk
        // back from there is what pays for the ellipsis' own room.
        let per_char = (width / chars.len().max(1) as f32).max(1.0);
        let mut keep = ((room / per_char) as usize).min(chars.len().saturating_sub(1));
        while keep > 1 {
            let kept: String = match self.clip {
                LabelClip::Start => chars[chars.len() - keep..].iter().collect(),
                LabelClip::End => chars[..keep].iter().collect(),
            };
            let candidate = match self.clip {
                LabelClip::Start => format!("{ELLIPSIS}{kept}"),
                LabelClip::End => format!("{kept}{ELLIPSIS}"),
            };
            if self.width_of(&candidate, app) <= room {
                return candidate;
            }
            keep -= 1;
        }
        ELLIPSIS.to_string()
    }
}

impl Element for TabLabel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let room = constraint.max.x;
        let name = self.fit(room, app);
        // A cut run stands for the whole room it was cut to, so a parent that
        // centres it leaves it at the inset it was measured from: the name only
        // steps in from there while it is short.
        let cut = name != self.name;
        let mut run = self.run(&name, app);
        let mut size = run.layout(constraint, ctx, app);
        if cut {
            size.x = room;
        }
        self.run = Some(run);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(run) = self.run.as_mut() {
            run.paint(origin, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The bar's tab area: the frames of tabs the bar draws, side by side.
///
/// It exists to hand the frames the room the bar has for tabs — the window's
/// width less everything the bar draws itself, which [`TopbarRow`] is what
/// measures. The room is sliced *by tab*, not by frame, so every tab of the
/// strip is exactly as wide as its neighbours; each frame is then given the
/// slice its own tabs need, and a frame draws no more than the tabs it was
/// given room for (see [`TabFrame`]).
struct TabStrip {
    frames: Vec<StripFrame>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// One frame of the tab strip: the frame's own element, and how many tabs it
/// draws. The frame does not read the count from its element — the element
/// keeps it to itself — so the strip can give the frame the room its tabs need
/// before laying it out.
struct StripFrame {
    element: Box<dyn Element>,
    tabs: usize,
}

impl StripFrame {
    fn new(element: Box<dyn Element>, tabs: usize) -> Self {
        Self { element, tabs }
    }
}

impl TabStrip {
    fn new(frames: Vec<StripFrame>) -> Self {
        Self {
            frames,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabStrip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let tabs: usize = self.frames.iter().map(|frame| frame.tabs).sum();
        // The strip divides the bar the way warp-new's tab-bar row does — flex
        // shares, the trailing spacer included (see [`tab_slot`]) — and hands
        // each frame the room its own tabs need. A frame only divides what it
        // is given, so the last of the room the tabs leave over stays after the
        // strip rather than inside it.
        let slot = tab_slot(constraint.max.x, tabs);
        let mut total = Vector2F::zero();
        for frame in &mut self.frames {
            let room = slot * frame.tabs as f32;
            let size = frame.element.layout(
                SizeConstraint::loose(vec2f(room, constraint.max.y)),
                ctx,
                app,
            );
            total.x += size.x;
            total.y = total.y.max(size.y);
        }
        self.size = Some(total);
        total
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let mut x = origin.x;
        for frame in &mut self.frames {
            frame.element.paint(vec2f(x, origin.y), ctx, app);
            x += frame.element.size().unwrap_or(Vector2F::zero()).x;
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
        self.frames
            .iter_mut()
            .any(|frame| frame.element.dispatch_event(event, ctx, app))
    }
}

/// One frame of tabs: its tabs laid out in slots of equal width — the room the
/// strip handed the frame divided evenly — and as tall as the bar.
///
/// The strip has already applied warp-new's flex distribution to the bar (see
/// [`tab_slot`]); a frame only divides the room it was handed among its own tabs,
/// so a tab is the same width in every frame of the strip. The frame itself
/// paints nothing: each tab draws its own fill inside its slot (see
/// [`WorkspaceChip`]), and the floating tab's rest fill is the absence of a
/// fill, so the strip's `Bg` shows through.
struct TabFrame {
    children: Vec<FrameChild>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// A child of a [`TabFrame`]: one of its tabs, and whether it is the selected
/// one. The frame paints in three layers: the floating tabs at rest, then the
/// selected tab, then a hovered (or pressed) floating tab. Chromium gives the
/// active tab an infinite z-value (`chrome/browser/ui/views/tabs/tab.cc:1214`)
/// so its flared feet cover the rounded bottom corners of the pills beside it;
/// a pill the pointer is on is the exception the owner asked for — it paints
/// above the active tab's inverted-corner fills, so hovering a tab never leaves
/// a dark square bitten out of its bottom corners.
struct FrameChild {
    element: Box<dyn Element>,
    selected: bool,
}

impl TabFrame {
    fn new(children: Vec<FrameChild>) -> Self {
        Self {
            children,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabFrame {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The slot every tab of the frame is drawn in: an equal share of the
        // room the frame was handed. The strip has divided the bar between the
        // frames already (see [`tab_slot`]), so nothing is divided twice.
        let tabs = self.children.len().max(1) as f32;
        let slot = constraint.max.x.max(0.0) / tabs;
        let mut total = 0.0;
        for child in &mut self.children {
            // A tab *is* its slot: warp-new's tabs are equal flexible children
            // of the row, filling the share the row gives them rather than
            // shrinking to their names.
            let size = child.element.layout(
                SizeConstraint::new(vec2f(slot, 0.0), vec2f(slot, WorkspaceChip::height())),
                ctx,
                app,
            );
            total += size.x;
        }
        let size = vec2f(total, WorkspaceChip::height());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // Three layers, in paint order: (0) the floating tabs at rest, (1) the
        // selected tab, whose flare and inverted corners merge it into the
        // content sheet, and (2) a hovered or pressed floating tab, painted last
        // so the active tab's background-coloured fillet discs never bite its
        // pill. Hover is read here, exactly as [`WorkspaceChip::paint`] reads it
        // (the whole slot is the tab's hit bounds), so the two agree.
        let mut slots: Vec<(usize, bool, bool, f32)> = Vec::with_capacity(self.children.len());
        let mut x = origin.x;
        for (index, child) in self.children.iter().enumerate() {
            let width = child.element.size().unwrap_or(Vector2F::zero()).x;
            let hovered =
                !child.selected && ctx.hovered(rectf(x, origin.y, width, WorkspaceChip::height()));
            slots.push((index, child.selected, hovered, width));
            x += width;
        }
        for layer in 0..3 {
            let mut x = origin.x;
            for (index, selected, hovered, width) in &slots {
                let in_layer = match layer {
                    0 => !*selected && !*hovered,
                    1 => *selected,
                    _ => !*selected && *hovered,
                };
                if in_layer {
                    self.children[*index]
                        .element
                        .paint(vec2f(x, origin.y), ctx, app);
                }
                x += width;
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
        // Dispatch order is the layout order: the selected tab's paint order
        // must not make it swallow a press that belongs to the tab beside it.
        for child in &mut self.children {
            if child.element.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

/// The bar's one row: the sidebar toggle, the tab strip, the "+ ▾" control, and
/// then the live cue, the inbox and Settings packed against the right end.
///
/// The engine's `Flex` gives a child either no bound along its row
/// (`MainAxisSize::Min`) or the whole row's width to grow into, so the strip
/// could not tell a wide window from a crowded bar — and a strip that was not
/// told took the room it wanted and pushed the "+ ▾" control and Settings off
/// the window. So the bar lays its own row out: the controls first, at their own
/// widths, the strip into the room that is left for it, and the right group as
/// one compact run on the row's right edge, with `group_spacing` between its
/// neighbours, every child centered on the bar's height (`SidebarLayout` solves
/// the same problem for the shell's split). The row is the bar's content: the
/// bar's padding is what sets it off the window's own edges.
struct TopbarRow {
    /// The controls before the strip, in draw order.
    leading: Vec<Box<dyn Element>>,
    /// The tab strip: the one child that takes the room the rest leaves.
    strip: Box<dyn Element>,
    /// The controls after the strip, before the bar's right-hand group.
    trailing: Vec<Box<dyn Element>>,
    /// The live cue, the inbox and Settings, packed against the row's right end.
    right: Vec<Box<dyn Element>>,
    left_spacing: f32,
    group_spacing: f32,
    /// The row's own rectangle, written each paint. The pane drop strip reads
    /// it so the whole bar is the zone a lifted pane can be dropped on, not
    /// only the room the drawn tabs occupy.
    bar_bounds: Option<Rc<RefCell<Option<RectF>>>>,
    /// Whether a pointer gesture owns the bar: a tab press/drag or a lifted
    /// pane. The strip is then offered every pointer event first, so the
    /// controls beside it cannot swallow the release of a gesture that
    /// travelled over them.
    gesture: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopbarRow {
    fn new(
        leading: Vec<Box<dyn Element>>,
        strip: Box<dyn Element>,
        trailing: Vec<Box<dyn Element>>,
        right: Vec<Box<dyn Element>>,
    ) -> Self {
        Self {
            leading,
            strip,
            trailing,
            right,
            left_spacing: 0.0,
            group_spacing: 0.0,
            bar_bounds: None,
            gesture: false,
            size: None,
            origin: None,
        }
    }

    fn with_bar_bounds(mut self, bounds: Rc<RefCell<Option<RectF>>>) -> Self {
        self.bar_bounds = Some(bounds);
        self
    }

    fn with_gesture(mut self, gesture: bool) -> Self {
        self.gesture = gesture;
        self
    }

    fn with_left_spacing(mut self, spacing: f32) -> Self {
        self.left_spacing = spacing;
        self
    }

    fn with_group_spacing(mut self, spacing: f32) -> Self {
        self.group_spacing = spacing;
        self
    }

    fn width_of(child: &dyn Element) -> f32 {
        child.size().unwrap_or(Vector2F::zero()).x
    }

    /// Where a child sits vertically: centered on the bar, as the flex row the
    /// tab strip used to sit in centered it.
    fn center_y(child: &dyn Element, height: f32) -> f32 {
        (height - child.size().unwrap_or(Vector2F::zero()).y).max(0.0) / 2.0
    }

    /// The widths of the row's fixed controls: everything but the strip, plus
    /// the gaps between them at their narrowest — `left_spacing` inside each
    /// group, and `group_spacing` between the right group's neighbours, whose
    /// last control sits on the row's own right edge.
    fn fixed_width(&self) -> f32 {
        let sum = |children: &Vec<Box<dyn Element>>| -> f32 {
            children
                .iter()
                .map(|child| Self::width_of(child.as_ref()))
                .sum()
        };
        let left_gaps = self.left_spacing * (self.leading.len() + self.trailing.len()) as f32;
        let group_gaps = self.group_spacing * self.right.len().saturating_sub(1) as f32;
        sum(&self.leading) + sum(&self.trailing) + sum(&self.right) + left_gaps + group_gaps
    }
}

impl Element for TopbarRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let room = constraint.max.x;
        // The controls take their own sizes, exactly as they did inside the flex
        // row this replaces — none of them reads its constraint.
        let inner = SizeConstraint::loose(vec2f(f32::INFINITY, f32::INFINITY));
        let mut cross_max: f32 = 0.0;
        for child in &mut self.leading {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        for child in &mut self.trailing {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        for child in &mut self.right {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        // The strip gets what the controls leave, and that room is what sizes
        // its tabs: an unbounded row leaves it to the strip's own cap.
        let strip_room = if room.is_finite() {
            (room - self.fixed_width()).max(0.0)
        } else {
            room
        };
        let strip = self.strip.layout(
            SizeConstraint::loose(vec2f(strip_room, constraint.max.y)),
            ctx,
            app,
        );
        cross_max = cross_max.max(strip.y);

        let size = vec2f(
            if room.is_finite() {
                room
            } else {
                self.fixed_width() + strip.x
            },
            cross_max,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let height = self.size.unwrap_or(Vector2F::zero()).y;
        let width = self.size.unwrap_or(Vector2F::zero()).x;
        if let Some(bounds) = self.bar_bounds.as_ref() {
            *bounds.borrow_mut() = Some(rectf(origin.x, origin.y, width, height));
        }

        // The left group: the controls, then the strip a `left_spacing` past
        // the last of them.
        let mut x = 0.0;
        for child in self.leading.iter_mut() {
            let y = Self::center_y(child.as_ref(), height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += Self::width_of(child.as_ref()) + self.left_spacing;
        }
        let y = Self::center_y(self.strip.as_ref(), height);
        self.strip.paint(origin + vec2f(x, y), ctx, app);
        x += Self::width_of(self.strip.as_ref());
        x += self.left_spacing;
        let trailing = self.trailing.len();
        for (index, child) in self.trailing.iter_mut().enumerate() {
            let y = Self::center_y(child.as_ref(), height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += Self::width_of(child.as_ref());
            if index + 1 < trailing {
                x += self.left_spacing;
            }
        }

        // The cue, the inbox and Settings are one compact run pinned to the
        // row's right edge: `group_spacing` between neighbours, and the last
        // control flush with that edge — which the bar's own trailing padding
        // ([`TOPBAR_RIGHT_PADDING`]) sets off the window's edge.
        let right_widths: Vec<f32> = self
            .right
            .iter()
            .map(|child| Self::width_of(child.as_ref()))
            .collect();
        let right_total: f32 = right_widths.iter().sum();
        let group_width =
            right_total + self.group_spacing * self.right.len().saturating_sub(1) as f32;
        let mut x = (width - group_width).max(x);
        for (child, child_width) in self.right.iter_mut().zip(right_widths) {
            let y = Self::center_y(child.as_ref(), height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += child_width + self.group_spacing;
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
        // A gesture in flight belongs to the strip, wherever the pointer has
        // travelled: the release must end the drag, not click a control the
        // pointer happens to be over. Anything the strip does not take still
        // falls through to the controls, so the bar stays usable.
        let pointer = matches!(
            event,
            DispatchedEvent::MouseDown { .. }
                | DispatchedEvent::MouseMove { .. }
                | DispatchedEvent::MouseUp { .. }
        );
        if self.gesture && pointer {
            if self.strip.dispatch_event(event, ctx, app) {
                return true;
            }
            for child in &mut self.leading {
                if child.dispatch_event(event, ctx, app) {
                    return true;
                }
            }
            for child in &mut self.trailing {
                if child.dispatch_event(event, ctx, app) {
                    return true;
                }
            }
            return self
                .right
                .iter_mut()
                .any(|child| child.dispatch_event(event, ctx, app));
        }
        for child in &mut self.leading {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        if self.strip.dispatch_event(event, ctx, app) {
            return true;
        }
        for child in &mut self.trailing {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        self.right
            .iter_mut()
            .any(|child| child.dispatch_event(event, ctx, app))
    }
}

/// The workspace row: one Chrome-style tab per space, in order, floating inside
/// the strip's `Bg`. The selected tab's fill continues into the content below
/// and, while renaming, swaps for the inline name field (behind which its fill
/// still paints).
///
/// The frame pins every tab to [`TOPBAR_HEIGHT`] — the bar's height — so a
/// label centred on it reads exactly as Chrome's tab title does on its strip;
/// each tab paints its own rounded fill inside the slot it is given (see
/// [`WorkspaceChip`]).
fn build_workspace_strip(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    bar_bounds: Rc<RefCell<Option<RectF>>>,
) -> StripFrame {
    // One slot per tab, filled with the rectangle that tab is drawn at while it
    // paints: the drop an insertion index resolves against (see
    // [`SpaceDropStrip`]) is read off exactly what the user can see.
    let tab_rects: Rc<RefCell<Vec<Option<RectF>>>> =
        Rc::new(RefCell::new(vec![None; state.spaces.len()]));
    let mut children = Vec::with_capacity(state.spaces.len());
    for (index, space) in state.spaces.iter().enumerate() {
        let active = index == state.active_space;
        let tab: Box<dyn Element> = if active && state.space_rename_editing {
            build_rename_tab(app, state, actions)
        } else {
            build_workspace_chip(
                app,
                actions,
                index,
                &space.name,
                active,
                space.color,
                state.command_held,
                label_clip(space, state, index),
            )
        };
        children.push(FrameChild {
            element: Box::new(TabRect::new(tab, index, Rc::clone(&tab_rects))),
            selected: active,
        });
    }
    // The drop target wraps the workspace frame alone, never the files' tabs:
    // the insertion index a lifted pane resolves is an index among the workspace
    // tabs and nothing else.
    let frame = TabFrame::new(children).finish();
    StripFrame::new(
        Box::new(SpaceDropStrip::new(
            frame,
            state.pane_drag.clone(),
            tab_rects,
            SpacePress {
                press: state.space_press,
                pressed_at: state.space_press_at,
                drag: state.space_drag,
            },
            state.pane_bounds.clone(),
            bar_bounds,
            actions.on_pane_lift_move.clone(),
            actions.on_pane_drop.clone(),
            actions.on_space_reorder.clone(),
            actions.on_space_release.clone(),
        )),
        state.spaces.len(),
    )
}

/// Which end of the tab at `index` an over-long name loses, warp-new's
/// `should_clip_text_start` (`app/src/tab.rs:1363`): the start for a label the
/// tab derives from where it runs — its focused pane's directory or file, where
/// the last component is the part worth reading — and the end for a name the
/// user typed and for an agent conversation's subject, where the words that tell
/// two tabs apart sit at the front.
fn label_clip(space: &Space, state: &UiSnapshot, index: usize) -> LabelClip {
    if space.named {
        return LabelClip::End;
    }
    // The focused leaf of the active tab, and the first leaf of a tab that is
    // not on screen — the pane a tab's label follows (see
    // `UiState::space_label`).
    let focused = if index == state.active_space && space.root.contains_leaf(state.active_pane_id) {
        state.active_pane_id
    } else {
        space.root.first_leaf_id()
    };
    match space.leaf_kind(focused) {
        Some(PaneKind::Chat) | Some(PaneKind::Worker) => LabelClip::End,
        _ => LabelClip::Start,
    }
}

/// One clickable workspace tab: the name plus an "x" that closes that
/// workspace. A single click selects the space; a second click within the
/// double-click window starts the inline rename — counted on the press, which
/// is what [`WorkspaceChip`] reports here. A right click opens the tab's menu
/// at the pointer.
///
/// While the chord modifier is held (`command_held`), the tab's trailing slot
/// carries the shortcut that selects it — `⌘1` on macOS, `Ctrl+1` elsewhere —
/// in place of the "x".
fn build_workspace_chip(
    app: &AppContext,
    actions: &UiActions,
    index: usize,
    name: &str,
    active: bool,
    color: Option<TabColor>,
    command_held: bool,
    clip: LabelClip,
) -> Box<dyn Element> {
    // The active tab reads in the theme's text colour and, as in warp-new, at
    // the medium weight its title carries; the others are muted and regular.
    let (label_color, label_weight) = if active {
        (ColorToken::Text, FontWeight::Medium)
    } else {
        (ColorToken::Muted, FontWeight::Regular)
    };
    let label = TabLabel::new(name, label_color, label_weight, clip).finish();
    // The hint reads as secondary text of the tab it sits in: the active tab's
    // own colour, the muted one everywhere else — the same split its name has.
    let hint = command_held.then(|| {
        Text::new(tab_hint(index + 1))
            .with_theme_color(label_color, app)
            .with_font_size(TAB_HINT_FONT_SIZE)
            .with_max_lines(1)
            .finish()
    });
    let on_click = actions.on_workspace_click.clone();
    let on_close = actions.on_close_space.clone();
    let on_press = actions.on_space_press.clone();
    let on_right_click = actions.on_space_menu.clone();
    Box::new(WorkspaceChip::new(WorkspaceChipParts {
        child: label,
        index,
        active,
        color,
        on_click: Rc::new(RefCell::new(move |i: usize| (on_click.borrow_mut())(i))),
        on_press: Rc::new(RefCell::new(move |i: usize, at: Vector2F| {
            (on_press.borrow_mut())(i, at)
        })),
        on_right_click: Rc::new(RefCell::new(move |i: usize, at: Vector2F| {
            (on_right_click.borrow_mut())(i, at)
        })),
        close: TopbarButton::new(
            Icon::new("x")
                .with_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(16.0)
        // A circle on the 16 pt box: Chrome's close control paints a circular
        // ink-drop hover highlight (tab_close_button.cc:38-70).
        .with_corner_radius(8.0)
        .with_on_click(move || (on_close.borrow_mut())(index))
        .finish(),
        hint,
    }))
}

/// The inline rename field, centred in the active tab's slot both ways — as
/// warp-new centres it (`Align::new(TextInput…)`, `app/src/tab.rs:1118`) and in
/// the tab's full height, so the strip keeps `TOPBAR_HEIGHT` while the name is
/// being edited.
fn build_rename_tab(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    // The column stretches to the tab's slot so the row below has the slot's
    // width to centre the fixed-width field in; the column alone would shrink
    // to the field and sit on the tab's left edge.
    let field = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(build_rename_field(app, state, actions))
                .finish(),
        )
        .finish();
    // The field replaces the tab's label and close control, but not the tab's
    // fill: the selected tab's surface and feet still paint behind it, so the
    // strip does not jump while a name is edited.
    let color = state
        .spaces
        .get(state.active_space)
        .and_then(|space| space.color);
    Stack::new()
        .with_children(vec![Box::new(TabBackdrop::new(color)), field])
        .finish()
}

/// The inline rename field shown in place of the active workspace tab.
fn build_rename_field(
    _app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let on_change = actions.on_space_rename_change.clone();
    let on_focus = actions.on_space_rename_focus.clone();
    let on_submit = actions.on_space_rename_commit.clone();
    let input = TextInput::new()
        .with_value(state.space_rename_draft.clone())
        .with_placeholder("Workspace name")
        .with_focused(state.space_rename_focused)
        .with_on_change(move |v| (on_change.borrow_mut())(v))
        .with_on_focus_change(move |f: bool| (on_focus.borrow_mut())(f))
        .with_on_submit(move || (on_submit.borrow_mut())())
        .finish();
    ConstrainedBox::new(input)
        .with_width(160.0)
        .with_height(22.0)
        .finish()
}

/// The selected tab's fill behind the inline rename field: the same body and
/// feet a [`WorkspaceChip`] paints when it is the active tab, so the tab still
/// reads as merged into the content while its name is edited. Paints only; it
/// takes no event.
struct TabBackdrop {
    color: Option<TabColor>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabBackdrop {
    fn new(color: Option<TabColor>) -> Self {
        Self {
            color,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabBackdrop {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(constraint.max.x.max(0.0), WorkspaceChip::height());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let base = app.theme.color(ColorToken::Surface);
        let fill = match self.color {
            Some(color) => color.tint_over(base, TabTint::Active),
            None => base,
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            paint_tab_surface(
                renderer,
                rectf(origin.x, origin.y, size.x, size.y),
                true,
                fill,
                app.theme.color(ColorToken::Bg),
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
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

/// A Chrome-style workspace tab: a rounded floating surface inside the strip,
/// and — when it is the selected tab — a fill that continues into the content
/// below (see [`paint_tab_surface`]). The strip draws no rules between tabs, so
/// the tab itself never strokes a border and nothing doubles a boundary.
///
/// A tab's state decides its fill exactly as Chrome's does: the selected tab is
/// filled with the pane surface, the pointer's tab with the hover fill, a press
/// with the selected fill, and a floating tab at rest paints nothing at all (so
/// the bar's own `Bg` shows through). A tab carrying a colour tints the fill its
/// state gives it — 60 % while it is the active tab, 40 % while the pointer is
/// over it, 20 % otherwise (see [`TabColor::tint_over`]).
///
/// The trailing slot holds one control at a time: the "x", which the selected
/// tab always shows and a floating tab shows while hovered, or — while the
/// chord modifier is held — the tab's own shortcut hint. The hint takes the
/// "x"'s place rather than sitting beside it, so the slot is reserved once, the
/// name keeps truncating into what is left, and a tab never carries two
/// controls at the same point.
pub(super) struct WorkspaceChip {
    child: Box<dyn Element>,
    close: Box<dyn Element>,
    /// The shortcut the tab advertises while the chord modifier is held; `None`
    /// on every other frame, when the tab draws its "x" as it always has.
    hint: Option<Box<dyn Element>>,
    index: usize,
    active: bool,
    color: Option<TabColor>,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    /// The press, reported before the click so the multi-click is counted from
    /// it (see [`UiState::space_press_run`](crate::state::SpacePressRun)), with
    /// the point it landed on so a drag can measure how far it has travelled.
    on_press: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    /// A right click on the tab, with the point the pointer was at: the tab's
    /// menu hangs from the pointer itself.
    on_right_click: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    state: goble_ui::elements::interactive::InteractiveState,
    /// The x the name is painted at inside the tab: the left inset while it
    /// fills the room it was given, and a share further in while it is short.
    child_x: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

pub(super) struct WorkspaceChipParts {
    pub child: Box<dyn Element>,
    pub index: usize,
    pub active: bool,
    pub color: Option<TabColor>,
    pub on_click: Rc<RefCell<dyn FnMut(usize)>>,
    pub on_press: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    pub on_right_click: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    pub close: Box<dyn Element>,
    /// The shortcut, drawn in the close control's slot while the chord modifier
    /// is held; `None` draws the tab with its "x" alone.
    pub hint: Option<Box<dyn Element>>,
}

impl WorkspaceChip {
    pub(super) fn new(parts: WorkspaceChipParts) -> Self {
        let WorkspaceChipParts {
            child,
            index,
            active,
            color,
            on_click,
            on_press,
            on_right_click,
            close,
            hint,
        } = parts;
        Self {
            child,
            close,
            hint,
            index,
            active,
            color,
            on_click,
            on_press,
            on_right_click,
            state: Default::default(),
            child_x: Self::h_pad(),
            size: None,
            origin: None,
        }
    }

    /// The tab's element is as tall as the whole bar block — the bar row plus its
    /// band — so its label centres on the block the window shows, exactly as
    /// Chrome's title centres on its strip, and the pill floats with an equal gap
    /// above and below it; the visible fill is inset inside that height (see
    /// [`tab_content_rect`]).
    fn height() -> f32 {
        bar_height()
    }

    fn h_pad() -> f32 {
        7.0
    }

    /// Gap between the workspace name and its close "x".
    fn close_gap() -> f32 {
        4.0
    }

    /// What of a tab's slot is left for the name once the pill's edge inset,
    /// the padding and the trailing slot are out of it — the slot the close "x"
    /// or, while the chord modifier is held, the shortcut hint is drawn in.
    /// Both kinds of tab measure their name against this, so a long name never
    /// runs under the control, and the name stays inside the floating pill
    /// rather than spilling into the gap beside it.
    fn inner(constraint: SizeConstraint, trailing_width: f32) -> SizeConstraint {
        let room = (constraint.max.x
            - TAB_EDGE_INSET * 2.0
            - Self::h_pad() * 2.0
            - Self::close_gap()
            - trailing_width)
            .max(0.0);
        SizeConstraint::new(Vector2F::zero(), vec2f(room, Self::height()))
    }

    /// The tab's own width: the slot the strip gave it. warp-new's tab is a
    /// flexible child of the tab-bar row that fills the share it is handed —
    /// `Shrinkable::new(1.0, ConstrainedBox::new(tab).with_max_width(200.))`
    /// (`app/src/tab.rs:1841`, `:1873`) — so a tab with a short name is as wide
    /// as one with a long name, and never wider than [`MAX_TAB_WIDTH`]. A tab
    /// laid out with no bound at all (a bare chip outside the strip) keeps what
    /// its name and its trailing control need.
    fn width(intrinsic: f32, constraint: SizeConstraint) -> f32 {
        if constraint.max.x.is_finite() {
            constraint.max.x.min(MAX_TAB_WIDTH)
        } else {
            intrinsic.min(MAX_TAB_WIDTH)
        }
    }

    /// The trailing control's origin inside a tab of `size`: pinned to the right
    /// edge of the tab's floating fill (so the control keeps `h_pad` from the
    /// pill, not from the slot), and a tab the strip narrowed still draws — and
    /// hits — the control inside its own bounds. The close "x" and the shortcut
    /// hint share the slot, so they share this origin.
    fn trailing_origin(origin: Vector2F, size: Vector2F, trailing_size: Vector2F) -> Vector2F {
        let left = (size.x - TAB_EDGE_INSET - Self::h_pad() - trailing_size.x)
            .max(TAB_EDGE_INSET + Self::h_pad());
        vec2f(
            origin.x + left,
            origin.y + (size.y - trailing_size.y).max(0.0) / 2.0,
        )
    }

    /// Whether this tab draws its close "x" on a frame: the selected tab always
    /// (Chrome shows the active tab's close), a floating tab while the pointer
    /// is over it; either only while the contents are wide enough for it and
    /// while the shortcut hint does not hold the slot.
    fn close_visible(&self, hovered: bool, content_width: f32) -> bool {
        self.hint.is_none() && (self.active || hovered) && content_width >= MIN_CONTENTS_FOR_CLOSE
    }

    /// The fill a tab of this state is drawn with: the theme's own surface for
    /// that state, carrying the tab's colour at the state's share of it.
    fn tinted(&self, app: &AppContext, base: ColorToken, tint: TabTint) -> goble_ui::ColorU {
        let base = app.theme.color(base);
        match self.color {
            Some(color) => color.tint_over(base, tint),
            None => base,
        }
    }
}

impl Element for WorkspaceChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The trailing slot — the close "x", or the shortcut hint while the
        // chord modifier is held — is reserved in the tab whether it is drawn
        // or not, and it is reserved *before* the name is measured: the name
        // gets what is left of the tab, so it never runs under the control. One
        // of the two is drawn, but the "x" is measured on every frame too: the
        // name is centred in the room it leaves, which is the same whether the
        // "x" or the hint holds the slot.
        let hint_size = self.hint.as_mut().map(|hint| {
            hint.layout(
                SizeConstraint::loose(vec2f(constraint.max.x, Self::height())),
                ctx,
                app,
            )
        });
        let close_size = self.close.layout(
            SizeConstraint::loose(vec2f(Self::height(), Self::height())),
            ctx,
            app,
        );
        let trailing_size = hint_size.unwrap_or(close_size);
        let child_size = self
            .child
            .layout(Self::inner(constraint, trailing_size.x), ctx, app);
        let intrinsic = child_size.x
            + Self::close_gap()
            + trailing_size.x
            + Self::h_pad() * 2.0
            + TAB_EDGE_INSET * 2.0;
        let size = vec2f(Self::width(intrinsic, constraint), Self::height());
        // Centre the name in the room the close slot leaves of the tab's own
        // width, inside the floating pill — the way warp-new centres its tab
        // titles (`app/src/tab.rs:1440-1457`, `MainAxisAlignment::Center`) —
        // and keep the left inset when the name fills that room, so a cut run
        // still starts there.
        let room = (size.x
            - TAB_EDGE_INSET * 2.0
            - Self::h_pad() * 2.0
            - Self::close_gap()
            - close_size.x)
            .max(0.0);
        let centred = (room - child_size.x).max(0.0) / 2.0;
        // The name was cut to the room the drawn trailing control leaves; it
        // never reaches back under a hint that is wider than the "x".
        let fit_room = Self::inner(constraint, trailing_size.x).max.x;
        self.child_x =
            TAB_EDGE_INSET + Self::h_pad() + centred.min((fit_room - child_size.x).max(0.0));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let slot = rectf(origin.x, origin.y, size.x, size.y);
        // Hover is decided over the whole slot, as Chrome hit-tests a tab's
        // bounds: the 3 pt side gaps belong to the tab's slot, not to its
        // neighbour.
        let hovered = ctx.hovered(slot);
        let content_width = (size.x - TAB_EDGE_INSET * 2.0).max(0.0);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // The state table — rest, hover, press, selected — maps onto the
            // three tones of the strip: the selected tab is the content's own
            // surface, the hovered one a step off the bar's `Bg`, a press the
            // selected tone. A floating tab at rest paints nothing (a coloured
            // one keeps its tint), and a selected tab ignores hover, as the
            // active tab does in Chrome.
            let fill = if self.active {
                Some(self.tinted(app, ColorToken::Surface, TabTint::Active))
            } else if hovered {
                let base = if app.pointer_down.get() {
                    ColorToken::Selected
                } else {
                    ColorToken::Hover
                };
                Some(self.tinted(app, base, TabTint::Hovered))
            } else if self.color.is_some() {
                Some(self.tinted(app, ColorToken::Bg, TabTint::Resting))
            } else {
                None
            };
            if let Some(fill) = fill {
                paint_tab_surface(
                    renderer,
                    slot,
                    self.active,
                    fill,
                    app.theme.color(ColorToken::Bg),
                );
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        self.child.paint(
            vec2f(
                origin.x + self.child_x,
                origin.y + (size.y - child_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        // The trailing slot draws the shortcut hint while the chord modifier is
        // held, and the close "x" otherwise: on the selected tab always (Chrome
        // keeps the active tab's close visible), on a floating tab while the
        // pointer is over it and its contents are wide enough. The slot stays
        // reserved in `layout`, so the strip's tabs keep their width (and their
        // drop targets) instead of shifting under the pointer.
        if let Some(hint) = self.hint.as_mut() {
            let hint_size = hint.size().unwrap_or(Vector2F::zero());
            hint.paint(Self::trailing_origin(origin, size, hint_size), ctx, app);
        } else if self.close_visible(hovered, content_width) {
            let close_size = self.close.size().unwrap_or(Vector2F::zero());
            self.close
                .paint(Self::trailing_origin(origin, size, close_size), ctx, app);
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
        // The close "x" gets the event first: it only consumes clicks inside its
        // own bounds, so clicking the name still selects the workspace. It is
        // only on screen on the selected tab and on a hovered floating tab with
        // room for it (see `paint`), so it is offered the pointer only there —
        // an invisible control has no hit target. While the shortcut hint holds
        // the trailing slot the "x" is not drawn at all, so it gets nothing.
        let content_width = self
            .size
            .map(|size| (size.x - TAB_EDGE_INSET * 2.0).max(0.0))
            .unwrap_or(0.0);
        let over_tab = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => self
                .bounds()
                .map(|bounds| contains(bounds, *position))
                .unwrap_or(false),
            _ => false,
        };
        if self.hint.is_none()
            && over_tab
            && content_width >= MIN_CONTENTS_FOR_CLOSE
            && self.close.dispatch_event(event, ctx, app)
        {
            return true;
        }
        // A right press is the tab's menu, hung from the point it landed on
        // rather than from the tab's own corner (warp-new's
        // `TabContextMenuAnchor::Pointer`). Nothing else sees the press: it
        // opens the menu, it is not a click on the tab.
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == BUTTON_SECONDARY && over_tab {
                let cb = Rc::clone(&self.on_right_click);
                let index = self.index;
                let at = *position;
                (cb.borrow_mut())(index, at);
                return true;
            }
        }
        // A primary press is what the tab's multi-click is counted from, so it
        // is reported before the release that follows it reads the count. The
        // point is the drag's origin: the strip only reorders once the pointer
        // has travelled away from it.
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == BUTTON_PRIMARY && over_tab {
                let cb = Rc::clone(&self.on_press);
                let index = self.index;
                (cb.borrow_mut())(index, *position);
            }
        }
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let cb = self.on_click.clone();
        let index = self.index;
        let mut on_click = move || (cb.borrow_mut())(index);
        goble_ui::elements::interactive::handle_mouse_event(
            &mut self.state,
            event,
            bounds,
            ctx,
            &mut on_click,
        )
    }
}

/// The "+ ▾" control: "+" opens a new workspace in the active environment, and
/// "▾" opens the environment menu (a new terminal, a new agent tab, a cloud
/// agent, the environments, plus "New environment…").
fn build_add_workspace_control(
    app: &AppContext,
    state: &UiSnapshot,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_add_space = actions.on_add_space.clone();
    let plus = TopbarButton::new(
        Icon::new("plus")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(24.0)
    .with_on_click(move || (on_add_space.borrow_mut())())
    .finish();

    let trigger = TopbarButton::new(
        Icon::new("chevron-down")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(20.0)
    .finish();

    // The menu opens with the kinds of space the workspace can start — a plain
    // PTY, an agent pane holding a fresh conversation, and the cloud agent
    // surface that runs on a worker — and then the environments themselves:
    // choosing one opens a new space that runs on it (the check mark still
    // marks the active environment), and the trailing entry adds a brand-new
    // one.
    let mut items = vec![
        PopupMenuItem::new("New terminal").with_icon("terminal"),
        PopupMenuItem::new("New Agent").with_icon("agent"),
        PopupMenuItem::new("Cloud agent").with_icon("cloud"),
    ];
    // `(medium id, label)` per environment row, in item order.
    let mut envs: Vec<(String, String)> = Vec::new();
    for m in &media.mediums {
        let mut item = PopupMenuItem::new(m.label.clone()).with_icon(medium_icon(&m.id));
        if m.id == media.selected_medium {
            item = item.selected();
        }
        items.push(item);
        envs.push((m.id.clone(), m.label.clone()));
    }
    // The trailing "New environment…" entry; its index is
    // `ENV_MENU_LEADING_ROWS + envs.len()`.
    items.push(PopupMenuItem::new("New environment…").with_icon("plus"));

    // The tray warp-new shows beside the row under the pointer: the
    // environment's name and its "Make default", which makes that environment
    // the one new spaces start in — "+"'s pick — without opening anything.
    // Choosing the row itself opens a space on it instead.
    let tray_envs = envs.clone();
    let tray_default = media.selected_medium.clone();
    let on_select_medium = media_actions.on_select_medium.clone();
    let tray_open = state.env_selector_open.clone();

    let on_add_space = actions.on_add_space.clone();
    let on_add_agent = actions.on_add_agent_space.clone();
    let on_add_space_with_medium = actions.on_add_space_with_medium.clone();
    let on_new_cloud_agent = actions.on_new_cloud_agent.clone();
    let on_open_add_medium = actions.on_open_add_medium.clone();
    let menu = PopupMenu::new(trigger, items)
        .with_open(state.env_selector_open.clone())
        .with_hover_index(state.env_selector_hover.clone())
        .with_hover_tray(ENV_TRAY_WIDTH, move |index, app| {
            // The leading rows start a space (a terminal, an agent pane, a
            // cloud agent) and the trailing row adds an environment: only the
            // rows in between are environments, and only they carry a tray.
            let (id, label) = tray_envs
                .get(index.checked_sub(ENV_MENU_LEADING_ROWS)?)?
                .clone();
            let is_default = id == tray_default;
            let select = on_select_medium.clone();
            let open = tray_open.clone();
            Some(build_environment_tray(app, &label, is_default, move || {
                (select.borrow_mut())(id.clone());
                // Setting the default closes the menu, as it does in warp-new.
                *open.borrow_mut() = false;
            }))
        })
        .with_on_select(move |index| {
            if index == 0 {
                (on_add_space.borrow_mut())();
            } else if index == 1 {
                // A fresh conversation: an agent pane in a space of its own,
                // opened in the same environment "+" would use.
                (on_add_agent.borrow_mut())();
            } else if index == 2 {
                // The cloud agent's own surface: a fresh conversation whose
                // pane starts as a viewer, because it runs on a worker.
                (on_new_cloud_agent.borrow_mut())();
            } else if index <= envs.len() + ENV_MENU_LEADING_ROWS - 1 {
                // An environment row opens a space running on that environment
                // (the tab lands at the end of the strip, as "+" does) instead
                // of only switching which environment is active. Remote opens
                // the cloud agent's own surface: its conversations run on a
                // worker, so the tab starts as a viewer rather than a shell.
                let (id, _) = &envs[index - ENV_MENU_LEADING_ROWS];
                if id == "remote" {
                    (on_new_cloud_agent.borrow_mut())();
                } else {
                    (on_add_space_with_medium.borrow_mut())(id.clone());
                }
            } else {
                (on_open_add_medium.borrow_mut())();
            }
        })
        .finish();

    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.25)
        .with_child(plus)
        .with_child(menu)
        .finish()
}

/// The tray shown beside the environment row under the pointer: the
/// environment's name and a "Make default" button, disabled — with the reason,
/// as a tooltip — while it already is the environment new spaces start in.
fn build_environment_tray(
    app: &AppContext,
    label: &str,
    is_default: bool,
    on_make_default: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let title = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(
            Text::new(label.to_string())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();
    let button = Button::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(
                Text::new("Make default")
                    .with_theme_color(
                        if is_default {
                            ColorToken::Muted
                        } else {
                            ColorToken::Text
                        },
                        app,
                    )
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish(),
    )
    .with_corner_radius(4.0)
    .with_disabled(is_default)
    .with_on_click(on_make_default)
    .finish();
    let button: Box<dyn Element> = if is_default {
        Tooltip::new(button, "Already the default").finish()
    } else {
        button
    };

    Container::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm * 0.5)
            .with_child(title)
            .with_child(button)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(app.theme.color(ColorToken::Border).into())
    .with_corner_radius(6.0)
    .finish()
}

/// The tab index a pane released at `x` lands before: how many tab centers the
/// pointer is past, so `0` is left of every tab and `tabs.len()` right of them
/// all. This is the same count the tab reorder resolves a drag with, and it is
/// monotonic in `x`, so the marker never oscillates between two tabs around a
/// midpoint.
pub(super) fn insertion_index(tabs: &[RectF], x: f32) -> usize {
    tabs.iter().filter(|tab| x > tab.center().x).count()
}

/// Where insertion point `index` is marked: on the left edge of the tab it
/// precedes, and on the right edge of the last tab when the pane lands at the
/// end of the strip.
fn insertion_x(tabs: &[RectF], index: usize) -> f32 {
    match tabs.get(index) {
        Some(tab) => tab.min_x(),
        None => tabs.last().map(|tab| tab.max_x()).unwrap_or(0.0),
    }
}

/// How far the pointer must travel from a press on a tab before that press
/// reads as a drag. A click that wobbles a pixel still selects the tab.
const TAB_DRAG_THRESHOLD: f32 = 4.0;

/// The index the tab at `from` belongs at for a pointer at `x`: how many of the
/// *other* tabs' centers lie left of the pointer.
///
/// Counting the other tabs rather than picking the nearest center is what makes
/// a drag monotonic in `x`: the tab moves toward the pointer and does not swap
/// back and forth around a midpoint. The count is also the index the tab takes
/// once it is moved, because the dragged tab is left out of the count.
pub(super) fn reorder_target_index(tabs: &[RectF], from: usize, x: f32) -> usize {
    tabs.iter()
        .enumerate()
        .filter(|(index, tab)| *index != from && x > tab.center().x)
        .count()
}

/// Records the rectangle a tab was drawn at into the strip's registry while it
/// paints. Every tab of the strip is wrapped in one — the chip and the inline
/// rename field alike — so a drop's insertion index always names a position
/// among all the tabs, never one that is missing from the list.
struct TabRect {
    inner: Box<dyn Element>,
    index: usize,
    rects: Rc<RefCell<Vec<Option<RectF>>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabRect {
    fn new(inner: Box<dyn Element>, index: usize, rects: Rc<RefCell<Vec<Option<RectF>>>>) -> Self {
        Self {
            inner,
            index,
            rects,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabRect {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
        if let Some(size) = self.size {
            if let Some(slot) = self.rects.borrow_mut().get_mut(self.index) {
                *slot = Some(rectf(origin.x, origin.y, size.x, size.y));
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
        self.inner.dispatch_event(event, ctx, app)
    }
}

/// The workspace tab strip as a drop target and a reorder handle.
///
/// A pane lifted out of its grid is dropped between these tabs and becomes a
/// tab of its own there; a workspace tab pressed and dragged is reordered along
/// the strip. Both gestures are resolved here and performed by app state — the
/// pane move is `UiState::release_pane_drag`, the reorder `UiState::reorder_space`
/// — and the ghost card is the bar's, which is what lets the rules be tested
/// without a pointer. It sees pointer events before the shell body (the bar is
/// layered above it), so while a gesture is in flight the strip holds the
/// pointer; while nothing is in flight it forwards to its tabs, whose own hit
/// test is what makes a tab click win a press that lands on a tab.
///
/// The tab rectangles it resolves against are the ones the tabs were drawn at
/// (see [`TabRect`]): the strip is rebuilt every frame, and a reorder must name
/// the place the user can see, not a slot the rebuild has moved.
type DragMoveHandler = Rc<RefCell<dyn FnMut(Vector2F, Option<usize>, Option<PaneDropTarget>)>>;
type DropHandler = Rc<RefCell<dyn FnMut(Option<usize>, Option<PaneDropTarget>)>>;

/// The workspace tab press, as app state holds it. A press becomes a drag only
/// once the pointer has travelled [`TAB_DRAG_THRESHOLD`] away from it, so a
/// click still selects and a double click still opens the rename; `drag` is the
/// tab's index after the last reorder, which is what makes the drag follow the
/// tab across the per-frame rebuild.
#[derive(Clone, Copy)]
struct SpacePress {
    press: Option<usize>,
    pressed_at: Option<Vector2F>,
    drag: Option<usize>,
}

struct SpaceDropStrip {
    inner: Box<dyn Element>,
    /// The lifted pane, from app state; `None` when nothing is in flight.
    drag: Option<PaneDrag>,
    tab_rects: Rc<RefCell<Vec<Option<RectF>>>>,
    space: SpacePress,
    /// The bounds the pane area's leaves were drawn at (see
    /// [`UiSnapshot::pane_bounds`]): what the in-area drop target resolves
    /// against, since a window is rebuilt every frame and the strip has no
    /// other way to know where the panes are.
    pane_bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
    /// The whole bar's rectangle, as the row painted it. The bar is the drop
    /// zone of a lifted pane, so a release beyond the drawn tabs still names a
    /// tab slot instead of cancelling.
    bar_bounds: Rc<RefCell<Option<RectF>>>,
    on_drag_move: DragMoveHandler,
    on_drop: DropHandler,
    on_space_reorder: Rc<RefCell<dyn FnMut(usize, usize)>>,
    on_space_release: Rc<RefCell<dyn FnMut()>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SpaceDropStrip {
    fn new(
        inner: Box<dyn Element>,
        drag: Option<PaneDrag>,
        tab_rects: Rc<RefCell<Vec<Option<RectF>>>>,
        space: SpacePress,
        pane_bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
        bar_bounds: Rc<RefCell<Option<RectF>>>,
        on_drag_move: DragMoveHandler,
        on_drop: DropHandler,
        on_space_reorder: Rc<RefCell<dyn FnMut(usize, usize)>>,
        on_space_release: Rc<RefCell<dyn FnMut()>>,
    ) -> Self {
        Self {
            inner,
            drag,
            tab_rects,
            space,
            pane_bounds,
            bar_bounds,
            on_drag_move,
            on_drop,
            on_space_reorder,
            on_space_release,
            size: None,
            origin: None,
        }
    }

    /// The strip's bounds in window coordinates, known once it has been painted
    /// (which is also when the tabs' rectangles are written).
    fn bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }

    /// The tab rectangles as this frame's paint left them, in tab order.
    fn drawn_tabs(&self) -> Vec<RectF> {
        self.tab_rects.borrow().iter().flatten().copied().collect()
    }

    /// The rectangle a lifted pane's release resolves against: the whole bar
    /// as the row painted it, so the empty room beside the tabs counts too.
    /// Before the bar has painted once, the strip's own room is the zone.
    fn drop_zone(&self) -> Option<RectF> {
        let bar = *self.bar_bounds.borrow();
        bar.or_else(|| self.bounds())
    }

    /// The insertion index a release at `position` names, or `None` when the
    /// pointer is not over the bar: a release there cancels the drag.
    fn drop_index_at(&self, position: Vector2F) -> Option<usize> {
        let bounds = self.drop_zone()?;
        if !contains(bounds, position) {
            return None;
        }
        Some(insertion_index(&self.drawn_tabs(), position.x))
    }

    /// The pane the lifted pane would land beside for a pointer at `position`,
    /// or `None` while it is over no other pane. Over the bar itself there is
    /// no in-area target: a release there gives the pane a tab of its own.
    fn drop_target_at(&self, position: Vector2F) -> Option<PaneDropTarget> {
        let drag = self.drag.as_ref()?;
        if self
            .drop_zone()
            .map(|bounds| contains(bounds, position))
            .unwrap_or(false)
        {
            return None;
        }
        let bounds = self.pane_bounds.borrow();
        super::super::pane::pane_drop_target(&bounds, position, drag.pane_id)
    }

    /// The workspace tab being dragged for a pointer at `position`: the tab the
    /// last reorder named, or — before the first crossing — the pressed tab once
    /// the pointer has travelled far enough to make the press a drag.
    fn dragged_tab(&self, position: Vector2F) -> Option<usize> {
        if let Some(index) = self.space.drag {
            return Some(index);
        }
        let press = self.space.press?;
        let origin = self.space.pressed_at?;
        let travelled = (position.x - origin.x)
            .abs()
            .max((position.y - origin.y).abs());
        (travelled > TAB_DRAG_THRESHOLD).then_some(press)
    }
}

impl Element for SpaceDropStrip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
        // The tabs painted above, so the registry holds this frame's rectangles.
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        if !drag.moved() {
            return;
        }
        let Some(index) = drag.drop_index else {
            return;
        };
        let Some(bounds) = self.bounds() else {
            return;
        };
        let tabs = self.drawn_tabs();
        if tabs.is_empty() {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        // The strip lifts while a pane is over it, and the marker names the
        // boundary the pane lands on.
        renderer.fill_rect(bounds, app.theme.color(ColorToken::Selected));
        let x = insertion_x(&tabs, index).clamp(bounds.min_x(), bounds.max_x());
        renderer.fill_rect(
            rectf(x - 1.0, bounds.min_y(), 2.0, bounds.height()),
            app.theme.color(ColorToken::Accent),
        );
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
        // A lifted pane owns every pointer move and the release that ends it,
        // wherever the pointer has travelled.
        if self.drag.is_some() {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    let index = self.drop_index_at(*position);
                    let target = self.drop_target_at(*position);
                    (self.on_drag_move.borrow_mut())(*position, index, target);
                    return true;
                }
                DispatchedEvent::MouseUp { position, .. } => {
                    let index = self.drop_index_at(*position);
                    let target = self.drop_target_at(*position);
                    (self.on_drop.borrow_mut())(index, target);
                    return true;
                }
                // A second press while a pane is in flight ends the drag, the
                // way a release off the strip would, instead of starting a
                // second lift.
                DispatchedEvent::MouseDown { .. } => {
                    (self.on_drop.borrow_mut())(None, None);
                    return true;
                }
                _ => {}
            }
        }
        match event {
            // A pressed tab that has travelled far enough is a drag: the tab
            // follows the pointer, moving one place whenever it crosses a
            // neighbouring tab's center.
            DispatchedEvent::MouseMove { position } => {
                if let Some(from) = self.dragged_tab(*position) {
                    let tabs = self.drawn_tabs();
                    if tabs.get(from).is_some() {
                        let to = reorder_target_index(&tabs, from, position.x);
                        (self.on_space_reorder.borrow_mut())(from, to);
                        return true;
                    }
                }
            }
            // A release ends the press. A plain click is left to the tab under
            // the pointer (its own hit test fires the select); a drag is ended
            // here so the tab that was dragged is never also selected or
            // renamed, and the press is cleared either way so a pointer move
            // after the button came up can never start a drag by itself.
            DispatchedEvent::MouseUp { position, button } => {
                if *button == BUTTON_PRIMARY {
                    if self.dragged_tab(*position).is_some() {
                        (self.on_space_release.borrow_mut())();
                        return true;
                    }
                    let handled = self.inner.dispatch_event(event, ctx, app);
                    if self.space.press.is_some() {
                        (self.on_space_release.borrow_mut())();
                    }
                    return handled;
                }
            }
            // A press while a tab is already being dragged ends the drag here
            // rather than letting the tab under the pointer take it. A stale
            // press — one whose release was lost with the window's focus — is
            // cleared the same way, so it can never turn a later pointer move
            // into a drag; the press that ended it still reaches the tab it
            // landed on.
            DispatchedEvent::MouseDown { button, .. } => {
                if *button == BUTTON_PRIMARY && self.space.press.is_some() {
                    let dragging = self.space.drag.is_some();
                    (self.on_space_release.borrow_mut())();
                    if dragging {
                        return true;
                    }
                }
            }
            _ => {}
        }
        self.inner.dispatch_event(event, ctx, app)
    }
}

/// The card a lifted pane drags under the pointer: the pane's own title on the
/// raised surface, so the drag reads as carrying that pane. It is zero-sized —
/// the bar keeps its height — and only draws, at the pointer.
struct PaneDragGhost {
    card: Option<Box<dyn Element>>,
    position: Option<Vector2F>,
    size: Option<Vector2F>,
}

impl Element for PaneDragGhost {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if let Some(card) = self.card.as_mut() {
            let _ = card.layout(constraint, ctx, app);
        }
        self.size = Some(Vector2F::zero());
        Vector2F::zero()
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let (Some(card), Some(position)) = (self.card.as_mut(), self.position) else {
            return;
        };
        card.paint(position + vec2f(GHOST_OFFSET_X, GHOST_OFFSET_Y), ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        None
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

/// Build the ghost a lifted pane draws, or nothing at all when no pane is in
/// flight — or when the press has not travelled far enough to be a drag.
fn build_pane_drag_ghost(app: &AppContext, drag: Option<&PaneDrag>) -> Box<dyn Element> {
    let moved = drag.map(|drag| drag.moved()).unwrap_or(false);
    let Some(drag) = drag.filter(|_| moved) else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let pad = app.theme.spacing_px(SpacingToken::Sm);
    let card: Box<dyn Element> = Container::new(
        Text::new(drag.title.clone())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(pad))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(app.theme.color(ColorToken::Accent).into())
    .with_corner_radius(6.0)
    .finish();
    Box::new(PaneDragGhost {
        card: Some(
            ConstrainedBox::new(card)
                .with_max_width(GHOST_MAX_WIDTH)
                .finish(),
        ),
        position: Some(drag.position),
        size: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::{Pane, PaneKind, Space};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// The whole app over a real store whose bar carries one tab per name in
    /// `names`, the first-run overlays out of the way.
    fn app_with_workspaces(
        names: Vec<String>,
    ) -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use std::sync::Arc;

        assert!(!names.is_empty(), "a bar needs at least one tab");
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            // Keep the bar deterministic: a pane in the checkout would show the
            // real working tree's diff control and squeeze the tabs under test.
            let cwd = dir.path().display().to_string();
            let active = s.active_pane_id;
            s.set_pane_path(active, cwd);
            // The space the app opens on takes the first name; the rest are new
            // spaces of their own, as "+" opens them.
            s.spaces[0].name = names[0].clone();
            s.spaces[0].named = true;
            for name in &names[1..] {
                let id = s.next_pane_id;
                s.next_pane_id += 1;
                s.spaces.push(Space::new(
                    name.clone(),
                    Pane::Leaf {
                        id,
                        kind: PaneKind::Terminal,
                    },
                ));
            }
        }
        (Box::new(view), state, dir)
    }

    /// Every text run drawn in the topbar band, which is where the tab labels
    /// live.
    fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The rectangle of the selected workspace tab's fill: the `Surface` fill
    /// that starts inside the bar and runs past its bottom edge into the
    /// content (the merge). It is the one fill of that colour in the bar band.
    fn selected_tab_body(commands: &[RenderCommand], app: &AppContext) -> RectF {
        let surface = app.theme.color(ColorToken::Surface);
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == surface
                        && (rect.min_y() - TAB_TOP_INSET).abs() < 0.01
                        && (rect.max_y() - tab_merge_bottom()).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the selected workspace tab is the filled tab of the strip")
    }

    /// One whole-app frame with the pointer at `cursor`: the layout pass rebuilds
    /// the tree from app state, then the paint pass reads the pointer — the
    /// order the app itself runs every frame.
    fn render_with_pointer(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<RenderCommand> {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut ctx = PaintContext::new(goble_ui::render::Renderer::new());
        if let Some(position) = cursor {
            ctx.cursor_inside = true;
            ctx.cursor_position = position;
        }
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The hover pill a floating tab paints, when it paints one: the rounded
    /// fill between [`TAB_TOP_INSET`] and [`tab_pill_bottom`].
    fn hover_pill(commands: &[RenderCommand], app: &AppContext) -> Option<(RectF, f32)> {
        let hover = app.theme.color(ColorToken::Hover);
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == hover
                && (rect.min_y() - TAB_TOP_INSET).abs() < 0.01
                && (rect.max_y() - tab_pill_bottom()).abs() < 0.01 =>
            {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
    }

    /// The rightmost x anything is drawn at inside the topbar band, so a test
    /// can tell whether the bar's own controls were pushed past the window.
    fn topbar_max_x(commands: &[RenderCommand]) -> f32 {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. } if rect.min_y() < TOPBAR_HEIGHT => {
                    Some(rect.max_x())
                }
                RenderCommand::DrawIcon { origin, size, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(origin.x + size)
                }
                // A run's own width is not in the command; where it starts is
                // what says whether it begins inside the bar.
                RenderCommand::DrawText { origin, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(origin.x)
                }
                _ => None,
            })
            .fold(0.0_f32, f32::max)
    }

    fn topbar_text(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text: drawn,
                origin,
                ..
            } if drawn == text && origin.y < TOPBAR_HEIGHT => Some(*origin),
            _ => None,
        })
    }

    /// The centre of the topbar icon drawn from `name`, which is the control
    /// that icon sits in.
    fn topbar_icon(commands: &[RenderCommand], name: &str) -> Option<Vector2F> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name: drawn,
                origin,
                size,
                ..
            } if drawn == name && origin.y < TOPBAR_HEIGHT => {
                Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
            }
            _ => None,
        })
    }

    /// A tab is a flexible child of the strip, as it is in warp-new: one
    /// short-named tab in a wide window takes the share of the bar the tab strip
    /// was given instead of shrinking to the width of its name, up to warp-new's
    /// 200 pt cap. The visible fill is the slot less the pill's 3 pt edge insets.
    #[test]
    fn a_tab_fills_its_slot_up_to_the_maximum() {
        let app = AppContext::default();
        let (mut root, _state, _threads) = app_with_workspaces(vec!["One".to_string()]);
        let commands = render_element(&mut root, window(), &app);
        let tab = selected_tab_body(&commands, &app);

        assert!(
            tab.width() > 80.0,
            "one short tab takes its share of the bar, not its name's width: {tab:?}"
        );
        assert!(
            (tab.width() - (MAX_TAB_WIDTH - TAB_EDGE_INSET * 2.0)).abs() < 0.01,
            "and stops at the maximum however wide the window is: {tab:?}"
        );
        assert_eq!(
            (tab.min_y(), tab.max_y()),
            (TAB_TOP_INSET, tab_merge_bottom()),
            "its fill floats inside the bar and merges into the content"
        );
    }

    /// The agent-management control sits beside the tools toggle: the bar reads
    /// "tools, then the dashboard", the order the two panels are opened in. The
    /// strip follows the last leading control after the bar's control gap and
    /// the tab's own pill inset, with no rule between them.
    #[test]
    fn the_agent_management_control_sits_beside_the_tools_toggle() {
        let app = AppContext::default();
        let (mut root, _state, _threads) = app_with_workspaces(vec!["One".to_string()]);
        let commands = render_element(&mut root, window(), &app);

        let tools = topbar_icon(&commands, "menu-01").expect("the tools toggle");
        let agents = topbar_icon(&commands, "robot").expect("the agent management control");
        assert!(
            tools.x < agents.x,
            "the dashboard control follows the tools toggle: {tools:?} against {agents:?}"
        );
        assert!(
            topbar_icon(&commands, "cloud").is_none(),
            "the cloud agent entry lives in the environment menu, not in the bar"
        );
        // The dashboard's box is a 16 pt face with 3 pt of padding, and the
        // sidebar's toggle reads as active because its panel is out.
        let selected = app.theme.color(ColorToken::Selected);
        let tools_box = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == selected
                        && rect.width() == TOPBAR_ICON_BUTTON_SIZE
                        && rect.height() == TOPBAR_ICON_BUTTON_SIZE =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the tools toggle draws its active fill");
        assert!(
            (tools_box.origin.x + TOPBAR_ICON_BUTTON_SIZE / 2.0 - tools.x).abs() < 0.5
                && (tools_box.origin.y + TOPBAR_ICON_BUTTON_SIZE / 2.0 - tools.y).abs() < 0.5,
            "the icon sits centered in its 22 pt box: {tools_box:?} around {tools:?}"
        );

        // The first tab's fill begins 3 pt (the bar's control gap) plus the
        // pill's own 3 pt inset after the dashboard button's trailing edge.
        let tab = selected_tab_body(&commands, &app);
        let expected =
            agents.x + TOPBAR_ICON_BUTTON_SIZE / 2.0 + TOPBAR_CONTROL_GAP + TAB_EDGE_INSET;
        assert!(
            (tab.min_x() - expected).abs() < 0.5,
            "the strip starts right after the bar's controls, with no rule between: {} against {expected}",
            tab.min_x()
        );
    }

    /// The strip draws no vertical rules and closes with no hairline: the bar's
    /// `Bg` and the pills' own gaps are the separation, as in Chrome.
    #[test]
    fn the_strip_draws_no_vertical_rules() {
        let app = AppContext::default();
        let (mut root, _state, _threads) =
            app_with_workspaces(vec!["One".to_string(), "Two".to_string()]);
        let commands = render_element(&mut root, window(), &app);

        let rules: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if rect.width() == 1.0 && (rect.height() - TOPBAR_HEIGHT).abs() < 0.5 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert!(
            rules.is_empty(),
            "no full-height rules surround or separate the tabs: {rules:?}"
        );
        let tab = selected_tab_body(&commands, &app);
        let plus = topbar_icon(&commands, "plus").expect("the new-workspace control");
        assert!(
            plus.x > tab.max_x(),
            "the new-workspace control follows the last tab: {plus:?} after {tab:?}"
        );
    }

    /// A tab stops at the maximum ([`MAX_TAB_WIDTH`]) however wide the window is
    /// and however long its name, and a name the user typed is cut at its **end**
    /// — warp-new's `ClipConfig::end()` for a custom title — so the words that
    /// tell two named tabs apart stay on screen.
    #[test]
    fn a_named_tab_cuts_its_long_name_at_the_end() {
        let app = AppContext::default();
        let name = "A workspace named far longer than any tab of the bar is wide";
        let (mut root, _state, _threads) = app_with_workspaces(vec![name.to_string()]);
        let commands = render_element(&mut root, window(), &app);
        let tab = selected_tab_body(&commands, &app);

        assert!(
            (tab.width() - (MAX_TAB_WIDTH - TAB_EDGE_INSET * 2.0)).abs() < 0.01,
            "the tab stops at the maximum in a wide window: {tab:?}"
        );
        assert!(
            topbar_text(&commands, name).is_none(),
            "the whole name does not fit the tab: {:?}",
            topbar_texts(&commands)
        );
        assert!(
            topbar_texts(&commands)
                .iter()
                .any(|text| text.starts_with("A workspace") && text.ends_with(ELLIPSIS)),
            "the tab reads the name cut at its own width: {:?}",
            topbar_texts(&commands)
        );
        // A cut name keeps the left inset the tab has always drawn it at: the
        // centring only applies while the name is short enough to fit whole.
        let cut = topbar_texts(&commands)
            .into_iter()
            .find(|text| text.starts_with("A workspace") && text.ends_with(ELLIPSIS))
            .expect("the tab reads the name cut at its own width");
        let origin = topbar_text(&commands, &cut).expect("the cut run's origin");
        assert!(
            (origin.x - (tab.min_x() + WorkspaceChip::h_pad())).abs() < 0.01,
            "a name too long for its tab starts at the pill's left inset: {origin:?} in {tab:?}"
        );
        let cut_width = Text::new(cut.as_str())
            .with_font_size(TAB_FONT_SIZE)
            .with_weight(FontWeight::Medium)
            .with_max_lines(1)
            .layout(
                SizeConstraint::loose(vec2f(f32::INFINITY, TOPBAR_HEIGHT)),
                &mut LayoutContext,
                &app,
            )
            .x;
        assert!(
            origin.x + cut_width <= tab.max_x() - WorkspaceChip::h_pad(),
            "the cut run ends inside the tab: {origin:?} + {cut_width} in {tab:?}"
        );
    }

    /// A tab whose label is derived from where it runs cuts that label at its
    /// **start** — warp-new's `ClipConfig::start()` for a plain title — so the
    /// last component of a long path stays readable in a narrow strip.
    #[test]
    fn a_tab_named_after_its_path_cuts_its_name_at_the_start() {
        use crate::ui::{Pane, PaneKind, Space};

        let app = AppContext::default();
        // A tab nobody named, holding a file: its label is the file's path, far
        // wider than a tab ever gets.
        let path = format!("{}/src/ui/shell/topbar.rs", env!("CARGO_MANIFEST_DIR"));
        let (mut root, state, _threads) = app_with_workspaces(vec!["One".to_string()]);
        {
            let mut s = state.borrow_mut();
            let id = s.spaces[0].root.first_leaf_id();
            s.spaces[0] = Space::unnamed(Pane::Leaf {
                id,
                kind: PaneKind::File { path },
            });
        }
        // Narrow enough that the label is far wider than the tab.
        let commands = render_element(&mut root, vec2f(500.0, 600.0), &app);
        let drawn = topbar_texts(&commands);

        assert!(
            drawn
                .iter()
                .any(|text| text.starts_with(ELLIPSIS) && text.ends_with("shell/topbar.rs")),
            "the tab keeps the end of the path it runs in: {drawn:?}"
        );
    }

    /// Every tab of a strip is as wide as its neighbours, whatever its name:
    /// the tabs divide the strip evenly (see [`tab_slot`]) instead of each one
    /// taking the room its own name needs. The selected tab's fill is measured
    /// twice, by selecting a different tab for the second frame.
    #[test]
    fn every_tab_of_a_strip_is_the_same_width_whatever_its_name() {
        let app = AppContext::default();
        let names = vec![
            "One".to_string(),
            "A workspace named far longer than any tab of the bar is wide".to_string(),
            "Two".to_string(),
        ];
        let (mut root, state, _threads) = app_with_workspaces(names);
        // A window narrow enough that the tabs stop short of the cap: the strip
        // is shared, not capped, so the name is the only thing that could set a
        // tab's width — and it does not.
        let first = selected_tab_body(&render_element(&mut root, vec2f(800.0, 600.0), &app), &app);
        state.borrow_mut().active_space = 1;
        let second = selected_tab_body(&render_element(&mut root, vec2f(800.0, 600.0), &app), &app);

        let slot = second.min_x() - first.min_x();
        assert!(
            (first.width() - second.width()).abs() < 0.01,
            "the short-named tab and the long-named one are the same width: {first:?} against {second:?}"
        );
        assert!(
            (slot - (first.width() + TAB_EDGE_INSET * 2.0)).abs() < 0.01,
            "the slots are contiguous and equal: step {slot} against fill {}",
            first.width()
        );
        assert!(
            first.width() > 80.0 && first.width() < MAX_TAB_WIDTH,
            "the strip is shared rather than capped in this window: {first:?}"
        );
    }

    /// Once the tabs reach the cap, the room they no longer take stays at the
    /// end of the strip — warp-new's `Shrinkable::new(0.5, Empty)` spacer: the
    /// tabs start at the strip's own start, the "+ ▾" control follows the last
    /// of them, and the right-hand group keeps the window's edge.
    #[test]
    fn the_room_past_the_cap_stays_at_the_end_of_the_strip() {
        let app = AppContext::default();
        let (mut root, state, _threads) =
            app_with_workspaces(vec!["One".to_string(), "Two".to_string()]);
        let first = selected_tab_body(&render_element(&mut root, window(), &app), &app);
        state.borrow_mut().active_space = 1;
        let second = selected_tab_body(&render_element(&mut root, window(), &app), &app);

        assert!(
            (first.width() - (MAX_TAB_WIDTH - TAB_EDGE_INSET * 2.0)).abs() < 0.01,
            "two tabs in a wide window are both at the cap: {first:?}"
        );
        assert!(
            first.min_x() < window().x / 4.0,
            "the tabs start at the strip's own start rather than centring in the room: {first:?}"
        );
        let last_tab_max_x = second.max_x() + TAB_EDGE_INSET;
        let plus = topbar_icon(&render_element(&mut root, window(), &app), "plus")
            .expect("the new-workspace control");
        assert!(
            plus.x > last_tab_max_x && plus.x - last_tab_max_x < 40.0,
            "the bar's next control follows the last tab: {plus:?} after {last_tab_max_x}"
        );
        let commands = render_element(&mut root, window(), &app);
        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert!(
            settings.x - last_tab_max_x > 200.0,
            "the room past the cap stays blank at the end of the strip: \
             {settings:?} after {last_tab_max_x}"
        );
    }

    /// The strip divides the window less both of the bar's paddings: the room
    /// the tabs share ends before the right-hand group, so no tab runs under
    /// the inbox or Settings and the bar's trailing margin stays empty.
    #[test]
    fn the_strip_stops_before_the_right_hand_group() {
        let app = AppContext::default();
        let names: Vec<String> = ["One", "Two", "Three", "Four", "Five"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        let (mut root, state, _threads) = app_with_workspaces(names);
        let first = selected_tab_body(&render_element(&mut root, window(), &app), &app);
        assert!(
            first.width() < MAX_TAB_WIDTH,
            "five tabs share the strip rather than each taking the cap: {first:?}"
        );
        state.borrow_mut().active_space = 4;
        let commands = render_element(&mut root, window(), &app);
        let last = selected_tab_body(&commands, &app);
        let last_tab_right = last.max_x() + TAB_EDGE_INSET;

        let inbox = topbar_icon(&commands, "inbox-01").expect("the inbox control");
        let inbox_left = inbox.x - TOPBAR_ICON_BUTTON_SIZE / 2.0;
        assert!(
            last_tab_right < inbox_left,
            "the strip stops before the right-hand group: {last_tab_right} against {inbox_left}"
        );

        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert_eq!(
            settings.x + TOPBAR_ICON_BUTTON_SIZE / 2.0,
            window().x - TOPBAR_RIGHT_PADDING,
            "the right-hand group keeps the bar's margin from the window's edge"
        );
    }

    /// warp-new draws the active tab's title at `Weight::Medium` over the
    /// inactive tabs' regular one (`TabStyles::default`, `app/src/tab.rs:800-812`),
    /// both at the UI font size.
    #[test]
    fn the_active_tab_reads_in_medium_and_the_others_in_regular() {
        let app = AppContext::default();
        let (mut root, _state, _threads) =
            app_with_workspaces(vec!["One".to_string(), "Two".to_string()]);
        let commands = render_element(&mut root, window(), &app);

        let runs: Vec<(&str, f32, FontWeight)> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    origin,
                    font_size,
                    font_weight,
                    ..
                } if origin.y < TOPBAR_HEIGHT => Some((text.as_str(), *font_size, *font_weight)),
                _ => None,
            })
            .collect();
        assert!(
            runs.contains(&("One", TAB_FONT_SIZE, FontWeight::Medium)),
            "the active tab's title is drawn medium, at the tab font size: {runs:?}"
        );
        assert!(
            runs.contains(&("Two", TAB_FONT_SIZE, FontWeight::Regular)),
            "an inactive tab's title is drawn regular: {runs:?}"
        );
    }

    /// More tabs than the bar can give the minimum: they share the room the bar
    /// has left, every one exactly as wide as its neighbours and none of them
    /// drawn past the window, so the "+ ▾" control and Settings keep the place
    /// inside the window they hold with a single tab. A tab too narrow for the
    /// close control draws none, even while hovered.
    #[test]
    fn many_tabs_share_the_bar_and_keep_the_controls_inside_it() {
        const COUNT: usize = 14;
        let app = AppContext::default();
        let names: Vec<String> = (0..COUNT)
            .map(|index| format!("Workspace {index}"))
            .collect();
        let (mut root, state, _threads) = app_with_workspaces(names);
        let first = selected_tab_body(&render_element(&mut root, window(), &app), &app);
        state.borrow_mut().active_space = COUNT - 1;
        let last = selected_tab_body(&render_element(&mut root, window(), &app), &app);

        let slot = (last.min_x() - first.min_x()) / (COUNT - 1) as f32;
        let share = first.width();
        assert!(share > 0.0, "the tabs have room between them: {share}");
        assert!(
            share < 80.0 - TAB_EDGE_INSET * 2.0,
            "the room is shared, narrower than the 80 pt a tab slot is read at: {share}"
        );
        assert!(
            (slot - (share + TAB_EDGE_INSET * 2.0)).abs() < 0.01,
            "every tab of the strip is the same width: step {slot} against fill {share}"
        );
        assert!(
            share + TAB_EDGE_INSET * 2.0 < MIN_CONTENTS_FOR_CLOSE,
            "this many tabs leave no room for the close control: {share}"
        );

        // Hover a middle tab: the pill appears, but no close "x".
        let hover_at = vec2f(
            first.min_x() - TAB_EDGE_INSET + slot * 6.5,
            TOPBAR_HEIGHT / 2.0,
        );
        let commands = render_with_pointer(&mut root, &app, Some(hover_at));
        assert!(
            hover_pill(&commands, &app).is_some(),
            "the hovered narrow tab still paints its pill"
        );
        let closes: Vec<Vector2F> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == "x-close" && origin.y < TOPBAR_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .collect();
        assert!(
            closes.is_empty(),
            "no tab has room for a close control in this crowded strip: {closes:?}"
        );

        let commands = render_element(&mut root, window(), &app);
        assert!(
            topbar_max_x(&commands) <= window().x + 0.5,
            "nothing the bar draws is pushed past the window's right edge: {commands:?}"
        );
        let chevron = topbar_icon(&commands, "chevron-down").expect("the environment menu");
        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert!(
            chevron.x < settings.x,
            "the bar's own controls keep their order: {chevron:?} against {settings:?}"
        );
        assert!(
            settings.x < window().x,
            "and Settings stays inside the window: {settings:?}"
        );
    }

    /// The environment menu names where each row runs with the same two
    /// glyphs the rest of the app uses: T3 Code's computer for a local
    /// environment and its cloud for a worker.
    #[test]
    fn the_environment_menu_marks_the_computer_and_the_cloud() {
        let app = AppContext::default();
        let (mut root, state, _threads) = app_with_workspaces(vec!["One".to_string()]);
        *state.borrow().env_selector_open.borrow_mut() = true;
        let commands = render_element(&mut root, window(), &app);
        let icons: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            icons.contains(&"environment-computer"),
            "the local row draws the computer: {icons:?}"
        );
        assert!(
            icons.contains(&"environment-cloud"),
            "the remote row draws the cloud: {icons:?}"
        );
    }

    /// A crowded bar: the tabs come down to an equal share of the room that is
    /// left — under the 80 pt a slot is read at — and cut their names, and the
    /// bar's own controls still land inside the window.
    #[test]
    fn a_narrow_window_still_holds_every_control() {
        // Narrow enough that two tabs share well under the 80 pt a slot is read
        // at, and still wide enough for a cut name in each pill.
        const WIDTH: f32 = 400.0;
        let app = AppContext::default();
        let names: Vec<String> = (0..2).map(|index| format!("Workspace {index}")).collect();
        let (mut root, state, _threads) = app_with_workspaces(names);
        let first = selected_tab_body(&render_element(&mut root, vec2f(WIDTH, 600.0), &app), &app);
        state.borrow_mut().active_space = 1;
        let second = selected_tab_body(&render_element(&mut root, vec2f(WIDTH, 600.0), &app), &app);

        let share = second.min_x() - first.min_x();
        assert!(
            share < 80.0,
            "a narrow bar shares its room instead of holding every tab at 80 pt: {share}"
        );
        let commands = render_element(&mut root, vec2f(WIDTH, 600.0), &app);
        assert!(
            topbar_texts(&commands)
                .iter()
                .any(|text| text.ends_with(ELLIPSIS)),
            "the squeezed tabs read their names cut: {:?}",
            topbar_texts(&commands)
        );
        assert!(
            topbar_max_x(&commands) <= WIDTH + 0.5,
            "nothing the bar draws is pushed past the window's right edge: {commands:?}"
        );
        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert!(
            settings.x < WIDTH,
            "Settings stays inside the window: {settings:?}"
        );
    }
}
