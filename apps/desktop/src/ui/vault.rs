//! Vault panel: unlock with passphrase, list secrets, add/delete.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Fill, Flex, Icon, Label, LabelSize, MainAxisSize, Scrollable, Spacer, Text, TextInput,
    TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{AiActions, AiSnapshot};

/// Right-anchored vault sheet: unlock form when locked, secret list + add
/// form when unlocked.
pub fn build_vault_sheet(
    app: &AppContext,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close = ai_actions.on_close_vault.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Vault")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish();

    let body = if ai.vault_unlocked {
        build_unlocked(app, ai, ai_actions)
    } else {
        build_locked(app, ai, ai_actions)
    };

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(body);

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Locked state: passphrase input + unlock button.
fn build_locked(app: &AppContext, ai: &AiSnapshot, ai_actions: &AiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);

    let on_draft = ai_actions.on_vault_unlock_draft_change.clone();
    let passphrase = TextInput::new()
        .with_value(ai.vault_unlock_draft.clone())
        .with_placeholder("Vault passphrase")
        .with_on_change(move |value| (on_draft.borrow_mut())(value))
        .finish();

    let on_unlock = ai_actions.on_vault_unlock.clone();
    let unlock_button = Button::new(Text::new("Unlock").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_unlock.borrow_mut())())
        .finish();

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    col = col.with_child(
        Text::new("Secrets are stored encrypted. Enter your passphrase to unlock the vault.")
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    col = col.with_child(passphrase);
    col = col.with_child(unlock_button);
    if let Some(error) = &ai.vault_error {
        col = col.with_child(
            Text::new(error.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }

    Container::new(col.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Unlocked state: secret rows with delete, plus a new-secret form.
fn build_unlocked(app: &AppContext, ai: &AiSnapshot, ai_actions: &AiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if ai.vault_secrets.is_empty() {
        list = list.with_child(
            Text::new("No secrets yet. Add the first one below.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for secret in &ai.vault_secrets {
        list = list.with_child(build_secret_row(app, secret, ai_actions));
    }

    // New secret form
    let on_key = ai_actions.on_vault_new_key_change.clone();
    let key_input = TextInput::new()
        .with_value(ai.vault_new_key.clone())
        .with_placeholder("Secret key, e.g. openai_api_key")
        .with_on_change(move |value| (on_key.borrow_mut())(value))
        .finish();
    let on_value = ai_actions.on_vault_new_value_change.clone();
    let value_input = TextInput::new()
        .with_value(ai.vault_new_value.clone())
        .with_placeholder("Secret value")
        .with_on_change(move |value| (on_value.borrow_mut())(value))
        .finish();
    let on_add = ai_actions.on_vault_secret_add.clone();
    let add_button = Button::new(Text::new("Add secret").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_add.borrow_mut())())
        .finish();

    let form = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(key_input)
        .with_child(value_input)
        .with_child(add_button)
        .finish();

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    col = col.with_child(
        Label::new("Secrets")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    col = col.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());
    col = col.with_child(Divider::horizontal().finish());
    col = col.with_child(
        Label::new("New secret")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    col = col.with_child(form);
    if let Some(error) = &ai.vault_error {
        col = col.with_child(
            Text::new(error.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }
    col.finish()
}

/// One secret row: key + updated timestamp + delete button.
fn build_secret_row(
    app: &AppContext,
    secret: &super::VaultSecretEntry,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let radius = app.theme.radius_px();

    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(secret.key.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("updated {}", secret.updated_at))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();

    let on_delete = ai_actions.on_vault_secret_delete.clone();
    let key = secret.key.clone();
    let delete_button = TopbarButton::new(
        Icon::new("trash")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_delete.borrow_mut())(key.clone()))
    .finish();

    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(delete_button)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(radius)
    .with_padding(EdgeInsets::uniform(spacing))
    .finish()
}
