use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Label,
    LabelSize, MainAxisAlignment, Select, SelectOption, Switch, Text, TextInput,
};
use crate::theme::{ColorToken, SpacingToken};

use super::row::SettingsRow;
use super::{SettingsPage, SettingsView};

fn section(
    title: impl Into<String>,
    children: Vec<Box<dyn Element>>,
    app: &AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(Label::new(title.into()).with_size(LabelSize::Sm).finish());
    for child in children {
        column = column.with_child(child);
    }
    Container::new(column.finish())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

impl SettingsView {
    pub(super) fn build_pane(&self, app: &AppContext) -> Box<dyn Element> {
        match self.current_page {
            SettingsPage::Profile => self.build_profile_page(app),
            SettingsPage::Llm => self.build_llm_page(app),
            SettingsPage::Appearance => self.build_appearance_page(app),
            SettingsPage::Account => self.build_account_page(app),
            SettingsPage::Cluster => self.build_cluster_page(app),
            SettingsPage::Workers => self.build_workers_page(app),
            SettingsPage::Keys => self.build_keys_page(app),
        }
    }

    fn build_profile_page(&self, app: &AppContext) -> Box<dyn Element> {
        let name_state = Rc::new(RefCell::new(self.profile_name.clone()));
        let email_state = Rc::new(RefCell::new(self.profile_email.clone()));

        let name_state_for_change = Rc::clone(&name_state);
        let name_input = TextInput::new()
            .with_value(self.profile_name.clone())
            .with_on_change(move |v| {
                *name_state_for_change.borrow_mut() = v;
            })
            .finish();
        let email_state_for_change = Rc::clone(&email_state);
        let email_input = TextInput::new()
            .with_value(self.profile_email.clone())
            .with_on_change(move |v| {
                *email_state_for_change.borrow_mut() = v;
            })
            .finish();

        let on_save = self.on_save_profile.clone();
        let save = Button::new(Text::new("Save").finish())
            .with_on_click(move || {
                if let Some(cb) = on_save.as_ref() {
                    let name = name_state.borrow().clone();
                    let email = email_state.borrow().clone();
                    (cb.borrow_mut())(name, email);
                }
            })
            .finish();

        section(
            "Profile",
            vec![
                SettingsRow::new(app, "Name", name_input)
                    .with_description("Shown to your agent and to peers on the cluster.")
                    .build(),
                SettingsRow::new(app, "Email", email_input)
                    .with_description("Used to identify your account.")
                    .build(),
                save,
            ],
            app,
        )
    }

    fn build_llm_page(&self, app: &AppContext) -> Box<dyn Element> {
        let provider_options = vec![
            SelectOption::new("OpenAI", "openai"),
            SelectOption::new("Anthropic", "anthropic"),
            SelectOption::new("Ollama", "ollama"),
            SelectOption::new("DeepSeek", "deepseek"),
            SelectOption::new("OpenRouter", "openrouter"),
        ];
        let selected = provider_options
            .iter()
            .position(|o| o.value == self.llm_provider);

        let provider_state = Rc::new(RefCell::new(self.llm_provider.clone()));
        let model_state = Rc::new(RefCell::new(self.llm_model.clone()));
        let api_key_state = Rc::new(RefCell::new(self.llm_api_key.clone()));
        let base_url_state = Rc::new(RefCell::new(self.llm_base_url.clone()));
        let temperature_state = Rc::new(RefCell::new(self.llm_temperature.clone()));

        let provider_state_for_change = Rc::clone(&provider_state);
        let mut provider_select = Select::new(provider_options).with_on_change(move |idx| {
            if let Some(i) = idx {
                let options = ["openai", "anthropic", "ollama", "deepseek", "openrouter"];
                if let Some(value) = options.get(i) {
                    *provider_state_for_change.borrow_mut() = value.to_string();
                }
            }
        });
        if let Some(idx) = selected {
            provider_select = provider_select.with_selected_index(idx);
        }
        let provider_select = provider_select.finish();

        let model_state_for_change = Rc::clone(&model_state);
        let model_input = TextInput::new()
            .with_value(self.llm_model.clone())
            .with_placeholder("e.g. gpt-4o")
            .with_on_change(move |v| {
                *model_state_for_change.borrow_mut() = v;
            })
            .finish();

        let api_key_state_for_change = Rc::clone(&api_key_state);
        let api_key_input = TextInput::new()
            .with_value(self.llm_api_key.clone())
            .with_placeholder("API key")
            .with_on_change(move |v| {
                *api_key_state_for_change.borrow_mut() = v;
            })
            .finish();

        let base_url_state_for_change = Rc::clone(&base_url_state);
        let base_url_input = TextInput::new()
            .with_value(self.llm_base_url.clone())
            .with_placeholder("Optional base URL")
            .with_on_change(move |v| {
                *base_url_state_for_change.borrow_mut() = v;
            })
            .finish();

        let temperature_state_for_change = Rc::clone(&temperature_state);
        let temperature_input = TextInput::new()
            .with_value(self.llm_temperature.clone())
            .with_placeholder("e.g. 0.7")
            .with_on_change(move |v| {
                *temperature_state_for_change.borrow_mut() = v;
            })
            .finish();

        let on_save = self.on_save_llm.clone();
        let save = Button::new(Text::new("Save").finish())
            .with_on_click(move || {
                if let Some(cb) = on_save.as_ref() {
                    let provider = provider_state.borrow().clone();
                    let model = model_state.borrow().clone();
                    let api_key = api_key_state.borrow().clone();
                    let base_url = base_url_state.borrow().clone();
                    let temperature = temperature_state.borrow().clone();
                    (cb.borrow_mut())(provider, model, api_key, base_url, temperature);
                }
            })
            .finish();

        section(
            "LLM Provider",
            vec![
                SettingsRow::new(app, "Provider", provider_select)
                    .with_tooltip("Which service serves chat and agent messages.")
                    .with_description("The provider used for chat and agent messages.")
                    .build(),
                SettingsRow::new(app, "Model", model_input)
                    .with_description("The model identifier for the selected provider.")
                    .build(),
                SettingsRow::new(app, "API key", api_key_input)
                    .with_tooltip("Saved locally, never synced to the cluster.")
                    .with_local_only()
                    .with_description("Stored locally and never sent to the cluster.")
                    .build(),
                SettingsRow::new(app, "Base URL", base_url_input)
                    .with_description("Leave blank to use the provider's default endpoint.")
                    .build(),
                SettingsRow::new(app, "Temperature", temperature_input)
                    .with_secondary("0.0 - 2.0")
                    .with_description("Higher values produce more varied output.")
                    .build(),
                save,
            ],
            app,
        )
    }

    fn build_appearance_page(&self, app: &AppContext) -> Box<dyn Element> {
        let on_toggle = self.on_toggle_dark_mode.clone();
        let switch = Switch::new()
            .with_checked(self.dark_mode)
            .with_on_change(move |v| {
                if let Some(cb) = on_toggle.as_ref() {
                    (cb.borrow_mut())(v);
                }
            })
            .finish();

        section(
            "Appearance",
            vec![SettingsRow::new(app, "Dark mode", switch)
                .with_tooltip("Toggles the in-app color theme.")
                .with_description("Use a dark color theme for the app.")
                .build()],
            app,
        )
    }

    fn build_account_page(&self, app: &AppContext) -> Box<dyn Element> {
        let passphrase_state = Rc::new(RefCell::new(String::new()));
        let passphrase_for_change = Rc::clone(&passphrase_state);
        let passphrase_input = TextInput::new()
            .with_placeholder("Vault passphrase")
            .with_on_change(move |v| {
                *passphrase_for_change.borrow_mut() = v;
            })
            .finish();

        let on_unlock = self.on_unlock_vault.clone();
        let unlock_state = Rc::clone(&passphrase_state);
        let unlock = Button::new(Text::new("Unlock vault").finish())
            .with_on_click(move || {
                if let Some(cb) = on_unlock.as_ref() {
                    let passphrase = unlock_state.borrow().clone();
                    (cb.borrow_mut())(passphrase);
                }
            })
            .finish();

        let mut children: Vec<Box<dyn Element>> = vec![
            SettingsRow::new(app, "Passphrase", passphrase_input)
                .with_local_only()
                .with_description("Encrypts the vault stored on this device.")
                .build(),
            unlock,
        ];

        if self.vault_unlocked {
            let secret_key_state = Rc::new(RefCell::new(String::new()));
            let secret_value_state = Rc::new(RefCell::new(String::new()));

            let key_for_change = Rc::clone(&secret_key_state);
            let key_input = TextInput::new()
                .with_placeholder("Secret name")
                .with_on_change(move |v| {
                    *key_for_change.borrow_mut() = v;
                })
                .finish();

            let value_for_change = Rc::clone(&secret_value_state);
            let value_input = TextInput::new()
                .with_placeholder("Secret value")
                .with_on_change(move |v| {
                    *value_for_change.borrow_mut() = v;
                })
                .finish();

            let on_add = self.on_add_vault_secret.clone();
            let add_key_state = Rc::clone(&secret_key_state);
            let add_value_state = Rc::clone(&secret_value_state);
            let add = Button::new(Text::new("Add secret").finish())
                .with_on_click(move || {
                    if let Some(cb) = on_add.as_ref() {
                        let key = add_key_state.borrow().clone();
                        let value = add_value_state.borrow().clone();
                        (cb.borrow_mut())(key, value);
                    }
                })
                .finish();

            children.push(section(
                "Add secret",
                vec![
                    SettingsRow::new(app, "Name", key_input)
                        .with_description("A name for the secret, used to reference it.")
                        .build(),
                    SettingsRow::new(app, "Value", value_input)
                        .with_description("The value stored for this secret.")
                        .build(),
                    add,
                ],
                app,
            ));

            if !self.vault_secrets.is_empty() {
                let mut list = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
                for secret in &self.vault_secrets {
                    list = list.with_child(
                        Container::new(
                            Text::new(secret.clone())
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Md)))
                        .finish(),
                    );
                }
                children.push(section("Saved secrets", vec![list.finish()], app));
            }
        }

        section("Account / Vault", children, app)
    }

    fn build_cluster_page(&self, app: &AppContext) -> Box<dyn Element> {
        let passphrase_state = Rc::new(RefCell::new(String::new()));
        let passphrase_for_change = Rc::clone(&passphrase_state);
        let passphrase_input = TextInput::new()
            .with_placeholder("Cluster passphrase")
            .with_on_change(move |v| {
                *passphrase_for_change.borrow_mut() = v;
            })
            .finish();

        let mut children: Vec<Box<dyn Element>> = vec![];

        if self.cluster_configured {
            let status = format!("Cluster configured: {}", self.cluster_name);
            children.push(
                Container::new(
                    Text::new(status)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Md)))
                .finish(),
            );
            let on_unlock = self.on_unlock_cluster.clone();
            let unlock_state = Rc::clone(&passphrase_state);
            let unlock = Button::new(Text::new("Unlock cluster").finish())
                .with_on_click(move || {
                    if let Some(cb) = on_unlock.as_ref() {
                        let passphrase = unlock_state.borrow().clone();
                        (cb.borrow_mut())(passphrase);
                    }
                })
                .finish();
            children.push(
                SettingsRow::new(app, "Passphrase", passphrase_input)
                    .with_local_only()
                    .with_description("Encrypts the cluster identity on this device.")
                    .build(),
            );
            children.push(unlock);
        } else {
            let name_state = Rc::new(RefCell::new(self.cluster_name.clone()));
            let name_for_change = Rc::clone(&name_state);
            let name_input = TextInput::new()
                .with_value(self.cluster_name.clone())
                .with_placeholder("Cluster name")
                .with_on_change(move |v| {
                    *name_for_change.borrow_mut() = v;
                })
                .finish();

            let on_create = self.on_create_cluster.clone();
            let create_name_state = Rc::clone(&name_state);
            let create_pass_state = Rc::clone(&passphrase_state);
            let create = Button::new(Text::new("Create cluster").finish())
                .with_on_click(move || {
                    if let Some(cb) = on_create.as_ref() {
                        let name = create_name_state.borrow().clone();
                        let passphrase = create_pass_state.borrow().clone();
                        (cb.borrow_mut())(name, passphrase);
                    }
                })
                .finish();

            children.push(
                SettingsRow::new(app, "Name", name_input)
                    .with_description("A label for this cluster.")
                    .build(),
            );
            children.push(
                SettingsRow::new(app, "Passphrase", passphrase_input)
                    .with_local_only()
                    .with_description("Encrypts the cluster identity on this device.")
                    .build(),
            );
            children.push(create);
        }

        section("Cluster identity", children, app)
    }

    fn build_workers_page(&self, app: &AppContext) -> Box<dyn Element> {
        let mut children: Vec<Box<dyn Element>> = vec![];

        let name_state = Rc::new(RefCell::new(String::new()));
        let url_state = Rc::new(RefCell::new(String::new()));
        let code_state = Rc::new(RefCell::new(String::new()));

        let name_state_for_change = Rc::clone(&name_state);
        let name_input = TextInput::new()
            .with_placeholder("Worker name")
            .with_on_change(move |v| *name_state_for_change.borrow_mut() = v)
            .finish();
        let url_state_for_change = Rc::clone(&url_state);
        let url_input = TextInput::new()
            .with_placeholder("wss://host:port/ws")
            .with_on_change(move |v| *url_state_for_change.borrow_mut() = v)
            .finish();
        let code_state_for_change = Rc::clone(&code_state);
        let code_input = TextInput::new()
            .with_placeholder("Pairing code")
            .with_on_change(move |v| *code_state_for_change.borrow_mut() = v)
            .finish();

        let on_add = self.on_add_worker.clone();
        let name_state_for_add = Rc::clone(&name_state);
        let add = Button::new(Text::new("Add worker").finish())
            .with_on_click(move || {
                if let Some(cb) = on_add.as_ref() {
                    let name = name_state_for_add.borrow().clone();
                    let url = url_state.borrow().clone();
                    (cb.borrow_mut())(name, url);
                }
            })
            .finish();
        let on_pair = self.on_pair_worker.clone();
        let pair_name_state = Rc::clone(&name_state);
        let pair = Button::new(Text::new("Pair worker").finish())
            .with_on_click(move || {
                if let Some(cb) = on_pair.as_ref() {
                    let id = pair_name_state.borrow().clone();
                    let code = code_state.borrow().clone();
                    (cb.borrow_mut())(id, code);
                }
            })
            .finish();

        children.push(section(
            "Register / pair",
            vec![
                SettingsRow::new(app, "Name", name_input)
                    .with_description("A human-readable label for the worker.")
                    .build(),
                SettingsRow::new(app, "URL", url_input)
                    .with_description("The WebSocket endpoint the app can reach.")
                    .build(),
                SettingsRow::new(app, "Pairing code", code_input)
                    .with_description("A one-time code from the worker's pairing command.")
                    .build(),
                add,
                pair,
            ],
            app,
        ));

        if !self.workers.is_empty() {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
            for (id, name, url, paired) in &self.workers {
                let status = if *paired { "paired" } else { "unpaired" };
                let line = format!(
                    "{} | {} | {} | {}",
                    &id[..id.len().min(8)],
                    name,
                    url,
                    status
                );
                let on_remove = self.on_remove_worker.clone();
                let id_for_remove = id.clone();
                let row = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Remove").finish())
                            .with_on_click(move || {
                                if let Some(cb) = on_remove.as_ref() {
                                    (cb.borrow_mut())(id_for_remove.clone());
                                }
                            })
                            .finish(),
                    )
                    .finish();
                list = list.with_child(
                    Container::new(row)
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Md)))
                        .finish(),
                );
            }
            children.push(section("Workers", vec![list.finish()], app));
        }

        section("Workers", children, app)
    }

    fn build_keys_page(&self, app: &AppContext) -> Box<dyn Element> {
        let mut children: Vec<Box<dyn Element>> = vec![];

        let name_state = Rc::new(RefCell::new(String::new()));
        let pem_state = Rc::new(RefCell::new(String::new()));
        let fp_state = Rc::new(RefCell::new(String::new()));

        let name_state_for_change = Rc::clone(&name_state);
        let name_input = TextInput::new()
            .with_placeholder("Key label")
            .with_on_change(move |v| *name_state_for_change.borrow_mut() = v)
            .finish();
        let pem_state_for_change = Rc::clone(&pem_state);
        let pem_input = TextInput::new()
            .with_placeholder("Public key PEM")
            .with_on_change(move |v| *pem_state_for_change.borrow_mut() = v)
            .finish();
        let fp_state_for_change = Rc::clone(&fp_state);
        let fp_input = TextInput::new()
            .with_placeholder("Fingerprint")
            .with_on_change(move |v| *fp_state_for_change.borrow_mut() = v)
            .finish();

        let on_add = self.on_add_authorized_key.clone();
        let add = Button::new(Text::new("Add key").finish())
            .with_on_click(move || {
                if let Some(cb) = on_add.as_ref() {
                    let name = name_state.borrow().clone();
                    let pem = pem_state.borrow().clone();
                    let fp = fp_state.borrow().clone();
                    (cb.borrow_mut())(name, pem, fp);
                }
            })
            .finish();

        children.push(section(
            "Add authorized key",
            vec![
                SettingsRow::new(app, "Name", name_input)
                    .with_description("A label used to recognize this key.")
                    .build(),
                SettingsRow::new(app, "Public key", pem_input)
                    .with_tooltip("Paste a public key in PEM or SSH format.")
                    .with_description("Public key in PEM or SSH format.")
                    .build(),
                SettingsRow::new(app, "Fingerprint", fp_input)
                    .with_description("A hash used to verify the key on the cluster.")
                    .build(),
                add,
            ],
            app,
        ));

        if !self.authorized_keys.is_empty() {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
            for (id, name, fingerprint) in &self.authorized_keys {
                let line = format!("{} | {}", name, fingerprint);
                let on_remove = self.on_remove_authorized_key.clone();
                let id_for_remove = id.clone();
                let row = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Remove").finish())
                            .with_on_click(move || {
                                if let Some(cb) = on_remove.as_ref() {
                                    (cb.borrow_mut())(id_for_remove.clone());
                                }
                            })
                            .finish(),
                    )
                    .finish();
                list = list.with_child(
                    Container::new(row)
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Md)))
                        .finish(),
                );
            }
            children.push(section("Authorized keys", vec![list.finish()], app));
        }

        section("Authorized keys", children, app)
    }
}
