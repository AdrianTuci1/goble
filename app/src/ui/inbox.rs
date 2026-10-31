//! The global Inbox overlay: approvals, questions and terminal reports from
//! every pane, newest first.
//!
//! It is an overlay rather than a page: the workspace stays mounted underneath,
//! and the panel owns the keyboard while it is up. `j`/`k` walk the rows,
//! `Enter` jumps to the row's pane, `a` acknowledges the row, `A` acknowledges
//! everything and `Esc` closes the panel. Rows the user has acknowledged stay
//! in the list, drawn muted, until the pane they belong to closes.

use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Fill,
    Flex, HoverRow, Icon, KeyHandler, MainAxisSize, Scrollable, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::UiActions;
use super::snapshot::UiSnapshot;
use crate::state::{InboxItem, InboxKind};

/// The panel's width: a little narrower than the shortcuts sheet, so it reads
/// as a list rather than a document.
pub const INBOX_PANEL_WIDTH: f32 = 520.0;

/// The overlay's content. [`super::build`] wraps it in a [`goble_ui::Dialog`]
/// so the backdrop and the inset margin match the workflow-runs overlay.
pub fn build_inbox_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close = actions.on_close_inbox.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(0.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Inbox")
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(if state.inbox_unread_count == 0 {
                "all read".to_string()
            } else {
                format!("{} unread", state.inbox_unread_count)
            })
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_child(close_button)
        .finish();

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    if let Some(error) = &state.inbox_error {
        column = column.with_child(
            Text::new(error.clone())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }

    let body: Box<dyn Element> = if state.inbox_items.is_empty() {
        // Nothing was reported: say so instead of drawing an empty list.
        Container::new(
            Text::new("Your inbox is empty. Approvals, questions and turn reports appear here.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
    } else {
        // The rows size to their content: inside the scroll viewport the
        // available height is unbounded, so `Max` would report an infinite
        // content extent and there would be no scroll range to clamp against.
        let mut rows = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (index, item) in state.inbox_items.iter().enumerate() {
            rows = rows.with_child(item_row(app, item, index == state.inbox_index, sm));
        }
        // `Expanded` gives the list the height left after the header and the
        // footer, so it scrolls inside the panel instead of pushing the footer
        // below the window.
        Expanded::new(
            Scrollable::new(rows.finish(), Axis::Vertical)
                .with_state(Rc::clone(&state.inbox_scroll))
                .finish(),
        )
        .finish()
    };
    column = column.with_child(body);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(
        Text::new("j/k move · Enter jump · a ack · A ack all · Esc close")
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );

    let on_escape = actions.on_close_inbox.clone();
    let on_move = actions.on_inbox_move.clone();
    let on_ack = actions.on_inbox_ack.clone();
    let on_ack_all = actions.on_inbox_ack_all.clone();
    let on_jump = actions.on_inbox_jump.clone();
    KeyHandler::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
        move |key: &str, modifiers| {
            // Global chords are handled before the tree; a modified key is not
            // the inbox's to consume.
            if modifiers.ctrl || modifiers.command || modifiers.alt {
                return false;
            }
            match key {
                "Escape" => {
                    (on_escape.borrow_mut())();
                    true
                }
                "j" | "ArrowDown" => {
                    (on_move.borrow_mut())(1);
                    true
                }
                "k" | "ArrowUp" => {
                    (on_move.borrow_mut())(-1);
                    true
                }
                "a" => {
                    (on_ack.borrow_mut())();
                    true
                }
                "A" => {
                    (on_ack_all.borrow_mut())();
                    true
                }
                "Enter" => {
                    (on_jump.borrow_mut())();
                    true
                }
                // Any other printable key belongs to the panel while it is up,
                // so it cannot leak into the composer under the overlay.
                _ => key
                    .chars()
                    .next()
                    .map(|ch| !ch.is_control() && key.chars().count() == 1)
                    .unwrap_or(false),
            }
        },
    )
    .finish()
}

/// One inbox row: its kind, title, detail and read state.
fn item_row(
    app: &AppContext,
    item: &InboxItem,
    selected: bool,
    spacing: f32,
) -> Box<dyn Element> {
    let title_color = if item.acknowledged {
        ColorToken::Muted
    } else {
        ColorToken::Text
    };
    let line = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(
            Text::new(item.kind.label())
                .with_font_size(9.0)
                .with_theme_color(kind_color(item.kind), app)
                .finish(),
        )
        .with_child(
            Text::new(item.title.clone())
                .with_font_size(12.0)
                .with_theme_color(title_color, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(if item.acknowledged { "acknowledged" } else { "" })
                .with_font_size(9.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    let detail = Text::new(item.detail.clone())
        .with_font_size(10.0)
        .with_theme_color(ColorToken::Muted, app)
        .with_max_lines(1)
        .finish();
    HoverRow::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(line)
            .with_child(detail)
            .finish(),
    )
    .with_selected(selected)
    .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
    .with_corner_radius(4.0)
    .finish()
}

fn kind_color(kind: InboxKind) -> ColorToken {
    match kind {
        InboxKind::Approval => ColorToken::Warning,
        InboxKind::Question => ColorToken::Accent,
        InboxKind::Done => ColorToken::Success,
        InboxKind::Error => ColorToken::Error,
        InboxKind::Agent => ColorToken::Muted,
        InboxKind::Task => ColorToken::Success,
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{
        AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
    };
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{vec2f, Vector2F};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::theme::ColorToken;

    use crate::root_view::RootView;
    use crate::state::{InboxKind, UiState};
    use super::INBOX_PANEL_WIDTH;
    use crate::ui::shell::TOPBAR_HEIGHT;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
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
        }
        (Box::new(view), state, dir)
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

    fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(text.clone())
                }
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
        let _ = root.layout(
            SizeConstraint::loose(window()),
            &mut LayoutContext,
            app,
        );
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

    #[test]
    fn the_overlay_lists_items_newest_first_with_the_live_unread_count() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(1, "c1", InboxKind::Approval, "Approve the command", "git status");
            s.raise_inbox(1, "c1", InboxKind::Question, "Which branch?", "main or release");
            s.inbox_open = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts = drawn_texts(&commands);
        assert!(texts.iter().any(|text| text == "Inbox"));
        assert!(
            texts.iter().any(|text| text == "2 unread"),
            "the header carries the unread count: {texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Which branch?"));
        assert!(texts.iter().any(|text| text == "Approve the command"));
        assert_eq!(
            state.borrow().inbox[0].kind,
            InboxKind::Question,
            "the newest item is drawn first"
        );
        assert!(
            topbar_texts(&commands).iter().any(|text| text == "2"),
            "the topbar control shows the same unread count: {:?}",
            topbar_texts(&commands)
        );
    }

    #[test]
    fn a_populated_overlay_keeps_the_footer_and_list_inside_the_viewport() {
        let app = AppContext::default();
        for (width, height) in [(768.0_f32, 600.0_f32), (600.0, 400.0)] {
            let (mut root, state, _dir) = root();
            {
                let mut s = state.borrow_mut();
                // Enough rows that the list has to scroll in either window.
                for index in 0..30 {
                    s.raise_inbox(
                        1,
                        format!("c{index}"),
                        InboxKind::Approval,
                        format!("Approval {index}"),
                        format!("command {index}"),
                    );
                }
                s.inbox_open = true;
            }
            let commands = render_element(&mut root, vec2f(width, height), &app);
            let (footer_y, footer_height) = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText {
                        text,
                        origin,
                        line_height,
                        ..
                    } if text.starts_with("j/k move") => Some((origin.y, *line_height)),
                    _ => None,
                })
                .expect("the footer is drawn");
            assert!(
                footer_y + footer_height <= height,
                "the footer stays in a {width}x{height} window: its bottom is {}",
                footer_y + footer_height
            );

            let panel = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. }
                        if *color == app.theme.color(ColorToken::Surface)
                            && (rect.width() - INBOX_PANEL_WIDTH).abs() < 0.5 =>
                    {
                        Some(*rect)
                    }
                    _ => None,
                })
                .expect("the panel background is drawn");
            assert!(
                panel.max_y() <= height,
                "the panel is constrained to the viewport: {} > {height}",
                panel.max_y()
            );
            assert!(
                footer_y >= panel.min_y()
                    && footer_y + footer_height <= panel.max_y(),
                "the footer sits inside the panel, not below it"
            );

            let viewport = state.borrow().inbox_scroll.borrow().viewport();
            assert!(viewport > 0.0, "the list has a viewport");
            assert!(
                viewport <= panel.height() - 60.0,
                "the list viewport stops above the footer instead of using the \
                 whole dialog height: viewport {viewport}, panel {}",
                panel.height()
            );
        }
    }

    #[test]
    fn the_overlay_shows_an_honest_empty_state() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().inbox_open = true;
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text.contains("Your inbox is empty")),
            "an empty inbox says so: {texts:?}"
        );
    }

    #[test]
    fn the_overlay_shows_the_reason_a_jump_did_not_land() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.inbox_error = Some("Pane 9 is no longer open; the item has nothing to jump to.".to_string());
            s.inbox_open = true;
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text.contains("Pane 9 is no longer open")),
            "the error is drawn, not swallowed: {texts:?}"
        );
    }

    #[test]
    fn the_overlays_keys_move_ack_jump_and_close() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(1, "c1", InboxKind::Approval, "Approve the command", "git status");
            s.raise_inbox(1, "c1", InboxKind::Question, "Which branch?", "main or release");
            s.inbox_open = true;
        }
        assert!(press(&mut root, &app, "j", ModifiersState::none()));
        assert_eq!(state.borrow().inbox_index, 1, "j walks down");
        assert!(press(&mut root, &app, "k", ModifiersState::none()));
        assert_eq!(state.borrow().inbox_index, 0, "k walks up");
        assert!(press(&mut root, &app, "a", ModifiersState::none()));
        assert_eq!(state.borrow().unread_inbox_count(), 1, "a acks the selected row");
        assert!(press(&mut root, &app, "A", ModifiersState::none()));
        assert_eq!(state.borrow().unread_inbox_count(), 0, "A acks all");
        assert!(press(&mut root, &app, "Enter", ModifiersState::none()));
        assert!(!state.borrow().inbox_open, "Enter jumps and puts the panel away");
        // Reopen and close with Escape.
        state.borrow_mut().inbox_open = true;
        assert!(press(&mut root, &app, "Escape", ModifiersState::none()));
        assert!(!state.borrow().inbox_open);
    }

    #[test]
    fn cmd_shift_i_toggles_the_inbox() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let chord = ModifiersState {
            command: true,
            shift: true,
            ..Default::default()
        };
        assert!(press(&mut root, &app, "i", chord));
        assert!(state.borrow().inbox_open, "Cmd+Shift+I opens the inbox");
        assert!(press(&mut root, &app, "i", chord));
        assert!(!state.borrow().inbox_open, "the chord toggles it shut");
    }
}
