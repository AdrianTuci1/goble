use super::*;
use crate::elements::{AppContext, EventContext, LayoutContext, PaintContext, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::vec2f;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn settings_view_layouts() {
    let app = AppContext::default();
    let mut view = SettingsView::new(SettingsPage::Profile)
        .with_profile("Ada", "ada@example.com")
        .with_llm("openai", "gpt-4o", "", "", "");
    let size = view.layout(
        SizeConstraint::loose(vec2f(800.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
}

#[test]
fn settings_view_renders_back_button_when_callback_set() {
    let app = AppContext::default();
    let mut element: Box<dyn Element> = SettingsView::new(SettingsPage::Profile)
        .with_on_back(|| {})
        .finish();
    let commands = crate::test_util::render_element(&mut element, vec2f(800.0, 600.0), &app);
    let has_back = commands.iter().any(
        |c| matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text == "Back"),
    );
    let has_chevron = commands.iter().any(|c| {
        matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "chevron-left")
    });
    assert!(
        has_back,
        "settings with a back callback should render a Back button"
    );
    assert!(
        has_chevron,
        "settings back button should render a chevron icon"
    );
}

#[test]
fn settings_view_back_callback_fires() {
    let clicked = Rc::new(RefCell::new(false));
    let clicked_clone = clicked.clone();
    let app = AppContext::default();
    let mut element: Box<dyn Element> = SettingsView::new(SettingsPage::Profile)
        .with_on_back(move || *clicked_clone.borrow_mut() = true)
        .finish();
    element.layout(
        SizeConstraint::loose(vec2f(800.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );
    element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut event_ctx = EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: vec2f(40.0, 40.0),
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position: vec2f(40.0, 40.0),
        button: 0,
    };
    let _ = element.dispatch_event(&down, &mut event_ctx, &app);
    let _ = element.dispatch_event(&up, &mut event_ctx, &app);
    assert!(*clicked.borrow());
}

#[test]
fn settings_row_renders_description_and_tooltip_icon() {
    let app = AppContext::default();
    let mut element: Box<dyn Element> = SettingsView::new(SettingsPage::Appearance)
        .with_dark_mode(true)
        .finish();
    let commands = crate::test_util::render_element(&mut element, vec2f(800.0, 600.0), &app);
    let has_description = commands.iter().any(|c| {
        matches!(c, crate::render::RenderCommand::DrawText { text, .. }
            if text == "Use a dark color theme for the app.")
    });
    let has_info_icon = commands.iter().any(
        |c| matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "info"),
    );
    assert!(
        has_description,
        "a settings row with a description should render that text"
    );
    assert!(
        has_info_icon,
        "a settings row with a tooltip should render an info icon"
    );
}

#[test]
fn settings_row_renders_local_only_icon() {
    let app = AppContext::default();
    let mut element: Box<dyn Element> = SettingsView::new(SettingsPage::Llm)
        .with_llm("openai", "gpt-4o", "", "", "")
        .finish();
    let commands = crate::test_util::render_element(&mut element, vec2f(800.0, 600.0), &app);
    let has_local_only = commands.iter().any(|c| {
        matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "cloud-off")
    });
    assert!(
        has_local_only,
        "the API key row is local-only and should render a cloud-off icon"
    );
}
