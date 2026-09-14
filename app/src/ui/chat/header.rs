use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Empty, Fill, Flex, Icon, MainAxisSize, PopupMenu, PopupMenuItem,
    PopupMenuPosition, Spacer, Text, Tooltip, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};


use super::super::{UiActions, UiSnapshot};
use crate::state::PaneWorkItem;

/// Height of the agent header's tallest control (`TopbarButton`'s default
/// size); the header is padded out to `shell::TOPBAR_HEIGHT` around it.
const HEADER_CONTROL_HEIGHT: f32 = 32.0;

/// Agent header row: a right-aligned work chip, a 3-dots tray and a close X.
/// Every pane gets its own app-owned tray flag (`agent_header_menus[pane_id]`)
/// so opening the tray in one split pane does not open it in the other panes
/// sharing the view; the copy / restart / rename / clear-transcript /
/// fullscreen actions are folded into the 3-dots menu instead of cluttering
/// the header.
/// The pane's topbar — the one bar a pane has, shared by both of its surfaces.
///
/// The agent view and the terminal pane's shell view draw this same bar (see
/// `terminal::build_terminal`): a draft's directory and branch are not repeated
/// here, because the rich input at the bottom of the pane already carries them.
/// A terminal pane adds the "Run agent" menu, since that pane can host a real
/// TUI agent, and while its harness is open the bar leads with `esc` — the way
/// back to the plain shell, spelled out at the front.
///
/// The 3-dots menu carries what acts on *this pane*. Surfaces that open a
/// drawer rather than act — the scheduled-tasks sheet and the right-hand side
/// panel — are not in it: both stay reachable from the ⌘K palette and from a
/// slash command (`/scheduled`, `/sidebar`, see `ui::palette`).
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

    let on_rename = actions.on_rename_agent.clone();
    let on_clear = actions.on_clear_transcript.clone();
    let on_toggle_fullscreen = actions.on_toggle_fullscreen.clone();
    let on_copy = actions.on_copy.clone();
    let on_restart = actions.on_restart.clone();

    // The open flag is per-pane (app-owned) so it survives the per-frame
    // element rebuild and stays isolated between split panes.
    let menu_open = state
        .agent_header_menus
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| Rc::new(RefCell::new(false)));
    let fullscreen_item = if state.fullscreen {
        PopupMenuItem::new("Fullscreen").with_icon("maximize").selected()
    } else {
        PopupMenuItem::new("Fullscreen").with_icon("maximize")
    };
    let menu_items = vec![
        PopupMenuItem::new("Copy"),
        PopupMenuItem::new("Restart").with_icon("refresh"),
        PopupMenuItem::new("Rename"),
        PopupMenuItem::new("Clear transcript").with_icon("trash"),
        fullscreen_item,
    ];
    let dots_button = TopbarButton::new(
        Icon::new("dots-horizontal")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .finish();
    let menu = PopupMenu::new(dots_button, menu_items)
        .with_open(menu_open)
        // The dots sit at the pane's right edge, so the tray opens to the left
        // and stays inside the window.
        .with_position(PopupMenuPosition::BelowEnd)
        .with_on_select(move |index| match index {
            0 => (on_copy.borrow_mut())(),
            1 => (on_restart.borrow_mut())(),
            2 => (on_rename.borrow_mut())(),
            3 => (on_clear.borrow_mut())(),
            4 => (on_toggle_fullscreen.borrow_mut())(),
            _ => {}
        });
    let header_menu = Box::new(menu);

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
        .with_child(header_menu)
        .with_child(close_button)
        .finish();

    Container::new(row)
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(0.0, v_pad, 0.0, v_pad))
    .finish()
}

/// The pane's own work cue, drawn between the header's empty space and the
/// 3-dots tray: a rule, then `◆ N` in the accent colour, the same shape and
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
    use goble_ui::event::DispatchedEvent;
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
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
        }
        (view, state, desktop, dir)
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

    /// Center of the top-most `name` icon drawn. The pane header's tray is the
    /// top-most one in the window, and it is what the band of the pane's own
    /// bar is measured from: the app's topbar sits above it and the transcript
    /// below it.
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
    /// tray's line, not in the app's topbar above it nor in the pane below it.
    fn in_header_band(y: f32, tray_y: f32) -> bool {
        (y - tray_y).abs() < 24.0
    }

    /// Every text the header band draws, with where it starts.
    fn header_texts(cmds: &[RenderCommand], tray_y: f32) -> Vec<(String, Vector2F)> {
        cmds.iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if in_header_band(origin.y, tray_y) => {
                    Some((text.clone(), *origin))
                }
                _ => None,
            })
            .collect()
    }

    /// The pane header's accent diamonds: where each was drawn and how big.
    fn header_diamonds(cmds: &[RenderCommand], app: &AppContext, tray_y: f32) -> Vec<(Vector2F, f32)> {
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
                    && in_header_band(origin.y, tray_y) =>
                {
                    Some((*origin, *size))
                }
                _ => None,
            })
            .collect()
    }

    /// The pane header's separators: the 1 pt rules on the header's own line.
    fn header_rules(cmds: &[RenderCommand], app: &AppContext, tray_y: f32) -> Vec<RectF> {
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
                    && in_header_band(rect.min_y(), tray_y) =>
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

    /// Every label the open tray draws, in the order it draws them (top to
    /// bottom), so the panel's item order is what the selection indices are
    /// read against. The tray opens below the pane's bar and to the left of its
    /// trigger, so its rows are the texts in that box — not the sidebar's,
    /// which share the band's height.
    fn tray_labels(cmds: &[RenderCommand], tray: (f32, f32)) -> Vec<String> {
        let mut rows: Vec<(f32, String)> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. }
                    if origin.y > tray.1 + 12.0
                        && origin.y < tray.1 + 12.0 + 6.0 * HEADER_CONTROL_HEIGHT
                        && origin.x < tray.0
                        && origin.x > tray.0 - 260.0 =>
                {
                    Some((origin.y, text.clone()))
                }
                _ => None,
            })
            .collect();
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        rows.into_iter().map(|(_, text)| text).collect()
    }

    /// The two entries that opened a drawer or a sheet — the scheduled-tasks
    /// sheet and the right-hand side panel — are not the pane's own actions, so
    /// the tray no longer offers them; everything that acts on this pane is
    /// still there, in the order the selection indices are numbered.
    #[test]
    fn the_tray_carries_the_panes_own_actions_and_no_drawer() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let tray = topmost_icon(&cmds, "dots-horizontal");
        click(&mut root, &app, tray);

        let pane_id = state.borrow().active_pane_id;
        let open = state
            .borrow()
            .agent_header_menus
            .get(&pane_id)
            .cloned()
            .expect("an app-owned tray flag for the active pane");
        assert!(*open.borrow(), "the click opens the tray");

        let cmds = frame(&mut root, &app, None);
        let tray = topmost_icon(&cmds, "dots-horizontal");
        let drawn: Vec<String> = cmds
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for gone in ["Scheduled tasks", "Toggle panel"] {
            assert!(
                !drawn.iter().any(|text| text == gone),
                "the drawer entry {gone} is not the pane's own action: {drawn:?}"
            );
        }
        assert_eq!(
            tray_labels(&cmds, tray),
            vec![
                "Copy".to_string(),
                "Restart".to_string(),
                "Rename".to_string(),
                "Clear transcript".to_string(),
                "Fullscreen".to_string(),
            ],
            "the tray offers exactly what acts on this pane, in selection order"
        );
    }

    /// The shifted indices still select what they draw: `Clear transcript` is
    /// the fourth row now, and clicking it runs the clear rather than the
    /// rename that used to sit there.
    #[test]
    fn the_shifted_rows_still_run_the_action_they_draw() {
        let app = AppContext::default();
        let (mut root, state, _desktop, _dir) = pane_root();
        let pane_id = {
            let mut s = state.borrow_mut();
            let pane_id = s.active_pane_id;
            s.pane_runtime.entry(pane_id).or_default().queued_prompt = Some("later".into());
            pane_id
        };
        let _ = frame(&mut root, &app, None);
        let cmds = frame(&mut root, &app, None);
        let tray = topmost_icon(&cmds, "dots-horizontal");
        click(&mut root, &app, tray);

        let cmds = frame(&mut root, &app, None);
        let row = cmds
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "Clear transcript" => {
                    Some((origin.x + 8.0, origin.y + 6.0))
                }
                _ => None,
            })
            .expect("the tray draws the clear row");
        click(&mut root, &app, row);

        let s = state.borrow();
        assert!(
            s.pane_runtime
                .get(&pane_id)
                .and_then(|rt| rt.queued_prompt.as_ref())
                .is_none(),
            "the row the tray draws as Clear transcript is the clear action"
        );
        assert!(
            !*s.agent_header_menus
                .get(&pane_id)
                .cloned()
                .expect("the pane's tray flag")
                .borrow(),
            "choosing a row closes the tray, so the click reached the item"
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
        let tray = topmost_icon(&cmds, "dots-horizontal");

        assert!(
            header_diamonds(&cmds, &app, tray.1).is_empty(),
            "a pane with no work in flight draws no diamond"
        );
        assert!(
            header_rules(&cmds, &app, tray.1).is_empty(),
            "and no separator, so the header looks exactly as it did"
        );
        assert!(
            header_texts(&cmds, tray.1).is_empty(),
            "and no count: {:?}",
            header_texts(&cmds, tray.1)
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
        let tray = topmost_icon(&cmds, "dots-horizontal");

        let diamonds = header_diamonds(&cmds, &app, tray.1);
        assert_eq!(diamonds.len(), 1, "one diamond per pane: {diamonds:?}");
        let (diamond, size) = diamonds[0];
        assert_eq!(size, 8.0, "the diamond is the topbar cue's 8 pt one");
        let counts: Vec<(String, Vector2F)> = header_texts(&cmds, tray.1)
            .into_iter()
            .filter(|(text, _)| text == "2")
            .collect();
        assert_eq!(counts.len(), 1, "the count is drawn once: {counts:?}");
        let (_, count_origin) = counts[0];
        assert!(
            count_origin.x > diamond.x,
            "the number follows the diamond: {count_origin:?} vs {diamond:?}"
        );

        let rules = header_rules(&cmds, &app, tray.1);
        assert_eq!(rules.len(), 1, "one separator: {rules:?}");
        assert!(
            rules[0].max_x() < diamond.x,
            "the rule sits left of the diamond: {:?} vs {diamond:?}",
            rules[0]
        );
        assert!(
            diamond.x < tray.0,
            "the whole chip sits left of the 3-dots tray: {diamond:?} vs {tray:?}"
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
        let tray = topmost_icon(&cmds, "dots-horizontal");
        let (diamond, size) = header_diamonds(&cmds, &app, tray.1)
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
        let tray = topmost_icon(&cmds, "dots-horizontal");
        let diamond = header_diamonds(&cmds, &app, tray.1)
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
        let tray = topmost_icon(&cmds, "dots-horizontal");

        assert_eq!(
            header_diamonds(&cmds, &app, tray.1).len(),
            2,
            "both panes of the split draw their own chip"
        );
        assert_eq!(header_rules(&cmds, &app, tray.1).len(), 2);

        let counts: Vec<(String, Vector2F)> = header_texts(&cmds, tray.1)
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
