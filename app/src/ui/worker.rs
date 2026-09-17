//! The viewer pane: a conversation whose shell is not on this machine.
//!
//! A conversation routed to a remote worker (`PaneKind::Worker`) is shown as a
//! viewer session. What makes it a *viewer* and not a terminal is what is
//! missing, and it is missing by construction:
//!
//! * the pane is built here, as the conversation's own surface — the transcript
//!   and the composer, the same shape a chat pane draws — and never through
//!   [`crate::ui::terminal::build_terminal`], which is the only builder that
//!   mounts a pty. Rendering this pane therefore spawns no shell;
//! * the transcript leads with the connection's own report
//!   ([`WorkerPaneSnapshot::connection_line`]): what the worker streamed is the
//!   whole of what this pane has, and when the connection drops the pane says so
//!   in place rather than becoming a shell;
//! * a command typed at the pane is refused by the app, because there is no
//!   local shell to run it in (`crate::actions::run_terminal_command`).
//!
//! The pixels are unverified: no window is built or launched here.

use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon, MainAxisSize,
    Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::chat;
use super::{UiActions, UiSnapshot, WorkerPaneSnapshot};

/// Build the pane a `PaneKind::Worker` leaf mounts: the conversation, with the
/// worker connection's state over it.
///
/// Escape wires nothing: the way out of a chat pane's agent view is its own
/// shell, and a viewer pane has none to return to.
pub fn build_worker_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
) -> Box<dyn Element> {
    chat::build_agent_chat(app, state, actions, pane_id, active, None)
}

/// The band a viewer pane leads its transcript with: the worker, and the state
/// its connection is in.
///
/// It is drawn in the slot the chat surface gives its other inline notice, so
/// the pane's report sits over the conversation it is about — a band of rows,
/// this app's flat idiom, not a thrown-over sheet. Which state it is decides
/// how loudly it says so: a connected session is chrome, a drop or a failure is
/// the pane's own warning.
pub(crate) fn build_worker_notice(
    app: &AppContext,
    worker: &WorkerPaneSnapshot,
) -> Box<dyn Element> {
    use crate::state::PaneAttach;
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let (icon, text, accent) = match &worker.attach {
        PaneAttach::Attached => (
            "conversation-remote",
            ColorToken::Muted,
            ColorToken::Success,
        ),
        PaneAttach::Connecting => ("conversation-remote", ColorToken::Muted, ColorToken::Accent),
        PaneAttach::Detached { .. } => ("cloud-off", ColorToken::Text, ColorToken::Error),
        PaneAttach::Failed { .. } => ("cloud-off", ColorToken::Text, ColorToken::Error),
    };
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new(icon)
                .with_size(16.0)
                .with_theme_color(accent, app)
                .finish(),
        )
        .with_child(
            Text::new(worker.connection_line())
                .with_theme_color(text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::new(md, sm, md, sm))
        .finish()
}
