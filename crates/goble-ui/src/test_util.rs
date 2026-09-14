use crate::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use crate::geometry::{vec2f, RectF, Vector2F};
use crate::render::{RenderCommand, Renderer};

/// Lays out and paints `element` into a headless command list.
///
/// This lets tests verify that a component produces the expected render
/// commands without opening a window or initializing wgpu.
pub fn render_element(
    element: &mut Box<dyn Element>,
    size: Vector2F,
    app: &AppContext,
) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(size);
    let mut layout_ctx = LayoutContext::default();
    let _ = element.layout(constraint, &mut layout_ctx, app);

    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    element.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// Counts how many commands of each variant the element emitted.
pub fn command_counts(commands: &[RenderCommand]) -> RenderCommandCounts {
    let mut counts = RenderCommandCounts::default();
    for command in commands {
        match command {
            RenderCommand::FillRect { .. } => counts.fill_rect += 1,
            RenderCommand::StrokeRect { .. } => counts.stroke_rect += 1,
            RenderCommand::DrawText { .. } => counts.draw_text += 1,
            RenderCommand::DrawIcon { .. } => counts.draw_icon += 1,
            RenderCommand::ClipRect { .. } => counts.clip_rect += 1,
            RenderCommand::PopClip => counts.pop_clip += 1,
            RenderCommand::FillRectFadeRight { .. } => counts.fill_rect_fade += 1,
            RenderCommand::DrawImage { .. } => counts.draw_image += 1,
        }
    }
    counts
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct RenderCommandCounts {
    pub fill_rect: usize,
    pub fill_rect_fade: usize,
    pub stroke_rect: usize,
    pub draw_text: usize,
    pub draw_icon: usize,
    pub clip_rect: usize,
    pub pop_clip: usize,
    pub draw_image: usize,
}

/// A bordered control: a border and a rounded fill drawn over the same rect.
/// `Container`'s card (the shared terminal command block) and `Button` both
/// paint that pair, and the theme's radius is what makes it a control's corner
/// rather than a row's wrapped box.
///
/// The largest such rect is returned, so a card that holds controls of its own
/// (the command block's copy and filter buttons) is recognized as the card and
/// the controls inside it lie within it.
pub fn bordered_control(commands: &[RenderCommand], app: &AppContext) -> Option<RectF> {
    let radius = app.theme.radius_px();
    if radius <= 0.0 {
        return None;
    }
    let pairs = commands.iter().filter_map(|command| match command {
        RenderCommand::StrokeRect { rect, .. } => commands.iter().find_map(|other| match other {
            RenderCommand::FillRect {
                rect: fill,
                corner_radius,
                ..
            } if fill == rect && *corner_radius == radius => Some(*rect),
            _ => None,
        }),
        _ => None,
    });
    pairs.max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
}

/// Whether `command` is a pill: a rounded box wrapping a row, or a border with
/// no control under it. Two shapes are not pills:
///
/// - a bordered control (a border and a fill over the same rect) — the shared
///   terminal command block's card, or a button;
/// - a text highlight: a rounded fill no taller than the line of text it backs
///   (an inline code span in prose), which is not chrome around a row.
///
/// `commands` is the whole row's list, because a border and its fill are only
/// recognizable together.
pub fn is_pill(command: &RenderCommand, commands: &[RenderCommand]) -> bool {
    let has_border = |rect: &RectF| {
        commands.iter().any(
            |other| matches!(other, RenderCommand::StrokeRect { rect: border, .. } if border == rect),
        )
    };
    let has_rounded_fill = |rect: &RectF| {
        commands.iter().any(|other| {
            matches!(other, RenderCommand::FillRect { rect: fill, corner_radius, .. }
                if fill == rect && *corner_radius > 0.0)
        })
    };
    let is_text_highlight = |rect: &RectF| {
        commands.iter().any(|other| match other {
            RenderCommand::DrawText {
                origin, font_size, ..
            } => {
                origin.x >= rect.min_x()
                    && origin.x <= rect.max_x()
                    && origin.y >= rect.min_y()
                    && origin.y <= rect.max_y()
                    && rect.height() <= font_size * 1.8
            }
            _ => false,
        })
    };
    match command {
        RenderCommand::FillRect {
            rect,
            corner_radius,
            ..
        }
        | RenderCommand::FillRectFadeRight {
            rect,
            corner_radius,
            ..
        } => *corner_radius > 0.0 && !has_border(rect) && !is_text_highlight(rect),
        RenderCommand::StrokeRect { rect, .. } => !has_rounded_fill(rect),
        _ => false,
    }
}

/// Every pill `commands` paint, described for a failure message.
pub fn pills(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter(|command| is_pill(command, commands))
        .map(|command| match command {
            RenderCommand::FillRect {
                rect,
                corner_radius,
                ..
            }
            | RenderCommand::FillRectFadeRight {
                rect,
                corner_radius,
                ..
            } => format!("rounded fill {rect:?} radius {corner_radius}"),
            RenderCommand::StrokeRect { rect, width, .. } => {
                format!("border {rect:?} width {width}")
            }
            _ => String::new(),
        })
        .collect()
}

/// The rect of every pill `commands` paint, so a host can assert that no pill
/// covers one of its own rows.
pub fn pill_rects(commands: &[RenderCommand]) -> Vec<RectF> {
    commands
        .iter()
        .filter(|command| is_pill(command, commands))
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, .. }
            | RenderCommand::FillRectFadeRight { rect, .. }
            | RenderCommand::StrokeRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect()
}

/// Whether `point` lies inside `rect`.
pub fn rect_contains(rect: RectF, point: Vector2F) -> bool {
    point.x >= rect.min_x()
        && point.x <= rect.max_x()
        && point.y >= rect.min_y()
        && point.y <= rect.max_y()
}

/// Assert the agent row `what` is pill-free: no rounded box wrapping it and no
/// unexplained border. Panics with the offending commands, so a regression
/// names the box that appeared.
pub fn assert_pill_free(commands: &[RenderCommand], what: &str) {
    let pills = pills(commands);
    assert!(
        pills.is_empty(),
        "{what} draws no pill, got {}",
        pills.join(", ")
    );
}

/// Assert the agent row `what` draws no border of its own. A border that is a
/// control's own outline — a rounded fill under it, over the same rect (the
/// shared terminal command block's card, a button) — is not the row's; every
/// other border is, and fails. The rows that must draw no border whatsoever
/// are locked one by one as well (a tool call, a pager card, the footer).
pub fn assert_no_row_border(commands: &[RenderCommand], what: &str) {
    let borders: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect { rect, .. } => {
                let backed = commands.iter().any(|other| {
                    matches!(other, RenderCommand::FillRect { rect: fill, corner_radius, .. }
                        if fill == rect && *corner_radius > 0.0)
                });
                (!backed).then(|| format!("border {rect:?}"))
            }
            _ => None,
        })
        .collect();
    assert!(
        borders.is_empty(),
        "{what} draws no border, got {}",
        borders.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::chat_content::{ChatFragment, ChatMessage, ChatRole};
    use crate::elements::{
        AgentCard, AppContext, Avatar, Button, ButtonVariant, Checkbox, ConnectorCard, Container,
        Expanded, Fill, Flex, Icon, LayoutContext, MainAxisSize, Rect, ShellState, ShellView,
        SizeConstraint, Switch, Text, TitleBar,
    };
    use crate::geometry::vec2f;
    use crate::views::settings_view::SettingsPage;
    use crate::{ChatView, SettingsView, ThreadKind, ThreadListEntry, ThreadsContainer};

    fn app() -> AppContext {
        AppContext::default()
    }

    #[test]
    fn render_element_emits_commands() {
        let app = app();
        let mut element = Container::new(Text::new("hello").finish())
            .with_background(Fill::Solid(crate::color::ColorU::new(255, 0, 0, 255)))
            .finish();
        let commands = render_element(&mut element, vec2f(200.0, 200.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "container should emit a fill rect");
        assert!(counts.draw_text > 0, "text should emit a draw text command");
    }

    #[test]
    fn button_renders_and_handles_click() {
        let app = app();
        let clicked = std::rc::Rc::new(std::cell::RefCell::new(false));
        let clicked_clone = clicked.clone();
        let mut element = Button::new(Text::new("Click me").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || *clicked_clone.borrow_mut() = true)
            .finish();

        let commands = render_element(&mut element, vec2f(200.0, 60.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "button should render a background");
        assert!(counts.draw_text > 0, "button should render a label");

        element.layout(
            SizeConstraint::loose(vec2f(200.0, 60.0)),
            &mut LayoutContext::default(),
            &app,
        );
        element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        element.dispatch_event(
            &crate::event::DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        element.dispatch_event(
            &crate::event::DispatchedEvent::MouseUp {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert!(*clicked.borrow(), "button click callback should fire");
    }

    #[test]
    fn agent_card_renders() {
        let app = app();
        let mut element = AgentCard::new(
            Avatar::new("A").finish(),
            "Coder",
            "Code agent.",
            ["rust"],
            &app,
        )
        .finish();
        let commands = render_element(&mut element, vec2f(400.0, 200.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.fill_rect > 0,
            "agent card should render a background"
        );
        assert!(counts.draw_text > 0, "agent card should render text");
    }

    #[test]
    fn connector_card_renders() {
        let app = app();
        let mut element = ConnectorCard::new(
            Icon::new("plug").finish(),
            "GitHub",
            "Connector.",
            ["issue"],
            None,
            &app,
        )
        .finish();
        let commands = render_element(&mut element, vec2f(400.0, 200.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.fill_rect > 0,
            "connector card should render a background"
        );
        assert!(counts.draw_text > 0, "connector card should render text");
    }

    #[test]
    fn chat_view_renders() {
        let app = app();
        let messages = vec![
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Hello")]),
            ChatMessage::from_markdown(ChatRole::Assistant, "Hi **there**"),
        ];
        let mut element = ChatView::new()
            .with_messages(messages)
            .with_on_send(|_| ())
            .finish();
        let commands = render_element(&mut element, vec2f(600.0, 800.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "chat view should render backgrounds");
        assert!(counts.draw_text > 0, "chat view should render text");
    }

    #[test]
    fn threads_container_renders() {
        let app = app();
        let threads = vec![ThreadListEntry {
            id: "t1".to_string(),
            title: "General".to_string(),
            kind: ThreadKind::Channel,
            selected: true,
            unread_count: 0,
        }];
        let messages = vec![ChatMessage::from_markdown(ChatRole::User, "msg")];
        let mut element = ThreadsContainer::new("t1")
            .with_threads(threads)
            .with_messages("t1", messages)
            .finish();
        let commands = render_element(&mut element, vec2f(800.0, 600.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.fill_rect > 0,
            "threads container should render backgrounds"
        );
        assert!(counts.draw_text > 0, "threads container should render text");
    }

    #[test]
    fn settings_view_renders() {
        let app = app();
        let mut element = SettingsView::new(SettingsPage::Profile)
            .with_profile("Ada", "ada@example.com")
            .with_llm("openai", "gpt-4o", "", "", "0.7")
            .with_dark_mode(true)
            .finish();
        let commands = render_element(&mut element, vec2f(800.0, 600.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.fill_rect > 0,
            "settings view should render backgrounds"
        );
        assert!(counts.draw_text > 0, "settings view should render text");
    }

    #[test]
    fn shell_view_renders() {
        let app = app();
        let mut element = ShellView::new(ShellState::default(), &app).finish();
        let commands = render_element(&mut element, vec2f(1024.0, 768.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "shell view should render backgrounds");
        assert!(counts.draw_text > 0, "shell view should render text");
    }

    #[test]
    fn title_bar_renders() {
        let app = app();
        let tabs: Vec<(String, bool, Box<dyn FnMut()>)> = vec![
            ("Chat".to_string(), true, Box::new(|| {})),
            ("Settings".to_string(), false, Box::new(|| {})),
        ];
        let mut element = TitleBar::new("Goble", tabs, vec![], &app).finish();
        let commands = render_element(&mut element, vec2f(1024.0, 48.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.fill_rect >= 3,
            "title bar should render traffic lights and background"
        );
        assert!(
            counts.draw_text > 0,
            "title bar should render title and tabs"
        );
    }

    #[test]
    fn switch_renders() {
        let app = app();
        let mut element = Switch::new().with_checked(true).finish();
        let commands = render_element(&mut element, vec2f(100.0, 40.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "switch should render a track");
    }

    #[test]
    fn checkbox_renders() {
        let app = app();
        let mut element = Checkbox::new().with_checked(true).finish();
        let commands = render_element(&mut element, vec2f(100.0, 40.0), &app);
        let counts = command_counts(&commands);
        assert!(counts.fill_rect > 0, "checkbox should render a box");
    }

    #[test]
    fn expanded_fills_remaining_flex_space() {
        let app = app();
        let mut element = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(10.0)
            .with_child(Text::new("header").finish())
            .with_child(
                Expanded::new(
                    Container::new(Rect::new().finish())
                        .with_background(Fill::Solid(crate::color::ColorU::new(0, 0, 255, 255)))
                        .finish(),
                )
                .finish(),
            )
            .finish();
        let commands = render_element(&mut element, vec2f(400.0, 600.0), &app);
        assert_eq!(
            element.size(),
            Some(vec2f(400.0, 600.0)),
            "flex should fill the viewport"
        );

        let mut expanded_rect = None;
        for command in &commands {
            if let RenderCommand::FillRect { rect, color, .. } = command {
                if color.b == 255 && color.r == 0 && color.g == 0 {
                    expanded_rect = Some(*rect);
                }
            }
        }
        let rect = expanded_rect.expect("expanded child should paint a background");
        assert!(
            rect.height() > 500.0 && rect.height() < 590.0,
            "expanded rect should consume the remaining space, got height {}",
            rect.height()
        );
        assert_eq!(
            rect.width(),
            400.0,
            "expanded child should fill the cross axis"
        );
    }
}
