//! Acceptance tests for the sidebar session menu, the archived filter and the
//! archived row: every DSH row wired to real data, the quiet vs live archive
//! paths, rename/copy/fork/star durability, the archived click guard, the
//! toast's own deadline, and the hover no-resize invariant at the app level.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::conversation_list_item::{
    SIDEBAR_ROW_HEIGHT, SIDEBAR_ROW_RADIUS,
};
use goble_ui::elements::{AppContext, Element, EventContext};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;
use goble_ui::theme::ColorToken;

use crate::root_view::RootView;
use crate::state::{PaneSession, ToastKind, UiState};
use crate::ui::ArchivedFilter;

fn window() -> Vector2F {
    vec2f(1024.0, 768.0)
}

/// The menu's row labels, in the order DSH's session menu draws them (Goble's
/// own Delete row last). Used to pull the tray's rows out of a frame the same
/// way a reader sees them.
const MENU_LABELS: [&str; 8] = [
    "Star",
    "Unstar",
    "Rename conversation",
    "Fork conversation",
    "Copy conversation ID",
    "Archive conversation",
    "Unarchive conversation",
    "Delete conversation",
];

/// The app over a real store with two answered conversations.
fn seeded() -> (
    Box<dyn Element>,
    Rc<RefCell<UiState>>,
    Arc<DesktopState>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let first = desktop
        .create_chat("Redesign the sidebar", None, None)
        .expect("create the first conversation");
    desktop
        .add_chat_message(&first, "user", "salut")
        .expect("the user's prompt");
    desktop
        .add_chat_message(&first, "assistant", "Bine ai venit!")
        .expect("the agent's reply");
    desktop
        .set_chat_working_dir(&first, "/tmp/goble-work")
        .expect("the conversation's directory");
    let second = desktop
        .create_chat("Automate Instagram posting", None, None)
        .expect("create the second conversation");
    desktop
        .add_chat_message(&second, "assistant", "done")
        .expect("the agent's reply");
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.refresh_conversations(&desktop);
    }
    (Box::new(view), state, desktop, dir)
}

fn id_of(state: &Rc<RefCell<UiState>>, name: &str) -> String {
    state
        .borrow()
        .conversations
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.id.clone())
        .unwrap_or_else(|| panic!("the conversation {name:?} is listed"))
}

fn render(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
    render_element(root, window(), app)
}

fn dispatch(root: &mut Box<dyn Element>, app: &AppContext, event: DispatchedEvent) {
    let mut ctx = EventContext::default();
    root.dispatch_event(&event, &mut ctx, app);
}

fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
    dispatch(
        root,
        app,
        DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
    );
    dispatch(
        root,
        app,
        DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        },
    );
}

fn press(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
    dispatch(
        root,
        app,
        DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
    );
}

/// Hover the row (the shared cell the rows read from) and press its ellipsis,
/// the way a click on the 3-dots does. Returns the frame with the tray open.
fn open_row_menu(
    root: &mut Box<dyn Element>,
    state: &Rc<RefCell<UiState>>,
    app: &AppContext,
    id: &str,
) -> Vec<RenderCommand> {
    state.borrow_mut().sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
    let commands = render(root, app);
    let at = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon { name, origin, .. }
                if name == "dots-horizontal"
                    && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin + vec2f(8.0, 8.0))
            }
            _ => None,
        })
        .expect("the hovered row draws its ellipsis");
    press(root, app, at);
    render(root, app)
}

/// The menu row labels a frame draws, in vertical order (the tray paints its
/// rows top to bottom; the frame draws it twice — inside the row's card and in
/// the top layer — so equal labels are collapsed).
fn menu_labels(commands: &[RenderCommand]) -> Vec<String> {
    let mut rows: Vec<(f32, String)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text,
                origin,
                font_size,
                ..
            } if (*font_size - 12.0).abs() < 0.01
                && origin.y > crate::ui::shell::TOPBAR_HEIGHT
                && MENU_LABELS.contains(&text.as_str()) =>
            {
                Some((origin.y, text.clone()))
            }
            _ => None,
        })
        .collect();
    rows.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut labels: Vec<String> = Vec::new();
    for (_, label) in rows {
        if !labels.iter().any(|seen| seen == &label) {
            labels.push(label);
        }
    }
    labels
}

/// Click a row of the open tray by its label.
fn click_menu_label(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    commands: &[RenderCommand],
    label: &str,
) {
    let at = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == label => {
                Some(*origin + vec2f(4.0, 4.0))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the tray draws {label:?}"));
    click(root, app, at);
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

fn toast_kind(state: &Rc<RefCell<UiState>>) -> Option<ToastKind> {
    state
        .borrow()
        .toast
        .as_ref()
        .map(|notice| notice.kind.clone())
}

fn archive_id(state: &Rc<RefCell<UiState>>) -> String {
    id_of(state, "Redesign the sidebar")
}

/// Every DSH session-menu row, in DSH's order, for a listed conversation; the
/// tray is app-supplied, so this is the app's own list.
#[test]
fn the_row_menu_lists_every_dsh_option_in_order() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded();
    let id = archive_id(&state);
    let commands = open_row_menu(&mut root, &state, &app, &id);
    assert_eq!(
        menu_labels(&commands),
        vec![
            "Star",
            "Rename conversation",
            "Fork conversation",
            "Copy conversation ID",
            "Archive conversation",
            "Delete conversation",
        ]
    );
}

/// An archived row resolves DSH's archived menu: no Star and no Archive (it
/// becomes Unarchive); Rename, Fork, Copy and Delete stay.
#[test]
fn the_archived_row_menu_offers_unarchive_not_star_or_archive() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    desktop
        .set_chat_archived(&id, true)
        .expect("archive the conversation");
    state.borrow_mut().refresh_conversations(&desktop);
    state.borrow().set_archived_filter(ArchivedFilter::Show);

    let commands = open_row_menu(&mut root, &state, &app, &id);
    assert_eq!(
        menu_labels(&commands),
        vec![
            "Rename conversation",
            "Fork conversation",
            "Copy conversation ID",
            "Unarchive conversation",
            "Delete conversation",
        ]
    );
}

/// Star/Unstar is durable: the row's menu writes `chats.starred` and the
/// Starred set is seeded from the store on the next start.
#[test]
fn starring_from_the_menu_persists() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Star");

    assert!(desktop.chat_starred(&id).unwrap(), "the store keeps the pin");
    assert!(
        state.borrow().starred_conversations.contains(&id),
        "and the running UI's Starred block lists it"
    );
    let restarted = UiState::from_desktop(desktop.as_ref());
    assert!(
        restarted.starred_conversations.contains(&id),
        "a restart restores the pinned block"
    );
}

/// Rename opens the real dialog and commits through the durable title path;
/// cancelling writes nothing.
#[test]
fn renaming_a_conversation_from_the_menu_persists_the_title() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Rename conversation");
    {
        let opened = state.borrow();
        let rename = opened.sidebar_rename.as_ref().expect("the dialog opened");
        assert_eq!(rename.id, id);
        assert_eq!(rename.draft, "Redesign the sidebar");
        assert!(rename.focused);
    }

    state.borrow_mut().rename_draft("Sidebar subjects".to_string());
    state.borrow_mut().commit_rename(Some(desktop.as_ref()));
    assert_eq!(
        desktop.store_clone().get_chat_title(&id).unwrap(),
        Some("Sidebar subjects".to_string()),
        "the store holds the new title"
    );
    assert!(
        state
            .borrow()
            .conversations
            .iter()
            .any(|entry| entry.id == id && entry.name == "Sidebar subjects"),
        "and the sidebar row reads it"
    );
    assert!(state.borrow().sidebar_rename.is_none(), "the dialog closed");

    // Escape / the backdrop cancels without writing.
    state.borrow_mut().begin_rename(&id);
    state.borrow_mut().rename_draft("Thrown away".to_string());
    state.borrow_mut().cancel_rename();
    assert_eq!(
        desktop.store_clone().get_chat_title(&id).unwrap(),
        Some("Sidebar subjects".to_string())
    );
}

/// Fork copies the transcript and the source's environment, and leaves the
/// current selection alone (DSH forks without changing what is on screen).
#[test]
fn forking_a_conversation_from_the_menu_creates_the_inheriting_fork() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    let before_chats = desktop.list_chats().len();
    let before_selection = state.borrow().selected_id.clone();

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Fork conversation");

    assert_eq!(desktop.list_chats().len(), before_chats + 1, "a new chat");
    let forked = desktop
        .list_chats()
        .into_iter()
        .find(|chat| chat.title == "Fork of Redesign the sidebar")
        .expect("the fork row");
    assert_eq!(
        desktop.list_chat_messages(&forked.id).unwrap().len(),
        2,
        "the source's messages are the fork's context"
    );
    assert_eq!(
        forked.working_dir.as_deref(),
        Some("/tmp/goble-work"),
        "the fork lands in the source's directory"
    );
    assert_eq!(
        state.borrow().selected_id,
        before_selection,
        "forking does not change the selection"
    );
}

/// Copy Session ID uses the real clipboard helper and toasts its outcome.
#[test]
fn copying_a_conversation_id_writes_the_clipboard_and_toasts() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded();
    let id = archive_id(&state);
    // What this environment's clipboard can honestly do decides the tone: an
    // OS helper that runs is a success, none is a warning (the in-process copy
    // still holds the value).
    let expected = if crate::clipboard::copy_text_checked("goble-clipboard-probe") {
        ToastKind::ConversationIdCopied
    } else {
        ToastKind::ConversationIdCopyFailed
    };

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Copy conversation ID");

    assert_eq!(toast_kind(&state), Some(expected.clone()), "the copy toasted");
    if expected == ToastKind::ConversationIdCopied {
        assert_eq!(expected.message(), "Conversation ID copied");
    }
}

/// Archiving a quiet conversation persists, hides the row under the default
/// filter, draws it greyed under "Show archived" and only it under "Archived
/// only"; the notice offers undo and the filter.
#[test]
fn archiving_a_quiet_conversation_hides_the_row_and_persists() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Archive conversation");

    assert!(
        desktop.archived_chat_ids().unwrap().contains(&id),
        "the archive stamp is durable"
    );
    assert!(
        matches!(toast_kind(&state), Some(ToastKind::Archived { .. })),
        "the archive notice is up: {:?}",
        toast_kind(&state)
    );
    assert!(state.borrow().archive_confirm.is_none(), "no confirm was needed");
    state.borrow_mut().refresh_conversations(&desktop);

    let commands = render(&mut root, &app);
    assert!(
        !drawn_texts(&commands).iter().any(|text| text == "Redesign the sidebar"),
        "the default filter hides the archived row"
    );

    state.borrow().set_archived_filter(ArchivedFilter::Show);
    let commands = render(&mut root, &app);
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "Redesign the sidebar"),
        "Show archived draws it in its slot"
    );

    state.borrow().set_archived_filter(ArchivedFilter::Only);
    let commands = render(&mut root, &app);
    let texts = drawn_texts(&commands);
    assert!(texts.iter().any(|text| text == "Redesign the sidebar"));
    assert!(
        !texts.iter().any(|text| text == "Automate Instagram posting"),
        "Archived only hides the listed conversation"
    );
}

/// A brand-new database with nothing archived says so under the archived-only
/// filter instead of claiming the environment has no conversations.
#[test]
fn the_archived_only_empty_state_says_no_archived_conversations() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded();
    state.borrow().set_archived_filter(ArchivedFilter::Only);
    let commands = render(&mut root, &app);
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "No archived conversations yet."),
        "the archived empty state: {:?}",
        drawn_texts(&commands)
    );
}

/// An archived row cannot be opened: clicking it raises the notice and opens
/// no tab/pane.
#[test]
fn clicking_an_archived_row_does_not_open_it_and_toasts() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    desktop
        .set_chat_archived(&id, true)
        .expect("archive the conversation");
    state.borrow_mut().refresh_conversations(&desktop);
    state.borrow().set_archived_filter(ArchivedFilter::Show);

    let commands = render(&mut root, &app);
    let at = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Redesign the sidebar" => {
                Some(*origin + vec2f(10.0, 8.0))
            }
            _ => None,
        })
        .expect("the archived row is drawn under Show archived");
    let panes_before = state.borrow().pane_sessions.len();
    click(&mut root, &app, at);

    assert_eq!(
        toast_kind(&state),
        Some(ToastKind::ArchivedNotOpenable),
        "the blocked open is explained"
    );
    assert_eq!(
        state.borrow().pane_sessions.len(),
        panes_before,
        "no pane/tab was opened"
    );
}

/// Archiving a live conversation asks first (with the honest activity list),
/// cancelling leaves it running, and Stop and archive stops the work and
/// archives it.
#[test]
fn archiving_a_live_conversation_asks_first_then_stops_and_archives() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    {
        let mut s = state.borrow_mut();
        let pane_id = s.active_pane_id;
        s.pane_sessions.insert(
            pane_id,
            PaneSession {
                conversation_id: id.clone(),
                draft: String::new(),
                path: String::new(),
                daemon_session_id: String::new(),
            },
        );
        s.pane_runtime.entry(pane_id).or_default().busy = true;
    }

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Archive conversation");
    {
        let s = state.borrow();
        let confirm = s.archive_confirm.as_ref().expect("the confirm opened");
        assert_eq!(confirm.id, id);
        assert!(
            confirm
                .activities
                .iter()
                .any(|line| line == "The turn in progress"),
            "the confirm names the turn: {:?}",
            confirm.activities
        );
        assert!(s.conversations.iter().any(|entry| entry.id == id && !entry.archived));
    }
    assert!(
        !desktop.archived_chat_ids().unwrap().contains(&id),
        "nothing is archived before the confirmation"
    );

    // Cancel leaves the conversation running and listed.
    state.borrow_mut().cancel_archive();
    assert!(state.borrow().archive_confirm.is_none());
    assert!(!desktop.archived_chat_ids().unwrap().contains(&id));

    // Reopen the menu and confirm: the work is stopped, then archived.
    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Archive conversation");
    assert!(state.borrow().archive_confirm.is_some());
    state.borrow_mut().confirm_archive(Some(desktop.as_ref()));

    assert!(
        desktop.archived_chat_ids().unwrap().contains(&id),
        "the confirmed archive is durable"
    );
    assert!(
        matches!(
            toast_kind(&state),
            Some(ToastKind::StoppedAndArchived { .. })
        ),
        "the stopped-and-archived notice is up: {:?}",
        toast_kind(&state)
    );
    let active_pane = state.borrow().active_pane_id;
    assert!(
        state
            .borrow()
            .pane_runtime
            .get(&active_pane)
            .map(|runtime| !runtime.busy)
            .unwrap_or(true),
        "the stopped turn is not left reported as running"
    );
}

/// Unarchive returns the row to the default list and clears the store stamp.
#[test]
fn unarchiving_restores_the_row_in_its_slot() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    desktop
        .set_chat_archived(&id, true)
        .expect("archive the conversation");
    state.borrow_mut().refresh_conversations(&desktop);
    state.borrow().set_archived_filter(ArchivedFilter::Show);

    let commands = open_row_menu(&mut root, &state, &app, &id);
    click_menu_label(&mut root, &app, &commands, "Unarchive conversation");

    assert!(
        !desktop.archived_chat_ids().unwrap().contains(&id),
        "the store stamp is cleared"
    );
    assert!(
        !state
            .borrow()
            .conversations
            .iter()
            .find(|entry| entry.id == id)
            .expect("the conversation")
            .archived,
        "the row itself is restored"
    );
    state.borrow().set_archived_filter(ArchivedFilter::Default);
    let commands = render(&mut root, &app);
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "Redesign the sidebar"),
        "the default list draws it again"
    );
}

/// The view-options menu carries the three archived filters and picking one
/// changes which rows the list draws.
#[test]
fn the_view_options_menu_offers_the_three_archived_filters() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = seeded();
    let id = archive_id(&state);
    desktop
        .set_chat_archived(&id, true)
        .expect("archive the conversation");
    state.borrow_mut().refresh_conversations(&desktop);

    let commands = render(&mut root, &app);
    let sliders = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon { name, origin, .. } if name == "sliders-two" => {
                Some(*origin + vec2f(8.0, 8.0))
            }
            _ => None,
        })
        .expect("the header draws its view-options control");
    click(&mut root, &app, sliders);
    let commands = render(&mut root, &app);
    let texts = drawn_texts(&commands);
    for expected in [
        "Hide archived",
        "Show archived",
        "Archived only",
    ] {
        assert!(
            texts.iter().any(|text| text == expected),
            "the view options offer {expected:?}: {texts:?}"
        );
    }

    click_menu_label(&mut root, &app, &commands, "Archived only");
    assert_eq!(state.borrow().archived_filter(), ArchivedFilter::Only);
    let commands = render(&mut root, &app);
    let texts = drawn_texts(&commands);
    assert!(texts.iter().any(|text| text == "Redesign the sidebar"));
    assert!(!texts.iter().any(|text| text == "Automate Instagram posting"));
}

/// The notice runs out on its own deadline while the frame clock reports
/// animation, then the window idles again.
#[test]
fn the_toast_runs_out_on_its_own() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded();
    state.borrow_mut().show_toast(ToastKind::ConversationIdCopied);
    assert!(state.borrow().toast.is_some());
    assert!(
        root.wants_animation(),
        "the frame clock runs while the notice is up"
    );
    {
        let mut s = state.borrow_mut();
        s.toast_until = Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
        s.expire_toast();
    }
    assert!(state.borrow().toast.is_none(), "the deadline cleared it");
    assert!(
        !root.wants_animation(),
        "a quiet app idles once the notice is gone"
    );
    let _ = render(&mut root, &app);
}

/// Hovering a row moves nothing: the hovered row keeps its 32pt band and the
/// row below keeps its exact position.
#[test]
fn the_hovered_session_row_does_not_move_the_rows_below_it() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded();
    let hovered = archive_id(&state);

    let resting = render(&mut root, &app);
    let below_rest = resting
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Automate Instagram posting" => {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the row below is drawn");

    state.borrow_mut().sidebar_hover_row.borrow_mut() =
        Some(format!("session:{hovered}"));
    let hovered_frame = render(&mut root, &app);
    let below_hover = hovered_frame
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Automate Instagram posting" => {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the row below is still drawn");
    assert_eq!(
        below_rest.y, below_hover.y,
        "the row below does not move when the row above is hovered"
    );

    let band = hovered_frame
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::Hover)
                && (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the hovered row paints its band");
    assert!(
        (band.height() - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
        "the hovered row is 32pt, not 36: {band:?}"
    );
}
