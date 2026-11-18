//! Settings → Shortcuts: the read-only list of chords this build answers.
//!
//! Remapping is not in this build, and the page says so in the same breath as it
//! lists the bindings rather than hiding the section: a user who came here to
//! change a key should learn that the feature is absent, not that the page is.

use super::super::model::{PageModel, RowModel, SectionModel};
use super::super::state::{SettingsUiState, StatusKind};
use super::unavailable_row;

/// One chord, with the key caps the row's control draws.
struct Binding {
    keys: &'static [&'static str],
    label: &'static str,
    hint: &'static str,
}

/// The bindings the app answers today. This is a report, not a source of truth:
/// the root view owns the actual dispatch; the list is kept beside the pages so
/// it is checked when a chord changes.
const BINDINGS: &[Binding] = &[
    Binding {
        keys: &["⌘", "K"],
        label: "Command palette",
        hint: "Jump to a conversation, a file or a command.",
    },
    Binding {
        keys: &["⌘", "T"],
        label: "New tab",
        hint: "Open a new tab in the active space.",
    },
    Binding {
        keys: &["⌘", "W"],
        label: "Close tab",
        hint: "Close the tab on screen.",
    },
    Binding {
        keys: &["⌘", "⇧", "W"],
        label: "Close space",
        hint: "Close the space and every tab in it.",
    },
    Binding {
        keys: &["⌘", "N"],
        label: "New conversation",
        hint: "Start a conversation in the active pane.",
    },
    Binding {
        keys: &["⌘", "⇧", "N"],
        label: "New space",
        hint: "Add a space to the workspace.",
    },
    Binding {
        keys: &["⌘", "⇧", "E"],
        label: "Toggle sidebar",
        hint: "Show or hide the sidebar.",
    },
    Binding {
        keys: &["⌘", ","],
        label: "Settings",
        hint: "Open this tab.",
    },
    Binding {
        keys: &["⌘", "Enter"],
        label: "Agent view",
        hint: "Show the agent view of a terminal pane, or the shell behind an agent.",
    },
    Binding {
        keys: &["⌘", "⇧", "Enter"],
        label: "Queue prompt",
        hint: "Queue a prompt for the running turn instead of interrupting it.",
    },
    Binding {
        keys: &["Esc"],
        label: "Back",
        hint: "Step back one level on the page that owns the keyboard.",
    },
];

pub fn page(_state: &SettingsUiState) -> PageModel {
    let mut section = SectionModel::new("bindings", "Keyboard")
        .with_description(
            "The chords this build answers. Remapping is not available in this build; the list is read-only.",
        );
    for binding in BINDINGS {
        let caps = binding
            .keys
            .iter()
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let mut row = RowModel::new(
            format!("shortcut.{}", binding.label),
            binding.label,
            binding.hint,
        )
        .read_only(caps);
        // The chord is reported in the status line too, so a screen reader and
        // the test both read one string per binding.
        row = row.with_status(StatusKind::Muted, binding.keys.join(" "));
        section = section.with_row(row);
    }
    PageModel::new(vec![
        section,
        SectionModel::new("shortcuts-unavailable", "Not in this build")
            .with_row(unavailable_row(
                "remap",
                "Remap a shortcut",
                "Choose a new chord for an action.",
                "keybinding remapping is not available",
            ))
            .with_row(unavailable_row(
                "shortcut-conflicts",
                "Resolve a conflict",
                "See which action wins when two bindings share a chord.",
                "remapping is not available",
            )),
    ])
}
