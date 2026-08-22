use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::AgentId;
use goble_core::thread::{MessageId, Participant, ParticipantId, ThreadId, UserId};
use goble_desktop_service::DesktopState;
use goble_ui::elements::chat_content::{ChatAction, ChatMessage};
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Checkbox, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point,
    SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::views::thread_list_view::{ThreadKind, ThreadListEntry};
use goble_ui::ThreadsContainer;

use crate::app::UiState;

fn map_thread_kind(kind: goble_core::thread::ThreadKind) -> ThreadKind {
    match kind {
        goble_core::thread::ThreadKind::Direct => ThreadKind::Direct,
        goble_core::thread::ThreadKind::Channel => ThreadKind::Channel,
        goble_core::thread::ThreadKind::Chat => ThreadKind::Chat,
    }
}

fn owner_participant_id(store: &goble_desktop_service::thread_store::ThreadStore) -> ParticipantId {
    store
        .get_profile()
        .map(|p| ParticipantId::user(p.id.0))
        .unwrap_or_else(|| ParticipantId::user(UserId::generate().0))
}

pub struct ThreadsViewPanel {
    content: Box<dyn Element>,
}

impl ThreadsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let bg = app.theme.color(ColorToken::Bg);
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let store = state.thread_store();

        let owner = store
            .get_profile()
            .map(|p| goble_core::thread::UserId(p.id.0))
            .unwrap_or_else(goble_core::thread::UserId::generate);

        let threads = store.list_threads();
        let mut selected_id = if !ui_state.borrow().selected_thread_id.is_empty() {
            ui_state.borrow().selected_thread_id.clone()
        } else {
            threads.first().map(|t| t.id.0.clone()).unwrap_or_default()
        };

        if selected_id.is_empty() && threads.is_empty() {
            let participant = Participant::User(owner.clone());
            match store.create_thread(
                goble_core::thread::ThreadKind::Chat,
                "New thread",
                owner.clone(),
                false,
                vec![participant],
                vec![],
            ) {
                Ok(thread) => {
                    selected_id = thread.id.0.clone();
                    ui_state.borrow_mut().selected_thread_id = selected_id.clone();
                }
                Err(e) => {
                    log::error!("failed to auto-create thread: {}", e);
                }
            }
        }

        let selected_thread_id = ThreadId(selected_id.clone());
        let selected_thread = store.get_thread(&selected_thread_id).ok();
        let is_direct = selected_thread
            .as_ref()
            .map(|t| t.kind == goble_core::thread::ThreadKind::Direct)
            .unwrap_or(false);
        let thread_kind_label = selected_thread
            .as_ref()
            .map(|t| match t.kind {
                goble_core::thread::ThreadKind::Channel => "Channel",
                goble_core::thread::ThreadKind::Direct => "Direct",
                goble_core::thread::ThreadKind::Chat => "Chat",
            })
            .unwrap_or("Thread");

        let entries: Vec<ThreadListEntry> = threads
            .iter()
            .map(|t| {
                let unread = if let Some(last_read) = store.get_last_read_at(&t.id) {
                    store
                        .list_messages(&t.id)
                        .unwrap_or_default()
                        .iter()
                        .filter(|m| m.created_at > last_read)
                        .count()
                } else {
                    store.list_messages(&t.id).unwrap_or_default().len()
                };
                ThreadListEntry {
                    id: t.id.0.clone(),
                    title: t.title.clone(),
                    kind: map_thread_kind(t.kind),
                    selected: t.id.0 == selected_id,
                    unread_count: unread,
                }
            })
            .collect();

        let messages: Vec<ChatMessage> = if selected_id.is_empty() {
            Vec::new()
        } else {
            store
                .list_messages(&selected_thread_id)
                .map(|msgs| {
                    msgs.iter()
                        .map(|m| {
                            let mut cm = ChatMessage::from_thread_message(m);
                            if let (Some(parent_id), Some(preview)) =
                                (m.reply_to.as_ref(), &mut cm.reply_to_preview)
                            {
                                *preview = msgs
                                    .iter()
                                    .find(|p| &p.id == parent_id)
                                    .map(|p| p.content.clone())
                                    .unwrap_or_default();
                            }
                            cm
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let state_for_send = Arc::clone(&state);
        let state_for_new = Arc::clone(&state);
        let state_for_action = Arc::clone(&state);
        let ui_state_for_select = Rc::clone(&ui_state);
        let ui_state_for_new = Rc::clone(&ui_state);
        let ui_state_for_send = Rc::clone(&ui_state);
        let ui_state_for_action = Rc::clone(&ui_state);
        let dirty_for_select = Rc::clone(&dirty);
        let dirty_for_send = Rc::clone(&dirty);
        let dirty_for_new = Rc::clone(&dirty);
        let dirty_for_action = Rc::clone(&dirty);

        let selected_id_for_action = selected_id.clone();
        let selected_id_for_send = selected_id.clone();

        // Header with participant management and metadata.
        let mut header_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);

        let kind_text = Text::new(format!("{} • {}", thread_kind_label, selected_id.chars().take(8).collect::<String>()))
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        let mark_read_selected_id = selected_id.clone();
        let state_for_mark_read = Arc::clone(&state);
        let dirty_for_mark_read = Rc::clone(&dirty);
        let mark_read_button = Button::new(Text::new("Mark read").with_theme_color(ColorToken::Text, app).finish())
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                if let Err(e) = state_for_mark_read.thread_store().mark_thread_read(&ThreadId(mark_read_selected_id.clone())) {
                    log::error!("failed to mark thread read: {}", e);
                }
                *dirty_for_mark_read.borrow_mut() = true;
            })
            .finish();
        let title_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(kind_text)
            .with_child(mark_read_button)
            .finish();
        header_column = header_column.with_child(title_row);

        // Participants list and removal.
        if let Ok(participants) = store.list_participants(&selected_thread_id) {
            let mut participant_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm);
            participant_row = participant_row.with_child(
                Text::new("Participants:")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
            for participant in participants {
                let state_for_remove = Arc::clone(&state);
                let dirty_for_remove = Rc::clone(&dirty);
                let pid = participant.participant_id();
                let label = pid.0.clone();
                let remove_selected_id = selected_id.clone();
                participant_row = participant_row.with_child(
                    Button::new(
                        Text::new(format!("{} ✕", label))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || {
                        if let Err(e) = state_for_remove
                            .thread_store()
                            .remove_participant(&ThreadId(remove_selected_id.clone()), &pid)
                        {
                            log::error!("failed to remove participant: {}", e);
                        }
                        *dirty_for_remove.borrow_mut() = true;
                    })
                    .finish(),
                );
            }
            header_column = header_column.with_child(participant_row.finish());
        }

        // Add participant form.
        if !is_direct {
            let add_selected_id = selected_id.clone();
            let add_id = Rc::new(RefCell::new(String::new()));
            let add_is_agent = Rc::new(RefCell::new(false));
            let add_id_change = Rc::clone(&add_id);
            let add_is_agent_change = Rc::clone(&add_is_agent);
            let state_for_add = Arc::clone(&state);
            let dirty_for_add = Rc::clone(&dirty);
            let add_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    TextInput::new()
                        .with_placeholder("participant id")
                        .with_on_change(move |v| *add_id_change.borrow_mut() = v)
                        .finish(),
                )
                .with_child(
                    Checkbox::new()
                        .with_label(Text::new("agent").with_theme_color(ColorToken::Text, app).finish())
                        .with_on_change(move |v| *add_is_agent_change.borrow_mut() = v)
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Add").with_theme_color(ColorToken::Text, app).finish())
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || {
                            let id = add_id.borrow().clone();
                            let is_agent = *add_is_agent.borrow();
                            if id.is_empty() {
                                return;
                            }
                            let participant = if is_agent {
                                Participant::Agent(AgentId(id))
                            } else {
                                Participant::User(UserId(id))
                            };
                            if let Err(e) = state_for_add
                                .thread_store()
                                .add_participant(&ThreadId(add_selected_id.clone()), participant)
                            {
                                log::error!("failed to add participant: {}", e);
                            }
                            *dirty_for_add.borrow_mut() = true;
                        })
                        .finish(),
                )
                .finish();
            header_column = header_column.with_child(add_row);

            // Invite by public key.
            let invite_selected_id = selected_id.clone();
            let pem = Rc::new(RefCell::new(String::new()));
            let name = Rc::new(RefCell::new(String::new()));
            let pem_change = Rc::clone(&pem);
            let name_change = Rc::clone(&name);
            let state_for_invite = Arc::clone(&state);
            let dirty_for_invite = Rc::clone(&dirty);
            let invite_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    TextInput::new()
                        .with_placeholder("public key PEM")
                        .with_on_change(move |v| *pem_change.borrow_mut() = v)
                        .finish(),
                )
                .with_child(
                    TextInput::new()
                        .with_placeholder("name")
                        .with_on_change(move |v| *name_change.borrow_mut() = v)
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Invite").with_theme_color(ColorToken::Text, app).finish())
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || {
                            if let Err(e) = state_for_invite
                                .thread_store()
                                .invite_user_by_public_key(
                                    &ThreadId(invite_selected_id.clone()),
                                    pem.borrow().clone(),
                                    name.borrow().clone(),
                                )
                            {
                                log::error!("failed to invite by public key: {}", e);
                            }
                            *dirty_for_invite.borrow_mut() = true;
                        })
                        .finish(),
                )
                .finish();
            header_column = header_column.with_child(invite_row);
        }

        // Reply target indicator.
        if let Some(reply_id) = ui_state.borrow().thread_reply_to_id.as_ref() {
            let ui_state_for_clear = Rc::clone(&ui_state);
            let dirty_for_clear = Rc::clone(&dirty);
            let reply_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new(format!("Replying to {}", reply_id.chars().take(8).collect::<String>()))
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Clear").with_theme_color(ColorToken::Text, app).finish())
                        .with_variant(ButtonVariant::Ghost)
                        .with_on_click(move || {
                            ui_state_for_clear.borrow_mut().thread_reply_to_id = None;
                            *dirty_for_clear.borrow_mut() = true;
                        })
                        .finish(),
                )
                .finish();
            header_column = header_column.with_child(reply_row);
        }

        let header = Container::new(header_column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish();

        let container = ThreadsContainer::new(selected_id_for_send.clone())
            .with_threads(entries)
            .with_messages(selected_id_for_send.clone(), messages)
            .with_header(header)
            .with_on_select(move |id| {
                ui_state_for_select.borrow_mut().selected_thread_id = id;
                *dirty_for_select.borrow_mut() = true;
            })
            .with_on_action(move |action| {
                let store = state_for_action.thread_store();
                let thread_id = ThreadId(selected_id_for_action.clone());
                match action {
                    ChatAction::ThreadReact { message_id, emoji } => {
                        let participant_id = owner_participant_id(&store);
                        if let Err(e) = store.add_reaction(
                            &thread_id,
                            &MessageId(message_id),
                            participant_id,
                            emoji,
                        ) {
                            log::error!("failed to add reaction: {}", e);
                        }
                    }
                    ChatAction::ThreadReplyTo { message_id } => {
                        ui_state_for_action.borrow_mut().thread_reply_to_id = Some(message_id);
                    }
                    ChatAction::ThreadMarkRead { thread_id: _ } => {
                        if let Err(e) = store.mark_thread_read(&thread_id) {
                            log::error!("failed to mark thread read: {}", e);
                        }
                    }
                    _ => {}
                }
                *dirty_for_action.borrow_mut() = true;
            })
            .with_on_send(move |text| {
                if selected_id_for_send.is_empty() {
                    log::warn!("no thread selected; cannot send message");
                    return;
                }
                let store = state_for_send.thread_store();
                let thread_id = ThreadId(selected_id_for_send.clone());
                let author = store
                    .get_profile()
                    .map(|p| Participant::User(UserId(p.id.0)))
                    .unwrap_or_else(|| Participant::User(UserId::generate()));
                let reply_to = ui_state_for_send.borrow().thread_reply_to_id.as_ref().map(|id| MessageId(id.clone()));
                let mentions = goble_desktop_service::thread_store::ThreadStore::extract_mentions(&text);
                if let Err(e) = store.post_message(
                    &thread_id,
                    author,
                    text,
                    reply_to,
                    vec![],
                    mentions,
                    None,
                ) {
                    log::error!("failed to post thread message: {}", e);
                } else {
                    ui_state_for_send.borrow_mut().thread_reply_to_id = None;
                }
                *dirty_for_send.borrow_mut() = true;
            })
            .with_on_new(move || {
                let store = state_for_new.thread_store();
                let owner = store
                    .get_profile()
                    .map(|p| goble_core::thread::UserId(p.id.0))
                    .unwrap_or_else(goble_core::thread::UserId::generate);
                let participant = goble_core::thread::Participant::User(owner.clone());
                match store.create_thread(
                    goble_core::thread::ThreadKind::Chat,
                    "New thread",
                    owner,
                    false,
                    vec![participant],
                    vec![],
                ) {
                    Ok(thread) => {
                        ui_state_for_new.borrow_mut().selected_thread_id = thread.id.0.clone();
                        log::info!("created new thread {}", thread.id.0);
                    }
                    Err(e) => log::error!("failed to create thread: {}", e),
                }
                *dirty_for_new.borrow_mut() = true;
            });

        let content = Container::new(container.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(EdgeInsets::uniform(padding))
            .finish();

        Self { content }
    }
}

impl Element for ThreadsViewPanel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.content.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.content.size()
    }

    fn origin(&self) -> Option<Point> {
        self.content.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.content.dispatch_event(event, ctx, app)
    }
}
