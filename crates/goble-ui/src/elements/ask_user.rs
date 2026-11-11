//! The inline "ask the user" panel.
//!
//! When the harness suspends on a question, the panel closes the agent's output
//! exactly where the answer belongs. It is grok-new's *Agent questions* band: a
//! full-width flat surface, a header naming the question with its two controls,
//! the question on its own row and one full-width row per answer. The rows are
//! numbered, so a digit answers as well as a click; the arrows move the picked
//! row and Enter sends it, like the command approval card. The last row
//! ("Other…") opens the free-text field.
//!
//! The selected row, the free-text answer and the credential fields live in the
//! [`AskUserUi`] the card is built from, so a click or a key survives the
//! per-frame rebuild of the element tree.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Border, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Empty, Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, LayoutContext, MainAxisSize,
    PaintContext, Point, SizeConstraint, Text, TextArea,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, SpacingToken};

/// The answer the user is composing: the picked row, the free text "Other…"
/// opened, and the optional credential the harness stores by name.
///
/// Shared between every clone of the [`AskUserUi`] the panel is built from, so
/// the app's per-frame snapshot keeps the selection the user made last frame.
/// Which of the panel's fields holds the keyboard. The credential fields share
/// the panel with the free-text answer, so a single flag cannot say which one
/// is focused (one field's blur arrives before the other's focus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AskFocus {
    FreeText,
    CredentialName,
    CredentialValue,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AskUserState {
    /// The picked quick reply, when the user picked one instead of "Other…".
    selected: Option<usize>,
    /// Whether the free-text row is the picked one.
    other: bool,
    free_text: String,
    credential_name: String,
    credential: String,
    /// Which field holds the keyboard, if any.
    focused: Option<AskFocus>,
}

/// The renderable state of a pending ask: the agent's question and its suggested
/// quick replies. Mirrors `PendingAsk` from goble-core minus the internal ids.
#[derive(Clone, Default)]
pub struct AskUserUi {
    pub question: String,
    pub quick_replies: Vec<String>,
    pub(crate) state: Rc<RefCell<AskUserState>>,
}

impl AskUserUi {
    pub fn new(question: impl Into<String>, quick_replies: Vec<String>) -> Self {
        Self {
            question: question.into(),
            quick_replies,
            state: Rc::default(),
        }
    }
}

impl AskUserUi {
    /// The quick reply the panel has picked, for a host or a test that reads
    /// the answer state without rebuilding the card.
    #[cfg(test)]
    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.state.borrow().selected
    }
}

impl std::fmt::Debug for AskUserUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskUserUi")
            .field("question", &self.question)
            .field("quick_replies", &self.quick_replies)
            .finish()
    }
}

impl PartialEq for AskUserUi {
    fn eq(&self, other: &Self) -> bool {
        self.question == other.question && self.quick_replies == other.quick_replies
    }
}

impl Eq for AskUserUi {}

/// An inline panel that renders an "ask the user" interaction in the
/// conversation stream (grok-new's `Agent questions` band). It is not pinned to
/// the bottom: it sits among the chat messages and scrolls with them.
///
/// Offers numbered quick replies, an "Other…" row that opens a free-text answer
/// and, under it, an optional masked credential. `Next` (or Enter) calls
/// `on_answer` with the composed response; `Skip all` (or Ctrl-C / Escape) calls
/// `on_skip`.
pub struct AskUserCard {
    question: String,
    quick_replies: Vec<String>,
    state: Rc<RefCell<AskUserState>>,
    /// Whether the panel's own keys answer it. The transcript turns them off
    /// while the rich input holds the keyboard, so a draft keeps its digits.
    keys_enabled: bool,
    on_answer: Option<Rc<RefCell<dyn FnMut(String, Option<(String, String)>) + 'static>>>,
    on_skip: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AskUserCard {
    pub fn new(question: impl Into<String>, quick_replies: Vec<String>) -> Self {
        Self {
            question: question.into(),
            quick_replies,
            state: Rc::default(),
            keys_enabled: true,
            on_answer: None,
            on_skip: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Build the panel for `ui`, sharing its answer state so the selection and
    /// the typed answer outlive this frame's element tree.
    pub fn from_ui(ui: &AskUserUi) -> Self {
        Self {
            question: ui.question.clone(),
            quick_replies: ui.quick_replies.clone(),
            state: ui.state.clone(),
            keys_enabled: true,
            on_answer: None,
            on_skip: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_answer<F: FnMut(String, Option<(String, String)>) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_answer = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_skip<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_skip = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the arrows, digits, Enter, Escape and Ctrl-C answer the panel.
    /// Off while another surface holds the keyboard, so its own typing is not
    /// intercepted by a panel scrolled into the transcript.
    pub fn with_keys_enabled(mut self, enabled: bool) -> Self {
        self.keys_enabled = enabled;
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);

        let state = self.state.clone();
        let replies = self.quick_replies.clone();
        let (selected, other, focused, free_text_value, credential_name_value, credential_value) = {
            let state = state.borrow();
            (
                state.selected,
                state.other,
                state.focused,
                state.free_text.clone(),
                state.credential_name.clone(),
                state.credential.clone(),
            )
        };

        let submit: Rc<RefCell<dyn FnMut()>> = {
            let handler = self.on_answer.clone();
            let state = state.clone();
            let replies = replies.clone();
            Rc::new(RefCell::new(move || {
                let Some(cb) = handler.as_ref() else {
                    return;
                };
                let (answer, credential) = compose(&state.borrow(), &replies);
                if answer.is_empty() && credential.is_none() {
                    return;
                }
                (cb.borrow_mut())(answer, credential);
            }))
        };
        let skip: Rc<RefCell<dyn FnMut()>> = {
            let handler = self.on_skip.clone();
            Rc::new(RefCell::new(move || {
                if let Some(cb) = handler.as_ref() {
                    (cb.borrow_mut())();
                }
            }))
        };

        // Header: the amber mark, the title, then "Skip all" with its chord and
        // the filled "Next" control with Return written on it.
        let mark = Container::new(Empty::new().with_size(vec2f(10.0, 10.0)).finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Warning)))
            .finish();
        let title = Text::new("Agent questions")
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(13.0)
            .with_weight(FontWeight::SemiBold)
            .finish();
        let skip_all = {
            let skip = skip.clone();
            HoverRow::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.0)
                    .with_child(
                        Text::new("Skip all")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(keycap(
                        Icon::new("key-control")
                            .with_size(10.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                        app,
                    ))
                    .with_child(keycap(
                        Text::new("C")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(10.0)
                            .finish(),
                        app,
                    ))
                    .finish(),
            )
            .with_padding(EdgeInsets::new(8.0, 4.0, 8.0, 4.0))
            .with_on_click(move || (skip.borrow_mut())())
            .finish()
        };
        let next = {
            let submit = submit.clone();
            let return_cap = Container::new(
                Icon::new("key-return")
                    .with_size(10.0)
                    .with_theme_color(ColorToken::Bg, app)
                    .finish(),
            )
            .with_background(Fill::Solid(
                app.theme
                    .color(ColorToken::Accent)
                    .mix(&app.theme.color(ColorToken::Bg), 0.45),
            ))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Bg)))
            .with_corner_radius(3.0)
            .with_padding(EdgeInsets::new(4.0, 2.0, 4.0, 2.0))
            .finish();
            // The accent band's own hairline names the button as a control on
            // the flat header rather than a rounded plaque.
            Container::new(
                Button::new(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(5.0)
                        .with_child(
                            Text::new("Next")
                                .with_theme_color(ColorToken::Bg, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_child(return_cap)
                        .finish(),
                )
                .with_variant(ButtonVariant::Primary)
                .with_corner_radius(5.0)
                .with_on_click(move || (submit.borrow_mut())())
                .finish(),
            )
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Accent)))
            .with_corner_radius(5.0)
            .finish()
        };
        let header = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(mark)
                .with_child(title)
                .with_child(Expanded::new(Empty::new().finish()).finish())
                .with_child(skip_all)
                .with_child(next)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::new(md, sm, md, sm))
        .finish();

        // The question reads on its own full-width row, under the header. A
        // model that repeated the replies as a lettered list inside it would
        // otherwise draw every option twice: once as prose and once as a row.
        let question = Text::new(question_without_repeated_replies(&self.question, &replies))
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(13.0)
            .finish();

        let mut rows = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(6.0)
            .with_child(question);
        for (index, label) in replies.iter().enumerate() {
            rows = rows.with_child(answer_row(
                (index + 1).to_string(),
                label.clone(),
                selected == Some(index),
                {
                    let state = state.clone();
                    move || {
                        let mut state = state.borrow_mut();
                        state.selected = Some(index);
                        state.other = false;
                        state.free_text.clear();
                        state.focused = None;
                    }
                },
                app,
            ));
        }
        // The free-text row closes the list: its number follows the quick
        // replies, so the keyboard answers it like any other row.
        rows = rows.with_child(answer_row(
            (replies.len() + 1).to_string(),
            "Other...".to_string(),
            other,
            {
                let state = state.clone();
                move || {
                    let mut state = state.borrow_mut();
                    state.selected = None;
                    state.other = true;
                    state.focused = Some(AskFocus::FreeText);
                }
            },
            app,
        ));

        // "Other…" opens the free-text field. It is not drawn at rest, so the
        // panel reads exactly like the reference until the user asks for it.
        if other {
            let free_text = state.clone();
            rows = rows.with_child(
                TextArea::new()
                    .with_value(free_text_value)
                    .with_placeholder("Type your answer…")
                    .with_min_height(38.0)
                    .with_full_width(true)
                    .with_focused(focused == Some(AskFocus::FreeText))
                    .with_on_focus_change({
                        let state = state.clone();
                        move |focused| {
                            let mut state = state.borrow_mut();
                            if focused {
                                state.focused = Some(AskFocus::FreeText);
                                state.other = true;
                                state.selected = None;
                            } else if state.focused == Some(AskFocus::FreeText) {
                                state.focused = None;
                            }
                        }
                    })
                    .with_on_change({
                        let state = free_text;
                        move |text| {
                            let mut state = state.borrow_mut();
                            state.free_text = text;
                            state.other = true;
                            state.selected = None;
                            state.focused = Some(AskFocus::FreeText);
                        }
                    })
                    .finish(),
            );

            // The credential stays behind "Other…": a name and a masked value,
            // returned separately so the secret never enters the transcript.
            let credential_name = state.clone();
            let credential_name_focus = state.clone();
            let credential = state.clone();
            let credential_focus = state.clone();
            rows = rows.with_child(
                Flex::column()
                    .with_spacing(4.0)
                    .with_child(
                        TextArea::new()
                            .with_value(credential_name_value)
                            .with_placeholder("Credential name (e.g. github_token)…")
                            .with_min_height(32.0)
                            .with_full_width(true)
                            .with_on_focus_change({
                                let state = credential_name_focus;
                                move |focused| {
                                    set_field_focus(&state, AskFocus::CredentialName, focused);
                                }
                            })
                            .with_on_change(move |text| {
                                credential_name.borrow_mut().credential_name = text;
                            })
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(sm)
                            .with_child(
                                Icon::new("key")
                                    .with_size(14.0)
                                    .with_theme_color(ColorToken::Muted, app)
                                    .finish(),
                            )
                            .with_child(
                                Expanded::new(
                                    TextArea::new()
                                        .with_value(credential_value)
                                        .with_placeholder("Credential (optional)…")
                                        .with_min_height(34.0)
                                        .with_full_width(true)
                                        .with_masked(true)
                                        .with_on_focus_change({
                                            let state = credential_focus;
                                            move |focused| {
                                                set_field_focus(
                                                    &state,
                                                    AskFocus::CredentialValue,
                                                    focused,
                                                );
                                            }
                                        })
                                        .with_on_change(move |text| {
                                            credential.borrow_mut().credential = text;
                                        })
                                        .finish(),
                                )
                                .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
            );
        }

        let body = Container::new(rows.finish())
            .with_padding(EdgeInsets::new(md, md, md, md))
            .finish();

        // A full-width flat band: the header's band, a hairline, then the
        // question and its rows. Nothing rounds the outer surface, so the
        // panel reads as part of the transcript like the command approval card.
        let band = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(header)
                .with_child(Divider::horizontal().finish())
                .with_child(body)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish();

        // The keyboard answers the panel before the free-text field sees the
        // key: the arrows move the picked row, digits pick one by number, Enter
        // sends the picked answer (or opens the text field when "Other…" is the
        // picked row), and Ctrl-C / Escape skip.
        let key_state = state.clone();
        let key_submit = submit.clone();
        let key_skip = skip.clone();
        let key_replies = replies.clone();
        let key_handler = move |key: &str, modifiers: ModifiersState| -> bool {
            if modifiers.ctrl && key.eq_ignore_ascii_case("c") {
                (key_skip.borrow_mut())();
                return true;
            }
            match key {
                "Escape" if !modifiers.command && !modifiers.ctrl => {
                    (key_skip.borrow_mut())();
                    true
                }
                "ArrowDown" | "ArrowUp"
                    if !modifiers.command && !modifiers.ctrl && !modifiers.alt =>
                {
                    let mut state = key_state.borrow_mut();
                    // A focused field owns the arrows: they move its caret.
                    if state.focused.is_some() {
                        return false;
                    }
                    let count = key_replies.len() + 1;
                    let current = state.selected.map(|row| row as isize).or(if state.other {
                        Some(key_replies.len() as isize)
                    } else {
                        None
                    });
                    let next = match key {
                        "ArrowDown" => current.map_or(0, |row| row + 1).min(count as isize - 1),
                        _ => current.map_or(count as isize - 1, |row| row - 1).max(0),
                    };
                    if next as usize == key_replies.len() {
                        state.selected = None;
                        state.other = true;
                    } else {
                        state.selected = Some(next as usize);
                        state.other = false;
                        state.free_text.clear();
                        state.focused = None;
                    }
                    true
                }
                "Enter" if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                    // "Other…" picked with the arrows opens its field on Enter;
                    // the field's own text is sent by the next Enter.
                    let opens_free_text = {
                        let state = key_state.borrow();
                        state.other && state.focused.is_none()
                    };
                    if opens_free_text {
                        key_state.borrow_mut().focused = Some(AskFocus::FreeText);
                        return true;
                    }
                    (key_submit.borrow_mut())();
                    true
                }
                _ => {
                    // A focused field owns the digits: they are what the user is
                    // typing, not another row's number.
                    if key_state.borrow().focused.is_some() || key.len() != 1 {
                        return false;
                    }
                    let Some(number) = key.chars().next().and_then(|c| c.to_digit(10)) else {
                        return false;
                    };
                    let index = number as usize;
                    if index >= 1 && index <= key_replies.len() {
                        let mut state = key_state.borrow_mut();
                        state.selected = Some(index - 1);
                        state.other = false;
                        state.free_text.clear();
                        state.focused = None;
                        true
                    } else if index == key_replies.len() + 1 {
                        let mut state = key_state.borrow_mut();
                        state.selected = None;
                        state.other = true;
                        state.focused = Some(AskFocus::FreeText);
                        true
                    } else {
                        false
                    }
                }
            }
        };

        self.root = Some(if self.keys_enabled {
            KeyHandler::new(band, key_handler).finish()
        } else {
            band
        });
    }
}

impl Default for AskUserCard {
    fn default() -> Self {
        Self::new(String::new(), Vec::new())
    }
}

/// Compose the answer delivered to the harness. The credential, when present,
/// is returned separately as `(name, value)` so the secret never enters the
/// answer string (and thus never the transcript); the harness stores it by
/// name and references it by name.
pub(crate) fn compose(
    state: &AskUserState,
    quick_replies: &[String],
) -> (String, Option<(String, String)>) {
    let mut parts = Vec::new();
    if let Some(idx) = state.selected {
        if let Some(label) = quick_replies.get(idx) {
            parts.push(label.clone());
        }
    }
    let free_text = state.free_text.trim();
    if !free_text.is_empty() {
        parts.push(free_text.to_string());
    }
    let answer = parts.join("\n");
    let credential = state.credential.trim();
    if credential.is_empty() {
        return (answer, None);
    }
    let name = state.credential_name.trim();
    (answer, Some((name.to_string(), credential.to_string())))
}

/// Record that `field` gained or lost the keyboard, keeping the field that is
/// still focused when a neighbour's blur arrives after this field's focus.
fn set_field_focus(state: &Rc<RefCell<AskUserState>>, field: AskFocus, focused: bool) {
    let mut state = state.borrow_mut();
    if focused {
        state.focused = Some(field);
    } else if state.focused == Some(field) {
        state.focused = None;
    }
}

/// The question as the panel reads it. A model that lists the answers inside
/// the question ("A) ~8 seconds", "2. production") would draw every option
/// twice: once as prose and once as the row it is answered with. Drop exactly
/// those lines, keeping prose — and a question that names no options — intact.
fn question_without_repeated_replies(question: &str, replies: &[String]) -> String {
    if replies.is_empty() {
        return question.to_string();
    }
    let wanted: Vec<String> = replies
        .iter()
        .map(|reply| normalize_option(reply))
        .collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = false;
    for line in question.lines() {
        let repeated = option_body(line)
            .is_some_and(|body| wanted.iter().any(|reply| *reply == normalize_option(body)));
        if repeated {
            dropped = true;
            continue;
        }
        kept.push(line);
    }
    if !dropped {
        return question.to_string();
    }
    // The gap an option list leaves behind reads as a blank row of its own.
    let mut out: Vec<&str> = Vec::new();
    for line in kept {
        if line.trim().is_empty() && out.last().is_some_and(|last| last.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|last| last.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// The body of a line that enumerates an answer: `A) x`, `(b) x`, `3. x`,
/// `- x`. `None` for prose, so only a listed option can be matched against a
/// reply.
fn option_body(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    for bullet in ["- ", "* ", "• "] {
        if let Some(body) = trimmed.strip_prefix(bullet) {
            let body = body.trim();
            return (!body.is_empty()).then_some(body);
        }
    }
    if let Some(inner) = trimmed.strip_prefix('(') {
        let (marker, rest) = inner.split_once(')')?;
        if !marker.is_empty() && marker.chars().all(|c| c.is_ascii_alphanumeric()) {
            let body = rest.trim();
            return (!body.is_empty()).then_some(body);
        }
        return None;
    }
    let (marker, rest) = trimmed.split_once(|c: char| matches!(c, ')' | '.' | ':'))?;
    if marker.is_empty() || marker.len() > 3 || !marker.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let body = rest.trim();
    (!body.is_empty()).then_some(body)
}

/// The comparison key for an option: spacing and case do not distinguish a
/// listed answer from the reply row that draws it, nor does a trailing period.
fn normalize_option(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(|c: char| matches!(c, '.' | ':' | ',' | ';'))
        .to_lowercase()
}

/// One answer band: its number in a keycap and the label, with the picked row
/// highlighted. The whole row is the hit target.
fn answer_row(
    number: String,
    label: String,
    selected: bool,
    on_click: impl FnMut() + 'static,
    app: &AppContext,
) -> Box<dyn Element> {
    let keycap = Container::new(
        Text::new(number)
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(11.0)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
    .with_corner_radius(4.0)
    .with_padding(EdgeInsets::new(7.0, 2.0, 7.0, 2.0))
    .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(keycap)
        .with_child(
            Expanded::new(
                Text::new(label)
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(13.0)
                    .finish(),
            )
            .finish(),
        )
        .finish();
    let border = if selected {
        ColorToken::Accent
    } else {
        ColorToken::Border
    };
    // The picked row is the row's own fill, not a highlight over it: the
    // container's hairline and fill stay a control, and the band never reads as
    // a pill wrapped around the row.
    let background = if selected {
        ColorToken::Selected
    } else {
        ColorToken::SurfaceRaised
    };
    Container::new(
        HoverRow::new(row)
            .with_padding(EdgeInsets::new(10.0, 7.0, 10.0, 7.0))
            .with_corner_radius(3.0)
            .with_on_click(on_click)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(background)))
    .with_border(Border::all(1.0).with_border_color(app.theme.color(border)))
    .with_corner_radius(4.0)
    .with_padding_uniform(1.0)
    .finish()
}

/// A small control-sized key cap: the key's glyph or icon over the raised
/// surface, with the same hairline as the controls it sits beside.
fn keycap(content: Box<dyn Element>, app: &AppContext) -> Box<dyn Element> {
    Container::new(content)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(3.0)
        .with_padding(EdgeInsets::new(4.0, 1.0, 4.0, 1.0))
        .finish()
}

impl Element for AskUserCard {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{EventContext, LayoutContext, PaintContext};
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    type Answers = Rc<RefCell<Vec<(String, Option<(String, String)>)>>>;
    type Skips = Rc<RefCell<usize>>;

    fn answers() -> (
        Answers,
        impl FnMut(String, Option<(String, String)>) + 'static,
    ) {
        let answers: Answers = Rc::default();
        let sink = answers.clone();
        let handler = move |answer: String, credential: Option<(String, String)>| {
            sink.borrow_mut().push((answer, credential));
        };
        (answers, handler)
    }

    fn skips() -> (Skips, impl FnMut() + 'static) {
        let skips: Skips = Rc::default();
        let sink = skips.clone();
        let handler = move || *sink.borrow_mut() += 1;
        (skips, handler)
    }

    fn card() -> AskUserCard {
        AskUserCard::new(
            "Which database should I query?".to_string(),
            vec!["Postgres".to_string(), "SQLite".to_string()],
        )
    }

    fn drawn(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn key(name: &str, modifiers: ModifiersState) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers,
        }
    }

    fn press(element: &mut Box<dyn Element>, name: &str, app: &AppContext) -> bool {
        element.dispatch_event(
            &key(name, ModifiersState::none()),
            &mut EventContext::default(),
            app,
        )
    }

    /// Click the run of `text`: the row the run sits in takes the press.
    fn click_text(
        element: &mut Box<dyn Element>,
        commands: &[RenderCommand],
        text: &str,
        app: &AppContext,
    ) {
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text: drawn,
                    origin,
                    ..
                } if drawn == text => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} is drawn"));
        element.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(origin.x + 2.0, origin.y + 4.0),
                button: 0,
            },
            &mut EventContext::default(),
            app,
        );
    }

    #[test]
    fn ask_user_card_layouts_and_paints() {
        let app = AppContext::default();
        let mut card = card();
        let size = card.layout(
            SizeConstraint::loose(vec2f(520.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        card.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
    }

    /// The header names the question, the question is drawn, and every answer
    /// is a numbered row ending in "Other…" — no checkbox and no separate send
    /// button.
    #[test]
    fn the_panel_draws_the_header_the_question_and_numbered_rows() {
        let app = AppContext::default();
        let mut element = card().finish();
        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        let drawn = drawn(&commands);

        for expected in [
            "Agent questions",
            "Which database should I query?",
            "Skip all",
            "Next",
            "1",
            "Postgres",
            "2",
            "SQLite",
            "3",
            "Other...",
        ] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "{expected:?} is drawn: {drawn:?}"
            );
        }
        assert!(
            !drawn
                .iter()
                .any(|text| text == "Agent asks" || text == "Send answer"),
            "the old surface is gone: {drawn:?}"
        );
    }

    /// Enter answers the picked row: the digit picks it, Return sends it.
    #[test]
    fn enter_submits_the_selected_option() {
        let app = AppContext::default();
        let (answers, handler) = answers();
        let mut element = card().with_on_answer(handler).finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        assert!(press(&mut element, "2", &app), "a digit is the panel's");
        press(&mut element, "Enter", &app);
        assert_eq!(
            answers.borrow().as_slice(),
            [("SQLite".to_string(), None)],
            "Enter sends the picked row"
        );
    }

    /// A digit picks the row with that number; the next frame draws it picked.
    #[test]
    fn a_number_key_selects_the_matching_row() {
        let app = AppContext::default();
        let card = card();
        let state = card.state.clone();
        let mut element = card.finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        assert!(press(&mut element, "2", &app), "the digit is consumed");
        assert_eq!(state.borrow().selected, Some(1));
        assert!(!state.borrow().other, "the quick reply is the answer");
    }

    /// The arrows walk the answer rows and Enter sends the picked one: the
    /// same keyboard the command approval card gives its variants.
    #[test]
    fn arrow_keys_pick_a_row_and_enter_sends_it() {
        let app = AppContext::default();
        let (answers, handler) = answers();
        let card = card();
        let state = card.state.clone();
        let mut element = card.with_on_answer(handler).finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        assert!(
            press(&mut element, "ArrowDown", &app),
            "the first arrow is the panel's"
        );
        assert_eq!(state.borrow().selected, Some(0), "it picks the first row");
        assert!(press(&mut element, "ArrowDown", &app));
        assert_eq!(state.borrow().selected, Some(1), "the next moves down");
        assert!(press(&mut element, "ArrowUp", &app));
        assert_eq!(state.borrow().selected, Some(0), "and back up");
        element.dispatch_event(
            &key("Enter", ModifiersState::none()),
            &mut EventContext::default(),
            &app,
        );
        assert_eq!(
            answers.borrow().as_slice(),
            [("Postgres".to_string(), None)],
            "Enter sends the highlighted row"
        );
    }

    /// Enter on "Other…" opens its field instead of sending an empty answer; the
    /// field's own text goes out on the next Enter.
    #[test]
    fn enter_on_other_opens_the_free_text_instead_of_sending() {
        let app = AppContext::default();
        let (answers, handler) = answers();
        let card = card();
        let state = card.state.clone();
        let mut element = card.with_on_answer(handler).finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        // From no selection, Up picks the last row, which is "Other…".
        assert!(press(&mut element, "ArrowUp", &app));
        assert!(state.borrow().other, "Other… is the picked row");
        assert!(press(&mut element, "Enter", &app));
        assert_eq!(
            state.borrow().focused,
            Some(AskFocus::FreeText),
            "Enter opens the field"
        );
        assert!(answers.borrow().is_empty(), "nothing was sent yet");

        // The next frame draws the field, which takes the typed answer.
        render_element(&mut element, vec2f(520.0, 800.0), &app);
        for ch in ["n", "o"] {
            press(&mut element, ch, &app);
        }
        press(&mut element, "Enter", &app);
        assert_eq!(
            answers.borrow().as_slice(),
            [("no".to_string(), None)],
            "Enter sends the typed answer"
        );
    }

    /// A question that lists its answers in prose draws each option once: the
    /// list is dropped, the question and the numbered rows stay.
    #[test]
    fn the_question_does_not_repeat_the_answer_rows() {
        let app = AppContext::default();
        let mut element = AskUserCard::new(
            "Pick a host:\n\nA) staging\nB) production",
            vec!["staging".to_string(), "production".to_string()],
        )
        .finish();
        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        let texts = drawn(&commands);

        assert!(
            texts.iter().any(|text| text == "Pick a host:"),
            "the question stays: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "A) staging"),
            "the lettered list is gone: {texts:?}"
        );
        for label in ["staging", "production"] {
            assert_eq!(
                texts.iter().filter(|text| *text == label).count(),
                1,
                "{label} is drawn once, as its row: {texts:?}"
            );
        }
    }

    /// Ctrl-C and Escape both skip the question.
    #[test]
    fn ctrl_c_and_escape_skip() {
        let app = AppContext::default();
        let (skips, handler) = skips();
        let mut element = card().with_on_skip(handler).finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        element.dispatch_event(
            &key(
                "c",
                ModifiersState {
                    ctrl: true,
                    ..ModifiersState::none()
                },
            ),
            &mut EventContext::default(),
            &app,
        );
        assert_eq!(*skips.borrow(), 1, "Ctrl-C skips");
        press(&mut element, "Escape", &app);
        assert_eq!(*skips.borrow(), 2, "Escape skips");
    }

    /// A click anywhere on a row picks it, and the picked row is highlighted.
    #[test]
    fn clicking_a_row_selects_it() {
        let app = AppContext::default();
        let card = card();
        let state = card.state.clone();
        let mut element = card.finish();
        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        click_text(&mut element, &commands, "SQLite", &app);
        assert_eq!(state.borrow().selected, Some(1));

        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        let size = element.size().expect("the panel laid out");
        let selected_band = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Selected) && rect.width() > size.x * 0.8
            )
        });
        assert!(selected_band, "the picked row draws the selection band");
    }

    /// "Other…" opens the free-text field, and typing there clears the picked
    /// row so one answer is composed.
    #[test]
    fn other_opens_free_text_and_typing_clears_the_selection() {
        let app = AppContext::default();
        let card = card();
        let state = card.state.clone();
        let mut element = card.finish();
        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        click_text(&mut element, &commands, "Other...", &app);
        assert!(state.borrow().other, "Other… is picked");
        assert!(
            state.borrow().focused == Some(AskFocus::FreeText),
            "and the field holds the keys"
        );

        let commands = render_element(&mut element, vec2f(520.0, 800.0), &app);
        assert!(
            drawn(&commands)
                .iter()
                .any(|text| text == "Type your answer…"),
            "the free-text field is revealed"
        );

        // A row picked while the field is open is dropped as soon as a
        // character lands in it.
        state.borrow_mut().selected = Some(0);
        for ch in ["n", "o"] {
            element.dispatch_event(
                &key(ch, ModifiersState::none()),
                &mut EventContext::default(),
                &app,
            );
        }
        let state = state.borrow();
        assert_eq!(state.selected, None, "typing clears the picked row");
        assert_eq!(state.free_text, "no");
    }

    /// With the keys turned off (the rich input holds the keyboard), the panel
    /// leaves a typed digit to the field that has it.
    #[test]
    fn disabled_keys_leave_the_digit_to_the_focused_field() {
        let app = AppContext::default();
        let card = card();
        let state = card.state.clone();
        let mut element = card.with_keys_enabled(false).finish();
        render_element(&mut element, vec2f(520.0, 800.0), &app);

        assert!(!press(&mut element, "1", &app), "the digit falls through");
        assert_eq!(state.borrow().selected, None);
    }

    /// The composed answer keeps the credential apart from the answer text.
    #[test]
    fn ask_user_card_threads_credential_out_of_answer() {
        let app = AppContext::default();
        let mut card = AskUserCard::new("What's the token?".to_string(), vec![]);
        card.layout(
            SizeConstraint::loose(vec2f(520.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        {
            let mut state = card.state.borrow_mut();
            state.credential_name = "github_token".to_string();
            state.credential = "ghs_secret".to_string();
        }
        let (answer, credential) = compose(&card.state.borrow(), &card.quick_replies);
        // The secret is not embedded in the answer text.
        assert!(!answer.contains("ghs_secret"));
        assert_eq!(
            credential,
            Some(("github_token".to_string(), "ghs_secret".to_string()))
        );
    }

    /// The selection survives a rebuild when the card is built from the same
    /// `AskUserUi` the app carries between frames.
    #[test]
    fn the_answer_state_survives_a_rebuild() {
        let app = AppContext::default();
        let ui = AskUserUi::new(
            "Pick one".to_string(),
            vec!["A".to_string(), "B".to_string()],
        );
        let mut first = AskUserCard::from_ui(&ui).finish();
        render_element(&mut first, vec2f(520.0, 800.0), &app);
        assert!(press(&mut first, "2", &app));

        // The next frame's card is a fresh element over the same UI state.
        let mut second = AskUserCard::from_ui(&ui).finish();
        let commands = render_element(&mut second, vec2f(520.0, 800.0), &app);
        let size = second.size().expect("the panel laid out");
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Selected) && rect.width() > size.x * 0.8
            )),
            "the picked row is still picked on the next frame"
        );
    }

    #[test]
    fn the_ask_user_surface_paints_a_band_not_a_bordered_card() {
        let app = AppContext::default();
        let mut card: Box<dyn Element> = card().finish();
        let commands = render_element(&mut card, vec2f(520.0, 800.0), &app);

        // The rows and keycaps outline themselves; nothing strokes the surface's
        // full rectangle.
        let size = card.size().expect("the surface laid out");
        let outer_border = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::StrokeRect { rect, .. }
                    if (rect.width() - size.x).abs() < 0.5
                        && (rect.height() - size.y).abs() < 0.5
            )
        });
        assert!(!outer_border, "the ask-user surface draws no border");
    }
}
