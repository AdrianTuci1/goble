use std::cell::RefCell;
use std::rc::Rc;

use super::line::TerminalLineKind;

/// The copy handler a terminal block's header button fires with the block's
/// full display text. App-owned, so it is shared like the block's filter state.
pub type TerminalCopyHandler = Rc<RefCell<dyn FnMut(String) + 'static>>;

/// App-owned, per-block filter state for a terminal block. Both cells live in
/// app state (`UiState.terminal_filters`) so the tray's open flag and the
/// selected filter survive the per-frame element rebuild; each terminal block
/// resolves its own entry by content key.
#[derive(Clone, Debug)]
pub struct TerminalFilter {
    pub open: Rc<RefCell<bool>>,
    pub selected: Rc<RefCell<usize>>,
}

impl Default for TerminalFilter {
    fn default() -> Self {
        Self {
            open: Rc::new(RefCell::new(false)),
            selected: Rc::new(RefCell::new(0)),
        }
    }
}

const FILTER_LABELS: &[&str] = &["All", "Commands", "Output", "Notes", "Success", "Errors"];

pub(super) const FILTERS: &[( &str, fn(TerminalLineKind) -> bool)] = &[
    (FILTER_LABELS[0], |_| true),
    (FILTER_LABELS[1], |k| k == TerminalLineKind::Command),
    (FILTER_LABELS[2], |k| k == TerminalLineKind::Output),
    (FILTER_LABELS[3], |k| k == TerminalLineKind::Info),
    (FILTER_LABELS[4], |k| k == TerminalLineKind::Success),
    (FILTER_LABELS[5], |k| k == TerminalLineKind::Error),
];

/// The filter option labels, in order, shared by the per-block tray and the
/// whole-transcript filter bar.
pub fn filter_option_labels() -> &'static [&'static str] {
    FILTER_LABELS
}
