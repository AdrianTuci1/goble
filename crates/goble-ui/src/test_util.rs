use crate::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
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
    paint_ctx.renderer.take().map(|r| r.commands().to_vec()).unwrap_or_default()
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
        }
    }
    counts
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct RenderCommandCounts {
    pub fill_rect: usize,
    pub stroke_rect: usize,
    pub draw_text: usize,
    pub draw_icon: usize,
    pub clip_rect: usize,
    pub pop_clip: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::chat_content::{ChatFragment, ChatMessage, ChatRole};
    use crate::elements::{
        AgentCard, AppContext, Avatar, Button, ButtonVariant, Checkbox, ConnectorCard, Container,
        Fill, Icon, LayoutContext, ShellState, ShellView, SizeConstraint, Switch, Text, TitleBar,
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

        element.layout(SizeConstraint::loose(vec2f(200.0, 60.0)), &mut LayoutContext::default(), &app);
        element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        element.dispatch_event(
            &crate::event::DispatchedEvent::MouseDown { position: vec2f(10.0, 10.0), button: 0 },
            &mut event_ctx,
            &app,
        );
        element.dispatch_event(
            &crate::event::DispatchedEvent::MouseUp { position: vec2f(10.0, 10.0), button: 0 },
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
        assert!(counts.fill_rect > 0, "agent card should render a background");
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
        assert!(counts.fill_rect > 0, "connector card should render a background");
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
        assert!(counts.fill_rect > 0, "threads container should render backgrounds");
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
        assert!(counts.fill_rect > 0, "settings view should render backgrounds");
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
        assert!(counts.fill_rect >= 3, "title bar should render traffic lights and background");
        assert!(counts.draw_text > 0, "title bar should render title and tabs");
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
}
