//! Answering control-socket verbs on the winit thread.
//!
//! The control server runs off-thread; this is the one place a command is
//! turned into UI state. It runs before the frame's rebuild, so a control
//! change is visible in the same frame. A request that is still queued when
//! the root drops is answered by its channel closing, never by leaving the
//! handler blocked.

use super::RootView;

impl RootView {
    /// Drain every waiting control verb and answer it with a typed result or a
    /// structured error. Never panics: a handler that fails is still answered.
    #[cfg(unix)]
    pub(super) fn process_control_requests(&mut self) {
        loop {
            let Some(pending) = self.control.as_ref().and_then(|control| control.try_recv()) else {
                break;
            };
            let command = pending.command.clone();
            let result = {
                let mut state = self.state.borrow_mut();
                crate::control::handle_ui_command(
                    &mut state,
                    self.desktop.as_ref(),
                    &self.media_state,
                    command,
                )
            };
            pending.reply(result);
        }
    }
}
