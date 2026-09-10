use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Expanded, Fill, Flex, Icon, Label, LabelSize, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext, Point, Select, SelectOption, SizeConstraint, Switch, Text, TextInput, Tooltip,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Profile,
    Llm,
    Appearance,
    Account,
    Cluster,
    Workers,
    Keys,
}

fn nav_item(
    label: impl Into<String>,
    page: SettingsPage,
    selected: bool,
    app: &AppContext,
    on_navigate: Option<Rc<RefCell<dyn FnMut(SettingsPage) + 'static>>>,
) -> Box<dyn Element> {
    let padding = app.theme.spacing_px(SpacingToken::Md);
    let bg = if selected {
        Fill::Solid(app.theme.color(ColorToken::Selected))
    } else {
        Fill::None
    };
    let label_text = Text::new(label.into()).finish();
    let on_navigate = on_navigate.clone();
    let button = Button::new(label_text)
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || {
            if let Some(cb) = on_navigate.as_ref() {
                (cb.borrow_mut())(page);
            }
        })
        .finish();

    Container::new(button)
        .with_background(bg)
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(padding))
        .finish()
}

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

const ROW_FONT_SIZE: f32 = 12.0;
const ROW_HEADER_PADDING: f32 = 15.0;
const ROW_CONTROL_RIGHT_PADDING: f32 = 5.0;
const ROW_DESCRIPTION_RIGHT_PAD: f32 = 100.0;
const ROW_ICON_SIZE: f32 = 13.0;
const ROW_LOCAL_ONLY_TOOLTIP: &str = "This setting is not synced to your other devices";

/// A settings row at the warp-new level of complexity: a leading title (plus
/// optional muted secondary text, an info/tooltip icon and a not-cloud-synced
/// icon), the control on the right, and an optional muted description paragraph
/// below that wraps before reaching the control.
struct SettingsRow<'a> {
    app: &'a AppContext,
    label: String,
    control: Box<dyn Element>,
    description: Option<String>,
    tooltip: Option<String>,
    secondary: Option<String>,
    local_only: bool,
}

impl<'a> SettingsRow<'a> {
    fn new(app: &'a AppContext, label: impl Into<String>, control: Box<dyn Element>) -> Self {
        Self {
            app,
            label: label.into(),
            control,
            description: None,
            tooltip: None,
            secondary: None,
            local_only: false,
        }
    }

    fn with_description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    fn with_tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    fn with_secondary(mut self, text: impl Into<String>) -> Self {
        self.secondary = Some(text.into());
        self
    }

    /// Marks the setting as local-only (not synced to other devices).
    fn with_local_only(mut self) -> Self {
        self.local_only = true;
        self
    }

    fn build(self) -> Box<dyn Element> {
        let app = self.app;
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        // Leading label: the title with any icons / secondary text hanging
        // inline to the right of it. The row expands to fill the header, so the
        // control is pushed to the right edge of the card.
        let mut label_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                Text::new(self.label.clone())
                    .with_font_size(ROW_FONT_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );

        if let Some(tooltip_text) = self.tooltip {
            label_row = label_row.with_child(
                Tooltip::new(
                    Icon::new("info")
                        .with_size(ROW_ICON_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    tooltip_text,
                )
                .finish(),
            );
        }
        if self.local_only {
            label_row = label_row.with_child(
                Tooltip::new(
                    Icon::new("cloud-off")
                        .with_size(ROW_ICON_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    ROW_LOCAL_ONLY_TOOLTIP,
                )
                .finish(),
            );
        }
        if let Some(secondary) = self.secondary {
            label_row = label_row.with_child(
                Text::new(secondary)
                    .with_font_size(ROW_FONT_SIZE)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        let label_row = label_row.finish();

        // Header row: the (expanded) label followed by the control inset from
        // the right edge, mirroring warp-new's Shrinkable + control padding.
        let header = Expanded::new(label_row).finish();
        let control = Container::new(self.control)
            .with_padding_right(ROW_CONTROL_RIGHT_PADDING)
            .finish();

        let mut header_row = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(header)
                .with_child(control)
                .finish(),
        );
        if self.description.is_none() {
            header_row = header_row.with_padding_bottom(ROW_HEADER_PADDING);
        }
        let header_row = header_row.finish();

        // Description paragraph wraps before the control and sits under the row.
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header_row);
        if let Some(description) = self.description {
            let description = Text::new(description)
                .with_font_size(ROW_FONT_SIZE)
                .with_theme_color(ColorToken::Muted, app)
                .finish();
            column = column.with_child(
                Container::new(description)
                    .with_padding_right(ROW_DESCRIPTION_RIGHT_PAD)
                    .with_padding_bottom(ROW_HEADER_PADDING)
                    .finish(),
            );
        }

        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(spacing))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_corner_radius(app.theme.radius_px())
            .finish()
    }
}

pub struct SettingsView {
    current_page: SettingsPage,
    profile_name: String,
    profile_email: String,
    llm_provider: String,
    llm_model: String,
    llm_api_key: String,
    llm_base_url: String,
    llm_temperature: String,
    dark_mode: bool,
    vault_unlocked: bool,
    vault_secrets: Vec<String>,
    cluster_name: String,
    cluster_configured: bool,
    workers: Vec<(String, String, String, bool)>, // id, name, url, paired
    authorized_keys: Vec<(String, String, String)>, // id, name, fingerprint
    on_navigate: Option<Rc<RefCell<dyn FnMut(SettingsPage) + 'static>>>,
    on_save_profile: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_save_llm: Option<Rc<RefCell<dyn FnMut(String, String, String, String, String) + 'static>>>,
    on_toggle_dark_mode: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_unlock_vault: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_vault_secret: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_create_cluster: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_unlock_cluster: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_worker: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_pair_worker: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_remove_worker: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_authorized_key: Option<Rc<RefCell<dyn FnMut(String, String, String) + 'static>>>,
    on_remove_authorized_key: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_back: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsView {
    pub fn new(current_page: SettingsPage) -> Self {
        Self {
            current_page,
            profile_name: String::new(),
            profile_email: String::new(),
            llm_provider: String::new(),
            llm_model: String::new(),
            llm_api_key: String::new(),
            llm_base_url: String::new(),
            llm_temperature: String::new(),
            dark_mode: false,
            vault_unlocked: false,
            vault_secrets: Vec::new(),
            cluster_name: String::new(),
            cluster_configured: false,
            workers: Vec::new(),
            authorized_keys: Vec::new(),
            on_navigate: None,
            on_save_profile: None,
            on_save_llm: None,
            on_toggle_dark_mode: None,
            on_unlock_vault: None,
            on_add_vault_secret: None,
            on_create_cluster: None,
            on_unlock_cluster: None,
            on_add_worker: None,
            on_pair_worker: None,
            on_remove_worker: None,
            on_add_authorized_key: None,
            on_remove_authorized_key: None,
            on_back: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_profile(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.profile_name = name.into();
        self.profile_email = email.into();
        self
    }

    pub fn with_llm(
        mut self,
        provider: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        temperature: impl Into<String>,
    ) -> Self {
        self.llm_provider = provider.into();
        self.llm_model = model.into();
        self.llm_api_key = api_key.into();
        self.llm_base_url = base_url.into();
        self.llm_temperature = temperature.into();
        self
    }

    pub fn with_dark_mode(mut self, enabled: bool) -> Self {
        self.dark_mode = enabled;
        self
    }

    pub fn with_vault_state(mut self, unlocked: bool, secrets: Vec<String>) -> Self {
        self.vault_unlocked = unlocked;
        self.vault_secrets = secrets;
        self
    }

    pub fn with_cluster_state(mut self, name: impl Into<String>, configured: bool) -> Self {
        self.cluster_name = name.into();
        self.cluster_configured = configured;
        self
    }

    pub fn with_workers(mut self, workers: Vec<(String, String, String, bool)>) -> Self {
        self.workers = workers;
        self
    }

    pub fn with_authorized_keys(mut self, keys: Vec<(String, String, String)>) -> Self {
        self.authorized_keys = keys;
        self
    }

    pub fn with_on_navigate<F: FnMut(SettingsPage) + 'static>(mut self, callback: F) -> Self {
        self.on_navigate = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_profile<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_save_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_llm<F: FnMut(String, String, String, String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_save_llm = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_dark_mode<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_dark_mode = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_unlock_vault<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_unlock_vault = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_vault_secret<F: FnMut(String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_add_vault_secret = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_create_cluster<F: FnMut(String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_create_cluster = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_unlock_cluster<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_unlock_cluster = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_worker<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_add_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_pair_worker<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_pair_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_remove_worker<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_remove_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_authorized_key<F: FnMut(String, String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_add_authorized_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_remove_authorized_key<F: FnMut(String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_remove_authorized_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set a callback fired by the top-left Back button (returns to the previous
    /// view). Without a callback the button is not rendered.
    pub fn with_on_back<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_back = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_nav(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let pages = [
            ("Profile", SettingsPage::Profile),
            ("LLM", SettingsPage::Llm),
            ("Appearance", SettingsPage::Appearance),
            ("Account", SettingsPage::Account),
            ("Cluster", SettingsPage::Cluster),
            ("Workers", SettingsPage::Workers),
            ("Keys", SettingsPage::Keys),
        ];
        for (label, page) in pages {
            let selected = self.current_page == page;
            let item = nav_item(label, page, selected, app, self.on_navigate.clone());
            column = column.with_child(item);
        }

        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(spacing))
            .finish()
    }

    fn build_pane(&self, app: &AppContext) -> Box<dyn Element> {
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

    fn rebuild(&mut self, app: &AppContext, width: f32) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let nav_width = 160.0_f32;
        let pane_width = (width - nav_width - 1.0).max(200.0);

        let nav = self.build_nav(app);
        let pane = self.build_pane(app);
        let pane = crate::elements::ConstrainedBox::new(pane)
            .with_max_width(pane_width)
            .finish();

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                crate::elements::ConstrainedBox::new(nav)
                    .with_max_width(nav_width)
                    .finish(),
            )
            .with_child(Divider::vertical().finish())
            .with_child(pane);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(spacing);

        // Top-left Back button returns to the previous view.
        if let Some(cb) = self.on_back.clone() {
            let back_label = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
                .with_child(
                    Icon::new("chevron-left")
                        .with_size(16.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Back")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .finish();
            let back = Button::new(back_label)
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
            column = column.with_child(
                Container::new(back)
                    .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Sm)))
                    .finish(),
            );
        }

        column = column.with_child(row.finish());

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for SettingsView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, EventContext, LayoutContext, PaintContext};
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
}
