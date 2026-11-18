//! The settings tab's own state: the file projection it renders, the keyboard's
//! position, the drafts of every field it is editing, and the dialogs it has
//! open.
//!
//! This is **one** field on [`UiState`](crate::state::UiState) (`settings`), one
//! on the snapshot and one on the actions — the smallest shared footprint the
//! Lane A plan allows. Everything the settings surface owns lives here, so the
//! shared `UiState` does not grow a field per row.
//!
//! The state is a *view*: the settings files are the source of truth (see
//! `crash course`: `~/.goble/config.toml`, `environment.toml`, `hosts.toml` via
//! `goble-core::config`). [`SettingsUiState::load`] reads them through the
//! desktop service (Lane B2) and a write replaces `config`/`environment`/`hosts`
//! with the fresh `ConfigView` the service returns, so the revision the next
//! write fences against is always the one on disk.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use goble_core::config::{ConfigFile, ConfigRevision, ConfigView, HostView, ModelView};
use goble_ui::elements::ScrollState;

use crate::ui::SettingsCategory;

/// Which of the settings surface's regions holds the keyboard: the rail, the
/// page, or the add/edit side sheet opened over the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsRegion {
    #[default]
    Rail,
    Page,
    /// A side sheet (Add/Edit model, connector, host, harness) owns the keys.
    Sheet,
}

/// The colours a status sentence can take (the row-level counterpart lives in
/// `goble_ui::elements::settings`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Muted,
    Info,
    Success,
    Warning,
    Error,
}

/// A one-line notice above the page: the file changed on disk, the write
/// conflicted, the probe returned something. Cleared by the next successful
/// action; never carries a secret.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsNotice {
    pub kind: StatusKind,
    pub text: String,
    /// The action a trailing button offers (a conflict notice offers Reload).
    pub action: Option<SettingsActionButton>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsActionButton {
    pub label: String,
    pub action: SettingsAction,
}

/// The Add/Edit model form. It is its own data because a provider has more
/// fields than a row and the API key is write-only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelForm {
    /// The slug being edited; `None` adds a new model.
    pub editing: Option<String>,
    pub slug: String,
    pub model_id: String,
    pub name: String,
    pub base_url: String,
    /// The replacement key being typed. Never filled from the file (the
    /// projection does not carry it).
    pub api_key: String,
    pub api_key_env: String,
    pub max_completion_tokens: String,
    pub context_window: String,
    pub hidden: bool,
    pub set_default: bool,
    /// The line the last connection test reported.
    pub probe: Option<(StatusKind, String)>,
}

impl ModelForm {
    pub fn add() -> Self {
        Self::default()
    }

    /// The form for editing an existing model, prefilled from the redacted
    /// view. The key and the env var name are left blank: the file's key is
    /// never read back, and an empty submission keeps it.
    pub fn edit(model: &ModelView) -> Self {
        Self {
            editing: Some(model.slug.clone()),
            slug: model.slug.clone(),
            model_id: model.model_id.clone(),
            name: model.name.clone().unwrap_or_default(),
            base_url: model.base_url.clone().unwrap_or_default(),
            api_key: String::new(),
            api_key_env: model.api_key_env.clone().unwrap_or_default(),
            max_completion_tokens: model
                .max_completion_tokens
                .map(|value| value.to_string())
                .unwrap_or_default(),
            context_window: model
                .context_window
                .map(|value| value.to_string())
                .unwrap_or_default(),
            hidden: model.hidden,
            set_default: model.is_default,
            probe: None,
        }
    }
}

/// The Add/Edit connector form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectorForm {
    pub editing: Option<String>,
    pub name: String,
    /// 0 = HTTP URL, 1 = stdio command.
    pub transport: usize,
    pub url: String,
    pub command: String,
    /// One argument per line; the form is a text area this build draws as a
    /// field with space-separated arguments.
    pub args: String,
    pub enabled: bool,
}

/// The Add/Edit saved-host form (`hosts.toml`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostForm {
    pub editing: Option<String>,
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: String,
    pub identity: String,
}

/// The Add/Edit agent-CLI override form (`[harness.<id>]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HarnessForm {
    /// The catalog id being edited; always `Some` (a harness exists in the
    /// catalog before its override does).
    pub editing: Option<String>,
    pub name: String,
    pub command: String,
    pub args: String,
    pub enabled: bool,
}

/// Which form is open, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsForm {
    Model(Box<ModelForm>),
    Connector(Box<ConnectorForm>),
    Host(Box<HostForm>),
    Harness(Box<HarnessForm>),
}

/// What the Secret page's vault section shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VaultState {
    pub unlocked: bool,
    pub secrets: Vec<(String, String)>,
    /// The passphrase being typed, in the unlock form.
    pub passphrase: String,
    pub new_key: String,
    pub new_value: String,
}

/// One page's worth of settings data plus the interaction state of the whole
/// tab.
#[derive(Clone)]
pub struct SettingsUiState {
    /// The page the settings leaf shows.
    pub page: SettingsCategory,
    /// `config.toml` as the service projects it (redacted).
    pub config: Option<ConfigView>,
    /// `environment.toml`.
    pub environment: Option<ConfigView>,
    /// `hosts.toml`.
    pub hosts: Option<ConfigView>,
    /// The keyboard's region.
    pub region: SettingsRegion,
    /// The focused rail row (flattened page index).
    pub nav_index: usize,
    /// The focused page row (interactive rows only, in draw order).
    pub row_index: usize,
    /// The focused row of the open side sheet (interactive rows only).
    pub sheet_row: usize,
    /// The field that holds the caret, by field id (`app.friendly_name`), when
    /// one does.
    pub focused_field: Option<String>,
    /// Uncommitted text, by field id.
    pub drafts: BTreeMap<String, String>,
    /// The rail search query.
    pub search: String,
    /// Whether the rail search field holds the keyboard.
    pub search_focused: bool,
    /// The notice above the page.
    pub notice: Option<SettingsNotice>,
    /// The last outcome line under the page header.
    pub status: Option<(StatusKind, String)>,
    /// A service call is in flight (a probe, a test, an unlock).
    pub busy: bool,
    /// Open folds, by fold id.
    pub folds: BTreeSet<String>,
    /// The open form, if any. A form is drawn as a side sheet docked to the
    /// pane's trailing edge, never as an inline page section.
    pub form: Option<SettingsForm>,
    /// Local agent-CLI detection, refreshed from the service's catalog scan
    /// (`PATH` resolution, no `--version` probe). The Providers page's Accounts
    /// and Agent CLIs sections render this.
    pub harnesses: Vec<goble_core::harness::HarnessStatus>,
    /// The theme channel the Appearance page's colour wheel edits (index into
    /// `color_picker::COLOR_TARGET_ORDER`).
    pub color_target: usize,
    /// The colour wheel's in-progress drag. It lives here, not in the element,
    /// because the settings tree is rebuilt every frame.
    pub color_drag: Rc<RefCell<Option<crate::ui::color_picker::ColorPickerDrag>>>,
    /// Whether the Appearance page's theme-preset list is open.
    pub theme_select_open: bool,
    /// The environment page's drafts: the new-group name and the open group.
    pub env_new_group: String,
    pub env_open_group: Option<String>,
    pub env_var_name: String,
    pub env_var_value: String,
    /// The Secrets page's vault.
    pub vault: VaultState,
    /// `~/.ssh` as read for the Hosts page's read-only section.
    pub ssh_hosts: Option<goble_core::ssh_hosts::SshHosts>,
    /// The home directory the `~/.ssh` read uses; tests point it at a fixture,
    /// the app leaves it `None` so the real home is read.
    pub ssh_home_override: Option<PathBuf>,
    /// The last probe/test line, by row id (a model slug, a connector name, a
    /// host name).
    pub test_results: BTreeMap<String, (StatusKind, String)>,
    /// The update checker's view (the Updates page).
    pub update_view: crate::update::UpdateView,
    /// The file diagnostics the Advanced page shows.
    pub paths: SettingsPaths,
    /// The page's scroll offset.
    pub scroll: Rc<RefCell<ScrollState>>,
    /// The open side sheet's scroll offset (its form can outgrow the pane).
    pub sheet_scroll: Rc<RefCell<ScrollState>>,
    /// Whether the last load failed; the page then draws the error rather than
    /// stale data.
    pub load_error: Option<String>,
    /// The revision the page was rendered from, per file (the fence for a
    /// write).
    pub revisions: BTreeMap<ConfigFile, ConfigRevision>,
}

impl std::fmt::Debug for SettingsUiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsUiState")
            .field("page", &self.page)
            .field("region", &self.region)
            .field("nav_index", &self.nav_index)
            .field("row_index", &self.row_index)
            .field("focused_field", &self.focused_field)
            .field("notice", &self.notice)
            .field("status", &self.status)
            .field("busy", &self.busy)
            .field("load_error", &self.load_error)
            .finish_non_exhaustive()
    }
}

/// The settings files' paths, read from the service. The Advanced page and the
/// header name them, so a user can find the file the page writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsPaths {
    pub config: Option<PathBuf>,
    pub environment: Option<PathBuf>,
    pub hosts: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self {
            page: SettingsCategory::General,
            config: None,
            environment: None,
            hosts: None,
            region: SettingsRegion::Rail,
            nav_index: 0,
            row_index: 0,
            sheet_row: 0,
            focused_field: None,
            drafts: BTreeMap::new(),
            search: String::new(),
            search_focused: false,
            notice: None,
            status: None,
            busy: false,
            folds: BTreeSet::new(),
            form: None,
            harnesses: Vec::new(),
            color_target: 0,
            color_drag: Rc::new(RefCell::new(None)),
            theme_select_open: false,
            env_new_group: String::new(),
            env_open_group: None,
            env_var_name: String::new(),
            env_var_value: String::new(),
            vault: VaultState::default(),
            ssh_hosts: None,
            ssh_home_override: None,
            test_results: BTreeMap::new(),
            update_view: crate::update::UpdateView::default(),
            paths: SettingsPaths::default(),
            scroll: Rc::new(RefCell::new(ScrollState::default())),
            sheet_scroll: Rc::new(RefCell::new(ScrollState::default())),
            load_error: None,
            revisions: BTreeMap::new(),
        }
    }
}

impl SettingsUiState {
    /// The `config.toml` projection, if it loaded.
    pub fn view(&self) -> Option<&ConfigView> {
        self.config.as_ref()
    }

    /// The revision the next `config.toml` write fences against.
    pub fn config_revision(&self) -> Option<ConfigRevision> {
        self.config.as_ref().map(|view| view.revision.clone())
    }

    pub fn revision(&self, file: ConfigFile) -> Option<ConfigRevision> {
        self.revisions.get(&file).cloned().or_else(|| match file {
            ConfigFile::Config => self.config_revision(),
            ConfigFile::Environment => self.environment.as_ref().map(|v| v.revision.clone()),
            ConfigFile::Hosts => self.hosts.as_ref().map(|v| v.revision.clone()),
        })
    }

    /// Adopt a freshly loaded projection. The page's focus slots are clamped to
    /// what the new data has, so a deleted row does not leave the ring past the
    /// end.
    pub fn adopt(&mut self, view: ConfigView) {
        self.revisions.insert(view.file, view.revision.clone());
        match view.file {
            ConfigFile::Config => self.config = Some(view),
            ConfigFile::Environment => self.environment = Some(view),
            ConfigFile::Hosts => self.hosts = Some(view),
        }
        self.load_error = None;
    }

    /// Adopt a view and keep the file's problems on the page as an honest
    /// notice ("config.toml could not be parsed at line 4").
    pub fn adopt_with_problems(&mut self, view: ConfigView) {
        let problems = view
            .problems
            .iter()
            .map(|problem| problem.line())
            .collect::<Vec<_>>();
        let file = view.file;
        self.adopt(view);
        if !problems.is_empty() {
            self.notice = Some(SettingsNotice {
                kind: StatusKind::Warning,
                text: format!(
                    "{} has problems this build will not overwrite: {}",
                    file.file_name(),
                    problems.join("; ")
                ),
                action: None,
            });
        }
    }

    pub fn models(&self) -> &[ModelView] {
        self.config
            .as_ref()
            .map(|view| view.sections.models.as_slice())
            .unwrap_or(&[])
    }

    pub fn saved_hosts(&self) -> &[HostView] {
        self.hosts
            .as_ref()
            .map(|view| view.sections.hosts.as_slice())
            .unwrap_or(&[])
    }

    /// The display line under a page's header: where the page's data lives.
    pub fn file_line(&self) -> String {
        match self.page {
            SettingsCategory::Environments => self
                .environment
                .as_ref()
                .map(|view| view.path.clone())
                .or_else(|| {
                    self.paths
                        .environment
                        .as_ref()
                        .map(|path| path.display().to_string())
                })
                .unwrap_or_else(|| "~/.goble/environment.toml".to_string()),
            SettingsCategory::Hosts => self
                .hosts
                .as_ref()
                .map(|view| view.path.clone())
                .or_else(|| {
                    self.paths
                        .hosts
                        .as_ref()
                        .map(|path| path.display().to_string())
                })
                .unwrap_or_else(|| "~/.goble/hosts.toml".to_string()),
            _ => self
                .config
                .as_ref()
                .map(|view| view.path.clone())
                .or_else(|| {
                    self.paths
                        .config
                        .as_ref()
                        .map(|path| path.display().to_string())
                })
                .unwrap_or_else(|| "~/.goble/config.toml".to_string()),
        }
    }

    /// What the Remote section says under its heading: the configuration's own
    /// answer, with the token's absence named rather than implied.
    pub fn remote_summary(&self) -> String {
        let remote = self
            .config
            .as_ref()
            .map(|view| view.sections.remote.clone())
            .unwrap_or_default();
        if remote.mode == "gateway" {
            return match (&remote.gateway_url, remote.worker_token.set) {
                (None, _) => "Gateway · no URL configured".to_string(),
                (Some(url), false) => format!("Gateway · {url} · missing worker token"),
                (Some(url), true) => format!("Gateway · {url}"),
            };
        }
        let agents = self
            .config
            .as_ref()
            .map(|view| view.sections.remote_agents.len())
            .unwrap_or(0);
        if agents == 0 {
            "Turns run on this machine.".to_string()
        } else {
            format!("Platform · {agents} remote agent table(s)")
        }
    }

    /// Whether the page is showing the file it can write: a parse problem means
    /// the editor refuses to write, and the page says so by disabling the rows.
    pub fn file_read_only(&self) -> bool {
        self.config
            .as_ref()
            .is_some_and(|view| !view.problems.is_empty())
    }

    pub fn draft<'a>(&'a self, id: &str, fallback: &'a str) -> &'a str {
        self.drafts.get(id).map(String::as_str).unwrap_or(fallback)
    }

    pub fn set_draft(&mut self, id: &str, value: String) {
        self.drafts.insert(id.to_string(), value);
    }

    pub fn clear_draft(&mut self, id: &str) {
        self.drafts.remove(id);
    }

    pub fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some((kind, text.into()));
    }

    pub fn notice(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.notice = Some(SettingsNotice {
            kind,
            text: text.into(),
            action: None,
        });
    }

    /// The notice a stale-revision write leaves behind: the file is the truth,
    /// so the page reloads it and asks the user to repeat the edit instead of
    /// overwriting someone else's change.
    pub fn conflict_notice(&mut self, file: ConfigFile) {
        self.notice = Some(SettingsNotice {
            kind: StatusKind::Warning,
            text: format!(
                "{} changed on disk while this page was open. The file was reloaded; repeat the edit to apply it.",
                file.file_name()
            ),
            action: Some(SettingsActionButton {
                label: "Reload".to_string(),
                action: SettingsAction::Reload,
            }),
        });
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// The rail's page index for `page`, so a page opened from elsewhere starts
    /// with the rail on its row.
    pub fn nav_index_of(page: SettingsCategory) -> usize {
        SettingsCategory::ALL
            .iter()
            .position(|candidate| *candidate == page)
            .unwrap_or(0)
    }
}

/// The actions the settings surface dispatches. Every button, toggle, field
/// commit and key goes through one closure, built in
/// `apps/desktop/src/actions/settings_actions.rs`: the app's single call site
/// for the file-backed settings service.
#[derive(Clone)]
pub struct SettingsActions {
    pub dispatch: Rc<RefCell<dyn FnMut(SettingsAction)>>,
}

impl SettingsActions {
    pub fn new(dispatch: impl FnMut(SettingsAction) + 'static) -> Self {
        Self {
            dispatch: Rc::new(RefCell::new(dispatch)),
        }
    }

    pub fn dispatch(&self, action: SettingsAction) {
        (self.dispatch.borrow_mut())(action);
    }
}

impl std::fmt::Debug for SettingsActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsActions")
    }
}

/// One user gesture on the settings surface.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    // ---- navigation and focus ------------------------------------------
    Navigate(SettingsCategory),
    FocusRail,
    FocusPage,
    FocusSearch(bool),
    Search(String),
    /// A character typed into the rail search: the handler appends it (or
    /// removes the last character for `"\u{8}"`), so the key handler does not
    /// need to read the query it already has a copy of.
    SearchChar(String),
    ClearSearch,
    Step(i32),
    Adjust(i32),
    Activate,
    Back,
    ToggleFold(String),
    /// Refresh the local agent-CLI detection from the service catalog.
    RefreshHarnesses,

    // ---- scalar rows (paths are `[section, key]`; see `fields.rs`) ------
    SetToggle {
        path: String,
        value: bool,
    },
    SetSelect {
        path: String,
        index: usize,
    },
    SetSlider {
        path: String,
        value: f32,
    },
    SetText {
        path: String,
        value: String,
    },
    Reset {
        path: String,
    },
    /// Restore every theme channel override and the preset to the built-in
    /// base: five unsets in one write.
    ResetThemeColours,
    SetDraft {
        id: String,
        value: String,
    },
    CommitField {
        id: String,
    },
    ClearField {
        id: String,
    },

    // ---- files ----------------------------------------------------------
    Reload,
    OpenFile(ConfigFile),
    OpenSettingsFolder,

    // ---- appearance -----------------------------------------------------
    /// Pick which theme channel the colour wheel edits (index into
    /// `color_picker::COLOR_TARGET_ORDER`).
    SetColorTarget(usize),
    /// Open or close the theme-preset list.
    ToggleThemeSelect(bool),
    /// A colour the wheel picked for `target`, written to `[theme] <channel>`.
    SetColor {
        target: usize,
        hex: String,
    },

    // ---- models ---------------------------------------------------------
    NewModel,
    EditModel(String),
    DeleteModel(String),
    TestModel(String),
    TestHarness(String),
    SetDefaultModel(String),
    ModelFormText {
        field: ModelFormField,
        value: String,
    },
    ModelFormToggle {
        field: ModelFormToggle,
        value: bool,
    },
    TestModelDraft,
    SaveModelForm,
    CancelForm,

    // ---- connectors -----------------------------------------------------
    NewConnector,
    EditConnector(String),
    DeleteConnector(String),
    TestConnector(String),
    ConnectorFormText {
        field: ConnectorFormField,
        value: String,
    },
    ConnectorFormTransport(usize),
    ConnectorFormEnabled(bool),
    SaveConnectorForm,

    // ---- hosts ----------------------------------------------------------
    NewHost,
    EditHost(String),
    DeleteHost(String),
    TestHost(String),
    HostFormText {
        field: HostFormField,
        value: String,
    },
    SaveHostForm,

    // ---- harnesses (side sheet) ----------------------------------------
    EditHarness(String),
    HarnessFormText {
        field: HarnessFormField,
        value: String,
    },
    HarnessFormEnabled(bool),
    SaveHarnessForm,

    // ---- environments ---------------------------------------------------
    EnvNewGroup(String),
    EnvCreateGroup,
    EnvOpenGroup(String),
    EnvDeleteGroup(String),
    EnvVarName {
        group: String,
        value: String,
    },
    EnvVarValue {
        group: String,
        value: String,
    },
    EnvAddVariable {
        group: String,
    },
    EnvDeleteVariable {
        group: String,
        name: String,
    },

    // ---- vault ----------------------------------------------------------
    VaultPassphrase(String),
    VaultUnlock,
    VaultNewKey(String),
    VaultNewValue(String),
    VaultAdd,
    VaultDelete(String),
    VaultLock,

    // ---- agent setup ----------------------------------------------------
    /// Open the agent-led Remote setup conversation (asks for what is missing
    /// and writes it through `goble-cli setup`).
    SetupRemoteWithAgent,
    /// The same conversation for the hub.
    SetupHubWithAgent,

    // ---- updates --------------------------------------------------------
    CheckUpdates,
    InstallUpdate,
}

/// The model form's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFormField {
    Slug,
    ModelId,
    Name,
    BaseUrl,
    ApiKey,
    ApiKeyEnv,
    MaxCompletionTokens,
    ContextWindow,
}

/// The model form's toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFormToggle {
    Hidden,
    SetDefault,
}

/// The harness override form's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessFormField {
    Command,
    Args,
}

/// The connector form's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorFormField {
    Name,
    Url,
    Command,
    Args,
}

/// The host form's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostFormField {
    Name,
    Host,
    User,
    Port,
    Identity,
}
