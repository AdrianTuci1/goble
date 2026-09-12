//! Agent chat tab: identity header + message transcript + composer.
//!
//! One module per surface of a pane: the agent view itself ([`agent`]), the
//! pane's topbar ([`header`]), the terminal pane's rich input ([`composer`]),
//! the sub-agent child view ([`sub_agent`]) and the pane's overlay sheets
//! ([`overlays`]). What the surfaces share — this pane's chat snapshot and the
//! relay that carries a fragment's action out of the view — lives here.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::ChatAction;

use super::{PaneChatSnapshot, UiSnapshot};

mod agent;
mod composer;
mod header;
mod overlays;
mod sub_agent;

pub(crate) use agent::build_agent_chat;
pub(crate) use composer::build_terminal_composer;
pub(crate) use header::build_agent_header;
pub(crate) use overlays::{
    build_agent_error, build_chat_panel_overlay, build_onboarding_tip, build_workspace_choice,
};
pub(crate) use sub_agent::build_sub_agent_child_view;

/// Relay a transcript fragment's action to the app: a link's `OpenUrl` is handed
/// to `on_open_url`, the app's guarded external-URL opener, and a sub-agent row
/// click is handed to `on_open_sub_agent`. Any other action is ignored here.
fn chat_action_relay(
    on_open_url: Rc<RefCell<dyn FnMut(String)>>,
    on_open_sub_agent: Rc<RefCell<dyn FnMut(String)>>,
) -> impl FnMut(ChatAction) + 'static {
    move |action| match action {
        ChatAction::OpenUrl(url) => (on_open_url.borrow_mut())(url),
        ChatAction::OpenSubAgent(child_id) => (on_open_sub_agent.borrow_mut())(child_id),
        ChatAction::RunCommand(_) | ChatAction::Custom(_) => {}
    }
}

/// This pane's own chat data, falling back to the global active view for a pane
/// that somehow has no entry yet (so the first frame still reads sane values).
fn pane_session_snapshot(state: &UiSnapshot, pane_id: u64) -> PaneChatSnapshot {
    state
        .pane_chat
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| PaneChatSnapshot {
            conversation_id: state.selected_id.clone().unwrap_or_default(),
            messages: state.chat_messages.clone(),
            composer_draft: state.composer_draft.clone(),
            composer_path: state.composer_path.clone(),
            pending_ask: state.pending_ask.clone(),
            pending_command: None,
            command_selection: Rc::new(RefCell::new(0)),
            queued_prompt: state.queued_prompt.clone(),
            agent_busy: state.agent_busy,
            turn_status: goble_ui::TurnStatus::Idle,
            inline_screen: None,
            screen_link: None,
            sub_agents: std::collections::HashMap::new(),
            sub_agent_view: None,
            scroll: Rc::new(RefCell::new(goble_ui::ScrollState::following())),
            usage: Default::default(),
            usage_open: Rc::new(RefCell::new(false)),
        })
}
