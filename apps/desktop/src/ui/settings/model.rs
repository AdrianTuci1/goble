//! The page model: what a settings page is made of, as data.
//!
//! Pages are built as data first ([`PageModel`]), then rendered by
//! [`super::render`]. One description is used twice: the renderer draws it and
//! the keyboard model counts and resolves its rows, so a page cannot draw a row
//! the keys cannot reach (the failure the old hand-written focus order kept
//! risking).
//!
//! A row is the unit. Its control is one of the settings controls; a row whose
//! label is a "setting" has exactly one primary action, so `Enter` on the focused
//! row does the obvious thing (flips a toggle, advances a select, commits a text
//! field, or presses the first button of a button group).

use goble_core::config::ConfigFile;
use goble_ui::color::ColorU;

use super::state::{SettingsAction, StatusKind};

/// How an action button reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionStyle {
    /// The secondary control (an outline/raised button).
    Default,
    /// The action the row exists for.
    Primary,
    /// Removes something; reads in the error colour.
    Destructive,
    /// A low-emphasis text action (a palette's "Use").
    Ghost,
}

/// A button in a row's control group (or a section header).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionModel {
    pub label: String,
    pub icon: Option<&'static str>,
    pub action: SettingsAction,
    pub style: ActionStyle,
    pub disabled: bool,
}

impl ActionModel {
    pub fn new(label: impl Into<String>, action: SettingsAction) -> Self {
        Self {
            label: label.into(),
            icon: None,
            action,
            style: ActionStyle::Default,
            disabled: false,
        }
    }

    pub fn primary(mut self) -> Self {
        self.style = ActionStyle::Primary;
        self
    }

    /// A low-emphasis text action.
    pub fn with_ghost(mut self) -> Self {
        self.style = ActionStyle::Ghost;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.style = ActionStyle::Destructive;
        self
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// The control on a row's right.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlModel {
    Toggle {
        /// The path a write goes to (`app.telemetry`).
        path: String,
        value: bool,
        disabled: bool,
    },
    Select {
        path: String,
        options: Vec<String>,
        selected: usize,
        disabled: bool,
    },
    Text {
        /// The field id the draft is kept under (usually the path).
        id: String,
        value: String,
        placeholder: String,
        /// A write-only field: the value is masked and the projection never had
        /// it, so the placeholder says whether one is configured.
        secret: bool,
        configured: bool,
        disabled: bool,
    },
    Slider {
        path: String,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    /// A two-or-more-way segmented switch. The selected index is the live
    /// value; a choice dispatches the same settings write a select would.
    Segmented {
        path: String,
        options: Vec<String>,
        selected: usize,
        disabled: bool,
    },
    /// A value the page reports but does not edit.
    ReadOnly { text: String },
    /// A grouped key-chip pill, with an optional trailing action drawn as an
    /// icon button (the Shortcuts page's read-only bindings and trash).
    KeyChips {
        keys: Vec<String>,
        action: Option<ActionModel>,
        /// The hover "add shortcut" action, after the trailing trash action.
        add_action: Option<ActionModel>,
    },
    /// The row's actions, in draw order. `Enter` runs the first one.
    Buttons(Vec<ActionModel>),
    /// A row with no control at all (a status or explanation row).
    None,
}

impl ControlModel {
    pub fn is_interactive(&self) -> bool {
        match self {
            ControlModel::Toggle { disabled, .. }
            | ControlModel::Select { disabled, .. }
            | ControlModel::Segmented { disabled, .. }
            | ControlModel::Text { disabled, .. } => !disabled,
            ControlModel::Slider { .. } => true,
            ControlModel::KeyChips { action, .. } => {
                action.as_ref().is_some_and(|action| !action.disabled)
            }
            ControlModel::ReadOnly { .. } | ControlModel::None => false,
            ControlModel::Buttons(actions) => actions.iter().any(|action| !action.disabled),
        }
    }
}

/// One row of a group.
#[derive(Clone, Debug, PartialEq)]
pub struct RowModel {
    /// Stable identity for focus/test lookups.
    pub id: String,
    pub label: String,
    pub hint: String,
    pub status: Option<(String, StatusKind)>,
    pub control: ControlModel,
    /// A row this build cannot perform; the hint/status say why.
    pub disabled: bool,
    /// The reset action, when the value is not the schema default.
    pub reset: Option<SettingsAction>,
    /// A leading colour chip (a theme channel or a palette row).
    pub swatch: Option<ColorU>,
    /// A full-width child block under the row (a form, a nested list). It is
    /// not itself focusable; the form's rows are.
    pub children: Vec<RowModel>,
}

impl RowModel {
    pub fn new(id: impl Into<String>, label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
            status: None,
            control: ControlModel::None,
            disabled: false,
            reset: None,
            swatch: None,
            children: Vec::new(),
        }
    }

    pub fn toggle(mut self, path: impl Into<String>, value: bool) -> Self {
        self.control = ControlModel::Toggle {
            path: path.into(),
            value,
            disabled: self.disabled,
        };
        self
    }

    pub fn select(
        mut self,
        path: impl Into<String>,
        options: Vec<String>,
        selected: usize,
    ) -> Self {
        self.control = ControlModel::Select {
            path: path.into(),
            options,
            selected,
            disabled: self.disabled,
        };
        self
    }

    pub fn text(
        mut self,
        id: impl Into<String>,
        value: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        self.control = ControlModel::Text {
            id: id.into(),
            value: value.into(),
            placeholder: placeholder.into(),
            secret: false,
            configured: false,
            disabled: self.disabled,
        };
        self
    }

    /// A segmented on/off (or wider) choice whose value is written through
    /// `path` — `app.send_immediately` in General's Execution section.
    pub fn segmented(
        mut self,
        path: impl Into<String>,
        options: Vec<String>,
        selected: usize,
    ) -> Self {
        self.control = ControlModel::Segmented {
            path: path.into(),
            selected: selected.min(options.len().saturating_sub(1)),
            options,
            disabled: self.disabled,
        };
        self
    }

    pub fn secret(
        mut self,
        id: impl Into<String>,
        value: impl Into<String>,
        configured: bool,
    ) -> Self {
        self.control = ControlModel::Text {
            id: id.into(),
            value: value.into(),
            placeholder: String::new(),
            secret: true,
            configured,
            disabled: self.disabled,
        };
        self
    }

    pub fn slider(
        mut self,
        path: impl Into<String>,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    ) -> Self {
        self.control = ControlModel::Slider {
            path: path.into(),
            value,
            min,
            max,
            step,
        };
        self
    }

    pub fn read_only(mut self, text: impl Into<String>) -> Self {
        self.control = ControlModel::ReadOnly { text: text.into() };
        self
    }

    /// A grouped key-chip control: the chords in one rounded pill, plus an
    /// optional trailing icon action. The action is honest about what the build
    /// can do — the Shortcuts page's trash reports that remapping is absent
    /// rather than deleting the binding.
    pub fn key_chips(mut self, keys: Vec<String>, action: Option<ActionModel>) -> Self {
        self.control = ControlModel::KeyChips {
            keys,
            action,
            add_action: None,
        };
        self
    }

    pub fn key_chips_with_add(
        mut self,
        keys: Vec<String>,
        action: Option<ActionModel>,
        add_action: Option<ActionModel>,
    ) -> Self {
        self.control = ControlModel::KeyChips {
            keys,
            action,
            add_action,
        };
        self
    }

    pub fn buttons(mut self, actions: Vec<ActionModel>) -> Self {
        self.control = ControlModel::Buttons(actions);
        self
    }

    pub fn with_status(mut self, kind: StatusKind, text: impl Into<String>) -> Self {
        self.status = Some((text.into(), kind));
        self
    }

    /// Set the row's control directly, for a control the builder methods do not
    /// cover.
    pub fn with_control(mut self, control: ControlModel) -> Self {
        self.control = control;
        self
    }

    /// A leading colour chip before the label.
    pub fn with_swatch(mut self, color: ColorU) -> Self {
        self.swatch = Some(color);
        self
    }

    pub fn with_reset(mut self, action: SettingsAction) -> Self {
        self.reset = Some(action);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self.control = match self.control {
            ControlModel::Toggle { path, value, .. } => ControlModel::Toggle {
                path,
                value,
                disabled: true,
            },
            ControlModel::Select {
                path,
                options,
                selected,
                ..
            } => ControlModel::Select {
                path,
                options,
                selected,
                disabled: true,
            },
            ControlModel::Segmented {
                path,
                options,
                selected,
                ..
            } => ControlModel::Segmented {
                path,
                options,
                selected,
                disabled: true,
            },
            ControlModel::Text {
                id,
                value,
                placeholder,
                secret,
                configured,
                ..
            } => ControlModel::Text {
                id,
                value,
                placeholder,
                secret,
                configured,
                disabled: true,
            },
            other => other,
        };
        self
    }

    /// The action `Enter` runs on this row.
    pub fn primary_action(&self) -> Option<SettingsAction> {
        match &self.control {
            ControlModel::Toggle { path, value, .. } => Some(SettingsAction::SetToggle {
                path: path.clone(),
                value: !value,
            }),
            ControlModel::Select {
                path,
                options,
                selected,
                ..
            } => {
                if path == "theme.preset" {
                    // The theme row is a real dropdown, not a cycle: Enter puts
                    // its menu up (the renderer's anchored panel); the menu
                    // itself commits through `SetSelect`.
                    Some(SettingsAction::ToggleThemeSelect(true))
                } else if options.is_empty() {
                    None
                } else {
                    Some(SettingsAction::SetSelect {
                        path: path.clone(),
                        index: (selected + 1) % options.len(),
                    })
                }
            }
            ControlModel::Text { id, .. } => Some(SettingsAction::CommitField { id: id.clone() }),
            ControlModel::Segmented {
                path,
                options,
                selected,
                disabled,
            } => {
                if *disabled || options.is_empty() {
                    None
                } else {
                    Some(segment_action(path, (*selected + 1) % options.len()))
                }
            }
            ControlModel::Slider { .. } => None,
            ControlModel::KeyChips { action, .. } => action
                .as_ref()
                .filter(|action| !action.disabled)
                .map(|action| action.action.clone()),
            ControlModel::Buttons(actions) => actions
                .iter()
                .find(|action| !action.disabled)
                .map(|action| action.action.clone()),
            ControlModel::ReadOnly { .. } | ControlModel::None => None,
        }
    }

    /// The action `Left`/`Right` runs on this row, if the control has discrete
    /// values (a select steps, a slider steps by `step`).
    pub fn adjust_action(&self, delta: i32) -> Option<SettingsAction> {
        let delta = delta.signum();
        if delta == 0 {
            return None;
        }
        match &self.control {
            ControlModel::Select {
                path,
                options,
                selected,
                disabled,
            } => {
                if path == "theme.preset" || *disabled || options.is_empty() {
                    // The theme menu owns Left/Right while it is open; a closed
                    // row does not silently cycle the palette.
                    return None;
                }
                let count = options.len() as i64;
                let next = (*selected as i64 + delta as i64).rem_euclid(count) as usize;
                Some(SettingsAction::SetSelect {
                    path: path.clone(),
                    index: next,
                })
            }
            ControlModel::Slider {
                path,
                value,
                min,
                max,
                step,
            } => {
                let next = (*value + *step * delta as f32).clamp(*min, *max);
                Some(SettingsAction::SetSlider {
                    path: path.clone(),
                    value: next,
                })
            }
            ControlModel::Segmented {
                path,
                options,
                selected,
                disabled,
            } => {
                if *disabled || options.is_empty() {
                    return None;
                }
                let count = options.len() as i64;
                let next = (*selected as i64 + delta as i64).rem_euclid(count) as usize;
                Some(segment_action(path, next))
            }
            _ => None,
        }
    }
}

/// The write a segmented choice dispatches. `app.send_immediately` keeps its
/// dedicated action (the composer reads the same flag), while every other
/// segmented path is a normal select write.
pub fn segment_action(path: &str, index: usize) -> SettingsAction {
    if path == "app.send_immediately" {
        SettingsAction::SetSendImmediately(index != 0)
    } else {
        SettingsAction::SetSelect {
            path: path.to_string(),
            index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segmented_row_toggles_between_its_choices() {
        let row = RowModel::new("execution.queued", "Queued Messages", "").segmented(
            "app.send_immediately",
            vec!["Queue".to_string(), "Send Immediately".to_string()],
            0,
        );
        assert!(row.control.is_interactive());
        assert_eq!(
            row.primary_action(),
            Some(SettingsAction::SetSendImmediately(true))
        );
        assert_eq!(
            row.adjust_action(1),
            Some(SettingsAction::SetSendImmediately(true))
        );

        let row = row.segmented(
            "app.send_immediately",
            vec!["Queue".to_string(), "Send Immediately".to_string()],
            1,
        );
        assert_eq!(
            row.primary_action(),
            Some(SettingsAction::SetSendImmediately(false))
        );
        assert_eq!(
            row.adjust_action(-1),
            Some(SettingsAction::SetSendImmediately(false))
        );
    }
}

/// A section that folds: its heading is the trigger and its rows are drawn
/// only while it is open. The open state lives in the settings state (a fold id
/// in `SettingsUiState::folds`), so it survives a pane rebuild.
#[derive(Clone, Debug, PartialEq)]
pub struct FoldSection {
    pub open: bool,
    /// The action the heading dispatches (`ToggleFold(<id>)`).
    pub action: SettingsAction,
}

/// A titled group of rows.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionModel {
    pub id: String,
    pub title: String,
    pub description: String,
    pub header_action: Option<ActionModel>,
    pub rows: Vec<RowModel>,
    /// Draw the group as rows against the page instead of a card (a plain
    /// section).
    pub plain: bool,
    /// A collapsible heading (the Providers page's per-CLI groups).
    pub fold: Option<FoldSection>,
}

impl SectionModel {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            header_action: None,
            rows: Vec::new(),
            plain: false,
            fold: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_header_action(mut self, action: ActionModel) -> Self {
        self.header_action = Some(action);
        self
    }

    pub fn with_row(mut self, row: RowModel) -> Self {
        self.rows.push(row);
        self
    }

    pub fn with_rows(mut self, rows: impl IntoIterator<Item = RowModel>) -> Self {
        self.rows.extend(rows);
        self
    }

    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    /// Draw the section as a fold whose heading dispatches `action` and whose
    /// rows appear only while `open`.
    pub fn with_fold(mut self, open: bool, action: SettingsAction) -> Self {
        self.fold = Some(FoldSection { open, action });
        self
    }

    /// Whether the section's rows are drawn (always for a non-fold section).
    pub fn is_open(&self) -> bool {
        self.fold.as_ref().map(|fold| fold.open).unwrap_or(true)
    }

    /// Every row in this section, in draw order, children flattened after their
    /// parent (a child block is part of the same reading order).
    fn all_rows(&self) -> Vec<&RowModel> {
        let mut rows = Vec::new();
        for row in &self.rows {
            rows.push(row);
            rows.extend(row.children.iter());
        }
        rows
    }
}

/// Which page-level search a [`PageSearch`] drives. The actions live on the
/// model rather than in the renderer, so the page and its keys share one
/// definition of what typing in the field means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSearchKind {
    Shortcuts,
}

impl PageSearchKind {
    pub fn change_action(self, value: String) -> SettingsAction {
        match self {
            PageSearchKind::Shortcuts => SettingsAction::ShortcutsSearch(value),
        }
    }

    pub fn focus_action(self, focused: bool) -> SettingsAction {
        match self {
            PageSearchKind::Shortcuts => SettingsAction::ShortcutsSearchFocus(focused),
        }
    }

    pub fn clear_action(self) -> SettingsAction {
        match self {
            PageSearchKind::Shortcuts => SettingsAction::ShortcutsClearSearch,
        }
    }
}

/// A search field at the top of a page, above its sections. The value and the
/// caret's focus are app-owned (the tree is rebuilt every frame).
#[derive(Clone, Debug, PartialEq)]
pub struct PageSearch {
    pub id: String,
    pub value: String,
    pub placeholder: String,
    pub focused: bool,
    pub kind: PageSearchKind,
}

impl PageSearch {
    pub fn shortcuts(value: impl Into<String>, focused: bool) -> Self {
        Self {
            id: "shortcuts-search".to_string(),
            value: value.into(),
            placeholder: "Search shortcuts...".to_string(),
            focused,
            kind: PageSearchKind::Shortcuts,
        }
    }
}

/// One page: its sections plus the header line under the title.
#[derive(Clone, Debug, PartialEq)]
pub struct PageModel {
    pub sections: Vec<SectionModel>,
    /// A page-level search field drawn above the sections.
    pub search: Option<PageSearch>,
}

impl PageModel {
    pub fn new(sections: Vec<SectionModel>) -> Self {
        Self {
            sections,
            search: None,
        }
    }

    pub fn with_search(mut self, search: PageSearch) -> Self {
        self.search = Some(search);
        self
    }

    /// The interactive rows, in draw order — the page's focus order. A folded
    /// section contributes nothing while closed, matching what is drawn.
    pub fn interactive_rows(&self) -> Vec<&RowModel> {
        self.sections
            .iter()
            .filter(|section| section.is_open())
            .flat_map(SectionModel::all_rows)
            .filter(|row| !row.disabled && row.control.is_interactive())
            .collect()
    }

    pub fn control_count(&self) -> usize {
        self.interactive_rows().len()
    }

    /// The row at focus slot `index`.
    pub fn row_at(&self, index: usize) -> Option<&RowModel> {
        self.interactive_rows().into_iter().nth(index)
    }
}

/// The file a scalar path belongs to. Paths are dotted (`app.friendly_name`);
/// the first segment names the section, and every scalar row the settings
/// surface edits lives in `config.toml`.
pub fn path_file(_path: &str) -> ConfigFile {
    ConfigFile::Config
}

/// Split a dotted path into its segments.
pub fn path_segments(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}
