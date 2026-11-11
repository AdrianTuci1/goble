use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::block::{
    terminal_block_styled, terminal_block_with_cursor_styled, terminal_block_with_selection_styled,
    TerminalBlockCursor, TerminalLineGeometry, TerminalTextSelection, TerminalTextStyle,
};
use super::data::TerminalData;
use super::filter::{TerminalCopyHandler, TerminalFilter};
use crate::elements::Element;

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
    text_style: TerminalTextStyle,
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
            text_style: TerminalTextStyle::default(),
        }
    }

    /// Draw the surface's blocks at `style`. A terminal pane passes its grid
    /// font, so a captured row of `columns` cells fits the pane instead of
    /// wrapping; other surfaces keep the body default.
    pub fn with_text_style(mut self, style: TerminalTextStyle) -> Self {
        self.text_style = style;
        self
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
        terminal_block_styled(
            data,
            filter,
            self.global_filter.clone(),
            self.on_copy.clone(),
            self.text_style,
        )
    }

    /// Draw `data` as a terminal block with a text selection over its lines,
    /// recording each drawn line's box for the pointer that owns the selection.
    /// `line_base` is the block's first line in the caller's flattened text.
    pub fn element_with_selection(
        &self,
        data: &TerminalData,
        selection: Option<TerminalTextSelection>,
        line_base: usize,
        geometry: Rc<RefCell<Vec<TerminalLineGeometry>>>,
    ) -> Box<dyn Element> {
        let filter = self
            .filters
            .borrow_mut()
            .entry(data.filter_key())
            .or_default()
            .clone();
        terminal_block_with_selection_styled(
            data,
            filter,
            self.global_filter.clone(),
            self.on_copy.clone(),
            selection,
            line_base,
            geometry,
            self.text_style,
        )
    }

    /// The same block, also drawing the caret a command that is still running
    /// left in its output. `cursor.line` is the block's own line index, the
    /// command line included.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_selection_and_cursor(
        &self,
        data: &TerminalData,
        selection: Option<TerminalTextSelection>,
        cursor: Option<TerminalBlockCursor>,
        line_base: usize,
        geometry: Rc<RefCell<Vec<TerminalLineGeometry>>>,
    ) -> Box<dyn Element> {
        let filter = self
            .filters
            .borrow_mut()
            .entry(data.filter_key())
            .or_default()
            .clone();
        terminal_block_with_cursor_styled(
            data,
            filter,
            self.global_filter.clone(),
            self.on_copy.clone(),
            selection,
            cursor,
            line_base,
            geometry,
            self.text_style,
        )
    }

    /// The filter state of the block the pointer is over, if any: a block writes
    /// its own hover flag as the pointer moves over it, so at most one entry is
    /// set at a time.
    pub fn hovered_filter(&self) -> Option<TerminalFilter> {
        self.filters
            .borrow()
            .values()
            .find(|filter| filter.is_hovered())
            .cloned()
    }

    /// Show or hide the filter bar of the block under the pointer. Returns
    /// whether a block was found: with the pointer off every block there is
    /// nothing to filter.
    pub fn toggle_hovered_filter(&self) -> bool {
        match self.hovered_filter() {
            Some(filter) => {
                filter.toggle_bar();
                true
            }
            None => false,
        }
    }

    /// This surface's whole-output filter, when the app wired one: one filter
    /// over every block the surface draws.
    pub fn global_filter(&self) -> Option<TerminalFilter> {
        self.global_filter.clone()
    }

    /// Show or hide this surface's whole-output filter bar. Returns whether the
    /// app wired one, so a surface with no filter reports that nothing happened.
    pub fn toggle_global_filter(&self) -> bool {
        match self.global_filter() {
            Some(filter) => {
                filter.toggle_bar();
                true
            }
            None => false,
        }
    }
}
