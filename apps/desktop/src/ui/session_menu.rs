//! The sidebar session menu's surfaces: the "Rename conversation" dialog, the
//! stop-and-archive confirmation, and the transient toast notice.
//!
//! The builders take plain values and callbacks (no `UiState`/`UiSnapshot`), so
//! the surfaces can be composed and tested on their own; the root view mounts
//! them in its overlay stack when its state says so.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment, Dialog,
    EdgeInsets, Element, Empty, Expanded, Fill, Flex, Icon, KeyHandler, MainAxisAlignment,
    MainAxisSize, Modal, Spacer, Text, TextInput, DIALOG_DEFAULT_WIDTH,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::top_overlay::TopBanner;
use super::types::ArchivedFilter;
use super::{UiActions, UiSnapshot};

/// The toast's widest casing: DSH's toast caps at 640 and the window's edges
/// take it down from there.
pub(crate) const TOAST_MAX_WIDTH: f32 = 640.0;

/// The transient notice's tone: a circled check for a success, the info glyph
/// for a warning (DSH's two toast faces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastTone {
    Success,
    Warning,
}

impl ToastTone {
    fn icon(self) -> &'static str {
        match self {
            Self::Success => "check",
            Self::Warning => "info",
        }
    }

    fn color(self) -> ColorToken {
        match self {
            Self::Success => ColorToken::Success,
            Self::Warning => ColorToken::Warning,
        }
    }
}

/// Box a plain closure as a toast action's run callback.
fn toast_run(action: impl FnMut() + 'static) -> Rc<RefCell<dyn FnMut()>> {
    Rc::new(RefCell::new(action))
}

/// One inline action a toast offers ("undo", "filter archived conversations").
pub(crate) struct ToastAction {
    pub label: String,
    pub run: Rc<RefCell<dyn FnMut()>>,
}

#[cfg(test)]
impl ToastAction {
    pub(crate) fn new(
        label: impl Into<String>,
        run: impl FnMut() + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            run: toast_run(run),
        }
    }
}

/// Everything the Rename dialog needs to draw and act: the draft, whether the
/// field has the keyboard focus, an inline error, and the three callbacks.
pub(crate) struct RenameDialogState<'a> {
    /// The dialog's heading ("Rename conversation" / "Rename workspace").
    pub title: &'a str,
    /// The name field's placeholder ("Conversation name" / "Workspace name").
    pub placeholder: &'a str,
    pub draft: &'a str,
    pub focused: bool,
    pub error: Option<&'a str>,
    pub on_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_cancel: Rc<RefCell<dyn FnMut()>>,
}

/// The "Rename conversation" dialog (DSH's Rename session modal): one focused
/// text field, Enter saves, Escape/backdrop cancels, and the save is disabled
/// while the draft is blank.
pub(crate) fn build_rename_dialog(
    app: &AppContext,
    state: RenameDialogState<'_>,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let on_change = Rc::clone(&state.on_change);
    let on_submit = Rc::clone(&state.on_submit);
    let cancel_for_escape = Rc::clone(&state.on_cancel);
    let cancel_for_close = Rc::clone(&state.on_cancel);
    let save = Rc::clone(&state.on_submit);
    let cancel = Rc::clone(&state.on_cancel);
    let blank = state.draft.trim().is_empty();

    let field = TextInput::new()
        .with_value(state.draft)
        .with_placeholder(state.placeholder)
        .with_focused(state.focused)
        .with_full_width(true)
        .with_on_change(move |value| (on_change.borrow_mut())(value))
        .with_on_submit(move || (on_submit.borrow_mut())())
        .finish();

    let mut body = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(xs)
        .with_child(field)
        .with_child(
            Text::new("Enter saves · Esc cancels")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    if let Some(error) = state.error {
        body = body.with_child(
            Text::new(error.to_string())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }

    let footer = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::End)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Cancel").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (cancel.borrow_mut())())
                .finish(),
        )
        .with_child(
            Button::new(Text::new("Rename").finish())
                .with_variant(ButtonVariant::Primary)
                .with_disabled(blank)
                .with_on_click(move || (save.borrow_mut())())
                .finish(),
        )
        .finish();

    let modal = Modal::new(
        state.title,
        body.finish(),
        Some(footer),
        app,
    );
    let panel = KeyHandler::new(
        Box::new(modal),
        move |key: &str, _modifiers| {
            if key == "Escape" {
                (cancel_for_escape.borrow_mut())();
                return true;
            }
            false
        },
    )
    .finish();
    Dialog::new(panel)
        .with_open(true)
        .with_width(DIALOG_DEFAULT_WIDTH)
        .with_on_close(move || (cancel_for_close.borrow_mut())())
        .finish()
}

/// Everything the stop-and-archive confirmation needs to draw and act.
pub(crate) struct ArchiveConfirmState<'a> {
    /// The conversation's title, quoted in the description.
    pub title: &'a str,
    /// The work that will be stopped, one honest line per family (Goble can
    /// attribute the turn and its running subagents; not per-conversation jobs).
    pub activities: &'a [String],
    /// Whether the stop-and-archive call is in flight: the status line shows,
    /// both controls disable, and Escape/backdrop no longer cancel.
    pub busy: bool,
    pub error: Option<&'a str>,
    pub on_confirm: Rc<RefCell<dyn FnMut()>>,
    pub on_cancel: Rc<RefCell<dyn FnMut()>>,
}

/// The "Stop and archive this conversation?" confirmation (DSH's
/// ArchiveSession modal): the honest activity list, Cancel, and the outlined
/// danger "Stop and archive".
pub(crate) fn build_archive_confirm(
    app: &AppContext,
    state: ArchiveConfirmState<'_>,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let busy = state.busy;
    let confirm = Rc::clone(&state.on_confirm);
    let cancel = Rc::clone(&state.on_cancel);
    let cancel_for_escape = Rc::clone(&state.on_cancel);
    let cancel_for_close = Rc::clone(&state.on_cancel);

    let mut body = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(xs)
        .with_child(
            Text::new(format!(
                "“{}” still has work in progress. Archiving stops it first; you can \
                 restore it later from the archived filter in the view options, and the \
                 stopped work will not resume on its own.",
                state.title
            ))
            .with_font_size(12.0)
            .with_line_height(18.0 / 12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_child(
            Text::new("Work that will be stopped")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    for activity in state.activities {
        body = body.with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Container::new(
                        Container::new(
                            goble_ui::elements::Empty::new()
                                .with_size(goble_ui::geometry::vec2f(6.0, 6.0))
                                .finish(),
                        )
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Muted)))
                        .with_corner_radius(3.0)
                        .finish(),
                    )
                    .with_padding(EdgeInsets::uniform(2.0))
                    .finish(),
                )
                .with_child(
                    Expanded::new(
                        Text::new(activity.clone())
                            .with_font_size(12.0)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
        );
    }
    if busy {
        body = body.with_child(
            Text::new("Stopping and archiving…")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    if let Some(error) = state.error {
        body = body.with_child(
            Text::new(error.to_string())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }

    let danger_ink = app.theme.color(ColorToken::Error);
    let footer = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::End)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Cancel").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_disabled(busy)
                .with_on_click(move || (cancel.borrow_mut())())
                .finish(),
        )
        .with_child(
            Button::new(
                Text::new("Stop and archive")
                    .with_theme_color(ColorToken::Error, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Outline)
            .with_border_color(danger_ink)
            .with_disabled(busy)
            .with_on_click(move || (confirm.borrow_mut())())
            .finish(),
        )
        .finish();

    let modal = Modal::new(
        "Stop and archive this conversation?",
        body.finish(),
        Some(footer),
        app,
    );
    let panel = KeyHandler::new(
        Box::new(modal),
        move |key: &str, _modifiers| {
            if key == "Escape" && !busy {
                (cancel_for_escape.borrow_mut())();
                return true;
            }
            false
        },
    )
    .finish();
    Dialog::new(panel)
        .with_open(true)
        .with_width(DIALOG_DEFAULT_WIDTH)
        .with_on_close(move || {
            if !busy {
                (cancel_for_close.borrow_mut())();
            }
        })
        .finish()
}

/// The transient notice at the top of the window: one sentence, an optional
/// row of inline actions, and the success/warning glyph. It holds no room in
/// the tree (see [`TopBanner`]) and expires on the app's clock.
pub(crate) fn build_toast(
    app: &AppContext,
    message: &str,
    tone: ToastTone,
    actions: &[ToastAction],
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new(tone.icon())
                .with_size(16.0)
                .with_theme_color(tone.color(), app)
                .finish(),
        )
        .with_child(
            Expanded::new(
                Text::new(message.to_string())
                    .with_font_size(13.0)
                    .with_line_height(20.0 / 13.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .finish(),
        );
    for action in actions {
        let run = Rc::clone(&action.run);
        row = row.with_child(
            Button::new(
                Text::new(action.label.clone())
                    .with_theme_color(ColorToken::Focus, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || (run.borrow_mut())())
            .finish(),
        );
    }
    let card = Container::new(row.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_corner_radius(12.0)
        .with_padding(EdgeInsets::new(md, 12.0, md, 12.0))
        .finish();
    let card = ConstrainedBox::new(card)
        .with_max_width(TOAST_MAX_WIDTH)
        .finish();
    TopBanner::new(card)
        .with_max_width(TOAST_MAX_WIDTH)
        .finish()
}

/// The Rename dialog as the root view mounts it: an inert, zero-size dialog
/// unless the state holds one.
pub(crate) fn build_rename_dialog_surface(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let Some(rename) = state.sidebar_rename.as_ref() else {
        return Dialog::new(Empty::new().finish())
            .with_open(false)
            .finish();
    };
    build_rename_dialog(
        app,
        RenameDialogState {
            title: "Rename conversation",
            placeholder: "Conversation name",
            draft: &rename.draft,
            focused: rename.focused,
            error: rename.error.as_deref(),
            on_change: actions.on_session_rename_draft.clone(),
            on_submit: actions.on_session_rename_commit.clone(),
            on_cancel: actions.on_session_rename_cancel.clone(),
        },
    )
}

/// The Workspaces list's "Rename workspace" dialog as the root view mounts it:
/// an inert, zero-size dialog unless the state holds one. It is the session
/// Rename dialog's casing with the workspace copy — DSH's `Rename workspace`
/// modal, the same one-field form.
pub(crate) fn build_workspace_rename_dialog_surface(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let Some(rename) = state.sidebar_workspace_rename.as_ref() else {
        return Dialog::new(Empty::new().finish())
            .with_open(false)
            .finish();
    };
    build_rename_dialog(
        app,
        RenameDialogState {
            title: "Rename workspace",
            placeholder: "Workspace name",
            draft: &rename.draft,
            focused: rename.focused,
            error: rename.error.as_deref(),
            on_change: actions.on_sidebar_workspace_rename_draft.clone(),
            on_submit: actions.on_sidebar_workspace_rename_commit.clone(),
            on_cancel: actions.on_sidebar_workspace_rename_cancel.clone(),
        },
    )
}

/// The stop-and-archive confirmation as the root view mounts it: inert unless
/// the state holds a pending archive.
pub(crate) fn build_archive_confirm_surface(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let Some(confirm) = state.archive_confirm.as_ref() else {
        return Dialog::new(Empty::new().finish())
            .with_open(false)
            .finish();
    };
    build_archive_confirm(
        app,
        ArchiveConfirmState {
            title: &confirm.title,
            activities: &confirm.activities,
            busy: confirm.busy,
            error: confirm.error.as_deref(),
            on_confirm: actions.on_archive_confirm.clone(),
            on_cancel: actions.on_archive_cancel.clone(),
        },
    )
}

/// The notice as the root view mounts it, or `None` while none is up. The
/// archive/store notices carry their inline actions: undo, and — while the
/// archived filter is still the default — the filter that shows archived rows.
pub(crate) fn build_toast_surface(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Option<Box<dyn Element>> {
    let notice = state.toast.as_ref()?;
    let kind = &notice.kind;
    let tone = if kind.is_warning() {
        ToastTone::Warning
    } else {
        ToastTone::Success
    };
    let mut toast_actions: Vec<ToastAction> = Vec::new();
    if let Some(id) = kind.archived_id() {
        let unarchive = actions.on_unarchive_conversation.clone();
        let dismiss = actions.on_toast_dismiss.clone();
        let id = id.to_string();
        toast_actions.push(ToastAction {
            label: "undo".to_string(),
            run: toast_run(move || {
                (unarchive.borrow_mut())(id.clone());
                (dismiss.borrow_mut())();
            }),
        });
        // DSH offers the filter switch only while the archived filter is the
        // default: once the user shows archived rows the action would do
        // nothing.
        if state.sidebar_archived_filter == ArchivedFilter::Default {
            let filter = actions.on_archived_filter_change.clone();
            let dismiss = actions.on_toast_dismiss.clone();
            toast_actions.push(ToastAction {
                label: "filter archived conversations".to_string(),
                run: toast_run(move || {
                    (filter.borrow_mut())(ArchivedFilter::Show);
                    (dismiss.borrow_mut())();
                }),
            });
        }
    }
    Some(build_toast(app, &kind.message(), tone, &toast_actions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_ui::elements::{LayoutContext, SizeConstraint};
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    fn app() -> AppContext {
        AppContext::default()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn noop() -> Rc<RefCell<dyn FnMut()>> {
        Rc::new(RefCell::new(|| {}))
    }

    fn noop_change() -> Rc<RefCell<dyn FnMut(String)>> {
        Rc::new(RefCell::new(|_| {}))
    }

    #[test]
    fn the_rename_dialog_offers_the_field_and_its_actions() {
        let app = app();
        let mut dialog = build_rename_dialog(
            &app,
            RenameDialogState {
                title: "Rename conversation",
                placeholder: "Conversation name",
                draft: "Redesign the sidebar",
                focused: true,
                error: None,
                on_change: noop_change(),
                on_submit: noop(),
                on_cancel: noop(),
            },
        );
        let _ = dialog.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let drawn = render_element(&mut dialog, vec2f(1024.0, 768.0), &app);
        let labels = texts(&drawn);
        for expected in [
            "Rename conversation",
            "Cancel",
            "Rename",
            "Enter saves · Esc cancels",
        ] {
            assert!(
                labels.iter().any(|text| text == expected),
                "the dialog draws {expected:?}: {labels:?}"
            );
        }
        assert!(
            labels.iter().any(|text| text == "Redesign the sidebar"),
            "the draft is in the field: {labels:?}"
        );
    }

    #[test]
    fn the_archive_confirm_lists_the_work_that_will_stop() {
        let app = app();
        let activities = vec![
            "The turn in progress".to_string(),
            "2 running subagents: Explore, Review".to_string(),
        ];
        let mut dialog = build_archive_confirm(
            &app,
            ArchiveConfirmState {
                title: "Redesign the sidebar",
                activities: &activities,
                busy: false,
                error: None,
                on_confirm: noop(),
                on_cancel: noop(),
            },
        );
        let drawn = render_element(&mut dialog, vec2f(1024.0, 768.0), &app);
        let labels = texts(&drawn);
        for expected in [
            "Stop and archive this conversation?",
            "Work that will be stopped",
            "The turn in progress",
            "2 running subagents: Explore, Review",
            "Cancel",
            "Stop and archive",
        ] {
            assert!(
                labels.iter().any(|text| text == expected),
                "the confirm draws {expected:?}: {labels:?}"
            );
        }
        assert!(
            !labels.iter().any(|text| text == "Stopping and archiving…"),
            "the pending line is absent until the stop is in flight"
        );
    }

    #[test]
    fn the_archive_confirm_shows_the_pending_and_failure_lines() {
        let app = app();
        let activities = ["The turn in progress".to_string()];
        let mut dialog = build_archive_confirm(
            &app,
            ArchiveConfirmState {
                title: "Work",
                activities: &activities,
                busy: true,
                error: Some("Couldn’t archive the conversation."),
                on_confirm: noop(),
                on_cancel: noop(),
            },
        );
        let drawn = render_element(&mut dialog, vec2f(1024.0, 768.0), &app);
        let labels = texts(&drawn);
        assert!(labels.iter().any(|text| text == "Stopping and archiving…"));
        assert!(labels
            .iter()
            .any(|text| text == "Couldn’t archive the conversation."));
    }

    /// The toast is the top-center notice: its message, the tone's glyph and
    /// its inline actions. Success carries the check, a warning the info glyph.
    #[test]
    fn the_toast_reports_its_message_tone_and_actions() {
        let app = app();
        let ran = Rc::new(RefCell::new(0usize));
        let undo = Rc::clone(&ran);
        let actions = vec![ToastAction::new("undo", move || *undo.borrow_mut() += 1)];
        let mut toast = build_toast(
            &app,
            "Conversation archived. You can undo.",
            ToastTone::Success,
            &actions,
        );
        let drawn = render_element(&mut toast, vec2f(1024.0, 768.0), &app);
        let labels = texts(&drawn);
        assert!(labels
            .iter()
            .any(|text| text == "Conversation archived. You can undo."));
        assert!(labels.iter().any(|text| text == "undo"));
        assert!(
            drawn.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "check"
            )),
            "a success toast carries the check"
        );
        // Above the topbar, centered.
        let message_origin = drawn.iter().find_map(|command| match command {
            RenderCommand::DrawText { origin, .. } => Some(*origin),
            _ => None,
        });
        assert!(message_origin.is_some_and(|origin| origin.y > 0.0));

        let mut warning = build_toast(&app, "Copy failed.", ToastTone::Warning, &[]);
        let drawn = render_element(&mut warning, vec2f(1024.0, 768.0), &app);
        assert!(
            drawn.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "info"
            )),
            "a warning toast carries the info glyph"
        );
    }
}
