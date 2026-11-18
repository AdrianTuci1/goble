//! The sidebar session menu's state: the archived filter, the Rename dialog,
//! the stop-and-archive confirmation, the transient toast, and the actions that
//! back the row's tray (rename, fork, copy id, archive/unarchive, open guard).
//!
//! The durable writes go through the desktop service's `chats.*` columns
//! (`starred`, `archived_at`, `title`); the methods here keep the in-memory list
//! the frame is built from in step and raise the notices the surfaces draw.

use std::time::Duration;

use super::*;

use crate::ui::ArchivedFilter;

/// How long a copy-id / not-openable notice stays up.
const SHORT_TOAST: Duration = Duration::from_secs(3);
/// How long an archive notice stays up (DSH holds its archive notice 6s).
const ARCHIVE_TOAST: Duration = Duration::from_secs(6);
/// How long the "archived cannot be opened" notice stays up: it explains a
/// blocked action, so it lives as long as the archive notice.
const BLOCKED_TOAST: Duration = Duration::from_secs(6);

/// A transient notice raised by a session-menu action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    /// The conversation id reached the clipboard.
    ConversationIdCopied,
    /// No OS clipboard helper took the id (the in-process copy still holds it).
    ConversationIdCopyFailed,
    /// The conversation was archived; the notice offers undo and the filter.
    Archived { id: String },
    /// Live work was stopped and the conversation archived.
    StoppedAndArchived { id: String },
    /// A click on an archived row: it cannot be opened until unarchived.
    ArchivedNotOpenable,
    /// The store refused the archive write (the dialog/list stay honest).
    ArchiveFailed,
    /// The platform could not reveal a workspace's folder in Finder (DSH's
    /// Show in Finder reports the same failure instead of pretending the
    /// folder opened).
    RevealFailed,
}

impl ToastKind {
    /// The sentence the toast draws.
    pub fn message(&self) -> String {
        match self {
            Self::ConversationIdCopied => "Conversation ID copied".to_string(),
            Self::ConversationIdCopyFailed => "Couldn’t copy the conversation ID.".to_string(),
            Self::Archived { .. } => "Conversation archived.".to_string(),
            Self::StoppedAndArchived { .. } => "Conversation stopped and archived.".to_string(),
            Self::ArchivedNotOpenable => {
                "Archived conversations can’t be opened. Unarchive one to view.".to_string()
            }
            Self::ArchiveFailed => "Couldn’t archive the conversation.".to_string(),
            Self::RevealFailed => "Couldn’t show the folder in Finder.".to_string(),
        }
    }

    /// Whether the notice is a warning (the warning glyph) rather than a
    /// success.
    pub fn is_warning(&self) -> bool {
        matches!(
            self,
            Self::ConversationIdCopyFailed
                | Self::ArchivedNotOpenable
                | Self::ArchiveFailed
                | Self::RevealFailed
        )
    }

    /// The id an archive notice's actions operate on.
    pub fn archived_id(&self) -> Option<&str> {
        match self {
            Self::Archived { id } | Self::StoppedAndArchived { id } => Some(id),
            _ => None,
        }
    }

    fn hold(&self) -> Duration {
        match self {
            Self::ConversationIdCopied | Self::ConversationIdCopyFailed => SHORT_TOAST,
            Self::Archived { .. } | Self::StoppedAndArchived { .. } => ARCHIVE_TOAST,
            Self::ArchivedNotOpenable | Self::ArchiveFailed | Self::RevealFailed => {
                BLOCKED_TOAST
            }
        }
    }
}

/// The toast on screen, with a monotonically increasing id so the surface can
/// tell one notice from the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastNotice {
    pub kind: ToastKind,
    pub seq: u64,
}

/// The "Rename conversation" dialog's state: the conversation it names, the
/// draft, whether the field has the focus, and an inline error kept on failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarRename {
    pub id: String,
    pub draft: String,
    pub focused: bool,
    pub error: Option<String>,
}

/// The stop-and-archive confirmation's state: the conversation, the honest
/// activity list, whether the stop is in flight and the last failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveConfirm {
    pub id: String,
    pub title: String,
    pub activities: Vec<String>,
    pub busy: bool,
    pub error: Option<String>,
}

impl UiState {
    // -- the archived filter ------------------------------------------------

    /// The filter the view-options menu currently selects.
    pub fn archived_filter(&self) -> ArchivedFilter {
        *self.sidebar_archived_filter.borrow()
    }

    /// Set the archived filter from the view-options menu.
    pub fn set_archived_filter(&self, filter: ArchivedFilter) {
        *self.sidebar_archived_filter.borrow_mut() = filter;
    }

    /// Whether the conversation is archived, from the list the frame draws.
    pub fn conversation_is_archived(&self, id: &str) -> bool {
        self.conversations
            .iter()
            .any(|entry| entry.id == id && entry.archived)
    }

    // -- toast --------------------------------------------------------------

    /// Raise a transient notice at the top of the window.
    pub fn show_toast(&mut self, kind: ToastKind) {
        let hold = kind.hold();
        self.next_toast_seq += 1;
        self.toast = Some(ToastNotice {
            kind,
            seq: self.next_toast_seq,
        });
        self.toast_until = Some(std::time::Instant::now() + hold);
    }

    /// Put the notice away by hand.
    pub fn dismiss_toast(&mut self) {
        self.toast = None;
        self.toast_until = None;
    }

    /// Honour the notice's deadline. Called once per frame before the snapshot
    /// is taken, while `wants_animation` keeps the clock running.
    pub fn expire_toast(&mut self) {
        if self
            .toast_until
            .is_some_and(|until| std::time::Instant::now() >= until)
        {
            self.dismiss_toast();
        }
    }

    // -- copy id ------------------------------------------------------------

    /// Copy the conversation's raw id to the system clipboard and report the
    /// outcome with a toast: the checked helper says whether an OS helper
    /// actually wrote it, so a missing helper reads as a warning rather than a
    /// false success (the in-process copy still holds the value).
    pub fn copy_conversation_id(&mut self, id: &str) {
        let copied = crate::clipboard::copy_text_checked(id);
        self.show_toast(if copied {
            ToastKind::ConversationIdCopied
        } else {
            ToastKind::ConversationIdCopyFailed
        });
    }

    // -- rename -------------------------------------------------------------

    /// Open the Rename dialog for `id`, seeded with the title the row shows.
    pub fn begin_rename(&mut self, id: &str) {
        let Some(name) = self
            .conversations
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.name.clone())
        else {
            return;
        };
        self.sidebar_rename = Some(SidebarRename {
            id: id.to_string(),
            draft: name,
            focused: true,
            error: None,
        });
    }

    /// Replace the Rename dialog's draft.
    pub fn rename_draft(&mut self, draft: String) {
        if let Some(rename) = self.sidebar_rename.as_mut() {
            rename.draft = draft;
            rename.error = None;
        }
    }

    /// Cancel the Rename dialog without writing anything.
    pub fn cancel_rename(&mut self) {
        self.sidebar_rename = None;
    }

    /// Commit the Rename dialog's draft through the durable title path and
    /// close the dialog. A blank draft (or a write failure) keeps it open.
    pub fn commit_rename(&mut self, desktop: Option<&DesktopState>) {
        let Some(rename) = self.sidebar_rename.clone() else {
            return;
        };
        let title = rename.draft.trim().to_string();
        if title.is_empty() {
            return;
        }
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_chat_title(&rename.id, &title) {
                log::warn!("set_chat_title for {} failed: {error}", rename.id);
                if let Some(open) = self.sidebar_rename.as_mut() {
                    open.error = Some("Couldn’t rename the conversation.".to_string());
                }
                return;
            }
        } else {
            log::warn!("rename: no desktop state; the title is not durable");
        }
        if let Some(entry) = self
            .conversations
            .iter_mut()
            .find(|entry| entry.id == rename.id)
        {
            entry.name = title;
        }
        self.refresh_space_labels();
        self.sidebar_rename = None;
    }

    // -- fork ---------------------------------------------------------------

    /// Fork a conversation by id for the sidebar menu: the new conversation
    /// inherits the source's messages and environment, and the selection stays
    /// where it was (DSH forks without changing what the user is looking at).
    pub fn fork_conversation_by_id(
        &mut self,
        id: &str,
        desktop: Option<&DesktopState>,
    ) -> Option<String> {
        let forked = self.fork_conversation(id, desktop)?;
        if let Some(desktop) = desktop {
            self.refresh_conversations(desktop);
        }
        Some(forked)
    }

    // -- archive ------------------------------------------------------------

    /// The work Goble can honestly attribute to `id`: the turn in progress and
    /// the running sub-agents of its panes. Background jobs and scheduled
    /// reminders are app-wide, not per conversation, so they are not listed.
    pub fn conversation_activity(&self, id: &str) -> Vec<String> {
        let mut activities = Vec::new();
        if self.conversation_is_live(id) {
            activities.push("The turn in progress".to_string());
        }
        let mut names: Vec<String> = Vec::new();
        let panes: Vec<u64> = self
            .pane_sessions
            .iter()
            .filter(|(_, session)| session.conversation_id == id)
            .map(|(pane_id, _)| *pane_id)
            .collect();
        for pane_id in panes {
            for row in self.pane_sub_agents(pane_id) {
                let name = if row.description.trim().is_empty() {
                    row.child_id.clone()
                } else {
                    row.description.clone()
                };
                if !names.iter().any(|seen| seen == &name) {
                    names.push(name);
                }
            }
        }
        if !names.is_empty() {
            let suffix = if names.len() == 1 { "" } else { "s" };
            activities.push(format!(
                "{} running subagent{suffix}: {}",
                names.len(),
                names.join(", ")
            ));
        }
        activities
    }

    /// Archive a conversation from the row's menu. Live work opens the
    /// stop-and-archive confirmation first; a quiet conversation archives now.
    pub fn archive_conversation(&mut self, id: &str, desktop: Option<&DesktopState>) {
        let activities = self.conversation_activity(id);
        if activities.is_empty() {
            self.archive_now(id, desktop, false);
            return;
        }
        let title = self
            .conversations
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "This conversation".to_string());
        self.archive_confirm = Some(ArchiveConfirm {
            id: id.to_string(),
            title,
            activities,
            busy: false,
            error: None,
        });
    }

    /// Stop the confirmed conversation's live work and archive it.
    pub fn confirm_archive(&mut self, desktop: Option<&DesktopState>) {
        let Some(confirm) = self.archive_confirm.clone() else {
            return;
        };
        if let Some(dialog) = self.archive_confirm.as_mut() {
            dialog.busy = true;
            dialog.error = None;
        }
        let panes: Vec<u64> = self
            .pane_sessions
            .iter()
            .filter(|(_, session)| session.conversation_id == confirm.id)
            .map(|(pane_id, _)| *pane_id)
            .collect();
        for pane_id in panes {
            if let Some(desktop) = desktop {
                if !desktop.cancel_chat_turn(&confirm.id) {
                    log::warn!("cancel_chat_turn({}): no live turn to stop", confirm.id);
                }
            }
            self.stop_turn(pane_id);
        }
        self.agent_busy = false;
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_chat_archived(&confirm.id, true) {
                log::warn!("set_chat_archived({}) failed: {error}", confirm.id);
                if let Some(dialog) = self.archive_confirm.as_mut() {
                    dialog.busy = false;
                    dialog.error = Some("Couldn’t archive the conversation.".to_string());
                }
                return;
            }
        } else {
            log::warn!("archive: no desktop state; the archive is not durable");
        }
        self.archive_confirm = None;
        self.archive_finished(&confirm.id, true);
    }

    /// Dismiss the stop-and-archive confirmation, leaving the conversation
    /// running and listed.
    pub fn cancel_archive(&mut self) {
        self.archive_confirm = None;
    }

    /// Archive without a confirmation (the quiet path).
    fn archive_now(&mut self, id: &str, desktop: Option<&DesktopState>, stopped: bool) {
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_chat_archived(id, true) {
                log::warn!("set_chat_archived({id}) failed: {error}");
                self.show_toast(ToastKind::ArchiveFailed);
                return;
            }
        } else {
            log::warn!("archive: no desktop state; the archive is not durable");
        }
        self.archive_finished(id, stopped);
    }

    /// The list, selection and notice side of archiving: the row leaves the
    /// default list, a selection that was on it moves on, and the notice offers
    /// undo / the archived filter.
    fn archive_finished(&mut self, id: &str, stopped: bool) {
        if let Some(entry) = self.conversations.iter_mut().find(|entry| entry.id == id) {
            entry.archived = true;
        }
        if self.selected_id.as_deref() == Some(id) {
            let next = self
                .conversations
                .iter()
                .find(|entry| entry.id != id && !entry.archived && entry.has_agent_reply)
                .map(|entry| entry.id.clone());
            let owned_by_active = self
                .pane_sessions
                .get(&self.active_pane_id)
                .map(|session| session.conversation_id == id)
                .unwrap_or(false);
            self.selected_id = next.clone();
            if owned_by_active {
                if let Some(next) = next {
                    // The archive action's caller holds the desktop; the
                    // rebind only needs the id for the tab/space labels, which
                    // are refreshed by the caller's next refresh pass.
                    self.bind_active_pane_conversation(next, None);
                }
            }
        }
        self.show_toast(if stopped {
            ToastKind::StoppedAndArchived { id: id.to_string() }
        } else {
            ToastKind::Archived { id: id.to_string() }
        });
    }

    /// Restore an archived conversation. DSH has no success notice for this, so
    /// it is silent (a failure is logged).
    pub fn unarchive_conversation(&mut self, id: &str, desktop: Option<&DesktopState>) {
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_chat_archived(id, false) {
                log::warn!("set_chat_archived({id}, false) failed: {error}");
                return;
            }
        } else {
            log::warn!("unarchive: no desktop state; the restore is not durable");
        }
        if let Some(entry) = self.conversations.iter_mut().find(|entry| entry.id == id) {
            entry.archived = false;
        }
    }

    /// The click guard for an archived row: refuse to open it and say why.
    pub fn open_conversation_from_sidebar(
        &mut self,
        id: String,
        desktop: Option<&DesktopState>,
    ) {
        if self.conversation_is_archived(&id) {
            self.show_toast(ToastKind::ArchivedNotOpenable);
            return;
        }
        self.open_conversation_tab(id, desktop);
    }
}
