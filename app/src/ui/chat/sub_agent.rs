use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill, Flex,
    Icon, MainAxisSize, Spacer, Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use super::super::{PaneChatSnapshot, SubAgentViewSnapshot, UiActions, UiSnapshot};
use super::chat_action_relay;

/// The child view (S6): a sub-agent's own conversation, shown in the pane it was
/// entered from.
///
/// The framed title carries what the parent transcript's row carries — the
/// child's type, its description, its status and its elapsed time, all read from
/// the same live record — and the body is the child's own transcript: the rows
/// the store holds under the child's conversation id, drawn through the same
/// message renderer the parent's transcript uses. There is no composer: an input
/// line here would write into a conversation the user is only reading. Esc
/// returns to the conversation the row was clicked in, leaving the child's card
/// in the pane's block list (A7's mechanism, aimed at the child).
pub(crate) fn build_sub_agent_child_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
    session: &PaneChatSnapshot,
    child: &SubAgentViewSnapshot,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    // The title's status mark is the transcript card's own mark, so the row and
    // the view it enters agree on what each status looks like. A running child
    // turns, the way grok-build's takeover frame does; an ended one settles
    // into its status colour.
    let running = child.row.as_ref().is_some_and(|row| row.is_running());
    let (bullet, bullet_color) = match &child.row {
        Some(row) if running => (spinner_frame(session.work_dock_phase), ColorToken::Warning),
        Some(row) => row.status_affordance(),
        // A child whose live record the pane no longer holds still has its
        // conversation. The mark is the one every status draws.
        None => ("◆", ColorToken::Muted),
    };
    let (subject, status) = match &child.row {
        // The title names the child by its type and the description the parent
        // gave it, and states what it is doing — its activity while it runs,
        // its terminal phrase once it ends.
        Some(row) => (
            format!("{} · {}", row.subagent_type, row.description),
            row.panel_status(),
        ),
        // With no live record the conversation's own name is all there is.
        None => (child.child_id.clone(), "no live record".to_string()),
    };
    let elapsed = child
        .row
        .as_ref()
        .filter(|row| row.is_running())
        .map(|row| goble_ui::elements::chat_content::format_elapsed(row.elapsed));
    let on_close = actions.on_close_sub_agent.clone();
    let close = {
        let on_close = on_close.clone();
        Chip::flat(
            Icon::new("x-close")
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_on_click(move || (on_close.borrow_mut())(pane_id))
        .finish()
    };
    let mut title_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(bullet)
                .with_theme_color(bullet_color, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(
            Text::new(subject)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(
            Text::new(status)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        )
        .with_child(Spacer::new().finish());
    if let Some(elapsed) = elapsed {
        title_row = title_row.with_child(
            Text::new(elapsed)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        );
    }
    let title = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Container::new(
                title_row
                    .with_child(
                        Text::new("Esc to return")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .with_child(close)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::new(md, sm, md, sm))
            .finish(),
        )
        // The band is framed off from the transcript by the divider the chat
        // header uses; this renderer paints bands, not per-side borders.
        .with_child(Divider::horizontal().finish())
        .finish();

    // The child accepts a message of its own once it has ended: the text goes
    // to the child's conversation, the way grok-build's takeover lets the user
    // type in the child's own input. A child still working owns that
    // conversation, so its view stays read-only until it settles.
    let can_reply = child.row.as_ref().is_none_or(|row| !row.is_running());
    let draft = child.draft.clone();
    let on_change_draft = draft.clone();
    let on_send_child = actions.on_send_sub_agent_message.clone();
    // A child can spawn a child of its own: the card's controls enter and stop
    // that grandchild in this same pane, so the way back stays the pane's own.
    let on_open_child: Rc<RefCell<dyn FnMut(String)>> = {
        let open = actions.on_open_sub_agent.clone();
        Rc::new(RefCell::new(move |child_id: String| {
            (open.borrow_mut())(pane_id, child_id)
        }))
    };
    let on_stop_child: Rc<RefCell<dyn FnMut(String)>> = {
        let stop = actions.on_stop_sub_agent.clone();
        Rc::new(RefCell::new(move |child_id: String| {
            (stop.borrow_mut())(pane_id, child_id)
        }))
    };
    let mut view = ChatView::new()
        .with_header(title)
        // The child's own rows, read from the store by its conversation id on
        // the app side — the same message shape the parent's transcript arrives
        // as, drawn by the same renderer.
        .with_messages(child.messages.clone())
        .with_empty_state(
            "The child has not written anything yet",
            "its turns land here as it works",
        )
        // This pane's transcript scroll, block filters and fold map: a child's
        // rows are read with the same tools as any other conversation's.
        .with_scroll_state(session.scroll.clone())
        .with_terminal_filters(state.terminal_filters.clone())
        .with_reasoning_expanded(state.reasoning_expanded.clone())
        .with_tool_fold(state.tool_fold.clone())
        .with_tool_hover(state.tool_hover.clone())
        .with_on_copy_terminal(Some(actions.on_copy_terminal.clone()))
        // A child can itself spawn a child: the pane's live sub-agent records
        // feed this view's rows too, so a nested child is drawn here as the
        // same status line the parent's transcript draws.
        .with_sub_agents(session.sub_agents.clone())
        .with_sub_agent_model(state.selected_model.clone())
        .with_spinner_phase(session.work_dock_phase)
        .with_pane_active(active)
        .with_on_action(chat_action_relay(
            actions.on_open_url.clone(),
            actions.on_copy_terminal.clone(),
            actions.on_explorer_file_click.clone(),
            Some(on_open_child),
            Some(on_stop_child),
        ))
        .with_on_escape(move || (on_close.borrow_mut())(pane_id));
    if can_reply {
        view = view
            .with_composer_value(draft.borrow().clone())
            .with_composer_caret(child.caret.clone())
            .with_composer_focused(active)
            .with_composer_on_change(move |text| *on_change_draft.borrow_mut() = text)
            .with_on_send(move |text| (on_send_child.borrow_mut())(pane_id, text));
    } else {
        view = view.without_composer();
    }
    view.finish()
}

/// The spinner glyph the takeover frame draws for `phase`, the same frames the
/// pane's dock and the turn footer use.
fn spinner_frame(phase: f32) -> &'static str {
    let count = goble_ui::elements::SPINNER_FRAMES.len();
    let index = (phase.rem_euclid(1.0) * count as f32) as usize;
    goble_ui::elements::SPINNER_FRAMES[index.min(count - 1)]
}
