//! Settings → Shortcuts: the chords this build answers.
//!
//! Remapping is not in this build, and the page says so in the same breath as it
//! lists the bindings rather than hiding the section: a user who came here to
//! change a key should learn that the feature is absent, not that the page is.
//!
//! The page is searchable by label, hint and key. The query is app-owned
//! (`SettingsUiState::shortcuts_search`) so a rebuild each frame does not drop
//! it. Each row puts its chord in one grouped key-chip pill and a trailing
//! trash action; the action is honest — it reports that remapping is absent
//! instead of silently deleting the binding.

use super::super::model::{
    ActionModel, ActionStyle, PageModel, PageSearch, RowModel, SectionModel,
};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::unavailable_row;

/// The sentence the trash affordance reports. It is one constant so the page,
/// the dispatch and the tests say exactly the same thing.
pub const REMAP_UNAVAILABLE: &str = "Remapping is not available in this build.";

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

/// The names a user is likely to type for a key, after the glyph itself
/// (`cmd`, `shift`, `enter`).
fn key_names(key: &str) -> &'static [&'static str] {
    match key {
        "⌘" => &["cmd", "command"],
        "⇧" => &["shift"],
        "⌥" => &["option", "alt"],
        "⌃" => &["control", "ctrl"],
        "Enter" | "↵" => &["enter", "return"],
        "Esc" => &["escape"],
        _ => &[],
    }
}

/// Every contiguous spelling of a chord: the glyphs, and the combinations that
/// replace any key with one of its names (`⌘ ⇧ N`, `cmd shift n`, ...).
fn chord_spellings(keys: &[&str]) -> Vec<String> {
    let mut spellings = vec![String::new()];
    for key in keys {
        let mut next = Vec::new();
        for prefix in &spellings {
            for spelling in std::iter::once(*key).chain(key_names(key).iter().copied()) {
                if prefix.is_empty() {
                    next.push(spelling.to_string());
                } else {
                    next.push(format!("{prefix} {spelling}"));
                }
            }
        }
        spellings = next;
    }
    spellings
}

/// Whether a binding matches the page's query. The query is checked against the
/// label, the hint and every spelling of the chord, case-insensitively; an
/// empty query matches every binding.
fn binding_matches(query: &str, binding: &Binding) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let mut haystacks = vec![
        binding.label.to_lowercase(),
        binding.hint.to_lowercase(),
        binding.keys.join(" ").to_lowercase(),
        binding.keys.join("+").to_lowercase(),
    ];
    haystacks.extend(
        chord_spellings(binding.keys)
            .into_iter()
            .map(|spelling| spelling.to_lowercase()),
    );
    haystacks.iter().any(|haystack| haystack.contains(&query))
}

/// The row that says no binding matched, so a search never renders as a blank
/// page under its heading.
fn empty_row(query: &str) -> RowModel {
    RowModel::new(
        "shortcuts-empty",
        format!("No shortcuts match “{}”", query.trim()),
        "The search checks labels, descriptions and key chords.",
    )
    .with_status(StatusKind::Muted, "Clear the search to see every binding.")
}

pub fn page(state: &SettingsUiState) -> PageModel {
    let query = state.shortcuts_search.trim().to_string();
    let mut section = SectionModel::new("bindings", "Keyboard")
        .with_description(
            "The chords this build answers. Remapping is not available in this build; the list is read-only.",
        );
    let mut matched = 0usize;
    for binding in BINDINGS {
        if !binding_matches(&query, binding) {
            continue;
        }
        matched += 1;
        let keys = state
            .shortcuts
            .get(binding.label)
            .cloned()
            .unwrap_or_else(|| binding.keys.iter().map(|key| (*key).to_string()).collect());
        let remap = ActionModel::new(
            "Remove this custom chord.",
            SettingsAction::ShortcutRemap(format!("shortcut.{}", binding.label)),
        )
        .with_icon("trash")
        .with_ghost();
        let add = ActionModel::new(
            "Add a shortcut",
            SettingsAction::ShortcutAdd(format!("shortcut.{}", binding.label)),
        )
        .with_icon("plus")
        .with_ghost();
        section = section.with_row(
            RowModel::new(
                format!("shortcut.{}", binding.label),
                binding.label,
                binding.hint,
            )
            .key_chips_with_add(keys, Some(remap), Some(add)),
        );
        if state.shortcuts_recording.as_deref() == Some(binding.label) {
            let cancel = ActionModel::new("Cancel recording", SettingsAction::ShortcutCancel)
                .with_icon("x")
                .with_ghost();
            let recorded = if state.shortcuts_recorded.is_empty() {
                vec!["Press a chord…".to_string()]
            } else {
                state.shortcuts_recorded.clone()
            };
            section = section.with_row(
                RowModel::new(
                    format!("shortcut.record.{}", binding.label),
                    "Recording…",
                    "Press the next key combination; Escape cancels.",
                )
                .key_chips(recorded, Some(cancel)),
            );
        }
    }
    if matched == 0 {
        section = section.with_row(empty_row(&query));
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
    .with_search(PageSearch::shortcuts(
        state.shortcuts_search.clone(),
        state.shortcuts_search_focused,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::model::ControlModel;
    use crate::ui::settings::render;
    use crate::ui::settings::state::SettingsActions;
    use goble_ui::elements::{AppContext, Element};
    use goble_ui::geometry::vec2f;
    use goble_ui::test_util::render_element;

    fn binding_rows(model: &PageModel) -> Vec<&RowModel> {
        model
            .sections
            .iter()
            .find(|section| section.id == "bindings")
            .expect("the bindings section")
            .rows
            .iter()
            .collect()
    }

    fn drawn_text(model: &PageModel) -> Vec<String> {
        let state = SettingsUiState::default();
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let mut page: Box<dyn Element> = render::render_page(&app, model, &state, &actions);
        render_element(&mut page, vec2f(900.0, 2400.0), &app)
            .into_iter()
            .filter_map(|command| match command {
                goble_ui::render::RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn search_filters_the_binding_rows_by_label_hint_and_key() {
        let mut state = SettingsUiState::default();

        state.shortcuts_search = "sidebar".to_string();
        let model = page(&state);
        let labels: Vec<&str> = binding_rows(&model)
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec!["Toggle sidebar"],
            "the query is app-owned and filters"
        );

        // The hint is searchable too.
        state.shortcuts_search = "active space".to_string();
        let model = page(&state);
        let labels: Vec<&str> = binding_rows(&model)
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, vec!["New tab"]);

        // So is the chord, in glyphs and in the user's own spelling.
        state.shortcuts_search = "⌘ ⇧ W".to_string();
        let model = page(&state);
        let labels: Vec<&str> = binding_rows(&model)
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Close space"]);
        state.shortcuts_search = "cmd shift n".to_string();
        let model = page(&state);
        let labels: Vec<&str> = binding_rows(&model)
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, vec!["New space"]);

        // No match still draws the heading and says why the card is empty.
        state.shortcuts_search = "no such shortcut".to_string();
        let model = page(&state);
        assert_eq!(binding_rows(&model).len(), 1);
        assert_eq!(binding_rows(&model)[0].id, "shortcuts-empty");
        assert!(drawn_text(&model)
            .iter()
            .any(|text| text.contains("No shortcuts match")));

        // The search field itself is part of the page model and survives the
        // per-frame rebuild with the query intact.
        let search = model.search.as_ref().expect("the page has a search");
        assert_eq!(search.value, "no such shortcut");
        assert_eq!(search.placeholder, "Search shortcuts...");
    }

    #[test]
    fn a_binding_row_groups_its_keys_and_carries_the_honest_trash_action() {
        let state = SettingsUiState::default();
        let model = page(&state);
        let row = binding_rows(&model)
            .into_iter()
            .find(|row| row.id == "shortcut.Command palette")
            .expect("the command-palette row");
        match &row.control {
            ControlModel::KeyChips { keys, action, .. } => {
                assert_eq!(keys, &vec!["⌘".to_string(), "K".to_string()]);
                let action = action.as_ref().expect("the trailing trash action");
                assert_eq!(action.icon, Some("trash"));
                assert_eq!(action.style, ActionStyle::Ghost);
                assert_eq!(
                    action.action,
                    SettingsAction::ShortcutRemap("shortcut.Command palette".to_string())
                );
            }
            other => panic!("a binding row is a key-chip control, got {other:?}"),
        }

        // The chips are drawn as one grouped chord, and the trash icon with it.
        let commands = {
            let actions = SettingsActions::new(|_| {});
            let app = AppContext::default();
            let mut page: Box<dyn Element> = render::render_page(&app, &model, &state, &actions);
            render_element(&mut page, vec2f(900.0, 2400.0), &app)
        };
        assert!(commands.iter().any(|command| matches!(
            command,
            goble_ui::render::RenderCommand::DrawText { text, .. } if text == "⌘ + K"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            goble_ui::render::RenderCommand::DrawIcon { name, .. } if name == "trash-02"
        )));
    }

    /// 366-371: every binding row carries an Add action after the trash, and
    /// the recorder row is inserted directly below the active binding.
    #[test]
    fn the_shortcut_recorder_opens_below_its_row() {
        let mut state = SettingsUiState::default();
        state.shortcuts_recording = Some("Command palette".to_string());
        state.shortcuts_recorded = vec!["⌘".to_string(), "K".to_string()];
        let model = page(&state);
        let rows = binding_rows(&model);
        let first = rows.first().expect("a binding row");
        match &first.control {
            ControlModel::KeyChips {
                action, add_action, ..
            } => {
                assert!(action.is_some(), "the trash action stays");
                assert!(add_action.is_some(), "the add action follows it");
            }
            other => panic!("the row is not key chips: {other:?}"),
        }
        assert_eq!(rows[1].label, "Recording…");
        match &rows[1].control {
            ControlModel::KeyChips { keys, action, .. } => {
                assert_eq!(keys, &vec!["⌘".to_string(), "K".to_string()]);
                assert!(action.is_some(), "the recorder has its x control");
            }
            other => panic!("the recorder is not key chips: {other:?}"),
        }
    }
}
