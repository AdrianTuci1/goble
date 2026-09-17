//! The inline screen card in a pane: what it shows, and what closing it means.
//!
//! A handed-off desktop is shown by panes, not by the app: every pane that
//! names the source in its runtime draws its frame. Closing therefore belongs
//! to the last pane showing it — the card's dismissal and the session's end
//! both go through [`UiState::release_inline_screen`], which hands the source
//! back only when no pane still names it, so two panes on one source keep
//! their stream until the second one lets go.

use super::*;

impl UiState {
    /// The panes that still show `source` as their inline screen.
    pub fn inline_screen_viewers(&self, source: &str) -> usize {
        self.pane_runtime
            .values()
            .filter(|rt| rt.inline_screen_source.as_deref() == Some(source))
            .count()
    }

    /// Let `pane_id` go of the inline screen source it was showing, which is
    /// what dismisses its card.
    ///
    /// Returns the source when this pane was the **last** one showing it — the
    /// caller's signal to close the desktop — and `None` while another pane
    /// still shows it: one conversation dismissing its card must not stop a
    /// stream another conversation is watching.
    pub fn release_inline_screen(&mut self, pane_id: u64) -> Option<String> {
        let source = self
            .pane_runtime
            .get_mut(&pane_id)?
            .inline_screen_source
            .take()?;
        (self.inline_screen_viewers(&source) == 0).then_some(source)
    }

    /// Stop every pane drawing `source`, because the desktop behind it is gone
    /// (`screen:closed`). A card never outlives the desktop it shows.
    pub fn forget_inline_screen_source(&mut self, source: &str) {
        for rt in self.pane_runtime.values_mut() {
            if rt.inline_screen_source.as_deref() == Some(source) {
                rt.inline_screen_source = None;
            }
        }
    }
}
