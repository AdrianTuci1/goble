//! Closing a pane that holds unsaved file edits.
//!
//! A close gesture (the header's ✕, Cmd+W, the palette's `close`) goes through
//! [`UiState::request_close_pane`]. A clean pane, or one with no file buffer,
//! closes at once. A dirty file view asks first, unless the user turned on
//! Editor & Input → "Auto-save on close": then the buffer is written and the
//! pane closes. A save that fails leaves the pane open, where the buffer's own
//! band says why and the text can still be fixed.

use super::*;

impl UiState {
    /// Close `pane_id`, asking first when it holds unsaved file edits.
    pub fn request_close_pane(&mut self, pane_id: u64, desktop: Option<&DesktopState>) {
        if self.file_pane_dirty(pane_id) {
            if self.auto_save_on_close {
                if !self.save_pane_files(pane_id) {
                    return;
                }
            } else {
                self.pending_close_pane = Some(pane_id);
                return;
            }
        }
        self.close_pane(pane_id, desktop);
    }

    /// Whether any file the pane holds open has an edit that is not on disk.
    fn file_pane_dirty(&self, pane_id: u64) -> bool {
        self.file_buffers
            .iter()
            .any(|((pane, _), buffer)| *pane == pane_id && buffer.borrow().dirty())
    }

    /// Write every dirty file the pane holds open. A failed write stops the
    /// rest: the pane stays open with the buffer's error band, so no text is
    /// lost to a save that could not finish.
    fn save_pane_files(&mut self, pane_id: u64) -> bool {
        for ((pane, _), buffer) in self.file_buffers.iter() {
            if *pane == pane_id && buffer.borrow().dirty() {
                if buffer.borrow_mut().save().is_err() {
                    return false;
                }
            }
        }
        true
    }

    /// Close `pane_id` now, with no question asked.
    pub fn close_pane(&mut self, pane_id: u64, desktop: Option<&DesktopState>) {
        let Some(space_idx) = self.space_index_of_pane(pane_id) else {
            return;
        };
        let was_active = self.active_space == space_idx && self.active_pane_id == pane_id;
        let next = self
            .spaces
            .get_mut(space_idx)
            .and_then(|space| space.close(pane_id));
        // The closed pane owns nothing: its session/runtime/hover, a child view
        // open in it and its shell (killing the pty) go with it, and so does
        // its viewer session — a pane that is gone must not be found by a later
        // worker status report. Its file view's scroll and buffer handles go
        // with it too.
        self.forget_pane(pane_id);
        self.file_buffers
            .retain(|(pane, _), _| *pane != pane_id);
        self.pane_open_files.remove(&pane_id);
        self.file_scroll.remove(&pane_id);
        self.file_hscroll.remove(&pane_id);
        // An expanded pane that is closed gives the space back: the id is
        // dropped with the pane, so nothing is left expanded by name.
        if self.maximized_pane == Some(pane_id) {
            self.maximized_pane = None;
        }
        if was_active {
            if let Some(next) = next {
                self.active_pane_id = next;
            }
        }
        self.sync_active_view();
        if let Some(desktop) = desktop {
            self.refresh_messages(desktop);
            self.save_panes(desktop);
        }
    }

    /// The close dialog's "Save and close": write the buffer, then close.
    ///
    /// A write that fails leaves the dialog: the pane stays open with the
    /// buffer's error band, so the text is not lost to a failed save.
    pub fn close_pending_pane_save(&mut self, desktop: Option<&DesktopState>) {
        let Some(pane_id) = self.pending_close_pane else {
            return;
        };
        if self.save_pane_files(pane_id) {
            self.pending_close_pane = None;
            self.close_pane(pane_id, desktop);
        }
    }

    /// The close dialog's "Don't save": drop the edits and close the pane.
    pub fn close_pending_pane_discard(&mut self, desktop: Option<&DesktopState>) {
        let Some(pane_id) = self.pending_close_pane.take() else {
            return;
        };
        // Drop the handles first: nothing left in the state can save this text
        // after the user said not to.
        self.file_buffers
            .retain(|(pane, _), _| *pane != pane_id);
        self.close_pane(pane_id, desktop);
    }

    /// The close dialog's "Cancel", and `Escape`/the backdrop: keep the pane.
    pub fn cancel_pending_pane_close(&mut self) {
        self.pending_close_pane = None;
    }

    /// Close one file a pane holds open, asking first when that file has
    /// unsaved edits. The pane and its other files stay; closing the last file
    /// closes the pane.
    pub fn request_close_file(
        &mut self,
        pane_id: u64,
        path: String,
        desktop: Option<&DesktopState>,
    ) {
        if self.file_buffer_dirty(pane_id, &path) {
            if self.auto_save_on_close {
                if !self.save_file(pane_id, &path) {
                    return;
                }
            } else {
                self.pending_close_file = Some((pane_id, path));
                return;
            }
        }
        self.close_file_now(pane_id, &path, desktop);
    }

    fn file_buffer_dirty(&self, pane_id: u64, path: &str) -> bool {
        self.file_buffers
            .get(&(pane_id, path.to_string()))
            .is_some_and(|buffer| buffer.borrow().dirty())
    }

    /// Write one file's buffer. A failed write leaves the buffer's error band
    /// up and the chip in place.
    fn save_file(&mut self, pane_id: u64, path: &str) -> bool {
        match self.file_buffers.get(&(pane_id, path.to_string())) {
            Some(buffer) if buffer.borrow().dirty() => buffer.borrow_mut().save().is_ok(),
            _ => true,
        }
    }

    /// Drop one file from a pane. When it was the file the pane showed, the
    /// chip before it (or the first one left) takes its place, so the pane
    /// never shows a file it no longer holds.
    fn close_file_now(&mut self, pane_id: u64, path: &str, desktop: Option<&DesktopState>) {
        if self
            .pending_close_file
            .as_ref()
            .is_some_and(|(pane, file)| *pane == pane_id && file == path)
        {
            self.pending_close_file = None;
        }
        let Some(space_idx) = self.space_index_of_pane(pane_id) else {
            return;
        };
        let Some(PaneKind::File { path: active }) =
            self.spaces[space_idx].leaf_kind(pane_id).cloned()
        else {
            return;
        };
        let Some(files) = self.pane_open_files.get_mut(&pane_id) else {
            // A pane restored from a saved layout carries no chip list yet; the
            // file it shows is the only one it holds, so closing that closes
            // the pane.
            if active == path {
                self.close_pane(pane_id, desktop);
            }
            return;
        };
        let Some(index) = files.iter().position(|file| file == path) else {
            return;
        };
        files.remove(index);
        let remaining = files.clone();
        self.file_buffers.remove(&(pane_id, path.to_string()));
        if remaining.is_empty() {
            self.pane_open_files.remove(&pane_id);
            self.close_pane(pane_id, desktop);
            return;
        }
        if active == path {
            let next = remaining[index.saturating_sub(1).min(remaining.len() - 1)].clone();
            self.spaces[space_idx].set_leaf_kind(pane_id, PaneKind::File { path: next });
            self.sync_active_view();
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// The close dialog's "Save and close" for a file chip.
    pub fn close_pending_file_save(&mut self, desktop: Option<&DesktopState>) {
        let Some((pane_id, path)) = self.pending_close_file.clone() else {
            return;
        };
        if !self.save_file(pane_id, &path) {
            return;
        }
        self.pending_close_file = None;
        self.close_file_now(pane_id, &path, desktop);
    }

    /// The close dialog's "Don't save" for a file chip.
    pub fn close_pending_file_discard(&mut self, desktop: Option<&DesktopState>) {
        let Some((pane_id, path)) = self.pending_close_file.take() else {
            return;
        };
        self.close_file_now(pane_id, &path, desktop);
    }

    /// The close dialog's "Cancel" for a file chip.
    pub fn cancel_pending_file_close(&mut self) {
        self.pending_close_file = None;
    }
}
