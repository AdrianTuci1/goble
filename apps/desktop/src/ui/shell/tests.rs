use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element, Icon, Text, TopbarButton};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, TabColor};

use super::topbar::{
    build_live_indicator, insertion_index, left_padding, reorder_target_index, tab_fillet_bottom,
    tab_hint, tab_merge_bottom, tab_pill_bottom, tab_top_radius, WorkspaceChip, WorkspaceChipParts,
    MIN_CONTENTS_FOR_CLOSE, TAB_EDGE_INSET, TAB_FLARE_RADIUS, TAB_TOP_INSET, TAB_TOP_RADIUS,
    TOPBAR_BOTTOM_BAND, TOPBAR_FULLSCREEN_PADDING, TOPBAR_ICON_BUTTON_SIZE, TOPBAR_RIGHT_PADDING,
    TOPBAR_TRAFFIC_INSET,
};

use super::*;
use crate::root_view::RootView;
use crate::state::{PaneSession, UiState, NEW_CONVERSATION_TITLE};
use crate::ui::build::WINDOW_BORDER_WIDTH;
use crate::ui::{LayoutKind, Pane, PaneKind, Space, SplitDir};
use goble_ui::elements::running_indicator::SPINNER_FRAMES;
use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A cue at `count`.
fn cue(count: usize) -> Box<dyn Element> {
    build_live_indicator(&AppContext::default(), count, 0.0)
}

/// C3: while work is in flight the topbar cue shows the live count next to a
/// spinner frame and the diamond that marks the count.
#[test]
fn the_cue_shows_the_live_count_and_a_spinner() {
    let app = AppContext::default();
    let mut element = cue(3);
    let commands = render_element(&mut element, vec2f(200.0, 36.0), &app);
    let drawn = drawn_texts(&commands);

    assert!(
        drawn.iter().any(|t| t == "3"),
        "the live count is drawn: {drawn:?}"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { name, .. } if name == "diamond"
        )),
        "the count is marked with the diamond icon: {commands:?}"
    );
    assert!(
        drawn.iter().any(|t| SPINNER_FRAMES.contains(&t.as_str())),
        "a spinner frame is drawn: {drawn:?}"
    );
}

/// C3: at zero the cue is inert — it draws nothing and takes no room.
#[test]
fn the_cue_is_inert_at_zero() {
    let app = AppContext::default();
    let mut element = cue(0);
    let commands = render_element(&mut element, vec2f(200.0, 36.0), &app);

    assert!(commands.is_empty(), "zero work draws nothing: {commands:?}");
    assert_eq!(
        element.size(),
        Some(vec2f(0.0, 0.0)),
        "zero work takes no topbar room"
    );

    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(5.0, 5.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(5.0, 5.0),
            button: 0,
        },
    ] {
        assert!(
            !element.dispatch_event(&event, &mut ctx, &app),
            "an inert cue takes no pointer event"
        );
    }
}

/// C3: the cue shows the count and is not a hit target — a click passes
/// through it.
#[test]
fn the_cue_takes_no_pointer_event() {
    let app = AppContext::default();
    let mut element = cue(2);
    element.layout(
        SizeConstraint::loose(vec2f(200.0, 36.0)),
        &mut LayoutContext,
        &app,
    );
    element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        },
    ] {
        assert!(
            !element.dispatch_event(&event, &mut ctx, &app),
            "the live indicator takes no pointer event"
        );
    }
}

/// C3: the indicator slot does not change the established bar height.
#[test]
fn the_topbar_height_is_unchanged() {
    #[cfg(target_os = "macos")]
    assert_eq!(TOPBAR_HEIGHT, 36.0);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(TOPBAR_HEIGHT, 40.0);
}

/// The `x` the bar's leading control — the sidebar toggle — is drawn at: its
/// icon is centered in the button that opens the bar, and that button sits at
/// the bar's leading padding.
fn leading_control_x(commands: &[RenderCommand]) -> f32 {
    let center = topbar_icon_center(commands, "menu-01").expect("the sidebar toggle");
    center.x - TOPBAR_ICON_BUTTON_SIZE / 2.0
}

/// The rectangle of the selected workspace tab's fill: the `Surface` fill that
/// starts inside the bar and runs past its bottom edge into the content (the
/// merge). It is the one fill of that colour in the bar band.
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

/// The left edge of the strip's first workspace tab's fill.
fn first_tab_left(commands: &[RenderCommand], app: &AppContext) -> f32 {
    selected_tab_body(commands, app).min_x()
}

/// The bar clears the traffic lights while the window is windowed: macOS draws
/// them over the content, and the first control starts past their room.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_bar_starts_past_the_traffic_lights_while_the_window_is_windowed() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);

    assert_eq!(
        leading_control_x(&commands),
        TOPBAR_TRAFFIC_INSET,
        "the leading control sits at the traffic-light inset on macOS"
    );
}

/// A full-screen window hides the traffic lights, so the bar gives their room
/// to the tabs: the leading control moves to the small fullscreen padding and
/// the whole strip shifts left by the difference. Nothing else moves — the bar
/// keeps its height and Settings stays pinned to the right edge.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn a_fullscreen_window_moves_the_bar_into_the_traffic_lights_space() {
    let (mut windowed_root, _state, _desktop, _dir) = app_root();
    let (mut full_root, _state2, _desktop2, _dir2) = app_root();

    let windowed_app = AppContext::default();
    let windowed = render_element(&mut windowed_root, window(), &windowed_app);

    // The window itself reports the fullscreen state, as the event loop
    // records it every frame.
    let fullscreen_app = AppContext::default();
    fullscreen_app.window_control.note_window_fullscreen(true);
    let full = render_element(&mut full_root, window(), &fullscreen_app);

    assert_eq!(
        leading_control_x(&full),
        TOPBAR_FULLSCREEN_PADDING,
        "the leading control sits at the small padding once the traffic lights \
         are gone"
    );
    assert_eq!(
        first_tab_left(&full, &fullscreen_app),
        first_tab_left(&windowed, &windowed_app)
            - (TOPBAR_TRAFFIC_INSET - TOPBAR_FULLSCREEN_PADDING),
        "the strip takes the room the traffic lights left behind"
    );
    // The right-hand group is pinned to the window's right edge either way, so
    // it does not move with the padding.
    let settings = |commands: &[RenderCommand]| {
        topbar_icon_center(commands, "settings").expect("the settings control")
    };
    assert_eq!(
        settings(&full),
        settings(&windowed),
        "Settings stays pinned to the right edge"
    );
}

/// The shift is the padding's, not the bar's: a full-screen bar still spans the
/// window at the height the body reserves below it — the bar row plus its band —
/// and it is filled with the frame's `Bg`, the tone the selected tab's `Surface`
/// fill continues away from.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_fullscreen_padding_keeps_the_bar_height() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    app.window_control.note_window_fullscreen(true);
    let commands = render_element(&mut root, window(), &app);
    let bar = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Bg)
                    && (rect.height() - (TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND)).abs() < 0.01
                    && rect.width() >= window().x =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the bar block's background spans the window");
    assert_eq!(
        bar.height(),
        TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "the bar block keeps its height (the bar row plus its band)"
    );
    assert_eq!(bar.min_y(), 0.0, "the bar starts at the window's top edge");
}

/// The app flag still answers when no window has reported a state — a tree
/// rendered without a live window, where the platform's own state is missing.
/// When the window *has* answered, its answer is the one that counts.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_windows_state_answers_before_the_app_flag() {
    // Nothing has reported, and the app says full screen.
    let (mut root, state, _desktop, _dir) = app_root();
    state.borrow_mut().fullscreen = true;
    let app = AppContext::default();
    assert_eq!(
        app.window_control.window_fullscreen(),
        None,
        "no window has reported a state in this test"
    );
    let commands = render_element(&mut root, window(), &app);
    assert_eq!(
        leading_control_x(&commands),
        TOPBAR_FULLSCREEN_PADDING,
        "the app's own flag is honoured while the platform has nothing to say"
    );

    // The window reports it is windowed: that wins over a stale app flag.
    let (mut root, state, _desktop, _dir) = app_root();
    state.borrow_mut().fullscreen = true;
    let app = AppContext::default();
    app.window_control.note_window_fullscreen(false);
    let commands = render_element(&mut root, window(), &app);
    assert_eq!(
        leading_control_x(&commands),
        TOPBAR_TRAFFIC_INSET,
        "the window's own state wins once it has answered"
    );

    // And the other way round: the app flag is off, the window is full screen.
    let (mut root, state, _desktop, _dir) = app_root();
    state.borrow_mut().fullscreen = false;
    let app = AppContext::default();
    app.window_control.note_window_fullscreen(true);
    let commands = render_element(&mut root, window(), &app);
    assert_eq!(
        leading_control_x(&commands),
        TOPBAR_FULLSCREEN_PADDING,
        "a fullscreen entered another way (the green button) still moves the bar"
    );
}

/// The padding is the traffic lights' room on macOS and nothing elsewhere, in
/// both window states.
#[test]
fn the_bar_padding_follows_the_platform() {
    if cfg!(target_os = "macos") {
        assert_eq!(left_padding(false), TOPBAR_TRAFFIC_INSET);
        assert_eq!(left_padding(true), TOPBAR_FULLSCREEN_PADDING);
        assert!(
            TOPBAR_FULLSCREEN_PADDING < TOPBAR_TRAFFIC_INSET,
            "the fullscreen padding gives room back"
        );
    } else {
        assert_eq!(left_padding(false), 0.0, "no traffic lights to clear");
        assert_eq!(left_padding(true), 0.0, "and none to give back either");
    }
}

/// The x the Settings control's 22 pt button ends at: its icon is centred in
/// the button, and that button is the bar's last control.
fn settings_right_edge(commands: &[RenderCommand]) -> f32 {
    let center = topbar_icon_center(commands, "settings").expect("the settings control");
    center.x + TOPBAR_ICON_BUTTON_SIZE / 2.0
}

/// The right-hand group is inset from the window's edge by the bar's own
/// content margin, in both window states: Settings ends `TOPBAR_RIGHT_PADDING`
/// short of the edge instead of sitting on it, and far enough inside it to
/// clear the window's own 1 px rule. The traffic lights only buy room on the
/// leading side, so the group does not move when the window goes full screen.
#[test]
fn the_right_hand_group_keeps_the_bars_margin_from_the_windows_edge() {
    let (mut windowed_root, _state, _desktop, _dir) = app_root();
    let windowed_app = AppContext::default();
    let windowed = render_element(&mut windowed_root, window(), &windowed_app);

    let (mut full_root, _state2, _desktop2, _dir2) = app_root();
    let full_app = AppContext::default();
    full_app.window_control.note_window_fullscreen(true);
    let full = render_element(&mut full_root, window(), &full_app);

    for (commands, state) in [(&windowed, "windowed"), (&full, "full screen")] {
        let right = settings_right_edge(commands);
        assert_eq!(
            right,
            window().x - TOPBAR_RIGHT_PADDING,
            "Settings ends one bar margin short of the window's edge while {state}: {right} in a \
             {:?}-wide window",
            window()
        );
        assert!(
            window().x - right > WINDOW_BORDER_WIDTH,
            "the last control clears the window's rule while {state}: {right} against {:?}",
            window()
        );
    }
}

/// The bar's margin is the same on both sides: with the traffic lights gone
/// (full screen) the first control starts exactly as far from the leading edge
/// as Settings ends from the trailing one.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_bar_margin_is_the_same_on_both_sides_in_full_screen() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    app.window_control.note_window_fullscreen(true);
    let commands = render_element(&mut root, window(), &app);

    assert_eq!(
        leading_control_x(&commands),
        window().x - settings_right_edge(&commands),
        "the leading control's margin and the trailing one are the bar's own margin"
    );
    assert_eq!(
        window().x - settings_right_edge(&commands),
        TOPBAR_FULLSCREEN_PADDING,
        "which is the padding the bar keeps when nothing precedes its content"
    );
}

/// The trailing margin is the bar's own content margin on every platform, and
/// it does not depend on the window's state.
#[test]
fn the_bar_right_padding_is_the_bars_own_content_margin() {
    assert_eq!(TOPBAR_RIGHT_PADDING, TOPBAR_FULLSCREEN_PADDING);
    if cfg!(target_os = "macos") {
        assert_eq!(TOPBAR_RIGHT_PADDING, 10.0);
    } else {
        assert_eq!(TOPBAR_RIGHT_PADDING, 0.0, "the bar's content has no margin");
    }
}

/// A tab with the given close control, the shape `build_workspace_chip`
/// builds.
fn tab(
    app: &AppContext,
    active: bool,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    close: Box<dyn Element>,
) -> Box<dyn Element> {
    tab_colored(app, active, None, on_click, close)
}

/// The same tab, carrying `color`.
fn tab_colored(
    app: &AppContext,
    active: bool,
    color: Option<TabColor>,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    close: Box<dyn Element>,
) -> Box<dyn Element> {
    tab_hinted(app, active, color, None, on_click, close)
}

/// The same tab, drawing `hint` in its trailing slot in place of the close "x"
/// — the shape `build_workspace_chip` builds while the chord modifier is held.
fn tab_hinted(
    app: &AppContext,
    active: bool,
    color: Option<TabColor>,
    hint: Option<Box<dyn Element>>,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    close: Box<dyn Element>,
) -> Box<dyn Element> {
    let child = Text::new("Space 1")
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(12.0)
        .finish();
    tab_labelled(app, active, color, child, hint, on_click, close)
}

/// The same tab, drawing `child` as its name.
fn tab_labelled(
    _app: &AppContext,
    active: bool,
    color: Option<TabColor>,
    child: Box<dyn Element>,
    hint: Option<Box<dyn Element>>,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    close: Box<dyn Element>,
) -> Box<dyn Element> {
    let noop_press: Rc<RefCell<dyn FnMut(usize, Vector2F)>> =
        Rc::new(RefCell::new(|_: usize, _: Vector2F| {}));
    let noop_at: Rc<RefCell<dyn FnMut(usize, Vector2F)>> =
        Rc::new(RefCell::new(|_: usize, _: Vector2F| {}));
    Box::new(WorkspaceChip::new(WorkspaceChipParts {
        child,
        index: 1,
        active,
        color,
        on_click,
        on_press: noop_press,
        on_right_click: noop_at,
        close,
        hint,
    }))
}

fn close_button(app: &AppContext, on_close: Rc<RefCell<dyn FnMut(usize)>>) -> Box<dyn Element> {
    let index = 1;
    TopbarButton::new(
        Icon::new("x")
            .with_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(16.0)
    .with_corner_radius(8.0)
    .with_on_click(move || (on_close.borrow_mut())(index))
    .finish()
}

/// R2: the selected tab merges into the content with an inverted (concave) radius
/// on both sides. Its fill is the pane surface, it starts inside the bar, a foot
/// band widens it at the merge line so the tab reaches into the content plane, and
/// a quarter-circle of the bar's own tone is cut out of each outer corner — the
/// concave fillet that bends the tab's side into the surface below.
#[test]
fn the_selected_tab_merges_into_the_content_with_inverted_corners() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, true, noop.clone(), close_button(&app, noop));
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);
    let slot_width = element.size().expect("the tab was laid out").x;
    let content_width = slot_width - TAB_EDGE_INSET * 2.0;
    let surface = app.theme.color(ColorToken::Surface);
    let background = app.theme.color(ColorToken::Bg);
    let merge = tab_merge_bottom();
    let radius = TAB_FLARE_RADIUS;

    let surface_fills: Vec<(RectF, f32)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface => Some((*rect, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(
        surface_fills.len(),
        3,
        "the body, its foot band and the merge stem below the fillet: {surface_fills:?}"
    );
    let (body, body_radius) = surface_fills[0];
    assert_eq!(
        (body.min_y(), body.max_y()),
        (TAB_TOP_INSET, merge),
        "the fill starts inside the bar and reaches past its bottom edge: {body:?}"
    );
    assert_eq!(
        (body.min_x(), body.width()),
        (TAB_EDGE_INSET, content_width),
        "the fill is the slot inset by the pill's edge gap: {body:?}"
    );
    assert!(
        (body_radius - tab_top_radius(content_width)).abs() < 0.01,
        "the fill's top corners are Chrome's clamped radius: {body_radius} against {}",
        tab_top_radius(content_width)
    );

    // The foot band: the `radius` rows above the fillet line, widened by
    // `radius` on each side, so the flare sits inside the bar and the band.
    let fillet_bottom = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;
    let (band, band_radius) = surface_fills[1];
    assert_eq!(
        (band.min_y(), band.height()),
        (fillet_bottom - radius, radius),
        "the foot band ends at the band's bottom edge: {band:?}"
    );
    assert_eq!(
        band.max_y(),
        fillet_bottom,
        "the inverted corners bottom out inside the topbar+band reservation, \
         not over the pane area below: {band:?}"
    );
    assert_eq!(
        (band.min_x(), band.width()),
        (body.min_x() - radius, content_width + 2.0 * radius),
        "and runs a fillet radius past the body on each side: {band:?}"
    );
    assert_eq!(band_radius, 0.0, "the band itself is square: {band:?}");
    // Only the tab's own width continues below the fillet line, across the pane
    // gutter to the surface.
    let over: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && rect.max_y() > fillet_bottom + 0.01
                    && rect.width() > content_width + 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert!(
        over.is_empty(),
        "nothing wider than the tab continues below the fillet line: {over:?}"
    );
    let (stem, stem_radius) = surface_fills[2];
    assert_eq!(
        (stem.min_y(), stem.height()),
        (fillet_bottom, merge - fillet_bottom),
        "below the fillet the tab's own column continues to the merge line: {stem:?}"
    );
    assert_eq!(
        (stem.min_x(), stem.width()),
        (body.min_x(), content_width),
        "the merge stem is the tab's own width: {stem:?}"
    );
    assert_eq!(stem_radius, 0.0, "the stem is square: {stem:?}");

    // The concave fillets: a background-coloured quarter disc bites each outer
    // top corner of the band, clipped to that corner.
    let scoops: Vec<(RectF, f32)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == background => Some((*rect, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(scoops.len(), 2, "one fillet per side: {scoops:?}");
    for (disc, disc_radius) in &scoops {
        assert_eq!(
            (disc.width(), disc.height()),
            (radius * 2.0, radius * 2.0),
            "a fillet is a 24x24 square whose SDF radius renders a circle: {disc:?}"
        );
        assert_eq!(*disc_radius, radius, "the fillet's radius");
        assert_eq!(
            disc.min_y(),
            fillet_bottom - radius * 2.0,
            "the disc is centred on the band's outer top corner: {disc:?}"
        );
    }
    assert_eq!(
        scoops[0].0.min_x(),
        body.min_x() - radius * 2.0,
        "the left fillet disc is centred on the band's outer-left top corner"
    );
    assert_eq!(
        scoops[1].0.min_x(),
        body.max_x(),
        "the right fillet disc is centred on the band's outer-right top corner"
    );

    let clips: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::ClipRect(rect) => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(
        clips.len(),
        3,
        "one clip for the merge line and one per fillet: {clips:?}"
    );
    assert!(
        (clips[0].max_y() - merge).abs() < 0.01,
        "the fill is clipped at the merge line: {:?}",
        clips[0]
    );
    assert_eq!(
        (clips[1].min_x(), clips[1].min_y(), clips[1].width()),
        (body.min_x() - radius, fillet_bottom - radius, radius),
        "the left fillet is clipped to the band's outer top corner: {:?}",
        clips[1]
    );
    assert_eq!(
        (clips[2].min_x(), clips[2].min_y(), clips[2].width()),
        (body.max_x(), fillet_bottom - radius, radius),
        "the right fillet is clipped to the band's outer top corner: {:?}",
        clips[2]
    );
    assert_eq!(
        commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::PopClip))
            .count(),
        3,
        "every clip is popped"
    );

    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { .. })),
        "a merged tab never strokes a border"
    );
}

/// R2: only the selected tab paints a surface. A floating tab at rest paints
/// nothing at all — the bar's `Bg` shows through — and one with a colour paints
/// only its tinted pill.
#[test]
fn only_the_selected_tab_paints_a_surface() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, false, noop.clone(), close_button(&app, noop));
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);

    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { .. })),
        "an inactive tab at rest has no fill: {commands:?}"
    );
}

/// R2: clicking the tab body still selects that space.
#[test]
fn a_click_on_the_tab_selects_its_space() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| closed.borrow_mut().push(i)))
    };
    let mut element = tab(&app, true, on_click, close_button(&app, on_close));
    element.layout(
        SizeConstraint::loose(vec2f(400.0, TOPBAR_HEIGHT)),
        &mut LayoutContext,
        &app,
    );
    element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(5.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(5.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }

    assert_eq!(
        *selected.borrow(),
        vec![1],
        "the tab body selects its space"
    );
    assert!(
        closed.borrow().is_empty(),
        "the tab body is not the close control"
    );
}

/// One frame of a chip with the pointer at `cursor`. Hover is read at paint, so
/// the pointer is what decides whether the tab offers its close control and
/// which pill it paints.
fn render_with_pointer(
    element: &mut Box<dyn Element>,
    size: Vector2F,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let _ = element.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
    paint_with_pointer(element, app, cursor)
}

/// One frame of a chip laid out in a tab `width` wide — the tight slot the
/// strip hands every tab — with the pointer at `cursor`.
fn render_tab(
    element: &mut Box<dyn Element>,
    width: f32,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let _ = element.layout(
        SizeConstraint::new(vec2f(width, 0.0), vec2f(width, TOPBAR_HEIGHT)),
        &mut LayoutContext,
        app,
    );
    paint_with_pointer(element, app, cursor)
}

/// The commands of an element already laid out, painted with the pointer at
/// `cursor`.
fn paint_with_pointer(
    element: &mut Box<dyn Element>,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let mut ctx = PaintContext::new(goble_ui::render::Renderer::new());
    if let Some(position) = cursor {
        ctx.cursor_inside = true;
        ctx.cursor_position = position;
    }
    element.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// The origin and width of the run that reads exactly `text`, measured at the
/// size the frame drew it at.
fn run_geometry(
    commands: &[RenderCommand],
    text: &str,
    app: &AppContext,
) -> Option<(Vector2F, f32)> {
    let (origin, font_size) = commands.iter().find_map(|command| match command {
        RenderCommand::DrawText {
            text: drawn,
            origin,
            font_size,
            ..
        } if drawn == text => Some((*origin, *font_size)),
        _ => None,
    })?;
    let width = Text::new(text)
        .with_font_size(font_size)
        .layout(
            SizeConstraint::loose(vec2f(f32::INFINITY, TOPBAR_HEIGHT)),
            &mut LayoutContext,
            app,
        )
        .x;
    Some((origin, width))
}

/// The centre of the close "x" a frame drew, when it drew one at all.
fn close_point(commands: &[RenderCommand]) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawIcon {
            origin, name, size, ..
        } if name == "x-close" => Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0)),
        _ => None,
    })
}

/// The fill of a tab's surface, when the frame drew one: the largest fill it
/// paints. A selected tab also paints its two feet, which are much smaller than
/// its body; a floating tab paints a single pill.
fn surface_fill(commands: &[RenderCommand]) -> goble_ui::ColorU {
    let fills: Vec<(f32, goble_ui::ColorU)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                Some((rect.width() * rect.height(), *color))
            }
            _ => None,
        })
        .collect();
    assert!(!fills.is_empty(), "the tab draws a surface: {fills:?}");
    fills
        .into_iter()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, color)| color)
        .expect("the tab's largest fill")
}

/// R73: a tab carrying a colour tints the fill its own state gives it — the
/// selected tab 60 % over the content's `Surface`, a hovered floating tab 40 %
/// over `Hover`, a pressed one over `Selected`, a resting one 20 % over the
/// bar's `Bg`. A tab with no colour paints the bare theme fill of its state.
#[test]
fn a_coloured_tab_tints_the_fill_its_state_gives_it() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let red = TabColor::Red;
    let inside = vec2f(20.0, TOPBAR_HEIGHT / 2.0);

    let selected = surface_fill(&render_element(
        &mut tab_colored(
            &app,
            true,
            Some(red),
            noop.clone(),
            close_button(&app, noop.clone()),
        ),
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
    ));
    assert_eq!(
        selected,
        red.tint_over(
            app.theme.color(ColorToken::Surface),
            goble_ui::theme::TabTint::Active
        ),
        "the selected tab carries the colour over the content surface"
    );

    let resting = surface_fill(&render_element(
        &mut tab_colored(
            &app,
            false,
            Some(red),
            noop.clone(),
            close_button(&app, noop.clone()),
        ),
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
    ));
    assert_eq!(
        resting,
        red.tint_over(
            app.theme.color(ColorToken::Bg),
            goble_ui::theme::TabTint::Resting
        ),
        "a resting coloured tab keeps the bar's own backdrop under the colour"
    );

    let hovered = surface_fill(&render_with_pointer(
        &mut tab_colored(
            &app,
            false,
            Some(red),
            noop.clone(),
            close_button(&app, noop.clone()),
        ),
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(inside),
    ));
    assert_eq!(
        hovered,
        red.tint_over(
            app.theme.color(ColorToken::Hover),
            goble_ui::theme::TabTint::Hovered
        ),
        "under the pointer the colour is drawn over the hover fill"
    );

    app.pointer_down.set(true);
    let pressed = surface_fill(&render_with_pointer(
        &mut tab_colored(
            &app,
            false,
            Some(red),
            noop.clone(),
            close_button(&app, noop.clone()),
        ),
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(inside),
    ));
    app.pointer_down.set(false);
    assert_eq!(
        pressed,
        red.tint_over(
            app.theme.color(ColorToken::Selected),
            goble_ui::theme::TabTint::Hovered
        ),
        "a press paints the selected tone under the colour"
    );

    // The four shares are four different fills: the state the tab is in is
    // still readable on a coloured tab, which is the whole point of the split.
    assert_ne!(selected, hovered, "selected and hovered are not one shade");
    assert_ne!(hovered, resting, "nor are hovered and resting");
    assert_ne!(pressed, hovered, "nor are pressed and hovered");

    // An untinted tab paints the theme fill itself, unchanged.
    assert_eq!(
        surface_fill(&render_element(
            &mut tab_colored(&app, true, None, noop.clone(), close_button(&app, noop)),
            vec2f(400.0, TOPBAR_HEIGHT),
            &app,
        )),
        app.theme.color(ColorToken::Surface),
        "a tab with no colour draws the pane surface itself"
    );
}

/// R33: the close "x" follows Chrome's visibility rules — the selected tab
/// always carries it, a floating tab only while the pointer is over it, and
/// neither once the tab's contents are too narrow for it. A click on it closes
/// that space and does not select it; a click just beside it selects instead.
#[test]
fn the_close_control_appears_on_hover_and_closes_its_space() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| closed.borrow_mut().push(i)))
    };

    // (a) a floating tab at rest carries no close control: no invisible target.
    let mut floating = tab(
        &app,
        false,
        on_click.clone(),
        close_button(&app, on_close.clone()),
    );
    let away = render_with_pointer(
        &mut floating,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(380.0, TOPBAR_HEIGHT / 2.0)),
    );
    assert!(
        close_point(&away).is_none(),
        "an unhovered floating tab draws no close control"
    );

    // (b) the selected tab carries it even while the pointer is elsewhere.
    let mut element = tab(&app, true, on_click, close_button(&app, on_close));
    let selected_away = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(380.0, TOPBAR_HEIGHT / 2.0)),
    );
    assert!(
        close_point(&selected_away).is_some(),
        "the selected tab keeps its close control always, as Chrome's active tab does"
    );

    // Over the tab: the control is drawn, and a click on it closes that space.
    let hovered = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    let point = close_point(&hovered).expect("a hovered tab draws its close control");
    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: point,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: point,
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }
    assert_eq!(*closed.borrow(), vec![1], "the \"x\" closes its space");
    assert!(
        selected.borrow().is_empty(),
        "a click on the close control must not also select"
    );

    // A click one point outside the 16 pt close button still selects the tab:
    // the control's own hit target is what closes it.
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let noop_close: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, true, on_click, close_button(&app, noop_close));
    let hovered = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    let point = close_point(&hovered).expect("the selected tab draws its close control");
    let beside = vec2f(point.x - 9.0, point.y);
    for event in [
        DispatchedEvent::MouseDown {
            position: beside,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: beside,
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }
    assert_eq!(
        *selected.borrow(),
        vec![1],
        "a click beside the close button selects the tab"
    );

    // (c) contents narrower than Chrome's 68 pt threshold: no control even
    // while hovered.
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut narrow = tab(&app, true, noop.clone(), close_button(&app, noop));
    let narrow_width = MIN_CONTENTS_FOR_CLOSE + TAB_EDGE_INSET * 2.0 - 2.0;
    let commands = render_tab(
        &mut narrow,
        narrow_width,
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    assert!(
        close_point(&commands).is_none(),
        "a tab with {narrow_width} pt of slot has no room for the close control"
    );
}

/// A tab with no hint is the tab it always was: the name and, while the pointer
/// is over it, the close "x" — no shortcut anywhere.
#[test]
fn a_tab_shows_no_shortcut_hint_while_the_modifier_is_up() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, true, noop.clone(), close_button(&app, noop));

    let hovered = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    let drawn = drawn_texts(&hovered);
    assert!(
        !drawn.iter().any(|text| text == &tab_hint(1)),
        "a tab advertises no shortcut with the modifier up: {drawn:?}"
    );
    assert!(
        close_point(&hovered).is_some(),
        "the hovered tab still draws its close control"
    );
}

/// R7x: with the chord modifier held the tab's trailing slot carries the
/// shortcut that selects it, and the close "x" — which shares that slot — is
/// neither drawn nor hit, so the tab never carries two targets at one point.
#[test]
fn a_tab_shows_its_shortcut_hint_in_place_of_the_close_control() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| closed.borrow_mut().push(i)))
    };
    let hint = Text::new(tab_hint(1))
        .with_theme_color(ColorToken::Muted, &app)
        .with_font_size(11.0)
        .with_max_lines(1)
        .finish();
    let mut element = tab_hinted(
        &app,
        true,
        None,
        Some(hint),
        on_click,
        close_button(&app, on_close),
    );

    // The close "x" is drawn on hover; the hint holds the slot instead, so the
    // "x" is gone even with the pointer over the tab.
    let commands = render_tab(
        &mut element,
        200.0,
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    let drawn = drawn_texts(&commands);
    assert!(
        drawn.iter().any(|text| text == &tab_hint(1)),
        "the tab draws its own shortcut: {drawn:?}"
    );
    assert!(
        close_point(&commands).is_none(),
        "the hint takes the close control's slot"
    );

    // The hint stays inside the tab it belongs to: the tab's width accounts for
    // it, so the run cannot hang past the tab's own edge.
    let size = element.size().expect("the tab was laid out");
    let hint_origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == &tab_hint(1) => Some(*origin),
            _ => None,
        })
        .expect("the hint's run");
    let hint_width = Text::new(tab_hint(1))
        .with_font_size(11.0)
        .with_max_lines(1)
        .layout(
            SizeConstraint::loose(vec2f(f32::INFINITY, TOPBAR_HEIGHT)),
            &mut LayoutContext,
            &app,
        )
        .x;
    assert!(
        hint_origin.x >= 0.0 && hint_origin.x + hint_width <= size.x,
        "the hint is drawn inside the tab: {hint_origin:?} + {hint_width} in {size:?}"
    );

    // The name stays where it was: it is centred in the room the "x" leaves,
    // and the hint takes the slot without changing that room.
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut plain = tab(&app, true, noop.clone(), close_button(&app, noop));
    let plain_commands = render_tab(&mut plain, 200.0, &app, None);
    let hinted_name = run_geometry(&commands, "Space 1", &app).expect("the tab's name");
    let plain_name = run_geometry(&plain_commands, "Space 1", &app).expect("the tab's name");
    assert_eq!(
        hinted_name.0.x, plain_name.0.x,
        "the name does not move when the hint takes the close control's slot"
    );

    // And with the hint in the slot, the tab body is still the click target.
    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(20.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(20.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }
    assert_eq!(
        *selected.borrow(),
        vec![1],
        "a press on the tab still selects its space"
    );
    assert!(
        closed.borrow().is_empty(),
        "the close control is not a target while the hint holds its slot"
    );
}

/// A short name sits in the middle of the room its tab leaves it, the way
/// warp-new centres a tab title in the content row (`app/src/tab.rs:1440-1457`):
/// the gap to the inner area's left edge and the gap to its right edge are the
/// same, so an equal-width tab does not carry the name on one side.
#[test]
fn a_short_named_tab_centres_its_name() {
    const WIDTH: f32 = 200.0;
    // `WorkspaceChip`'s own inset inside the floating pill, the gap to the
    // close "x", and the 16 pt button the "x" is drawn in; the room the name
    // may use is between them, inside the pill's edge inset.
    const INSET: f32 = 7.0;
    const GAP: f32 = 4.0;
    const CLOSE: f32 = 16.0;

    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, true, noop.clone(), close_button(&app, noop));
    // The pointer is over the tab so the "x" is drawn; neither it nor the hint
    // moves the name.
    let commands = render_tab(
        &mut element,
        WIDTH,
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );

    let (origin, width) = run_geometry(&commands, "Space 1", &app).expect("the tab's name");
    let inner_left = TAB_EDGE_INSET + INSET;
    let inner_right = WIDTH - TAB_EDGE_INSET - INSET - GAP - CLOSE;
    let left_gap = origin.x - inner_left;
    let right_gap = inner_right - (origin.x + width);
    assert!(
        left_gap > 0.0,
        "a short name does not sit on the tab's left inset: {origin:?} + {width}"
    );
    assert!(
        (left_gap - right_gap).abs() < 1.0,
        "the name is centred in the room the close control leaves: {left_gap} before it and \
         {right_gap} after it, in {inner_left}..{inner_right}"
    );
}

/// The band under the bar is the bar's own `Bg`, not a lighter lip of the
/// content's surface: the bar block's darker background fills every pixel down
/// to the content sheet, and the sheet's `Surface` (the sidebar, and the panes'
/// header band) starts exactly on the band's bottom edge. The selected tab is
/// the one `Surface` fill that crosses that edge, which is what makes its merge
/// read.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_band_under_the_bar_is_the_bars_bg_up_to_the_content_sheet() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);
    let surface = app.theme.color(ColorToken::Surface);
    let bg = app.theme.color(ColorToken::Bg);
    let sheet_top = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;

    // No `Surface` fill starts at the bar's bottom edge across the window: the
    // old full-width lip is gone, so nothing lighter separates the bar from the
    // content.
    let lip = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && (rect.min_y() - TOPBAR_HEIGHT).abs() < 0.01
                && (rect.width() - window().x).abs() < 0.01 =>
        {
            Some(*rect)
        }
        _ => None,
    });
    assert!(lip.is_none(), "no lighter lip runs under the bar: {lip:?}");

    // The band is the window's own `Bg`, full width, exactly the band's rows:
    // the darker background reaches the window outline above it.
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == bg
                && (rect.min_y() - TOPBAR_HEIGHT).abs() < 0.01
                && (rect.height() - TOPBAR_BOTTOM_BAND).abs() < 0.01
                && (rect.width() - window().x).abs() < 0.01 =>
            {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
        .expect("the band under the bar is painted in the bar's own Bg");
    assert_eq!(
        (band.0.min_x(), band.0.min_y()),
        (0.0, TOPBAR_HEIGHT),
        "the band starts at the window's edge, on the bar's bottom row: {:?}",
        band.0
    );
    assert_eq!(band.1, 0.0, "the band is square: it is the bar's own foot");

    // The content sheet begins on the band's bottom edge: the sidebar's surface
    // starts there, and the panes' full-bleed header band is `Surface` from that
    // same row on. Nothing of the lighter sheet shows inside the band.
    let sidebar = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface && rect.min_x() == 0.0 && rect.height() > window().y / 2.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the sidebar paints its surface");
    assert_eq!(
        sidebar.min_y(),
        sheet_top,
        "the content sheet begins on the band's bottom edge: {sidebar:?}"
    );
    assert!(
        sidebar.min_y() > TOPBAR_HEIGHT,
        "so the band under the bar stays the window's own Bg: {sidebar:?}"
    );

    // The selected tab's fill is the sheet's own surface and runs from inside
    // the bar to the pane card's own surface below the sheet's first rows: the
    // bar block, the pane inset and the merge line agree.
    let body = selected_tab_body(&commands, &app);
    assert!(
        body.min_y() < TOPBAR_HEIGHT,
        "the selected fill starts inside the bar: {body:?}"
    );
    assert_eq!(
        body.max_y(),
        tab_merge_bottom(),
        "the fill crosses the band and the pane gutter to the card's surface: {body:?}"
    );
    assert_eq!(
        tab_fillet_bottom(),
        sheet_top,
        "the flare bottoms out on the sheet's top edge"
    );
    assert_eq!(
        tab_merge_bottom(),
        sheet_top + crate::ui::panes::PANE_INSET + 1.0,
        "the bar block, the pane inset and the merge line agree"
    );
    assert!(
        tab_pill_bottom() < TOPBAR_HEIGHT,
        "the floating pills end above the bar's own bottom edge"
    );
}

/// Run a git subcommand in `dir`, panicking when it fails.
fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// A repository with one commit, and either a clean tree or one file changed by
/// one added and one removed line.
fn diff_repository(dirty: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp repo dir");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    std::fs::write(dir.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "initial"]);
    if dirty {
        std::fs::write(dir.path().join("tracked.txt"), "alpha\ndelta\n").unwrap();
    }
    dir
}

/// The runs the topbar band drew, as (text, colour, weight).
fn topbar_runs(commands: &[RenderCommand]) -> Vec<(String, goble_ui::ColorU, FontWeight)> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text,
                origin,
                color,
                font_weight,
                ..
            } if origin.y < TOPBAR_HEIGHT => Some((text.clone(), *color, *font_weight)),
            _ => None,
        })
        .collect()
}

/// "Open changes" is a general-topbar control immediately left of the inbox:
/// it reads the selected pane's own working tree, shows the changed-file count
/// and the +added/-removed totals, and appears only while that pane has changes
/// (a clean tree, or a pane outside a repository, draws nothing).
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_open_changes_control_sits_left_of_the_inbox_for_a_pane_with_changes() {
    if !git_available() {
        return;
    }
    let app = AppContext::default();

    // A clean repository (and a pane before it has a directory at all) draws no
    // control.
    let (mut clean_root, clean_state, _desktop, _dir) = app_root();
    let clean_repo = diff_repository(false);
    let clean_pane = clean_state.borrow().active_pane_id;
    clean_state
        .borrow_mut()
        .set_pane_path(clean_pane, clean_repo.path().display().to_string());
    let clean = render_element(&mut clean_root, window(), &app);
    assert!(
        topbar_icon_center(&clean, "file").is_none(),
        "a clean working tree draws no Open changes control"
    );

    // A dirty repository draws it: one file, +1/-1.
    let (mut root, state, _desktop, _dir) = app_root();
    let repo = diff_repository(true);
    let repo_path = repo.path().display().to_string();
    let pane = state.borrow().active_pane_id;
    state.borrow_mut().set_pane_path(pane, repo_path.clone());
    let commands = render_element(&mut root, window(), &app);

    let file = topbar_icon_center(&commands, "file").expect("the Open changes control");
    let inbox = topbar_icon_center(&commands, "inbox-01").expect("the inbox control");
    let settings = topbar_icon_center(&commands, "settings").expect("the settings control");
    assert!(
        file.x < inbox.x && inbox.x < settings.x,
        "the control sits immediately left of the inbox: {file:?} / {inbox:?} / {settings:?}"
    );

    let runs = topbar_runs(&commands);
    let added = runs
        .iter()
        .find(|(text, ..)| text == "+1")
        .expect("the added lines are drawn");
    assert_eq!(
        added.1,
        app.theme.color(ColorToken::Success),
        "additions read in the success colour"
    );
    let removed = runs
        .iter()
        .find(|(text, ..)| text == "-1")
        .expect("the removed lines are drawn");
    assert_eq!(
        removed.1,
        app.theme.color(ColorToken::Error),
        "removals read in the error colour"
    );
    let count = runs
        .iter()
        .find(|(text, ..)| *text == "1")
        .expect("the changed-file count is drawn");
    assert_eq!(
        count.1,
        app.theme.color(ColorToken::Text),
        "the count reads in the text colour"
    );
    assert!(
        runs.iter().any(|(text, ..)| text == "•"),
        "the count and the totals are separated by the dot: {runs:?}"
    );

    // Hovering it lights the control with the bar's hover fill (no border).
    let hovered = render_with_cursor(&mut root, window(), &app, Some(file));
    assert!(
        hovered.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { rect, color, corner_radius }
                if *color == app.theme.color(ColorToken::Hover)
                    && rect.min_y() < TOPBAR_HEIGHT
                    && (*corner_radius - 6.0).abs() < 0.01
        )),
        "the control takes the bar's hover fill"
    );

    // A click opens the review pane on this pane's directory, the action the
    // composer's old diff control sent.
    for event in [
        DispatchedEvent::MouseDown {
            position: file,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: file,
            button: 0,
        },
    ] {
        root.dispatch_event(&event, &mut EventContext, &app);
    }
    let _ = render_element(&mut root, window(), &app);
    let state = state.borrow();
    let mut leaves = Vec::new();
    if let Some(space) = state.spaces.get(state.active_space) {
        space.root.leaves(&mut leaves);
    }
    assert!(
        leaves.iter().any(|(_, kind)| matches!(
            kind,
            PaneKind::Diff { cwd } if *cwd == repo_path
        )),
        "the click opens the review pane on the pane's directory: {leaves:?}"
    );
}

/// One whole-app frame with the pointer at `cursor`. The tree is rebuilt by the
/// layout pass, exactly as the app rebuilds it every frame, so the state the
/// caller set is what the frame draws.
fn render_with_cursor(
    root: &mut Box<dyn Element>,
    size: Vector2F,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let _ = root.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
    paint_with_pointer(root, app, cursor)
}

/// The pane surface under the topbar has no seam: the old 1 pt horizontal rule
/// is gone, and the selected workspace tab's fill runs from inside the bar,
/// across the [`TOPBAR_BOTTOM_BAND`], one point into the content sheet. The band
/// is what makes the merge read: the tab stands above the bar while the content
/// sheet starts on the band's bottom edge.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_pane_surface_under_the_selected_tab_has_no_seam() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);

    let border = app.theme.color(ColorToken::Border);
    let divider: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == border
                    && (rect.height() - 1.0).abs() < 0.01
                    && rect.min_y() == TOPBAR_HEIGHT
                    && rect.width() >= window().x =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert!(
        divider.is_empty(),
        "no rule runs under the topbar any more: {divider:?}"
    );

    let body = selected_tab_body(&commands, &app);
    assert!(
        body.min_y() < TOPBAR_HEIGHT,
        "the selected fill starts inside the bar: {body:?}"
    );
    assert!(
        body.max_y() > TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "and crosses the band between the bar and the content: {body:?}"
    );
    assert_eq!(
        body.max_y(),
        tab_merge_bottom(),
        "the fill ends where the merge line is: {body:?}"
    );

    // The panel surface's `Surface` begins on the body's own top edge: the
    // pane area paints the `PANE_AREA_INSET` frame itself, so there is no dark
    // strip between the band and the panes. The selected tab's fill crosses the
    // band and lands inside that surface, so the two surfaces meet with no gap.
    let surface = app.theme.color(ColorToken::Surface);
    let pane = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && rect.min_y() > TOPBAR_HEIGHT
                    && rect.height() > 20.0
                    && rect.width() > window().x / 2.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.min_y().total_cmp(&b.min_y()))
        .expect("the pane's surface is painted");
    assert_eq!(
        pane.min_y(),
        TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "the panel surface starts on the body origin, its inset frame painted: {pane:?}"
    );
    assert_eq!(
        pane.min_x(),
        crate::ui::SIDEBAR_WIDTH,
        "and on the sidebar's edge, its inset frame painted: {pane:?}"
    );
    assert_eq!(
        body.max_y(),
        pane.min_y() + crate::ui::panes::PANE_AREA_INSET + 1.0,
        "the selected fill crosses the panel inset and reaches into the panel surface: \
         {body:?} against {pane:?}"
    );
}

/// The band under the bar is the bar's own `Bg`: the shell body starts
/// `TOPBAR_BOTTOM_BAND` below the topbar, so the strip of window background is
/// visible under the floating tabs — and the bar itself keeps its height.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_shell_content_starts_one_band_below_the_bar() {
    let (mut root, _state, _desktop, _dir) = app_root();
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);
    let surface = app.theme.color(ColorToken::Surface);

    // The sidebar is the body's first surface: it starts at the body top, one
    // band below the bar, and the bar's own fill stays `TOPBAR_HEIGHT` tall.
    let sidebar = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface && rect.min_x() == 0.0 && rect.height() > window().y / 2.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the sidebar paints its surface");
    assert_eq!(
        sidebar.min_y(),
        TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "the body starts one band below the bar: {sidebar:?}"
    );
    assert!(
        sidebar.min_y() > TOPBAR_HEIGHT,
        "so the band under the bar is the window's own Bg: {sidebar:?}"
    );
}

/// Floating tabs float: a 6 pt gap between the selected fill and a neighbour's
/// pill at the top and the feet reaching a fillet radius past the selected slot
/// at the merge line. The neighbouring tab is hovered so it paints at all, and
/// it paints *last*: the owner wants a hovered pill never bitten by the selected
/// tab's inverted-corner discs, while the selected tab keeps its merge into the
/// content sheet.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn floating_tabs_float_with_gaps_and_a_hovered_pill_paints_last() {
    let (mut root, state, _desktop, _dir) = app_root();
    workspace_of_tabs(&state, 3);
    let app = AppContext::default();

    // The first tab is selected, so its fill is the one `Surface` body of the
    // bar; its width is the slot less the pill's edge insets.
    let plain = render_element(&mut root, window(), &app);
    let first = selected_tab_body(&plain, &app);
    let slot = first.width() + TAB_EDGE_INSET * 2.0;

    // Select the middle tab and hover the third: the selected fill moves one
    // slot right and the hovered floating tab paints its pill.
    state.borrow_mut().active_space = 1;
    let third_center = vec2f(
        first.min_x() - TAB_EDGE_INSET + slot * 2.5,
        TOPBAR_HEIGHT / 2.0,
    );
    let commands = render_with_cursor(&mut root, window(), &app, Some(third_center));
    let body = selected_tab_body(&commands, &app);
    assert!(
        (body.min_x() - (first.min_x() + slot)).abs() < 0.01,
        "the second tab's fill begins one slot after the first: {} against {}",
        body.min_x(),
        first.min_x() + slot
    );

    let hover = app.theme.color(ColorToken::Hover);
    let pill = commands
        .iter()
        .find_map(|command| match command {
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
        .expect("the hovered floating tab paints its pill");
    assert_eq!(
        (pill.0.min_y(), pill.0.max_y()),
        (TAB_TOP_INSET, tab_pill_bottom()),
        "the pill floats inside the bar"
    );
    assert!(
        pill.0.max_y() < TOPBAR_HEIGHT,
        "a floating pill ends above the band, leaving the Bg strip visible under it: {:?}",
        pill.0
    );
    assert!(
        body.max_y() > TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "the selected tab crosses the band into the content: {body:?}"
    );
    assert!(
        (pill.1 - tab_top_radius(pill.0.width())).abs() < 0.01,
        "the pill's corners are the clamped top radius"
    );
    assert_eq!(
        pill.0.min_x(),
        body.max_x() + 6.0,
        "6 pt between the selected fill and the neighbouring pill at the top"
    );

    // The foot band widens the fill a fillet radius past the body on each side
    // at the merge line, and a background disc bites each outer top corner: the
    // concave (inverted) fillet that bends the tab into the surface.
    let surface = app.theme.color(ColorToken::Surface);
    let background = app.theme.color(ColorToken::Bg);
    let slot_left = body.min_x() - TAB_EDGE_INSET;
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && (rect.height() - TAB_FLARE_RADIUS).abs() < 0.01
                    && (rect.max_y() - tab_fillet_bottom()).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the selected tab widens at the fillet line");
    assert!(
        (band.min_x() - (slot_left - 9.0)).abs() < 0.01,
        "the fill reaches 9 pt past its slot at the merge line: {band:?}"
    );
    assert_eq!(
        band.max_x(),
        body.max_x() + TAB_FLARE_RADIUS,
        "and the same on the right: {band:?}"
    );
    let fillets: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == background && (rect.width() - TAB_FLARE_RADIUS * 2.0).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        band.max_y(),
        tab_fillet_bottom(),
        "the widened part ends at the band's bottom edge: {band:?}"
    );
    assert_eq!(
        fillets.len(),
        2,
        "one background disc per inverted corner: {fillets:?}"
    );
    assert_eq!(
        fillets[0].min_x(),
        body.min_x() - TAB_FLARE_RADIUS * 2.0,
        "the left fillet disc is centred on the band's outer top corner"
    );
    assert_eq!(
        fillets[1].min_x(),
        body.max_x(),
        "the right fillet disc is centred on the band's outer top corner"
    );

    // Painted last: the neighbour's hovered pill sits above the selected body and
    // above its inverted-corner discs — a hovered pill is never bitten. The
    // selected tab still merges: its body is painted after the *rest* of the
    // neighbours, and nothing covers it once the pointer leaves.
    let body_index = commands
        .iter()
        .position(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                *color == surface && (rect.max_y() - tab_merge_bottom()).abs() < 0.01
            }
            _ => false,
        })
        .expect("the selected body");
    let disc_index = commands
        .iter()
        .position(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                *color == background && (rect.width() - TAB_FLARE_RADIUS * 2.0).abs() < 0.01
            }
            _ => false,
        })
        .expect("the selected tab's inverted-corner disc");
    let pill_index = commands
        .iter()
        .position(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                *color == hover && (rect.max_y() - tab_pill_bottom()).abs() < 0.01
            }
            _ => false,
        })
        .expect("the hovered pill");
    assert!(
        pill_index > body_index,
        "the hovered pill is painted after the selected body: pill {pill_index}, body {body_index}"
    );
    assert!(
        pill_index > disc_index,
        "and after the selected tab's inverted-corner fills: pill {pill_index}, disc {disc_index}"
    );
}

/// A floating tab's hover pill: one rounded fill inside the slot's side gaps,
/// and a pointer in the 3 pt gap still hovers *this* tab — Chrome hit-tests the
/// whole bounds, and the gap belongs to the tab it is drawn in.
#[test]
fn a_hovered_floating_tab_paints_a_pill_inside_its_slot() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, false, noop.clone(), close_button(&app, noop));
    let width = 180.0;
    let commands = render_tab(
        &mut element,
        width,
        &app,
        Some(vec2f(1.0, TOPBAR_HEIGHT / 2.0)),
    );

    let pill = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::Hover) => Some((*rect, *corner_radius)),
            _ => None,
        })
        .expect("a pointer inside the slot's edge gap still hovers the tab");
    assert_eq!(
        (
            pill.0.min_x(),
            pill.0.width(),
            pill.0.min_y(),
            pill.0.max_y()
        ),
        (
            TAB_EDGE_INSET,
            width - TAB_EDGE_INSET * 2.0,
            TAB_TOP_INSET,
            tab_pill_bottom()
        ),
        "the pill is the slot inset by 3 pt at the sides, floating inside the bar"
    );
    assert!(
        (pill.1 - tab_top_radius(pill.0.width())).abs() < 0.01,
        "the pill's top radius is the clamped one"
    );
}

/// The band makes the two tab shapes read apart: a floating pill ends above the
/// strip (its bottom keeps the old bar-bottom relation), while the selected
/// tab's fill crosses the whole band to the surface below.
#[test]
fn floating_pills_end_above_the_band_and_the_selected_tab_crosses_it() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));

    let mut floating = tab(&app, false, noop.clone(), close_button(&app, noop.clone()));
    let commands = render_tab(
        &mut floating,
        180.0,
        &app,
        Some(vec2f(90.0, TOPBAR_HEIGHT / 2.0)),
    );
    let pill = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Hover) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the hovered floating tab paints its pill");
    assert_eq!(
        pill.max_y(),
        tab_pill_bottom(),
        "the pill bottom keeps its relation to the bar's own bottom edge"
    );
    assert_eq!(
        pill.max_y(),
        TOPBAR_HEIGHT - 1.0,
        "the pill keeps the old bar-bottom relation, 1 pt clear of the bar"
    );
    assert!(
        pill.max_y() < TOPBAR_HEIGHT,
        "so the pill ends inside the bar and the band shows under it: {pill:?}"
    );
    assert!(
        (tab_merge_bottom() - pill.max_y() - (TOPBAR_BOTTOM_BAND + 6.0)).abs() < 0.01,
        "and sits the band plus Chrome's 6 pt gap above the merge line"
    );

    let mut selected = tab(&app, true, noop.clone(), close_button(&app, noop));
    let commands = render_tab(&mut selected, 180.0, &app, None);
    let body = surface_fill_rect(&commands);
    assert_eq!(
        body.max_y(),
        tab_merge_bottom(),
        "the selected fill reaches the merge line below the band: {body:?}"
    );
    assert!(
        body.max_y() > TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND,
        "which is past the band into the content: {body:?}"
    );
}

/// The rectangle of the largest fill a frame drew — a tab's own surface.
fn surface_fill_rect(commands: &[RenderCommand]) -> RectF {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        .expect("the tab paints its surface")
}

/// A press on a floating tab paints the selected tone; a press on the selected
/// tab changes nothing (Chrome's active tab ignores hover and press).
#[test]
fn a_pressed_floating_tab_paints_the_selected_tone() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    app.pointer_down.set(true);
    let mut floating = tab(&app, false, noop.clone(), close_button(&app, noop.clone()));
    let commands = render_tab(
        &mut floating,
        180.0,
        &app,
        Some(vec2f(90.0, TOPBAR_HEIGHT / 2.0)),
    );
    assert_eq!(
        surface_fill(&commands),
        app.theme.color(ColorToken::Selected),
        "a press paints the selected tone"
    );

    let mut selected = tab(&app, true, noop.clone(), close_button(&app, noop));
    let commands = render_tab(
        &mut selected,
        180.0,
        &app,
        Some(vec2f(90.0, TOPBAR_HEIGHT / 2.0)),
    );
    app.pointer_down.set(false);
    assert_eq!(
        surface_fill(&commands),
        app.theme.color(ColorToken::Surface),
        "the selected tab keeps its surface under a press"
    );
}

/// The strip draws no vertical rules at all: not between tabs, and not around
/// them — the bar's `Bg` and the floating pills' own gaps are the separation.
#[test]
fn the_topbar_draws_no_vertical_rules() {
    let (mut root, state, _desktop, _dir) = app_root();
    workspace_of_tabs(&state, 3);
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);

    let rules: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, .. }
                if rect.width() == 1.0
                    && (rect.height() - TOPBAR_HEIGHT).abs() < 0.5
                    && rect.min_y() < TOPBAR_HEIGHT =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert!(
        rules.is_empty(),
        "the tab strip draws no vertical rules: {rules:?}"
    );
}

/// A crowded strip clamps each tab's top radius, and a fill narrower than the
/// two feet keeps its own rounded bottom corners instead of notching.
#[test]
fn a_narrow_tab_clamps_its_radius_and_drops_the_feet() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));

    for slot in [200.0, 40.0] {
        let content = slot - TAB_EDGE_INSET * 2.0;
        let mut element = tab(&app, true, noop.clone(), close_button(&app, noop.clone()));
        let commands = render_tab(&mut element, slot, &app, None);
        let radius = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                } if rect.width() > 20.0 => Some(*corner_radius),
                _ => None,
            })
            .expect("the selected body");
        assert!(
            (radius - tab_top_radius(content)).abs() < 0.01,
            "slot {slot}: radius {radius} against the clamped {}",
            tab_top_radius(content)
        );
        if content > TAB_TOP_RADIUS {
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, RenderCommand::ClipRect(_))),
                "slot {slot} is wide enough for the feet"
            );
        }
    }

    // 30 pt: 24 pt of content is narrower than the two feet (24 + 4), so the
    // body keeps its own rounded bottom corners and nothing is clipped.
    let mut element = tab(
        &app,
        true,
        noop,
        close_button(&app, Rc::new(RefCell::new(|_| {}))),
    );
    let commands = render_tab(&mut element, 30.0, &app, None);
    let fills = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
        .count();
    assert_eq!(fills, 1, "no feet on a fill too narrow to notch");
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::ClipRect(_))),
        "and nothing to clip"
    );
}

/// A coloured selected tab still merges: its fill is the tinted content surface
/// and still runs past the bar's bottom edge with both feet.
#[test]
fn a_coloured_selected_tab_still_merges() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let red = TabColor::Red;
    let mut element = tab_colored(
        &app,
        true,
        Some(red),
        noop.clone(),
        close_button(&app, noop),
    );
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);

    let tinted = red.tint_over(
        app.theme.color(ColorToken::Surface),
        goble_ui::theme::TabTint::Active,
    );
    let body = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == tinted && rect.width() > 20.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("a coloured selected tab is filled with its tint");
    assert_eq!(body.max_y(), tab_merge_bottom(), "and still merges");
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == tinted
                    && (rect.height() - TAB_FLARE_RADIUS).abs() < 0.01
                    && (rect.max_y() - tab_fillet_bottom()).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the foot band paints in the tinted fill too");
    assert_eq!(
        band.width(),
        body.width() + TAB_FLARE_RADIUS * 2.0,
        "the tinted fill widens into the content plane: {band:?}"
    );
    let background = app.theme.color(ColorToken::Bg);
    let fillets = commands
        .iter()
        .filter(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                *color == background && (rect.width() - TAB_FLARE_RADIUS * 2.0).abs() < 0.01
            }
            _ => false,
        })
        .count();
    assert_eq!(fillets, 2, "the inverted corners are the bar's own tone");
}

/// While the selected tab is renamed, the field replaces the label and the
/// close control but not the merge: the tab's surface and feet still paint
/// behind it.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_rename_field_keeps_the_selected_tabs_merge() {
    let (mut root, state, _desktop, _dir) = app_root();
    state.borrow_mut().space_rename_editing = true;
    let app = AppContext::default();
    let commands = render_element(&mut root, window(), &app);

    let body = selected_tab_body(&commands, &app);
    assert_eq!(
        body.max_y(),
        tab_merge_bottom(),
        "the selected fill still reaches the content while renaming: {body:?}"
    );
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "Workspace name"),
        "the inline rename field is drawn: {:?}",
        drawn_texts(&commands)
    );
}

/// The window the shell tests lay the whole app out in.
fn window() -> Vector2F {
    vec2f(1024.0, 768.0)
}

/// The whole app over a real store, with the first-run overlays out of the way,
/// plus its live state, the service handle and the temp dir the store owns. The
/// tab labels are read off a real frame (`RootView::layout` rebuilds the tree
/// the topbar draws from), so these tests assert the drawn label, not a
/// helper's return value.
fn app_root() -> (
    Box<dyn Element>,
    Rc<RefCell<UiState>>,
    Arc<DesktopState>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        // Keep the bar deterministic: a pane in the checkout would show the
        // real working tree's Open-changes control.
        let cwd = dir.path().display().to_string();
        let active = s.active_pane_id;
        s.set_pane_path(active, cwd);
    }
    (Box::new(view), state, desktop, dir)
}

/// A pane session at `path` that owns no conversation of its own.
fn pane_at(path: &str) -> PaneSession {
    PaneSession {
        conversation_id: String::new(),
        draft: String::new(),
        path: path.to_string(),
        daemon_session_id: String::new(),
    }
}

/// A floating space draws every pane as an OS window: each one has its own
/// titlebar above the content, so the window titles are drawn once per pane and
/// the window controls exist for each. This is the whole app's frame, not a
/// helper's output, so it proves the chrome reached the screen.
#[test]
fn floating_panes_draw_their_own_window_chrome() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        let mut next = 8;
        s.spaces[0]
            .split(7, SplitDir::Horizontal, &mut next)
            .expect("the pane splits");
        s.spaces[0].set_layout(LayoutKind::Floating);
        s.active_space = 0;
        s.active_pane_id = 7;
        s.next_pane_id = next;
    }

    let commands = render_element(&mut root, window(), &app);
    let titles = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == "Terminal"))
        .count();
    assert_eq!(
        titles, 2,
        "each floating window draws its own titlebar: {commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "minimize-01")),
        "the window carries a minimize control"
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "maximize-01")),
        "the window carries a maximize control"
    );
}

/// A BSP pane is a tab in the panel: it wears no outline — only its own card
/// `Border` edge — and the focused pane is marked by the accent corner triangle
/// drawn over the card's top-left corner. It casts no soft shadow: the gutter
/// around it is the panel surface, not a darkened one.
#[test]
fn bsp_panes_cast_the_window_frame() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
    }

    let commands = render_element(&mut root, window(), &app);
    let accent = app.theme.color(ColorToken::Accent);
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == accent)),
        "the panel does not outline its tabs: {commands:?}"
    );
    let shadow = goble_ui::ColorU::new(0, 0, 0, 72);
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == shadow)),
        "a tiled tab casts no shadow: its gutter is the panel surface: {commands:?}"
    );
    // The focused pane's accent corner marker, over its card's top-left corner.
    let card = {
        let s = state.borrow();
        let bounds = s.pane_bounds.borrow();
        *bounds.get(&7).expect("the pane recorded its bounds")
    };
    let marker = commands.iter().find_map(|c| match c {
        RenderCommand::DrawIcon {
            origin, name, color, ..
        } if name == "upper-left-triangle" && *color == accent => Some(*origin),
        _ => None,
    });
    let marker = marker.expect("the focused pane wears its accent corner marker");
    assert!(
        (marker.x - card.min_x()).abs() < 0.01 && (marker.y - card.min_y()).abs() < 0.01,
        "the accent corner sits on the pane card's top-left corner: {marker:?} against {card:?}"
    );
}

/// A press on a floating window's titlebar starts a move, and the pointer
/// carries the window with it until the release: the rect the space stores is
/// the one the drag ended on. This is the OS-window gesture, through the whole
/// app's frame.
#[test]
fn dragging_a_window_titlebar_moves_the_floating_pane() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.spaces[0].set_layout(LayoutKind::Floating);
        s.active_space = 0;
        s.active_pane_id = 7;
    }

    let commands = render_element(&mut root, window(), &app);
    let title = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { text, origin, .. } if text == "Terminal" => Some(*origin),
            _ => None,
        })
        .expect("the window titlebar is drawn");
    let before = state.borrow().spaces[0]
        .floating_rect(7)
        .expect("the floating pane has a rect");

    let press = title + vec2f(12.0, 6.0);
    let mut ctx = EventContext;
    assert!(root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: press,
            button: 0,
        },
        &mut ctx,
        &app,
    ));
    assert!(root.dispatch_event(
        &DispatchedEvent::MouseMove {
            position: press + vec2f(120.0, 90.0),
        },
        &mut ctx,
        &app,
    ));
    assert!(root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: press + vec2f(120.0, 90.0),
            button: 0,
        },
        &mut ctx,
        &app,
    ));

    let after = state.borrow().spaces[0]
        .floating_rect(7)
        .expect("the pane is still floating");
    assert!(
        after.x > before.x + 0.05,
        "the window moved right: {before:?} -> {after:?}"
    );
    assert!(
        after.y > before.y + 0.03,
        "the window moved down: {before:?} -> {after:?}"
    );
    assert!(
        (after.w - before.w).abs() < 1e-6 && (after.h - before.h).abs() < 1e-6,
        "a move does not resize the window: {before:?} -> {after:?}"
    );
}

/// Minimizing a floating window takes it off the pane area and leaves a chip in
/// the dock below it, so the space keeps a visible way back to the pane.
#[test]
fn a_minimized_window_waits_in_the_dock() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        let mut next = 8;
        let second = s.spaces[0]
            .split(7, SplitDir::Horizontal, &mut next)
            .expect("the pane splits");
        s.spaces[0].set_layout(LayoutKind::Floating);
        s.spaces[0].minimize_pane(7);
        s.active_space = 0;
        s.active_pane_id = second;
        s.next_pane_id = next;
    }

    let commands = render_element(&mut root, window(), &app);
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "Minimized"),
        "the dock names itself: {:?}",
        drawn_texts(&commands)
    );
    // Only the window that is not docked carries the minimize control; the
    // docked one is the chip beside "Minimized" instead.
    let minimized_controls = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "minimize-01"))
        .count();
    assert_eq!(
        minimized_controls, 1,
        "the docked window is not drawn on the pane area: {commands:?}"
    );
}

/// The carousel is built from the same window frame: every pane is drawn, and
/// the focused one is the window the fan centers.
#[test]
fn carousel_draws_every_pane_as_a_window() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        let mut next = 8;
        let second = s.spaces[0]
            .split(7, SplitDir::Horizontal, &mut next)
            .expect("the pane splits");
        s.spaces[0]
            .split(second, SplitDir::Horizontal, &mut next)
            .expect("the second pane splits");
        s.spaces[0].set_layout(LayoutKind::Carousel);
        s.active_space = 0;
        s.active_pane_id = second;
        s.next_pane_id = next;
    }

    let commands = render_element(&mut root, window(), &app);
    let titles = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == "Terminal"))
        .count();
    assert_eq!(
        titles, 3,
        "every carousel window draws its own titlebar: {commands:?}"
    );
}

/// Every text run drawn in the topbar band, which is where the tab labels live.
/// Scoped to the bar because the same string is also drawn lower down (the
/// composer's working-directory pill shows the same shortened path).
fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The origin of the topbar run that reads exactly `label`.
fn topbar_label_origin(commands: &[RenderCommand], label: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { text, origin, .. }
            if text == label && origin.y < TOPBAR_HEIGHT =>
        {
            Some(*origin)
        }
        _ => None,
    })
}

/// The centre of the topbar icon drawn from `name`, which is the control that
/// icon sits in.
fn topbar_icon_center(commands: &[RenderCommand], name: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
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

/// The global inbox sits at the right end of the bar beside Settings, past the
/// tab strip and its "+ ▾" control — not next to the sidebar toggle it used to
/// flank.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn the_inbox_sits_beside_settings_at_the_right_end_of_the_bar() {
    let app = AppContext::default();
    let (mut root, _state, _desktop, _dir) = app_root();
    let commands = render_element(&mut root, window(), &app);

    let toggle = topbar_icon_center(&commands, "menu-01").expect("the sidebar toggle");
    let add = topbar_icon_center(&commands, "chevron-down").expect("the environment menu");
    let inbox = topbar_icon_center(&commands, "inbox-01").expect("the inbox control");
    let settings = topbar_icon_center(&commands, "settings").expect("the settings control");

    assert!(
        toggle.x < add.x,
        "the sidebar toggle leads the bar: {toggle:?} against {add:?}"
    );
    assert!(
        inbox.x > add.x,
        "the inbox is past the tab strip and its controls: {inbox:?} against {add:?}"
    );
    assert!(
        inbox.x < settings.x,
        "the inbox sits before Settings: {inbox:?} against {settings:?}"
    );
    // The right group is packed: one `group_spacing` between two topbar
    // buttons puts their centres `button + spacing` apart, and Settings sits
    // on the bar's right edge.
    let sm = app.theme.spacing_px(goble_ui::theme::SpacingToken::Sm);
    let packed = TOPBAR_ICON_BUTTON_SIZE + sm;
    assert!(
        (settings.x - inbox.x - packed).abs() < 0.5,
        "the inbox is Settings' immediate neighbour, {packed} points apart: {inbox:?} against \
         {settings:?}"
    );
    assert!(
        window().x - settings.x < 100.0,
        "Settings is the bar's last control on its right edge: {settings:?} in a {:?}-wide \
         window",
        window()
    );

    // In draw order the bar ends with the two of them: nothing stands between
    // the inbox and Settings.
    let icons: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        &icons[icons.len().saturating_sub(2)..],
        ["inbox-01", "settings"],
        "the inbox is the control right before Settings: {icons:?}"
    );
}

/// One key through the whole app tree, after a frame — so the field that reads
/// its own value was built from the last frame's state, as it is in the app.
fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) {
    let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
    let mut paint_ctx = PaintContext::default();
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext;
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        },
        &mut ctx,
        app,
    );
}

/// A full press+release at `pos`, with a frame between the two events — the way
/// the running app rebuilds the tree between them. The double-click window is
/// therefore read across frames, which is where it has to hold.
fn click(root: &mut Box<dyn Element>, app: &AppContext, pos: Vector2F) {
    let mut ctx = EventContext;
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: pos,
            button: 0,
        },
        &mut ctx,
        app,
    );
    let _ = render_element(root, window(), app);
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: pos,
            button: 0,
        },
        &mut ctx,
        app,
    );
}

/// A tab holding one terminal pane is named by that pane's working directory,
/// read the way the `~` symbol means it — the same shortening the composer's
/// path pill uses — and follows the pane when it moves. A tab opened on a path
/// of its own reads that path.
#[ignore = "chrome-panels: asserted the retired per-pane header/general-topbar structure"]
#[test]
fn a_terminal_tab_is_named_by_its_working_directory() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~"),
        "a terminal in the home directory is labelled ~: {drawn:?}"
    );

    state
        .borrow_mut()
        .set_pane_path(7, format!("{home}/Projects/goble"));
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~/Projects/goble"),
        "a directory under the home directory is labelled under ~: {drawn:?}"
    );

    state
        .borrow_mut()
        .set_pane_path(7, "/tmp/elsewhere".to_string());
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "/tmp/elsewhere"),
        "a tab opened on another path reads that path: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "~"),
        "the old directory is gone from the tab: {drawn:?}"
    );
}

/// A tab holding an agent pane reads `New Agent` while its conversation has no
/// subject, and the conversation's subject — the chat's title — once it has one.
#[ignore = "chrome-panels: asserted the retired per-pane header/general-topbar structure"]
#[test]
fn an_agent_tab_reads_new_agent_until_its_conversation_has_a_subject() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    let fresh = desktop
        .create_chat(NEW_CONVERSATION_TITLE, None, None)
        .expect("a conversation nobody has named");
    let subject = desktop
        .create_chat("Demo · Hot reload", None, None)
        .expect("a conversation with a subject");
    {
        let mut s = state.borrow_mut();
        s.refresh_conversations(&desktop);
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Chat,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        let mut session = pane_at("");
        session.conversation_id = fresh.clone();
        s.pane_sessions.insert(7, session);
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "New Agent"),
        "an agent with no conversation subject reads New Agent: {drawn:?}"
    );

    state
        .borrow_mut()
        .pane_sessions
        .get_mut(&7)
        .expect("the pane's session")
        .conversation_id = subject.clone();
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "Demo · Hot reload"),
        "the tab takes the conversation's subject: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "New Agent"),
        "the placeholder is gone once there is a subject: {drawn:?}"
    );
}

/// A tab holding several panes — a terminal and an agent split — reads as its
/// focused pane, so the tab names whatever the user is working in.
#[ignore = "chrome-panels: asserted the retired per-pane header/general-topbar structure"]
#[test]
fn a_tab_with_several_panes_reads_as_its_focused_pane() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    let subject = desktop
        .create_chat("Demo · Hot reload", None, None)
        .expect("a conversation with a subject");
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.refresh_conversations(&desktop);
        s.spaces = vec![Space::unnamed(Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            first: Box::new(Pane::Leaf {
                id: 7,
                kind: PaneKind::Terminal,
            }),
            second: Box::new(Pane::Leaf {
                id: 8,
                kind: PaneKind::Chat,
            }),
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
        let mut session = pane_at("");
        session.conversation_id = subject.clone();
        s.pane_sessions.insert(8, session);
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~"),
        "with the terminal focused the tab names its directory: {drawn:?}"
    );

    state.borrow_mut().active_pane_id = 8;
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "Demo · Hot reload"),
        "with the agent focused the tab names its subject: {drawn:?}"
    );
}

/// A tab the user renamed by hand keeps the name they typed, for good: the
/// label derived from what the tab holds never takes it back.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn a_tab_the_user_renamed_keeps_the_typed_name() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
    }

    let commands = render_element(&mut root, window(), &app);
    let chip = topbar_label_origin(&commands, "~").expect("the tab reads its directory");

    // Double-click the tab: the inline field opens on the label it draws now.
    // Both clicks go through a real frame, which is where the app's
    // double-click window is read.
    let spot = vec2f(chip.x + 4.0, chip.y + 6.0);
    click(&mut root, &app, spot);
    click(&mut root, &app, spot);
    {
        let s = state.borrow();
        assert!(
            s.space_rename_editing,
            "the second click opens the rename field"
        );
        assert_eq!(
            s.space_rename_draft, "~",
            "the field opens on the label the tab draws"
        );
    }

    // Clear the seeded label, type the user's name and commit with Return.
    press(&mut root, &app, "Backspace");
    for key in ["p", "r", "o", "i", "e", "c", "t"] {
        press(&mut root, &app, key);
    }
    press(&mut root, &app, "Enter");

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "proiect"),
        "the typed name is drawn: {drawn:?}"
    );

    // The pane moving elsewhere does not take the name back.
    state
        .borrow_mut()
        .set_pane_path(7, "/tmp/elsewhere".to_string());
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "proiect"),
        "the typed name survives a move: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "/tmp/elsewhere"),
        "the derived label does not come back over the typed name: {drawn:?}"
    );
}

/// Three tabs as the strip's own registry reports them after a paint: 60 pt
/// wide, side by side, so their centers are 130, 190 and 250.
fn three_drawn_tabs() -> Vec<RectF> {
    [
        rectf(100.0, 0.0, 60.0, 20.0),
        rectf(160.0, 0.0, 60.0, 20.0),
        rectf(220.0, 0.0, 60.0, 20.0),
    ]
    .to_vec()
}

/// The insertion index a pane dropped on the strip resolves to is counted off
/// the drawn tabs, the way the tab reorder counts them: left of the strip is
/// before the first tab, right of it is after the last one, and a pointer
/// between two tabs names the boundary it has passed.
#[test]
fn the_drop_index_counts_the_drawn_tab_centers_left_of_the_pointer() {
    let tabs = three_drawn_tabs();

    assert_eq!(
        insertion_index(&tabs, 0.0),
        0,
        "left of the strip: before the first tab"
    );
    assert_eq!(
        insertion_index(&tabs, 140.0),
        1,
        "just past the first tab's center"
    );
    assert_eq!(
        insertion_index(&tabs, 200.0),
        2,
        "past the second tab's center, before the third"
    );
    assert_eq!(
        insertion_index(&tabs, 400.0),
        3,
        "right of the strip: after the last tab"
    );
    assert_eq!(
        insertion_index(&tabs, 99.0),
        0,
        "just left of the strip is still before every tab"
    );
    assert_eq!(
        insertion_index(&[], 42.0),
        0,
        "no tabs, no other place to go"
    );
}

/// A tab drag names the place the dragged tab belongs at: how many of the
/// *other* tabs' centers the pointer has passed. The dragged tab is left out of
/// the count, so the number is the tab's own new index, and it never decreases
/// as the pointer moves right — a drag follows the pointer instead of swapping
/// back and forth around a midpoint.
#[test]
fn a_tab_drag_names_the_place_it_has_dragged_the_tab_to() {
    let tabs = three_drawn_tabs();
    let from = 0;
    assert_eq!(
        reorder_target_index(&tabs, from, 0.0),
        0,
        "left of the strip"
    );
    assert_eq!(
        reorder_target_index(&tabs, from, 140.0),
        0,
        "not past another tab's center yet"
    );
    assert_eq!(
        reorder_target_index(&tabs, from, 200.0),
        1,
        "past the second tab's center"
    );
    assert_eq!(
        reorder_target_index(&tabs, from, 300.0),
        2,
        "past the third's, and no further: the dragged tab is not counted"
    );
    assert_eq!(
        reorder_target_index(&tabs, from, 400.0),
        2,
        "right of the strip"
    );

    // Dragging the last tab back to the front counts only the two others.
    let last = 2;
    assert_eq!(reorder_target_index(&tabs, last, 0.0), 0);
    assert_eq!(
        reorder_target_index(&tabs, last, 140.0),
        1,
        "past the first"
    );
    assert_eq!(
        reorder_target_index(&tabs, last, 200.0),
        2,
        "past the second"
    );
    assert_eq!(reorder_target_index(&tabs, last, 400.0), 2);

    // Monotonic in the pointer: a drag never moves the tab backwards.
    let mut previous = 0;
    for x in 0..=400 {
        let target = reorder_target_index(&tabs, 1, x as f32);
        assert!(
            target >= previous,
            "the target went backwards at x={x}: {target} after {previous}"
        );
        previous = target;
    }
}

/// Ctrl+Tab, the chord that walks the tab strip forwards.
fn ctrl_tab() -> ModifiersState {
    ModifiersState {
        ctrl: true,
        ..Default::default()
    }
}

/// Ctrl+Shift+Tab, the same walk backwards.
fn ctrl_shift_tab() -> ModifiersState {
    ModifiersState {
        ctrl: true,
        shift: true,
        ..Default::default()
    }
}

/// One key with modifiers through the whole app tree, after a frame — so the
/// handlers exist and the tree was built from the last frame's state.
fn press_chord(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    name: &str,
    modifiers: ModifiersState,
) -> bool {
    let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
    let mut paint_ctx = PaintContext::default();
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext;
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers,
        },
        &mut ctx,
        app,
    )
}

/// `tabs` tabs, the first one on screen. The first tab is a split whose *second*
/// leaf holds the keyboard, so the tab it comes back to proves the switch lands
/// on the arriving tab's **first** leaf and not on the pane it left behind.
fn workspace_of_tabs(state: &Rc<RefCell<UiState>>, tabs: usize) {
    let mut spaces = vec![Space::unnamed(Pane::Split {
        id: 9,
        dir: SplitDir::Horizontal,
        ratio: 0.5,
        first: Box::new(Pane::Leaf {
            id: 1,
            kind: PaneKind::Terminal,
        }),
        second: Box::new(Pane::Leaf {
            id: 2,
            kind: PaneKind::Chat,
        }),
    })];
    for i in 0..tabs.saturating_sub(1) {
        spaces.push(Space::unnamed(Pane::Leaf {
            id: 3 + i as u64,
            kind: PaneKind::Terminal,
        }));
    }
    let mut s = state.borrow_mut();
    s.spaces = spaces;
    s.active_space = 0;
    s.active_pane_id = 2;
}

/// Ctrl+Tab walks the tab strip in the order it draws — tabs 1, 2, 3 — wraps
/// from the last tab to the first, and takes the keyboard to the arriving tab's
/// first leaf. A bare Tab is not the chord.
#[test]
fn ctrl_tab_walks_the_tab_strip_and_wraps_at_the_last_tab() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    workspace_of_tabs(&state, 3);

    assert!(
        press_chord(&mut root, &app, "Tab", ctrl_tab()),
        "Ctrl+Tab is consumed"
    );
    assert_eq!(
        state.borrow().active_space,
        1,
        "the next tab is the second one"
    );
    assert_eq!(
        state.borrow().active_pane_id,
        3,
        "the keyboard lands on the arriving tab's first leaf"
    );
    let saved = desktop.get_ui_panes().expect("the switch is persisted");
    assert!(
        saved.contains("\"active_space\":1"),
        "the tab the switch landed on is what was saved: {saved}"
    );

    assert!(press_chord(&mut root, &app, "Tab", ctrl_tab()));
    assert_eq!(state.borrow().active_space, 2, "and on to the third");

    assert!(press_chord(&mut root, &app, "Tab", ctrl_tab()));
    assert_eq!(
        state.borrow().active_space,
        0,
        "the last tab's next tab is the first"
    );
    assert_eq!(
        state.borrow().active_pane_id,
        1,
        "the wrapped-to tab hands the keyboard to its first leaf, not the pane it left"
    );

    press_chord(&mut root, &app, "Tab", ModifiersState::none());
    assert_eq!(
        state.borrow().active_space,
        0,
        "a bare Tab is not the chord"
    );
}

/// Ctrl+Shift+Tab walks the same strip backwards and wraps from the first tab to
/// the last.
#[test]
fn ctrl_shift_tab_walks_the_tab_strip_backwards_and_wraps_at_the_first_tab() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    workspace_of_tabs(&state, 3);

    assert!(
        press_chord(&mut root, &app, "Tab", ctrl_shift_tab()),
        "Ctrl+Shift+Tab is consumed"
    );
    assert_eq!(
        state.borrow().active_space,
        2,
        "the first tab's previous tab is the last"
    );
    assert_eq!(
        state.borrow().active_pane_id,
        4,
        "its own first leaf takes the keyboard"
    );

    assert!(press_chord(&mut root, &app, "Tab", ctrl_shift_tab()));
    assert_eq!(state.borrow().active_space, 1, "and on back to the second");

    assert!(press_chord(&mut root, &app, "Tab", ctrl_shift_tab()));
    assert_eq!(state.borrow().active_space, 0, "then the first");
    assert_eq!(
        state.borrow().active_pane_id,
        1,
        "the wrapped-to tab's first leaf takes the keyboard"
    );
}

/// One tab has nowhere to walk to: the chord is answered but nothing moves — not
/// the active space, not the keyboard, and not the persisted layout, which a
/// switch that did not happen must not rewrite.
#[test]
fn ctrl_tab_does_not_move_a_one_tab_workspace() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    // The layout the store already holds. A switch would replace it; a no-op
    // leaves it exactly as it is.
    let written = "{\"spaces\":[],\"active_space\":2,\"active_pane_id\":7,\"pane_sessions\":{}}";
    desktop
        .set_ui_panes(written)
        .expect("seed the persisted layout");
    let pane = state.borrow().active_pane_id;
    assert_eq!(state.borrow().spaces.len(), 1, "one tab to start");

    for modifiers in [ctrl_tab(), ctrl_shift_tab()] {
        assert!(
            press_chord(&mut root, &app, "Tab", modifiers),
            "the chord is still answered"
        );
        assert_eq!(state.borrow().spaces.len(), 1, "no tab appears");
        assert_eq!(
            state.borrow().active_space,
            0,
            "the one tab stays on screen"
        );
        assert_eq!(
            state.borrow().active_pane_id,
            pane,
            "and keeps the keyboard"
        );
    }
    assert_eq!(
        desktop.get_ui_panes().as_deref(),
        Some(written),
        "a switch that did not happen writes no layout"
    );
}

/// Arriving on the settings tab with the chord hands the keyboard to that tab's
/// rail: the page it was left on must not keep the keys the switch just took.
#[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
#[test]
fn ctrl_tab_onto_the_settings_tab_puts_the_keyboard_in_its_rail() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    state.borrow_mut().open_settings_tab(None);
    let settings_tab = state.borrow().active_space;

    // The page holds the keyboard, as it does after Tab from the rail, and the
    // chord steps off the tab and back onto it.
    state.borrow_mut().settings.region = crate::ui::settings::SettingsRegion::Page;
    assert_eq!(
        state.borrow().settings.region,
        crate::ui::settings::SettingsRegion::Page,
        "the page holds the keyboard to start"
    );
    assert!(press_chord(&mut root, &app, "Tab", ctrl_shift_tab()));
    assert_ne!(
        state.borrow().active_space,
        settings_tab,
        "the chord left the settings tab"
    );
    assert!(press_chord(&mut root, &app, "Tab", ctrl_tab()));
    assert_eq!(
        state.borrow().active_space,
        settings_tab,
        "and came back to it"
    );
    assert_eq!(
        state.borrow().settings.region,
        crate::ui::settings::SettingsRegion::Rail,
        "the arriving tab's rail takes the keyboard"
    );
}
