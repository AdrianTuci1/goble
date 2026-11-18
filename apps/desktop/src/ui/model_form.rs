//! Model-provider / API-endpoint form, rendered as a centered dialog with a
//! dimmed backdrop over the chat (first-run "configure a model key" flow).
//!
//! The editable fields live in app-owned `Rc<RefCell<...>>` values carried on
//! [`UiSnapshot`], so text and focus survive the per-frame element rebuild.

use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element, Fill,
    Flex, MainAxisAlignment, Select, SelectOption, Text, TextInput,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::{Dialog, DIALOG_DEFAULT_WIDTH};

use super::{LlmFormField, UiActions, UiSnapshot};

const PROVIDERS: [&str; 5] = ["openai", "anthropic", "ollama", "deepseek", "openrouter"];

/// Build the model-provider dialog. Always present in the tree; it only paints
/// and intercepts events when [`UiSnapshot::llm_dialog_open`] is set.
pub fn build_llm_dialog(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let on_close = actions.on_close_llm_dialog.clone();

    let title = Text::new("Connect a model provider")
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(16.0)
        .finish();

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_child(title)
        .with_child(form_field(app, "Provider", build_provider_select(state)))
        .with_child(form_field(app, "Model", build_model_input(app, state)))
        .with_child(form_field(app, "API key", build_api_key_input(app, state)))
        .with_child(form_field(
            app,
            "Base URL",
            build_base_url_input(app, state),
        ))
        .with_child(form_field(
            app,
            "Temperature",
            build_temperature_input(app, state),
        ))
        .with_child(build_actions(app, state, actions))
        .finish();

    let panel = Container::new(body)
        .with_padding(EdgeInsets::uniform(spacing))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_corner_radius(app.theme.radius_px())
        .finish();

    let on_dialog_close = on_close.clone();
    Dialog::new(panel)
        .with_open(state.llm_dialog_open)
        .with_width(DIALOG_DEFAULT_WIDTH)
        .with_on_close(move || (on_dialog_close.borrow_mut())())
        .finish()
}

/// A labeled, full-width input group.
fn form_field(app: &AppContext, label: &str, control: Box<dyn Element>) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(control)
        .finish()
}

fn build_provider_select(state: &UiSnapshot) -> Box<dyn Element> {
    let options = vec![
        SelectOption::new("OpenAI", "openai"),
        SelectOption::new("Anthropic", "anthropic"),
        SelectOption::new("Ollama", "ollama"),
        SelectOption::new("DeepSeek", "deepseek"),
        SelectOption::new("OpenRouter", "openrouter"),
    ];
    let current = state.llm_dialog_provider.borrow().clone();
    let selected = options.iter().position(|o| o.value == current);

    let provider_state = state.llm_dialog_provider.clone();
    let mut select = Select::new(options).with_on_change(move |idx| {
        if let Some(i) = idx {
            if let Some(value) = PROVIDERS.get(i) {
                *provider_state.borrow_mut() = value.to_string();
            }
        }
    });
    if let Some(idx) = selected {
        select = select.with_selected_index(idx);
    }
    select.finish()
}

fn build_model_input(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    build_text_input(
        app,
        state.llm_dialog_model.clone(),
        state.llm_dialog_focus.clone(),
        LlmFormField::Model,
        "e.g. gpt-4o",
    )
}

fn build_api_key_input(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    build_text_input(
        app,
        state.llm_dialog_api_key.clone(),
        state.llm_dialog_focus.clone(),
        LlmFormField::ApiKey,
        "API key",
    )
}

fn build_base_url_input(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    build_text_input(
        app,
        state.llm_dialog_base_url.clone(),
        state.llm_dialog_focus.clone(),
        LlmFormField::BaseUrl,
        "Optional base URL",
    )
}

fn build_temperature_input(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    build_text_input(
        app,
        state.llm_dialog_temperature.clone(),
        state.llm_dialog_focus.clone(),
        LlmFormField::Temperature,
        "e.g. 0.7",
    )
}

/// A text field whose value and focus live in app-owned ref cells, so both
/// survive the per-frame element rebuild while the dialog is open.
fn build_text_input(
    _app: &AppContext,
    value: std::rc::Rc<std::cell::RefCell<String>>,
    focus: std::rc::Rc<std::cell::RefCell<Option<LlmFormField>>>,
    field: LlmFormField,
    placeholder: &str,
) -> Box<dyn Element> {
    let focused = *focus.borrow() == Some(field);

    let on_change_value = value.clone();
    let on_focus = focus.clone();
    TextInput::new()
        .with_value(value.borrow().clone())
        .with_placeholder(placeholder)
        .with_focused(focused)
        .with_on_change(move |v| *on_change_value.borrow_mut() = v)
        .with_on_focus_change(move |is_focused: bool| {
            if is_focused {
                *on_focus.borrow_mut() = Some(field);
            } else if *on_focus.borrow() == Some(field) {
                *on_focus.borrow_mut() = None;
            }
        })
        .finish()
}

fn build_actions(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);

    let on_close = actions.on_close_llm_dialog.clone();
    let cancel = Button::new(Text::new("Cancel").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_close.borrow_mut())())
        .finish();

    let on_save = actions.on_save_llm.clone();
    let provider = state.llm_dialog_provider.clone();
    let model = state.llm_dialog_model.clone();
    let api_key = state.llm_dialog_api_key.clone();
    let base_url = state.llm_dialog_base_url.clone();
    let temperature = state.llm_dialog_temperature.clone();
    let save = Button::new(Text::new("Save").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || {
            (on_save.borrow_mut())(
                provider.borrow().clone(),
                model.borrow().clone(),
                api_key.borrow().clone(),
                base_url.borrow().clone(),
                temperature.borrow().clone(),
            );
        })
        .finish();

    Flex::row()
        .with_main_axis_alignment(MainAxisAlignment::End)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(cancel)
        .with_child(save)
        .finish()
}
