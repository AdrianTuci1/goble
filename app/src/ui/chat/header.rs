use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Empty, Fill, Flex, Icon, MainAxisSize, Spacer, Text, Tooltip,
    TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};


use super::super::{UiActions, UiSnapshot};
use crate::state::PaneWorkItem;

/// Height of the agent header's tallest control (`TopbarButton`'s default
/// size); the header is padded out to `shell::TOPBAR_HEIGHT` around it.
const HEADER_CONTROL_HEIGHT: f32 = 32.0;

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
/// back to the plain shell, spelled out at the front.
///
/// The expand control is the bar's only action besides closing the pane: it
/// draws this pane over the whole panes space and then draws itself as the way
/// back, once the space holds a pane to expand over. What used to be folded
/// into a 3-dots tray here now lives where it can be reached from any pane:
/// `/clear` and `/fullscreen` in the ⌘K palette (`ui::palette`), and Copy in
/// the macOS Edit menu (`platform::mac::menus`).
pub(crate) fn build_agent_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    harness_open: bool,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    // The agent header is the pane's topbar, so it is padded out to the same
    // height as the general topbar and the terminal pane header (36px on
    // macOS) instead of hugging its tallest control.
    let v_pad = ((super::super::shell::TOPBAR_HEIGHT - HEADER_CONTROL_HEIGHT) / 2.0).max(0.0);
    // The focused pane's corner mark is drawn over the left end of this bar, so
    // the bar starts past it — the mark is the pane's, the bar is what it opens.
    let marker_inset = if state.active_pane_id == pane_id {
        super::super::panes::FOCUS_MARKER_SIZE
    } else {
        0.0
    };

    // The pane's expand control: it grows this pane over the whole panes space,
    // and the control it becomes — drawn as the retract icon — puts the pane
    // back. The pane tree is left alone either way, so retracting restores the
    // layout the pane had rather than recomputing one. A pane has something to
    // expand over only while its space holds a second pane, so the control is
    // drawn inert there rather than gone: it keeps the bar's shape, and the
    // tooltip says what the space is missing (warp-new refuses the same case,
    // `pane_count() > 1`).
    let maximized = state.maximized_pane == Some(pane_id);
    let expandable = state
        .spaces
        .get(state.active_space)
        .is_some_and(|space| space.root.leaf_count() > 1);
    let on_toggle_maximized = actions.on_toggle_pane_maximized.clone();
    let expand_button = Tooltip::new(
        TopbarButton::new(
            Icon::new(if maximized { "minimize" } else { "maximize" })
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_disabled(!maximized && !expandable)
        .with_on_click(move || (on_toggle_maximized.borrow_mut())(pane_id))
        .finish(),
        match (maximized, expandable) {
            (true, _) => "Retract this pane",
            (false, true) => "Expand this pane",
            (false, false) => "Split this pane to expand it",
        },
    )
    .finish();

    let on_close = actions.on_close_pane.clone();
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing);
    if harness_open {
        // Clicking the hint is the switch itself: the harness closes and the
        // bar's editor lets go of the keys, which is what Esc does.
        let on_harness_mode = actions.on_set_pane_harness_mode.clone();
        let on_composer_focus = actions.on_composer_focus_change.clone();
        let hover = state
            .pane_hover
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(false)));
        row = row.with_child(super::super::panes::build_esc_hint(app, hover, move || {
            (on_harness_mode.borrow_mut())(pane_id, false);
            (on_composer_focus.borrow_mut())(false);
        }));
    }
    let row = row
        .with_child(Spacer::new().finish())
        .with_child(build_work_chip(
            app,
            state.pane_live_work.get(&pane_id).map(Vec::as_slice).unwrap_or(&[]),
        ))
        .with_child(expand_button)
        .with_child(close_button)
        .finish();

    Container::new(row)
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(marker_inset, v_pad, 0.0, v_pad))
    .finish()
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
    fn pane_root() -> (RootView, Rc<RefCell<UiState>>, Arc<DesktopState>, tempfile::TempDir) {
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
    fn split_root() -> (RootView, Rc<RefCell<UiState>>, Arc<DesktopState>, tempfile::TempDir) {
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
                    if name == "x-close" && in_header_band(origin.y, bar_y) =>
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
    /// topbar's own tab closes draw above it and are excluded by that.
    fn pane_bar_y(cmds: &[RenderCommand]) -> f32 {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. }
                    if name == "x-close" && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
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
    fn frame(root: &mut RootView, app: &AppContext, cursor: Option<Vector2F>) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(window()),
            &mut LayoutContext::default(),
            app,
        );
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
        let mut ctx = EventContext::default();
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
                    origin, name: drawn, size, ..
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
                    origin, name: drawn, size, ..
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
    fn header_diamonds(cmds: &[RenderCommand], app: &AppContext, bar_y: f32) -> Vec<(Vector2F, f32)> {
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
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
            status: ToolCallStatus::Running,
            result: None,
        }
    }

    fn sub_agent(child_id: &str, kind: &str, description: &str) -> SubAgentRecord {
        SubAgentRecord {
            parent_call_id: "call-1".to_string(),
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

    /// The pane bar's actions are gone from it: the header draws the expand
    /// control and the close X, and nothing that opens a tray. The acts that
    /// used to live in the tray are reached from where any pane can reach them
    /// (`/clear` and `/fullscreen` in the palette, Copy in the macOS menu).
    #[test]
    fn the_header_draws_the_expand_control_and_no_tray() {
        let app = AppContext::default();
        let (mut root, _state, _desktop, _dir) = pane_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);

        let expand = topmost_icon(&cmds, "maximize-01");
        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let icons: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !icons.iter().any(|name| name == "dots-horizontal"),
            "the 3-dots trigger is gone from the pane bar: {icons:?}"
        );
        for gone in [
            "Copy",
            "Restart",
            "Rename",
            "Clear transcript",
            "Fullscreen",
            "Scheduled tasks",
            "Toggle panel",
        ] {
            assert!(
                !drawn.iter().any(|text| text == gone),
                "the act {gone} is not drawn anywhere in the frame: {drawn:?}"
            );
        }
        assert!(
            expand.0 < topmost_icon(&cmds, "x-close").0,
            "the expand control sits left of the close X: {expand:?}"
        );
    }

    /// The focused pane carries an accent mark in its own top-left corner, and
    /// it is the only pane that does: warp-new draws the indicator into the
    /// active pane's header stack, so the mark is read at the corner the pane
    /// starts from and it walks with the focus rather than sitting on the space.
    #[test]
    fn the_focused_pane_carries_the_accent_mark_in_its_own_corner() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = split_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            state.borrow().active_pane_id,
            7,
            "the split starts on the left pane"
        );

        let bar_y = pane_bar_y(&cmds);
        let bar_xs: Vec<f32> = pane_bars(&cmds, bar_y).iter().map(|bar| bar.x).collect();
        let left_bar = bar_xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let marks = focus_markers(&cmds, &app);
        assert_eq!(
            marks.len(),
            1,
            "one pane holds the focus, so one mark is drawn: {marks:?}"
        );
        assert!(
            marks[0].x < left_bar && marks[0].y < bar_y,
            "the mark is in the focused pane's own top-left corner, left of and \
             above the bar it opens: {:?} against the bars at {bar_xs:?} on {bar_y}",
            marks[0]
        );

        // The focus walking right takes the mark with it: the right pane is the
        // active session then, and the left pane's bar carries nothing.
        let _ = frame(&mut root, &app, None);
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowRight".to_string(),
                modifiers: ModifiersState {
                    ctrl: true,
                    ..ModifiersState::default()
                },
            },
            &mut ctx,
            &app,
        );
        assert_eq!(
            state.borrow().active_pane_id,
            8,
            "the focus walked to the pane on the right"
        );
        let cmds = frame(&mut root, &app, None);
        let moved = focus_markers(&cmds, &app);
        assert_eq!(
            moved.len(),
            1,
            "and the mark is still one, now the right pane's: {moved:?}"
        );
        assert!(
            moved[0].x > marks[0].x,
            "the mark moved to the pane the focus moved to: {:?} was {:?}",
            moved[0],
            marks[0]
        );
    }

    /// The control is a toggle: a press expands the pane it belongs to, and a
    /// second press on the same pane puts it back. It is drawn as the way back
    /// while the pane is expanded, so the action is always on screen. The pane
    /// that takes the space is the pane the app works in, so the expansion
    /// carries the focus with it.
    #[test]
    fn the_expand_control_expands_and_retracts_the_pane_it_belongs_to() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = split_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            state.borrow().active_pane_id,
            7,
            "the split starts on the left pane"
        );
        assert_eq!(
            state.borrow().maximized_pane,
            None,
            "a pane starts in the tree's own layout"
        );

        // The right pane, which is not the focused one: the control belongs to
        // the pane whose bar carries it.
        click(&mut root, &app, rightmost_icon(&cmds, "maximize-01"));
        assert_eq!(
            state.borrow().maximized_pane,
            Some(8),
            "the press expands the pane whose bar carries the control"
        );
        assert_eq!(
            state.borrow().active_pane_id,
            8,
            "and the pane that takes the space is the one the app works in"
        );

        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            pane_bars(&cmds, pane_bar_y(&cmds)).len(),
            1,
            "the split's other pane gives way to the expanded one"
        );
        assert!(
            cmds.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "minimize-01"
            )),
            "the expanded pane draws the way back"
        );
        click(&mut root, &app, topmost_icon(&cmds, "minimize-01"));
        assert_eq!(
            state.borrow().maximized_pane,
            None,
            "the second press retracts it"
        );

        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            pane_bars(&cmds, pane_bar_y(&cmds)).len(),
            2,
            "and the tree is back"
        );
        assert!(
            cmds.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "maximize-01"
            )),
            "with the control the expand one again"
        );
    }

    /// The focus moving to another pane ends the expansion: the pane that gave
    /// up the focus cannot keep the whole space, so the tree comes back with the
    /// pane the focus moved to on screen (warp-new's focus rule). The chord is
    /// what a user would press, and it is answered by the root's own pane-focus
    /// handling.
    #[test]
    fn moving_the_focus_to_another_pane_ends_the_expansion() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = split_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        click(&mut root, &app, rightmost_icon(&cmds, "maximize-01"));
        assert_eq!(state.borrow().maximized_pane, Some(8));

        // The frame the press lands on is also the frame the chords are
        // dispatched through, so the tree is rebuilt before the key.
        let _ = frame(&mut root, &app, None);
        let mut ctx = EventContext::default();
        let handled = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowLeft".to_string(),
                modifiers: ModifiersState {
                    ctrl: true,
                    ..ModifiersState::default()
                },
            },
            &mut ctx,
            &app,
        );
        assert!(handled, "the workspace answers the pane-focus chord");
        assert_eq!(
            state.borrow().active_pane_id,
            7,
            "the focus walked to the pane on the left"
        );
        assert_eq!(
            state.borrow().maximized_pane,
            None,
            "and the expanded pane gave the space back"
        );

        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            pane_bars(&cmds, pane_bar_y(&cmds)).len(),
            2,
            "so both panes are drawn again"
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

    /// A pane its space holds alone has nothing to expand over: the tree already
    /// is that pane. The control stays on the bar — it is the bar's shape — but
    /// it is inert, and its tooltip says what the space is missing instead of
    /// letting the press do nothing and say nothing.
    #[test]
    fn a_pane_its_space_holds_alone_has_nothing_to_expand_over() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let control = topmost_icon(&cmds, "maximize-01");

        click(&mut root, &app, control);
        assert_eq!(
            state.borrow().maximized_pane,
            None,
            "the press expands nothing: the pane already is the space"
        );

        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, Some(vec2f(control.0, control.1)));
        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            drawn
                .iter()
                .any(|text| text == "Split this pane to expand it"),
            "the tooltip says why the control cannot act: {drawn:?}"
        );
    }

    /// A pane expanded in one space does not expand anything in another: the id
    /// names no leaf there, so that space keeps drawing its own tree.
    #[test]
    fn an_expansion_in_one_space_leaves_another_space_its_own_layout() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let expanded = {
            let mut s = state.borrow_mut();
            s.spaces.push(Space::unnamed(Pane::Split {
                id: 90,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: Box::new(Pane::Leaf {
                    id: 8,
                    kind: PaneKind::Chat,
                }),
                second: Box::new(Pane::Leaf {
                    id: 9,
                    kind: PaneKind::Chat,
                }),
            }));
            s.active_space = 1;
            s.active_pane_id = 8;
            for pane_id in [8, 9] {
                s.pane_sessions.insert(
                    pane_id,
                    PaneSession {
                        conversation_id: String::new(),
                        draft: String::new(),
                        path: String::new(),
                    },
                );
            }
            // The pane expanded while the first space was on screen.
            let expanded = s.spaces[0].root.first_leaf_id();
            s.maximized_pane = Some(expanded);
            expanded
        };
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);

        let bars = pane_bars(&cmds, pane_bar_y(&cmds));
        assert_eq!(
            bars.len(),
            2,
            "the second space draws both of its panes, not the first space's one: {bars:?}"
        );
        assert_eq!(
            state.borrow().maximized_pane,
            Some(expanded),
            "the flag is untouched; it simply names no leaf of this space"
        );
    }

    /// Expanding a pane draws it over the whole panes space: the split's other
    /// pane is not drawn at all while one is expanded, and its bar reaches the
    /// far edge of the space the two of them shared. Retracting brings the tree
    /// back exactly as it was.
    #[test]
    fn expanding_a_pane_draws_it_over_the_whole_panes_space() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = split_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let side_by_side = pane_bars(&cmds, pane_bar_y(&cmds));
        assert_eq!(
            side_by_side.len(),
            2,
            "the split draws a bar per pane: {side_by_side:?}"
        );
        let left_edge = side_by_side
            .iter()
            .map(|origin| origin.x)
            .fold(f32::INFINITY, f32::min);

        state.borrow_mut().maximized_pane = Some(8);
        let cmds = frame(&mut root, &app, None);
        let expanded = pane_bars(&cmds, pane_bar_y(&cmds));
        assert_eq!(
            expanded.len(),
            1,
            "only the expanded pane is drawn, so the other pane's bar is gone: {expanded:?}"
        );
        assert!(
            expanded[0].x > left_edge + 100.0,
            "the expanded pane's bar reaches past where the two bars ended: {expanded:?} vs left bar {left_edge}"
        );

        state.borrow_mut().maximized_pane = None;
        let cmds = frame(&mut root, &app, None);
        assert_eq!(
            pane_bars(&cmds, pane_bar_y(&cmds)),
            side_by_side,
            "retracting restores the layout the split had"
        );
    }

    /// The pane's work chip: a rule and `◆ N` in the accent colour at a
    /// non-zero count, and nothing at all — rule included — at zero.
    #[test]
    fn the_work_chip_draws_the_count_and_vanishes_at_zero() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let bar = topmost_icon(&cmds, "maximize-01");

        assert!(
            header_diamonds(&cmds, &app, bar.1).is_empty(),
            "a pane with no work in flight draws no diamond"
        );
        assert!(
            header_rules(&cmds, &app, bar.1).is_empty(),
            "and no separator, so the header looks exactly as it did"
        );
        assert!(
            header_texts(&cmds, bar.1).is_empty(),
            "and no count: {:?}",
            header_texts(&cmds, bar.1)
        );

        {
            let mut s = state.borrow_mut();
            let pane_id = s.active_pane_id;
            let rt = s.pane_runtime.entry(pane_id).or_default();
            rt.in_flight_tools
                .insert("call-1".to_string(), tool_call("call-1", "run_command"));
            rt.sub_agents.insert(
                "child-1".to_string(),
                sub_agent("child-1", "Explore", "Map the tree"),
            );
        }
        let cmds = frame(&mut root, &app, None);
        let bar = topmost_icon(&cmds, "maximize-01");

        let diamonds = header_diamonds(&cmds, &app, bar.1);
        assert_eq!(diamonds.len(), 1, "one diamond per pane: {diamonds:?}");
        let (diamond, size) = diamonds[0];
        assert_eq!(size, 8.0, "the diamond is the topbar cue's 8 pt one");
        let counts: Vec<(String, Vector2F)> = header_texts(&cmds, bar.1)
            .into_iter()
            .filter(|(text, _)| text == "2")
            .collect();
        assert_eq!(counts.len(), 1, "the count is drawn once: {counts:?}");
        let (_, count_origin) = counts[0];
        assert!(
            count_origin.x > diamond.x,
            "the number follows the diamond: {count_origin:?} vs {diamond:?}"
        );

        let rules = header_rules(&cmds, &app, bar.1);
        assert_eq!(rules.len(), 1, "one separator: {rules:?}");
        assert!(
            rules[0].max_x() < diamond.x,
            "the rule sits left of the diamond: {:?} vs {diamond:?}",
            rules[0]
        );
        assert!(
            diamond.x < bar.0,
            "the whole chip sits left of the expand control: {diamond:?} vs {bar:?}"
        );
    }

    /// The chip is information only: it carries no handler of its own, so a
    /// press on it reaches the pane header underneath.
    #[test]
    fn the_work_chip_takes_no_press_of_its_own() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        {
            let mut s = state.borrow_mut();
            let pane_id = s.active_pane_id;
            s.pane_runtime
                .entry(pane_id)
                .or_default()
                .in_flight_tools
                .insert("call-1".to_string(), tool_call("call-1", "run_command"));
        }
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let bar = topmost_icon(&cmds, "maximize-01");
        let (diamond, size) = header_diamonds(&cmds, &app, bar.1)
            .first()
            .copied()
            .expect("the pane's chip draws its diamond");

        let mut ctx = EventContext::default();
        let taken = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(diamond.x + size / 2.0, diamond.y + size / 2.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(taken, "the press is not swallowed by the chip");
        assert!(
            state.borrow().pane_drag.is_some(),
            "it reaches the pane header, which lifts the pane"
        );
    }

    /// The count is what the pane alone is charged with — its running work,
    /// and the tooltip names every item so the number is never a mystery.
    #[test]
    fn the_work_chip_counts_and_names_the_panes_own_work() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        {
            let mut s = state.borrow_mut();
            let pane_id = s.active_pane_id;
            let rt = s.pane_runtime.entry(pane_id).or_default();
            rt.in_flight_tools
                .insert("call-1".to_string(), tool_call("call-1", "run_command"));
            rt.sub_agents.insert(
                "child-1".to_string(),
                sub_agent("child-1", "Explore", "Map the tree"),
            );
            rt.sub_agents.insert(
                "child-2".to_string(),
                sub_agent("child-2", "Review", "Check the diff"),
            );
            rt.sub_agents.insert(
                "child-3".to_string(),
                finished_sub_agent("child-3", "Test", "Ran the suite"),
            );
        }
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let bar = topmost_icon(&cmds, "maximize-01");
        let diamond = header_diamonds(&cmds, &app, bar.1)
            .first()
            .map(|(origin, size)| vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
            .expect("the pane's chip draws its diamond");
        let cmds = frame(&mut root, &app, Some(diamond));

        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            drawn.iter().any(|text| text == "3"),
            "one tool call and two running children, not the finished one: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|text| text
                == "task · run_command\nsub-agent · Explore · Map the tree\nsub-agent · Review · Check the diff"),
            "the tooltip names every item with its kind: {drawn:?}"
        );
    }

    /// Two panes in one frame count their own work, not the window's: the left
    /// pane's call is not added to the right pane's children.
    #[test]
    fn each_pane_counts_only_its_own_work() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
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
                    },
                );
            }
            s.pane_runtime
                .entry(7)
                .or_default()
                .in_flight_tools
                .insert("call-1".to_string(), tool_call("call-1", "run_command"));
            let right = s.pane_runtime.entry(8).or_default();
            right.sub_agents.insert(
                "child-1".to_string(),
                sub_agent("child-1", "Explore", "Map the tree"),
            );
            right.sub_agents.insert(
                "child-2".to_string(),
                sub_agent("child-2", "Review", "Check the diff"),
            );
        }
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let bar = topmost_icon(&cmds, "maximize-01");

        assert_eq!(
            header_diamonds(&cmds, &app, bar.1).len(),
            2,
            "both panes of the split draw their own chip"
        );
        assert_eq!(header_rules(&cmds, &app, bar.1).len(), 2);

        let counts: Vec<(String, Vector2F)> = header_texts(&cmds, bar.1)
            .into_iter()
            .filter(|(text, _)| text == "1" || text == "2")
            .collect();
        let left = counts
            .iter()
            .find(|(text, _)| text == "1")
            .expect("the left pane draws its one item");
        let right = counts
            .iter()
            .find(|(text, _)| text == "2")
            .expect("the right pane draws its two");
        assert!(
            left.1.x < right.1.x,
            "each pane's count sits in that pane: {counts:?}"
        );
    }
}
