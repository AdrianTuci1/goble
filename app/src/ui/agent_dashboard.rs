//! The agent-management panel: the topbar's dashboard.
//!
//! A roster of every conversation this app holds, grouped by state or by the
//! directory the work happens in, with grok-build's dashboard controls: reply
//! to a row without leaving the panel, pin it, rename it, stop its turn, open
//! it or delete it. The bottom card is the selected agent's output, rendered as
//! markdown, over the harness's prompt line: an arrow and the text the user
//! writes, with no box around it. The line is always there and always full
//! width, replies to the selected row or dispatches a new agent, and the card
//! stays at prompt height until a conversation is selected or a dispatch lands.
//! The header counts the roster by state. The dashboard fills the workspace body
//! below the topbar, in place of the panes, beside the tools sidebar.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, ConversationEntry, ConversationStatus,
    CrossAxisAlignment, Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow, Icon,
    KeyHandler, MainAxisSize, Padding, Scrollable, Spacer, Text, TextInput, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::{ChatMessage, ChatMessageBubble, ChatRole};

use super::{AgentDashboardAction, AgentDashboardGrouping, UiActions, UiSnapshot};
use crate::state::InboxKind;

/// The key chord the footer names for a shortcut. The window's own chords are
/// Command on macOS and Ctrl elsewhere.
#[cfg(target_os = "macos")]
const CHORD: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const CHORD: &str = "Ctrl+";

/// The five states a roster row can be in, in the order the list sorts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RowState {
    /// A permission or a question is waiting on the user.
    Awaiting,
    /// A turn is running.
    Working,
    /// Nothing is running and nothing is left unreported.
    Idle,
    /// The last turn finished.
    Done,
    /// The last turn failed.
    Failed,
}

impl RowState {
    /// Every state, in roster order.
    const ALL: [RowState; 5] = [
        Self::Awaiting,
        Self::Working,
        Self::Idle,
        Self::Done,
        Self::Failed,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Awaiting => "Awaiting your input",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Done => "Done",
            Self::Failed => "Failed",
        }
    }

    /// The row's status mark: grok's diamond. The state itself is carried by
    /// the mark's colour, with a hollow diamond for a row that is at rest.
    fn glyph(self) -> &'static str {
        match self {
            Self::Awaiting => "◈",
            Self::Working | Self::Done | Self::Failed => "◆",
            Self::Idle => "◇",
        }
    }

    /// The label the header's count chips read: one word, so all five fit the
    /// header band together.
    fn short_label(self) -> &'static str {
        match self {
            Self::Awaiting => "awaiting",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn color(self) -> ColorToken {
        match self {
            Self::Awaiting => ColorToken::Warning,
            Self::Working => ColorToken::Accent,
            Self::Idle => ColorToken::Muted,
            Self::Done => ColorToken::Success,
            Self::Failed => ColorToken::Error,
        }
    }
}

/// The state one conversation is in: a pending approval or question wins over
/// the turn's own status, because that is the thing the row is waiting on.
fn row_state(state: &UiSnapshot, entry: &ConversationEntry) -> RowState {
    let awaiting = state.inbox_items.iter().any(|item| {
        !item.acknowledged
            && item.conversation_id == entry.id
            && matches!(item.kind, InboxKind::Approval | InboxKind::Question)
    });
    if awaiting {
        return RowState::Awaiting;
    }
    match entry.status {
        ConversationStatus::Waiting => RowState::Working,
        ConversationStatus::Error => RowState::Failed,
        ConversationStatus::Success => RowState::Done,
        ConversationStatus::Stopped | ConversationStatus::Default => RowState::Idle,
    }
}

/// What a row's activity column says: the pending question or approval when
/// there is one, the latest response otherwise.
fn row_activity(state: &UiSnapshot, entry: &ConversationEntry) -> String {
    state
        .inbox_items
        .iter()
        .rev()
        .find(|item| {
            !item.acknowledged
                && item.conversation_id == entry.id
                && matches!(item.kind, InboxKind::Approval | InboxKind::Question)
        })
        .map(|item| preview(&item.title, 80))
        .unwrap_or_else(|| preview(&entry.last_response, 80))
}

/// One-line text for the roster and the peek panel: every line break and run of
/// whitespace becomes a single space, and a long response is cut at `limit`
/// characters. A `Text` clamps its measured height, not its painted run, so a
/// multi-line preview would otherwise paint past its row.
fn preview(text: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut spaced = false;
    let mut truncated = false;
    for character in text.chars() {
        if count == limit {
            truncated = true;
            break;
        }
        if character.is_whitespace() {
            if out.is_empty() || spaced {
                continue;
            }
            spaced = true;
            out.push(' ');
        } else {
            spaced = false;
            out.push(character);
        }
        count += 1;
    }
    if truncated {
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('…');
    }
    out
}

fn is_pinned(state: &UiSnapshot, entry: &ConversationEntry) -> bool {
    state.starred.iter().any(|id| id == &entry.id)
}

/// The rows the roster draws for the current filter, pinned first and then by
/// state, each group in the order the conversations were listed in.
fn visible_rows(state: &UiSnapshot) -> Vec<&ConversationEntry> {
    let query = state.agent_dashboard.query.trim().to_lowercase();
    let mut rows: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| {
            // A conversation the agent has not answered yet is a tab the user
            // opened, not a thread with anything in it; the roster lists what
            // the sidebar lists, so both hide it.
            entry.has_agent_reply
                && (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.directory.to_lowercase().contains(&query)
                    || entry.folder.to_lowercase().contains(&query)
                    || entry.last_response.to_lowercase().contains(&query))
        })
        .collect();
    rows.sort_by_key(|entry| (!is_pinned(state, entry), row_state(state, entry)));
    rows
}

/// The roster split into the sections the current grouping draws, with each
/// group's label. Groups with no row are absent.
fn grouped_rows<'a>(
    state: &'a UiSnapshot,
    rows: &[&'a ConversationEntry],
) -> Vec<(String, Vec<&'a ConversationEntry>)> {
    match state.agent_dashboard.grouping {
        AgentDashboardGrouping::State => RowState::ALL
            .iter()
            .filter_map(|state_row| {
                let entries: Vec<&ConversationEntry> = rows
                    .iter()
                    .copied()
                    .filter(|entry| row_state(state, entry) == *state_row)
                    .collect();
                (!entries.is_empty()).then(|| (state_row.label().to_string(), entries))
            })
            .collect(),
        AgentDashboardGrouping::Directory => {
            let label = |entry: &ConversationEntry| -> String {
                let directory = entry.directory.trim();
                if directory.is_empty() {
                    return "No directory".to_string();
                }
                crate::state::display_path(directory)
            };
            let mut groups: std::collections::BTreeMap<String, Vec<&ConversationEntry>> =
                std::collections::BTreeMap::new();
            for entry in rows {
                groups.entry(label(entry)).or_default().push(*entry);
            }
            groups.into_iter().collect()
        }
    }
}

/// The panel's content. [`super::build`] puts it in the workspace body below
/// the topbar while it is open, in place of the panes.
pub fn build_agent_dashboard(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let dashboard = &state.agent_dashboard;

    let rows = visible_rows(state);
    let groups = grouped_rows(state, &rows);
    // The card follows the cursor even when the row is not in the roster yet:
    // a just-dispatched conversation has no answer, so the list hides it while
    // the card is what shows it is running.
    let selected = dashboard
        .selected
        .as_ref()
        .and_then(|id| state.conversations.iter().find(|entry| &entry.id == id));

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);

    column = column.with_child(header(app, state, actions, &rows, spacing));
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(search_field(app, state, actions));
    // The roster and the response card share the height under the filter. The
    // card grows only while a row is selected, when it holds the formatted
    // output; with no cursor it keeps its own prompt-height and leaves the
    // roster the rest.
    column = column.with_child(
        Expanded::new(roster(app, state, actions, &groups, sm, xs))
            .with_flex(2.0)
            .finish(),
    );
    let card = response_card(app, state, actions, selected);
    column = column.with_child(if selected.is_some() {
        Expanded::new(card).with_flex(3.0).finish()
    } else {
        card
    });
    // grok-build's own hint row: key first, action second, one pipe between
    // entries. The chords name Ctrl because that is the harness's own habit,
    // and the panel answers both Ctrl and Command.
    column = column.with_child(
        Text::new(
            "Enter: open | Tab: list | Esc: new agent | Ctrl+x: stop | Ctrl+t: pin | \
             Ctrl+r: rename | Ctrl+⌫: delete | Ctrl+/: search",
        )
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish(),
    );

    let on_action = actions.on_agent_dashboard.clone();
    let on_select = actions.on_select_conversation.clone();
    let on_star = actions.on_toggle_star.clone();

    // What Escape steps back through, captured now so the handler sees the
    // frame the user pressed the key in. The panel owns the keyboard while it
    // is up; a focused field keeps every key it needs.
    let typing = dashboard.search_focused
        || dashboard.reply_focused
        || dashboard.dispatch_focused
        || dashboard.rename_focused;
    let renaming = dashboard.rename.is_some();
    let selected_id = dashboard.selected.clone();
    let reply_draft = dashboard.reply.clone();
    let dispatch_draft = dashboard.dispatch.clone();
    let query = dashboard.query.clone();
    let reply_focused = dashboard.reply_focused;
    let search_focused = dashboard.search_focused;
    let dispatch_open = dashboard.dispatch_open;
    let visible_ids: Vec<String> = rows.iter().map(|entry| entry.id.clone()).collect();

    KeyHandler::new(
        Container::new(column.finish())
            // The panel is the workspace's black, not a raised surface: it
            // reads as the harness itself rather than as a card floating on
            // top of the workspace.
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
        move |key: &str, modifiers| {
            let modified = modifiers.ctrl || modifiers.command;
            if key == "Tab" {
                // grok-build's `Tab: list`: the keyboard leaves whichever
                // field has it and goes back to the roster, where the arrows
                // walk the rows.
                (on_action.borrow_mut())(AgentDashboardAction::ReplyFocus(false));
                (on_action.borrow_mut())(AgentDashboardAction::DispatchFocus(false));
                (on_action.borrow_mut())(AgentDashboardAction::SearchFocus(false));
                return true;
            }
            if key == "Escape" {
                // Fields give their draft back first; a step past the row is
                // grok-build's `Esc: New Agent`.
                if renaming {
                    (on_action.borrow_mut())(AgentDashboardAction::CancelRename);
                } else if !reply_draft.is_empty() {
                    (on_action.borrow_mut())(AgentDashboardAction::SetReplyDraft(String::new()));
                    (on_action.borrow_mut())(AgentDashboardAction::ReplyFocus(false));
                } else if reply_focused {
                    (on_action.borrow_mut())(AgentDashboardAction::ReplyFocus(false));
                } else if !query.is_empty() {
                    (on_action.borrow_mut())(AgentDashboardAction::Search(String::new()));
                    (on_action.borrow_mut())(AgentDashboardAction::SearchFocus(false));
                } else if search_focused {
                    (on_action.borrow_mut())(AgentDashboardAction::SearchFocus(false));
                } else if !dispatch_draft.is_empty() {
                    (on_action.borrow_mut())(AgentDashboardAction::SetDispatchDraft(String::new()));
                    (on_action.borrow_mut())(AgentDashboardAction::DispatchFocus(false));
                } else {
                    (on_action.borrow_mut())(AgentDashboardAction::NewAgent);
                }
                return true;
            }
            if typing {
                return false;
            }
            if modified {
                match key.to_ascii_lowercase().as_str() {
                    "o" => {
                        (on_action.borrow_mut())(AgentDashboardAction::ToggleDispatch);
                        true
                    }
                    "t" => {
                        if let Some(id) = &selected_id {
                            (on_star.borrow_mut())(id.clone());
                        }
                        true
                    }
                    "x" => {
                        if let Some(id) = &selected_id {
                            (on_action.borrow_mut())(AgentDashboardAction::Stop(id.clone()));
                        }
                        true
                    }
                    // Deleting has no control of its own in the card, so the
                    // chord stands in for the button grok-build's dashboard
                    // keeps out of the response area. The action keeps its
                    // two-step arm: the row says so on the first press.
                    "backspace" => {
                        if let Some(id) = &selected_id {
                            (on_action.borrow_mut())(AgentDashboardAction::Delete(id.clone()));
                        }
                        true
                    }
                    "g" => {
                        (on_action.borrow_mut())(AgentDashboardAction::ToggleGrouping);
                        true
                    }
                    "r" => {
                        if let Some(id) = &selected_id {
                            (on_action.borrow_mut())(AgentDashboardAction::BeginRename(id.clone()));
                        }
                        true
                    }
                    "/" => {
                        (on_action.borrow_mut())(AgentDashboardAction::SearchFocus(true));
                        true
                    }
                    _ => false,
                }
            } else {
                match key {
                    "ArrowDown" | "j" => {
                        (on_action.borrow_mut())(AgentDashboardAction::Select(step(
                            &visible_ids,
                            selected_id.as_deref(),
                            1,
                        )));
                        true
                    }
                    "ArrowUp" | "k" => {
                        (on_action.borrow_mut())(AgentDashboardAction::Select(step(
                            &visible_ids,
                            selected_id.as_deref(),
                            -1,
                        )));
                        true
                    }
                    "Enter" => {
                        if let Some(id) = &selected_id {
                            (on_select.borrow_mut())(id.clone());
                            (on_action.borrow_mut())(AgentDashboardAction::Close);
                        } else if dispatch_open {
                            (on_action.borrow_mut())(AgentDashboardAction::DispatchFocus(true));
                        } else {
                            (on_action.borrow_mut())(AgentDashboardAction::ToggleDispatch);
                        }
                        true
                    }
                    "/" => {
                        (on_action.borrow_mut())(AgentDashboardAction::SearchFocus(true));
                        true
                    }
                    _ => false,
                }
            }
        },
    )
    .finish()
}

/// The row one arrow-key step from `selected`: the first row when nothing is
/// selected, clamped at both ends.
fn step(ids: &[String], selected: Option<&str>, delta: i32) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    let current = selected.and_then(|id| ids.iter().position(|candidate| candidate == id));
    let next = match (current, delta.signum()) {
        (None, -1) => ids.len() - 1,
        (None, _) => 0,
        (Some(index), _) => (index as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize,
    };
    Some(ids[next].clone())
}

/// The panel's title band: the new-agent control, the state counts, and the
/// close button at the far right edge.
fn header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    rows: &[&ConversationEntry],
    spacing: f32,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let mut counts = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5);
    for row_state in RowState::ALL {
        let count = rows
            .iter()
            .filter(|entry| self::row_state(state, entry) == row_state)
            .count();
        if count == 0 {
            continue;
        }
        counts = counts.with_child(
            Text::new(format!(
                "{} {count} {}",
                row_state.glyph(),
                row_state.short_label()
            ))
            .with_font_size(11.0)
            .with_theme_color(row_state.color(), app)
            .with_max_lines(1)
            .finish(),
        );
    }

    let on_new = actions.on_agent_dashboard.clone();
    let new_agent = Button::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm * 0.5)
            .with_child(
                Icon::new("plus")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Accent, app)
                    .finish(),
            )
            .with_child(
                Text::new("New Agent")
                    .with_font_size(13.0)
                    .with_theme_color(
                        if state.agent_dashboard.dispatch_open {
                            ColorToken::Accent
                        } else {
                            ColorToken::Text
                        },
                        app,
                    )
                    .finish(),
            )
            .finish(),
    )
    .with_variant(ButtonVariant::Ghost)
    .with_on_click(move || {
        // The control is the dispatch mode's handle: one press clears the
        // cursor and puts the caret in the prompt line, the next drops the
        // draft and gives the keyboard back. The line itself stays drawn.
        (on_new.borrow_mut())(AgentDashboardAction::ToggleDispatch);
    })
    .finish();

    let on_close = actions.on_agent_dashboard.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(18.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(30.0)
    .with_corner_radius(0.0)
    .with_on_click(move || {
        (on_close.borrow_mut())(AgentDashboardAction::Close);
    })
    .finish();

    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(new_agent)
        .with_child(counts.finish())
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish()
}

/// The roster's filter field. Enter puts the keyboard back on the list.
fn search_field(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_change = actions.on_agent_dashboard.clone();
    let on_focus = actions.on_agent_dashboard.clone();
    let on_submit = actions.on_agent_dashboard.clone();
    let dashboard = &state.agent_dashboard;
    let field = TextInput::new()
        .with_value(dashboard.query.clone())
        .with_placeholder("Search agents by name, directory or response")
        .with_focused(dashboard.search_focused)
        .with_full_width(true)
        .with_on_change(move |value| {
            (on_change.borrow_mut())(AgentDashboardAction::Search(value));
        })
        .with_on_focus_change(move |focused: bool| {
            (on_focus.borrow_mut())(AgentDashboardAction::SearchFocus(focused));
        })
        .with_on_submit(move || {
            (on_submit.borrow_mut())(AgentDashboardAction::SearchFocus(false));
        })
        .finish();

    let _ = app;
    field
}

/// The scrollable roster: one section per group, one row per conversation.
fn roster(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    groups: &[(String, Vec<&ConversationEntry>)],
    sm: f32,
    xs: f32,
) -> Box<dyn Element> {
    if groups.is_empty() {
        let label = if state.agent_dashboard.query.trim().is_empty() {
            "No agents yet. Dispatch one below to start a conversation."
        } else {
            "No agent matches this search."
        };
        return Container::new(
            Text::new(label)
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(sm))
        .finish();
    }

    let mut list = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    for (index, (label, entries)) in groups.iter().enumerate() {
        list = list.with_child(section_header(app, label, entries.len(), sm, xs));
        for entry in entries {
            let selected = state.agent_dashboard.selected.as_deref() == Some(entry.id.as_str());
            let armed = state.agent_dashboard.confirm_delete.as_deref() == Some(entry.id.as_str());
            list = list.with_child(roster_row(app, state, actions, entry, selected, armed, sm));
        }
        if index + 1 < groups.len() {
            list = list.with_child(Empty::new().with_size(goble_ui::vec2f(0.0, xs)).finish());
        }
    }

    Scrollable::new(list.finish(), Axis::Vertical)
        .with_state(state.agent_dashboard.scroll.clone())
        .finish()
}

/// One group's title: its chevron, name, count and the rule that runs to the
/// panel's right edge, the way grok-build's dashboard heads each group.
fn section_header(
    app: &AppContext,
    label: &str,
    count: usize,
    sm: f32,
    xs: f32,
) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm * 0.75)
            .with_child(
                Text::new("▾")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(label.to_string())
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(count.to_string())
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(Expanded::new(Divider::horizontal().finish()).finish())
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, xs, 0.0, xs))
    .finish()
}

/// The last path segment of the directory the conversation works in: the
/// roster's stand-in for grok-build's branch chip.
fn directory_chip(entry: &ConversationEntry) -> Option<String> {
    let directory = entry.directory.trim();
    if directory.is_empty() {
        return None;
    }
    let display = crate::state::display_path(directory);
    let segment = display.rsplit('/').find(|part| !part.is_empty())?;
    (!segment.is_empty()).then(|| segment.to_string())
}

/// One roster row: the state glyph, name, directory and age over the agent's
/// latest activity, the way grok-build's dashboard lists a conversation.
fn roster_row(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    entry: &ConversationEntry,
    selected: bool,
    armed: bool,
    sm: f32,
) -> Box<dyn Element> {
    let row_state = row_state(state, entry);
    let pinned = is_pinned(state, entry);
    let hovered = state.agent_dashboard.hovered_row.borrow().as_deref() == Some(entry.id.as_str());

    let mut title = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.75)
        .with_child(
            Text::new(row_state.glyph())
                .with_font_size(13.0)
                .with_theme_color(row_state.color(), app)
                .finish(),
        )
        .with_child(
            Text::new(preview(&entry.name, 72))
                .with_font_size(14.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    if let Some(chip) = directory_chip(entry) {
        title = title.with_child(
            Text::new(format!("· {chip}"))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    title = title.with_child(Spacer::new().finish());
    if pinned {
        title = title.with_child(
            Icon::new("star-filled")
                .with_size(12.0)
                .with_theme_color(ColorToken::Warning, app)
                .finish(),
        );
    }
    if hovered {
        // Under the pointer the row names its state and swaps the age for the
        // way out of the conversation, the way grok-build's dashboard does.
        title = title.with_child(
            Text::new(row_state.label())
                .with_font_size(11.0)
                .with_theme_color(row_state.color(), app)
                .with_max_lines(1)
                .finish(),
        );
        title = title.with_child(close_control(app, actions, entry, armed));
    } else {
        title = title.with_child(
            Text::new(if armed {
                format!("{CHORD}⌫ again")
            } else {
                entry.timestamp.clone()
            })
            .with_font_size(12.0)
            .with_theme_color(
                if armed {
                    ColorToken::Error
                } else {
                    ColorToken::Muted
                },
                app,
            )
            .with_max_lines(1)
            .finish(),
        );
    }

    // The activity sits under the name, indented past the glyph, so the row
    // reads as a titled item rather than a single squeezed line.
    let activity = Padding::new(
        Text::new(row_activity(state, entry))
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .with_max_lines(1)
            .finish(),
        EdgeInsets::new(18.0, 0.0, 0.0, 0.0),
    )
    .finish();

    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(3.0)
        .with_child(title.finish())
        .with_child(activity)
        .finish();

    let on_select = actions.on_agent_dashboard.clone();
    let id = entry.id.clone();
    HoverRow::new(content)
        .with_selected(selected)
        .with_hover_key(state.agent_dashboard.hovered_row.clone(), entry.id.clone())
        .with_padding(EdgeInsets::new(10.0, 7.0, 10.0, 7.0))
        .with_corner_radius(4.0)
        .with_on_click(move || {
            (on_select.borrow_mut())(AgentDashboardAction::Select(Some(id.clone())));
        })
        .finish()
}

/// The close control a hovered row shows in place of its age: one press arms
/// the delete, the next removes the conversation.
fn close_control(
    app: &AppContext,
    actions: &UiActions,
    entry: &ConversationEntry,
    armed: bool,
) -> Box<dyn Element> {
    let id = entry.id.clone();
    let on_delete = actions.on_agent_dashboard.clone();
    TopbarButton::new(
        Icon::new("close")
            .with_size(12.0)
            .with_theme_color(
                if armed {
                    ColorToken::Error
                } else {
                    ColorToken::Muted
                },
                app,
            )
            .finish(),
    )
    .with_size(20.0)
    .with_corner_radius(4.0)
    .with_on_click(move || {
        (on_delete.borrow_mut())(AgentDashboardAction::Delete(id.clone()));
    })
    .finish()
}

/// The response card: the selected agent's name and age over its formatted
/// output, the harness's full-width prompt line under it, and the model the
/// draft runs on in the footer. It is grok-build's output box: the card owns
/// the surface, and nothing inside it is a button. Without a selection the card
/// keeps the prompt's own height; a dispatch on Enter gives it a conversation
/// to follow and the output takes the tall share.
fn response_card(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected: Option<&ConversationEntry>,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let dashboard = &state.agent_dashboard;
    let reply = selected.is_some();
    let renaming = selected.is_some_and(|entry| {
        dashboard
            .rename
            .as_ref()
            .is_some_and(|(id, _)| id == &entry.id)
    });

    let mut head = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(match selected {
                Some(entry) => preview(&entry.name, 48),
                None if dashboard.dispatch_open => "New agent".to_string(),
                None => "No agent selected".to_string(),
            })
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .with_max_lines(1)
            .finish(),
        )
        .with_child(Spacer::new().finish());
    if let Some(entry) = selected {
        head = head.with_child(
            Text::new(entry.timestamp.clone())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }

    let body: Box<dyn Element> = if renaming {
        rename_field(app, state, actions)
    } else if let Some(entry) = selected {
        // The agent's output is markdown, so it is rendered the way the
        // transcript renders a reply: headings, lists, code fences and inline
        // emphasis keep their shape, under the card's own scroll.
        let response: Box<dyn Element> = if entry.last_response.trim().is_empty() {
            Text::new("No response yet.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish()
        } else {
            let message =
                ChatMessage::from_markdown(ChatRole::Assistant, entry.last_response.clone());
            // The bubble sizes to its widest line; the row gives it the
            // card's whole width so paragraphs wrap at the card's edge.
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_child(
                    Expanded::new(Box::new(ChatMessageBubble::new(
                        message.role,
                        message.fragments,
                    )))
                    .finish(),
                )
                .finish()
        };
        Scrollable::new(response, Axis::Vertical)
            .with_state(dashboard.response_scroll.clone())
            .finish()
    } else {
        Text::new("Select an agent to read its response here.")
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish()
    };

    // The footer names the turn's model and approval policy the way
    // grok-build's does: plain text at the right end of a rule, with no pill
    // to click.
    let mode = state.approval_mode;
    let mut footer_parts: Vec<String> = Vec::new();
    if !state.selected_model.is_empty() {
        footer_parts.push(state.selected_model.clone());
    }
    footer_parts.push(mode.label().to_string());
    let footer = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(Expanded::new(Divider::horizontal().finish()).finish())
        .with_child(
            Text::new(footer_parts.join(" · "))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );

    let mut column = Flex::column()
        .with_main_axis_size(if reply {
            MainAxisSize::Max
        } else {
            MainAxisSize::Min
        })
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(xs)
        .with_child(head.finish());
    column = if reply {
        column.with_child(Expanded::new(body).finish())
    } else {
        column.with_child(body)
    };
    // The prompt line is always there, full width: an empty panel still lets
    // the user click and type a dispatch without raising anything first.
    column = column.with_child(prompt_line(app, state, actions, reply));
    column = column.with_child(footer.finish());

    Container::new(column.finish())
        // The agent's output sits on the panel's own black, not on a raised
        // graphite card: it reads as the panel's content, the way grok-build's
        // dashboard draws its output band.
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_padding(EdgeInsets::uniform(sm))
        .with_corner_radius(6.0)
        .finish()
}

/// The harness's prompt line: the accent arrow and the text the user writes,
/// with no box around it. The field is plain, so the line reads as part of the
/// output above rather than as a separate control.
fn prompt_line(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    reply: bool,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let dashboard = &state.agent_dashboard;
    let (value, focused, placeholder) = if reply {
        (
            dashboard.reply.clone(),
            dashboard.reply_focused,
            "Reply to this agent…",
        )
    } else {
        (
            dashboard.dispatch.clone(),
            dashboard.dispatch_focused,
            "Dispatch a new agent…",
        )
    };

    let on_change = actions.on_agent_dashboard.clone();
    let on_focus = actions.on_agent_dashboard.clone();
    let on_submit = actions.on_agent_dashboard.clone();
    let id = dashboard.selected.clone();
    let field = TextInput::new()
        .with_value(value)
        .with_placeholder(placeholder)
        .with_focused(focused)
        .with_full_width(true)
        .with_plain(true)
        .with_on_change(move |text| {
            (on_change.borrow_mut())(if reply {
                AgentDashboardAction::SetReplyDraft(text)
            } else {
                AgentDashboardAction::SetDispatchDraft(text)
            });
        })
        .with_on_focus_change(move |focused: bool| {
            (on_focus.borrow_mut())(if reply {
                AgentDashboardAction::ReplyFocus(focused)
            } else {
                AgentDashboardAction::DispatchFocus(focused)
            });
        })
        .with_on_submit(move || {
            (on_submit.borrow_mut())(match &id {
                Some(id) => AgentDashboardAction::Reply(id.clone()),
                None => AgentDashboardAction::Dispatch,
            });
        })
        .finish();

    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5)
        .with_child(
            Icon::new("prompt-arrow")
                .with_size(12.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(Expanded::new(field).finish())
        .finish()
}

/// The rename field: the draft name, saved with Enter and dropped with Escape.
fn rename_field(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let draft = state
        .agent_dashboard
        .rename
        .as_ref()
        .map(|(_, draft)| draft.clone())
        .unwrap_or_default();
    let on_change = actions.on_agent_dashboard.clone();
    let on_submit = actions.on_agent_dashboard.clone();
    let field = TextInput::new()
        .with_value(draft)
        .with_placeholder("Conversation name")
        .with_focused(state.agent_dashboard.rename_focused)
        .with_full_width(true)
        .with_on_change(move |value| {
            (on_change.borrow_mut())(AgentDashboardAction::RenameDraft(value));
        })
        .with_on_submit(move || {
            (on_submit.borrow_mut())(AgentDashboardAction::CommitRename);
        })
        .finish();

    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(xs)
        .with_child(field)
        .with_child(
            Text::new("Enter saves · Esc cancels")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{
        AppContext, ConversationEntry, ConversationStatus, Element, EventContext, LayoutContext,
        PaintContext, SizeConstraint,
    };
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{vec2f, RectF, Vector2F};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::theme::ColorToken;

    use crate::root_view::RootView;
    use crate::state::{InboxKind, UiState};

    fn window() -> Vector2F {
        vec2f(1280.0, 800.0)
    }

    fn root() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.composer_focused = false;
        }
        (Box::new(view), state, dir)
    }

    fn conversation(
        id: &str,
        name: &str,
        last_response: &str,
        status: ConversationStatus,
    ) -> ConversationEntry {
        ConversationEntry::new(id, name, last_response, "2m").with_status(status)
    }

    fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        key: &str,
        modifiers: ModifiersState,
    ) -> bool {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut paint = PaintContext::default();
        root.paint(vec2f(0.0, 0.0), &mut paint, app);
        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers,
            },
            &mut ctx,
            app,
        )
    }

    /// Lay the tree out, paint it, then press and release the primary pointer
    /// at `position` — the route a real press takes into the elements.
    fn click_at(root: &mut Box<dyn Element>, app: &AppContext, position: Vector2F) -> bool {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut paint = PaintContext::default();
        root.paint(vec2f(0.0, 0.0), &mut paint, app);
        let mut ctx = EventContext;
        let down = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let up = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position,
                button: 0,
            },
            &mut ctx,
            app,
        );
        down || up
    }

    #[test]
    fn the_panel_lists_the_roster_grouped_by_state() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![
                conversation(
                    "a",
                    "Fix the login flow",
                    "The fix is in.",
                    ConversationStatus::Default,
                ),
                conversation(
                    "b",
                    "Audit the diff",
                    "Audit finished.",
                    ConversationStatus::Default,
                ),
                conversation(
                    "c",
                    "Housekeeping",
                    "All clean.",
                    ConversationStatus::Default,
                )
                .with_has_agent_reply(false),
            ];
            s.agent_dashboard.open = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|text| text == "New Agent"),
            "the panel leads with the new-agent control: {texts:?}"
        );
        let panel_label = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New Agent" && origin.y > crate::ui::shell::TOPBAR_HEIGHT
            )
        });
        assert!(
            panel_label,
            "the new-agent control is the panel's own, not the toolbar's"
        );
        assert!(
            texts.iter().any(|text| text == "Fix the login flow"),
            "every answered conversation is listed: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Housekeeping"),
            "a conversation the agent has not answered is not listed: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "Done"),
            "the state grouping draws its Done header: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Idle"),
            "with nothing unanswered there is no Idle group: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "◆ 2 done"),
            "the header counts the answered conversations: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "◇ 1 idle"),
            "and counts no empty ones: {texts:?}"
        );
    }

    /// The roster's state mark sits on the row's title line: its centre is the
    /// name's centre, not the middle of the two-line item, which would put it
    /// between the name and the activity under it.
    #[test]
    fn the_roster_state_mark_sits_on_the_title_line() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Success,
            )];
            s.agent_dashboard.open = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let sidebar_width = state.borrow().sidebar_width;

        /// The vertical centre of the line a run was drawn in: its origin plus
        /// half the height it measured to. Only runs inside the panel count: the
        /// sidebar lists the same conversation beside it.
        fn run_centre(commands: &[RenderCommand], text: &str, sidebar_width: f32) -> f32 {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText {
                        text: drawn,
                        origin,
                        font_size,
                        line_height,
                        max_width,
                        ..
                    } if drawn == text && origin.x >= sidebar_width => {
                        let size = goble_ui::elements::text::measure_text(
                            drawn,
                            *font_size,
                            *line_height,
                            *max_width,
                        );
                        Some(origin.y + size.y / 2.0)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the roster draws {text:?}"))
        }

        let mark = run_centre(&commands, "◆", sidebar_width);
        let name = run_centre(&commands, "Fix the login flow", sidebar_width);
        let activity = run_centre(&commands, "The fix is in.", sidebar_width);
        assert!(
            (mark - name).abs() < 0.5,
            "the mark is centred on the name's line: mark {mark}, name {name}"
        );
        assert!(
            (mark - activity).abs() > 2.0,
            "and not between the name and the activity: mark {mark}, activity {activity}"
        );
    }

    /// The panel is the workspace body, not an overlay: it starts below the
    /// toolbar and to the right of the tools sidebar, and both of those stay
    /// drawn around it.
    #[test]
    fn the_panel_fills_the_pane_area_beside_the_toolbar_and_the_tools_sidebar() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Default,
            )];
            s.agent_dashboard.open = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let lead = commands
            .iter()
            .find_map(|command| match command {
                // The toolbar carries a "New Agent" control of its own, so the
                // panel's is the one below the toolbar.
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New Agent" && origin.y >= crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the panel draws its new-agent control");
        assert!(
            lead.y >= crate::ui::shell::TOPBAR_HEIGHT,
            "the panel starts below the toolbar, not over it: {lead:?}"
        );
        assert!(
            lead.x >= sidebar_width,
            "the panel starts beside the tools sidebar, not over it: {lead:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == "robot" && origin.y < crate::ui::shell::TOPBAR_HEIGHT
            )),
            "the toolbar control stays above the panel"
        );
        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|text| text == "Search"),
            "the tools sidebar stays drawn beside the panel: {texts:?}"
        );

        // The close control is pinned to the panel's right edge.
        let close = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "close" => Some((*origin, *size)),
                _ => None,
            })
            .expect("the panel draws its close control");
        assert!(
            close.0.x + close.1 >= window().x - 24.0,
            "the close control sits at the right end: {close:?}"
        );
    }

    /// The panel's inner bands run its full width: the filter field, a group
    /// header's rule, and the selected row's highlight all reach the pane's
    /// right edge instead of sizing to their text.
    #[test]
    fn the_inner_bands_span_the_panel_width() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Default,
            )];
            s.agent_dashboard.open = true;
            s.agent_dashboard.selected = Some("a".to_string());
        }
        let pane_width = window().x - sidebar_width - 24.0;
        let commands = render_element(&mut root, window(), &app);
        let strokes: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        let field = strokes
            .iter()
            .find(|rect| (rect.size.height - 38.4).abs() < 1.0)
            .expect("the filter field draws its box");
        assert!(
            field.size.width >= pane_width - 24.0,
            "the filter field spans the panel: {field:?}, pane {pane_width}"
        );

        let fills: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        // The selected row's highlight is the panel-wide band at the roster's
        // height; the section rule is the 1px band beside the group header.
        let selected_row = fills
            .iter()
            .find(|rect| {
                rect.size.width >= pane_width - 8.0
                    && rect.size.height > 40.0
                    && rect.origin.y > 150.0
                    && rect.origin.y < 400.0
            })
            .expect("the selected row paints its highlight");
        assert!(
            selected_row.size.width >= pane_width - 8.0,
            "the selected row spans the panel: {selected_row:?}"
        );
        let rule = fills
            .iter()
            .find(|rect| rect.size.height <= 1.0 && rect.size.width > pane_width * 0.8)
            .expect("the group header draws its rule");
        assert!(
            rule.size.width > pane_width * 0.8,
            "the group rule runs to the right edge: {rule:?}"
        );
    }

    /// A response is markdown: the roster flattens it to one clamped line,
    /// while the response card renders it the way the transcript renders a
    /// reply, headings and fences included, under its own scroll. Letting the
    /// raw text through in the row would paint past its clamped height and
    /// cover the rest of the roster.
    #[test]
    fn a_multi_line_response_is_flattened_in_the_roster_and_rendered_in_the_card() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let response = "# Steps\n\nFirst line of the response with enough words to fill it.\n\
                        Second line of the response that keeps going for a while.\n\n\
                        - first bullet\n- second bullet\n\n\
                        ```sh\ncargo test -p goble-app\n```\n\n\
                        Last line of the response that must stay out of the panel preview.";
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Long answer",
                response,
                ConversationStatus::Default,
            )];
            s.agent_dashboard.open = true;
            s.agent_dashboard.selected = Some("a".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let texts = drawn_texts(&commands);

        // Inside the panel, the roster's row is the only flattened preview.
        let flat: Vec<&String> = texts
            .iter()
            .filter(|text| text.starts_with("# Steps First line"))
            .collect();
        assert_eq!(
            flat.len(),
            1,
            "only the roster row flattens the response: {texts:?}"
        );
        let row = flat[0];
        assert!(
            row.ends_with('…') && row.chars().count() <= 81,
            "the row preview is cut to one short line: {row:?}"
        );
        assert!(
            !row.contains("must stay out of the panel preview"),
            "the end of the response stays out of the row: {row:?}"
        );

        // The card renders the markdown instead of printing it.
        assert!(
            texts.iter().any(|text| text == "Steps"),
            "the card draws the heading without its hashes: {texts:?}"
        );
        let panel_x = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New Agent" && origin.y >= crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(origin.x)
                }
                _ => None,
            })
            .expect("the panel draws its new-agent control");
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text.starts_with("# Steps") && origin.x >= panel_x
            )),
            "no raw markdown reaches the card: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with("First line of the response")),
            "the paragraph is drawn as prose: {texts:?}"
        );
        // The fence's body keeps its language colouring, which splits it into
        // highlighted runs; the runs still carry the command.
        assert!(
            texts.iter().any(|text| text == "cargo test")
                && texts.iter().any(|text| text == " goble-app"),
            "a code fence keeps its body: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text
                    == "Last line of the response that must stay out of the panel preview."),
            "the card keeps the whole response: {texts:?}"
        );
    }

    #[test]
    fn a_pending_question_names_the_row_and_counts_as_awaiting() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Waiting,
            )];
            s.raise_inbox(
                1,
                "a",
                InboxKind::Question,
                "Which branch?",
                "main or release",
            );
            s.agent_dashboard.open = true;
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "Which branch?"),
            "the row shows the question it waits on: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "◈ 1 awaiting"),
            "the header counts it as awaiting: {texts:?}"
        );
    }

    #[test]
    fn the_composer_switches_between_dispatch_and_reply() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Waiting,
            )];
            s.agent_dashboard.open = true;
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "Dispatch a new agent…"),
            "the prompt line is there with no selection: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Reply to this agent…"),
            "and the reply target is away: {texts:?}"
        );

        state.borrow_mut().agent_dashboard.selected = Some("a".to_string());
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "Reply to this agent…"),
            "the selected row retargets the prompt: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "The fix is in."),
            "the card shows the latest response: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Dispatch a new agent…"),
            "the dispatch prompt gives way to the reply: {texts:?}"
        );
    }

    /// The prompt line is clickable and typable with no agent selected, and it
    /// stays a compact band until Enter starts the conversation.
    #[test]
    fn the_prompt_line_takes_a_dispatch_with_no_selection() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().agent_dashboard.open = true;
        let commands = render_element(&mut root, window(), &app);
        let prompt = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Dispatch a new agent…" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the card draws its prompt line with no selection");
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
            )),
            "the prompt line leads with the arrow: {commands:?}"
        );

        assert!(click_at(
            &mut root,
            &app,
            vec2f(prompt.x + 10.0, prompt.y + 5.0)
        ));
        {
            let s = state.borrow();
            assert!(
                s.agent_dashboard.dispatch_open,
                "clicking the line puts the panel in dispatch mode"
            );
            assert!(
                s.agent_dashboard.dispatch_focused,
                "and the caret is in the line"
            );
        }
        assert!(press(&mut root, &app, "h", ModifiersState::none()));
        assert_eq!(
            state.borrow().agent_dashboard.dispatch,
            "h",
            "a key typed at the line lands in the dispatch draft"
        );
        assert_eq!(
            state.borrow().agent_dashboard.selected,
            None,
            "typing does not open an agent's output"
        );
    }

    /// The new-agent control is a drawer: one press raises the prompt with the
    /// caret in it, the next retracts it and drops the draft.
    #[test]
    fn the_new_agent_control_raises_and_retracts_the_dispatch_prompt() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().agent_dashboard.open = true;
        let commands = render_element(&mut root, window(), &app);
        let lead = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New Agent" && origin.y >= crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the header draws its new-agent control");
        let press_at = vec2f(lead.x + 4.0, lead.y + 4.0);

        assert!(click_at(&mut root, &app, press_at));
        {
            let s = state.borrow();
            assert!(
                s.agent_dashboard.dispatch_open,
                "the first press raises the dispatch prompt"
            );
            assert!(
                s.agent_dashboard.dispatch_focused,
                "and puts the caret in it"
            );
        }
        assert!(click_at(&mut root, &app, press_at));
        {
            let s = state.borrow();
            assert!(
                !s.agent_dashboard.dispatch_open,
                "the second press retracts it"
            );
            assert!(
                !s.agent_dashboard.dispatch_focused,
                "and gives the keyboard back"
            );
        }
    }

    #[test]
    fn the_search_field_filters_the_roster() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![
                conversation(
                    "a",
                    "Fix the login flow",
                    "one",
                    ConversationStatus::Default,
                ),
                conversation("b", "Audit the diff", "two", ConversationStatus::Default),
            ];
            s.agent_dashboard.open = true;
            s.agent_dashboard.query = "audit".to_string();
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "Audit the diff"),
            "the matching row stays: {texts:?}"
        );
        // The sidebar still lists both conversations, so the roster's own
        // header count is what the filter is checked on.
        assert!(
            texts.iter().any(|text| text == "◆ 1 done"),
            "only the matching row is counted on the roster: {texts:?}"
        );
    }

    /// The response card is grok-build's output box: the agent's name and age,
    /// its response, a plain prompt line and the model as text. Every row
    /// control lives on the keyboard instead, so nothing in the card is a
    /// button.
    #[test]
    fn the_response_card_draws_no_buttons() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Waiting,
            )];
            s.agent_dashboard.open = true;
            s.agent_dashboard.selected = Some("a".to_string());
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        for control in ["Rename", "Open", "Save", "Cancel", "Stop", "Delete"] {
            assert!(
                !texts.iter().any(|text| text == control),
                "the response card draws no {control} button: {texts:?}"
            );
        }
        assert!(
            texts.iter().any(|text| text.contains(" · ")),
            "the footer names the model and the approval policy as text: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "Reply to this agent…"),
            "the card keeps the prompt line: {texts:?}"
        );
    }

    /// The panel and its card sit on the workspace's black, the way the rest
    /// of the harness does; a graphite surface would read as a separate card.
    #[test]
    fn the_panel_and_the_response_card_sit_on_the_workspace_black() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Waiting,
            )];
            s.agent_dashboard.open = true;
            s.agent_dashboard.selected = Some("a".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let raised = app.theme.color(ColorToken::SurfaceRaised);
        let surface = app.theme.color(ColorToken::Surface);
        let black = app.theme.color(ColorToken::Bg);
        let graphite_band = |color| {
            commands.iter().any(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, color: fill, .. }
                        if *fill == color
                            && rect.width() > 200.0
                            && rect.height() > 100.0
                            && rect.origin.x >= sidebar_width
                )
            })
        };
        assert!(
            !graphite_band(raised) && !graphite_band(surface),
            "the panel draws no raised graphite band: {commands:?}"
        );
        assert!(
            graphite_band(black),
            "the panel paints the workspace black: {commands:?}"
        );
    }

    /// With no cursor the card is a prompt-height band under the roster; only
    /// a selected agent's output gives it the tall share of the panel.
    #[test]
    fn the_response_card_stays_compact_until_an_agent_is_selected() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Waiting,
            )];
            s.agent_dashboard.open = true;
        }
        let pane_width = window().x - sidebar_width - 24.0;
        let card_band = |commands: &[RenderCommand]| -> RectF {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::StrokeRect { rect, .. }
                        if rect.width() > pane_width * 0.5 && rect.height() > 40.0 =>
                    {
                        Some(*rect)
                    }
                    _ => None,
                })
                .expect("the response card draws its border")
        };

        let commands = render_element(&mut root, window(), &app);
        let compact = card_band(&commands);
        assert!(
            compact.height() < 130.0,
            "with no selection the card keeps its prompt height: {compact:?}"
        );

        state.borrow_mut().agent_dashboard.selected = Some("a".to_string());
        let commands = render_element(&mut root, window(), &app);
        let grown = card_band(&commands);
        assert!(
            grown.height() > 250.0,
            "a selected agent gives the output the tall share: {grown:?}"
        );
        assert!(
            grown.height() > compact.height() * 2.0,
            "the output card grows well past its resting height: compact {compact:?}, grown {grown:?}"
        );
    }

    /// Deleting has no button in the card, so Cmd+Backspace stands in for it
    /// and the row itself says the next press removes the agent.
    #[test]
    fn the_delete_chord_arms_the_row_and_the_row_says_so() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Default,
            )];
            s.agent_dashboard.open = true;
            s.agent_dashboard.selected = Some("a".to_string());
        }
        let chord = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert!(press(&mut root, &app, "Backspace", chord));
        assert_eq!(
            state.borrow().agent_dashboard.confirm_delete.as_deref(),
            Some("a"),
            "the first press arms the row"
        );
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "⌘⌫ again"),
            "the row asks for the second press: {texts:?}"
        );
    }

    /// Under the pointer a row names its state and swaps its age for the close
    /// control, so the two things a row can be doing live where the eye is.
    #[test]
    fn hover_reveals_the_state_and_the_close_control() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Default,
            )];
            s.raise_inbox(
                1,
                "a",
                InboxKind::Question,
                "Which branch?",
                "main or release",
            );
            s.agent_dashboard.open = true;
            *s.agent_dashboard.hovered_row.borrow_mut() = Some("a".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let panel_x = |origin: Vector2F| origin.x >= sidebar_width;
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Awaiting your input" && panel_x(*origin) && origin.x > window().x / 2.0
            )),
            "the hovered row names its state at the right end"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "2m" && panel_x(*origin) && origin.y > 150.0
            )),
            "the age gives way to the controls while hovered"
        );
        let closes: Vec<Vector2F> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == "close" && panel_x(*origin) =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            closes.len(),
            2,
            "the panel's own close and the row's close are both drawn: {closes:?}"
        );
        assert!(
            closes.iter().any(|origin| origin.y > 150.0),
            "one of them belongs to the row: {closes:?}"
        );
    }

    /// The row's close control deletes its conversation and does not select the
    /// row it sits in.
    #[test]
    fn the_rows_close_control_arms_the_delete_without_selecting_the_row() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let sidebar_width = state.borrow().sidebar_width;
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "The fix is in.",
                ConversationStatus::Default,
            )];
            s.agent_dashboard.open = true;
            *s.agent_dashboard.hovered_row.borrow_mut() = Some("a".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let close = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "close" && origin.x >= sidebar_width && origin.y > 150.0 => {
                    Some((*origin, *size))
                }
                _ => None,
            })
            .expect("the hovered row draws its close control");
        let click = vec2f(close.0.x + close.1 / 2.0, close.0.y + close.1 / 2.0);
        // The render above paints with no pointer, which clears the hover
        // memory the close control is built from; put it back for the tree the
        // press is delivered to.
        *state.borrow_mut().agent_dashboard.hovered_row.borrow_mut() = Some("a".to_string());
        assert!(click_at(&mut root, &app, click));
        assert_eq!(
            state.borrow().agent_dashboard.confirm_delete.as_deref(),
            Some("a"),
            "the first press arms the row's delete"
        );
        assert_eq!(
            state.borrow().agent_dashboard.selected,
            None,
            "the press on the control does not select the row"
        );
    }

    #[test]
    fn the_keys_walk_the_roster_and_escape_starts_a_new_agent() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![
                conversation(
                    "a",
                    "Fix the login flow",
                    "one",
                    ConversationStatus::Waiting,
                ),
                conversation("b", "Audit the diff", "two", ConversationStatus::Default),
            ];
            s.agent_dashboard.open = true;
        }
        // Tab leaves the search field and hands the keyboard to the list.
        state.borrow_mut().agent_dashboard.search_focused = true;
        assert!(press(&mut root, &app, "Tab", ModifiersState::none()));
        assert!(
            !state.borrow().agent_dashboard.search_focused,
            "Tab moves the keyboard back to the list"
        );
        assert!(press(&mut root, &app, "ArrowDown", ModifiersState::none()));
        assert_eq!(
            state.borrow().agent_dashboard.selected.as_deref(),
            Some("a"),
            "the first press lands on the first roster row"
        );
        assert!(press(&mut root, &app, "ArrowDown", ModifiersState::none()));
        assert_eq!(
            state.borrow().agent_dashboard.selected.as_deref(),
            Some("b"),
            "the next press walks down"
        );
        assert!(press(&mut root, &app, "ArrowDown", ModifiersState::none()));
        assert_eq!(
            state.borrow().agent_dashboard.selected.as_deref(),
            Some("b"),
            "the cursor stops at the end instead of wrapping"
        );
        assert!(press(&mut root, &app, "ArrowUp", ModifiersState::none()));
        assert_eq!(
            state.borrow().agent_dashboard.selected.as_deref(),
            Some("a")
        );
        assert!(press(&mut root, &app, "Escape", ModifiersState::none()));
        {
            let s = state.borrow();
            assert_eq!(s.agent_dashboard.selected, None, "Escape steps off the row");
            assert!(s.agent_dashboard.open, "the panel stays up");
            assert!(
                s.agent_dashboard.dispatch_open && s.agent_dashboard.dispatch_focused,
                "Escape is the New Agent shortcut and takes the caret"
            );
        }
        assert!(press(&mut root, &app, "Escape", ModifiersState::none()));
        assert!(
            state.borrow().agent_dashboard.open,
            "Escape keeps the panel up"
        );
        assert!(
            state.borrow().agent_dashboard.dispatch_focused,
            "and keeps the prompt ready for a new agent"
        );
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with("Enter: open | Tab: list | Esc: new agent")),
            "the footer names grok-build's shortcut order: {texts:?}"
        );
    }

    #[test]
    fn the_grouping_chord_switches_the_sections() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![conversation(
                "a",
                "Fix the login flow",
                "one",
                ConversationStatus::Waiting,
            )];
            s.conversations[0].directory = "/Users/me/Projects/goble".to_string();
            s.agent_dashboard.open = true;
        }
        let chord = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert!(press(&mut root, &app, "g", chord));
        {
            let s = state.borrow();
            assert_eq!(
                s.agent_dashboard.grouping,
                super::AgentDashboardGrouping::Directory,
                "Cmd+G switches the roster to directory groups"
            );
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text.contains("Projects/goble")),
            "the directory group header is drawn: {texts:?}"
        );
    }
}
