use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Fill, Flex, MainAxisSize, Spacer, Text,
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
    // The title's status mark is the transcript row's own mark, so the row and
    // the view it enters agree on what each status looks like.
    let (bullet, bullet_color, subject, status) = match &child.row {
        Some(row) => {
            let (mark, color) = row.status_affordance();
            (
                mark,
                color,
                format!("{} · {}", row.subagent_type, row.description),
                row.status_line(),
            )
        }
        // A child whose live record the pane no longer holds still has its
        // conversation. The title names it by id and leaves the status out rather
        // than inventing one.
        None => (
            "◇",
            ColorToken::Muted,
            child.child_id.clone(),
            "no live record".to_string(),
        ),
    };
    let title = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Text::new(bullet)
                            .with_theme_color(bullet_color, app)
                            .with_font_size(13.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new(subject)
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(13.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new(status)
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(Spacer::new().finish())
                    .with_child(
                        Text::new("Esc to return")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
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

    let on_close = actions.on_close_sub_agent.clone();
    // A nested child's row stays enterable: the view opens on the view rather
    // than dead-ending at the first generation.
    let open_sub_agent: Rc<RefCell<dyn FnMut(String)>> = {
        let open = actions.on_open_sub_agent.clone();
        Rc::new(RefCell::new(move |child_id: String| {
            (open.borrow_mut())(pane_id, child_id)
        }))
    };
    ChatView::new()
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
        .with_on_copy_terminal(Some(actions.on_copy_terminal.clone()))
        // A child can itself spawn a child: the pane's live sub-agent records
        // feed this view's rows too, so a nested spawn call inside the child's
        // transcript is enterable from here, exactly as it is in the parent's.
        .with_sub_agents(session.sub_agents.clone())
        .with_pane_active(active)
        .with_on_action(chat_action_relay(
            actions.on_open_url.clone(),
            open_sub_agent,
        ))
        .without_composer()
        .with_on_escape(move || (on_close.borrow_mut())(pane_id))
        .finish()
}
