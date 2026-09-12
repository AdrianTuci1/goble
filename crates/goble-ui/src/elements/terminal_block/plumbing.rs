use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::elements::{Element, terminal_block};
use super::data::TerminalData;
use super::filter::{TerminalCopyHandler, TerminalFilter};

/// What a terminal block needs from the app wherever it is drawn: the app-owned
/// per-block filter map, the surface's own whole-block filter and the copy
/// handler. Bundled so a command's segment in the transcript, a terminal
/// fragment and a terminal pane's own section of history all draw through one
/// renderer with one set of plumbing.
#[derive(Clone)]
pub struct TerminalBlockPlumbing {
    filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    global_filter: Option<TerminalFilter>,
    on_copy: Option<TerminalCopyHandler>,
}

impl TerminalBlockPlumbing {
    /// The plumbing a surface hands a terminal block: the shared per-block
    /// filter map (so a tray the user opened survives the rebuild), the
    /// surface's own filter, and the app's copy handler.
    pub fn new(
        filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
        global_filter: Option<TerminalFilter>,
        on_copy: Option<TerminalCopyHandler>,
    ) -> Self {
        Self {
            filters,
            global_filter,
            on_copy,
        }
    }

    /// Draw `data` as the terminal block, resolving its app-owned filter state
    /// by the block's content key.
    pub fn element(&self, data: &TerminalData) -> Box<dyn Element> {
        let filter = self
            .filters
            .borrow_mut()
            .entry(data.filter_key())
            .or_default()
            .clone();
        terminal_block(
            data,
            filter,
            self.global_filter.clone(),
            self.on_copy.clone(),
        )
    }
}
