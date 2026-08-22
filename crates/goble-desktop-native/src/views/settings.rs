use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    ActiveView, AppContext, Container, Element, EventContext, Fill, LayoutContext, PaintContext,
    Point, SettingsTab, ShellState, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken, Theme};
use goble_ui::views::settings_view::{SettingsPage, SettingsView};

use crate::app::UiState;

fn worker_id(id: &str) -> goble_core::worker::WorkerId {
    goble_core::worker::WorkerId(id.to_string())
}

fn map_tab_to_page(tab: SettingsTab) -> SettingsPage {
    match tab {
        SettingsTab::General => SettingsPage::Profile,
        SettingsTab::Appearance => SettingsPage::Appearance,
        SettingsTab::Account => SettingsPage::Account,
        SettingsTab::Cluster => SettingsPage::Cluster,
        SettingsTab::Workers => SettingsPage::Workers,
        SettingsTab::Keys => SettingsPage::Keys,
    }
}

fn map_page_to_tab(page: SettingsPage) -> SettingsTab {
    match page {
        SettingsPage::Profile => SettingsTab::General,
        SettingsPage::Llm => SettingsTab::General,
        SettingsPage::Appearance => SettingsTab::Appearance,
        SettingsPage::Account => SettingsTab::Account,
        SettingsPage::Cluster => SettingsTab::Cluster,
        SettingsPage::Workers => SettingsTab::Workers,
        SettingsPage::Keys => SettingsTab::Keys,
    }
}

pub struct SettingsViewPanel {
    content: Box<dyn Element>,
}

impl SettingsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<UiState>>,
        shell_state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        tab: SettingsTab,
        app: &AppContext,
        app_context: Rc<RefCell<AppContext>>,
    ) -> Self {
        let page = map_tab_to_page(tab);
        let padding = app.theme.spacing_px(SpacingToken::Md);

        let profile = state.thread_store().get_profile();
        let (name, email) = profile
            .as_ref()
            .map(|p| (p.name.clone(), p.email.clone()))
            .unwrap_or_else(|| (String::new(), String::new()));

        let llm = state.get_llm_setting("openai");
        let llm_provider = "openai".to_string();
        let model = llm.as_ref().map(|s| s.model.clone()).unwrap_or_default();
        let api_key = llm.as_ref().map(|s| s.api_key.clone()).unwrap_or_default();
        let base_url = llm
            .as_ref()
            .and_then(|s| s.base_url.clone())
            .unwrap_or_default();
        let temperature = llm
            .as_ref()
            .and_then(|s| s.temperature)
            .map(|t| t.to_string())
            .unwrap_or_default();

        let workers: Vec<(String, String, String, bool)> = state
            .list_workers()
            .into_iter()
            .map(|w| (w.id, w.name, w.url, w.paired))
            .collect();

        let authorized_keys: Vec<(String, String, String)> = state
            .thread_store()
            .list_authorized_keys()
            .into_iter()
            .map(|k| (k.id, k.name, k.fingerprint))
            .collect();

        let state_for_profile = Arc::clone(&state);
        let state_for_llm = Arc::clone(&state);
        let state_for_vault_unlock = Arc::clone(&state);
        let state_for_vault_add = Arc::clone(&state);
        let state_for_cluster_create = Arc::clone(&state);
        let state_for_cluster_unlock = Arc::clone(&state);
        let state_for_add_worker = Arc::clone(&state);
        let state_for_pair_worker = Arc::clone(&state);
        let state_for_remove_worker = Arc::clone(&state);
        let state_for_add_key = Arc::clone(&state);
        let state_for_remove_key = Arc::clone(&state);

        let shell_state_for_nav = Rc::clone(&shell_state);
        let dirty_for_nav = Rc::clone(&dirty);

        let dark_mode = ui_state.borrow().dark_mode;
        let vault_unlocked = state.is_vault_unlocked();
        let vault_secrets = state.list_vault_secrets().into_iter().map(|s| s.key).collect();
        let cluster_configured = state.has_stored_cluster_identity();
        let cluster_name = state
            .get_cluster_identity()
            .map(|i| i.cluster_name)
            .unwrap_or_default();

        let ui_state_for_dark = Rc::clone(&ui_state);
        let app_context_for_dark = Rc::clone(&app_context);
        let dirty_for_dark = Rc::clone(&dirty);

        let dirty_for_profile = Rc::clone(&dirty);
        let dirty_for_llm = Rc::clone(&dirty);
        let dirty_for_vault_unlock = Rc::clone(&dirty);
        let dirty_for_vault_add = Rc::clone(&dirty);
        let dirty_for_cluster_create = Rc::clone(&dirty);
        let dirty_for_cluster_unlock = Rc::clone(&dirty);
        let dirty_for_add_worker = Rc::clone(&dirty);
        let dirty_for_pair_worker = Rc::clone(&dirty);
        let dirty_for_remove_worker = Rc::clone(&dirty);
        let dirty_for_add_key = Rc::clone(&dirty);
        let dirty_for_remove_key = Rc::clone(&dirty);

        let settings = SettingsView::new(page)
            .with_profile(name, email)
            .with_llm(&llm_provider, &model, &api_key, &base_url, &temperature)
            .with_dark_mode(dark_mode)
            .with_workers(workers)
            .with_authorized_keys(authorized_keys)
            .with_vault_state(vault_unlocked, vault_secrets)
            .with_cluster_state(cluster_name.clone(), cluster_configured)
            .with_on_navigate(move |page| {
                shell_state_for_nav.borrow_mut().active_view =
                    ActiveView::Settings(map_page_to_tab(page));
                *dirty_for_nav.borrow_mut() = true;
            })
            .with_on_save_profile(move |name, email| {
                let store = state_for_profile.thread_store();
                let mut updated = store.get_profile().unwrap_or_else(|| goble_core::user::UserProfile {
                    id: goble_core::principal::PrincipalId::generate(),
                    name: String::new(),
                    email: String::new(),
                    avatar_url: None,
                    public_key_pem: None,
                });
                updated.name = name;
                updated.email = email;
                if let Err(e) = store.set_profile(updated) {
                    log::error!("failed to save profile: {}", e);
                }
                *dirty_for_profile.borrow_mut() = true;
            })
            .with_on_save_llm(move |provider, model, api_key, base_url, temperature| {
                let temperature = temperature.parse::<f32>().ok();
                let base_url = if base_url.is_empty() { None } else { Some(base_url.as_str()) };
                if let Err(e) = state_for_llm.set_llm_setting(
                    &provider,
                    &api_key,
                    base_url,
                    &model,
                    temperature,
                ) {
                    log::error!("failed to save llm setting: {}", e);
                }
                *dirty_for_llm.borrow_mut() = true;
            })
            .with_on_add_worker(move |name, url| {
                let id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = state_for_add_worker.add_worker(
                    worker_id(&id),
                    name,
                    url,
                ) {
                    log::error!("failed to add worker: {}", e);
                }
                *dirty_for_add_worker.borrow_mut() = true;
            })
            .with_on_pair_worker(move |id, code| {
                if let Err(e) = Arc::clone(&state_for_pair_worker)
                    .pair_worker(&worker_id(&id), code)
                {
                    log::error!("failed to pair worker: {}", e);
                }
                *dirty_for_pair_worker.borrow_mut() = true;
            })
            .with_on_remove_worker(move |id| {
                state_for_remove_worker.remove_worker(&worker_id(&id));
                *dirty_for_remove_worker.borrow_mut() = true;
            })
            .with_on_add_authorized_key(move |name, pem, fp| {
                let id = uuid::Uuid::new_v4().to_string();
                let key = goble_core::user::AuthorizedKey::new(&id, name, &pem, &fp);
                if let Err(e) = state_for_add_key.thread_store().add_authorized_key(key) {
                    log::error!("failed to add authorized key: {}", e);
                }
                *dirty_for_add_key.borrow_mut() = true;
            })
            .with_on_remove_authorized_key(move |id| {
                state_for_remove_key.thread_store().remove_authorized_key(&id);
                *dirty_for_remove_key.borrow_mut() = true;
            })
            .with_on_toggle_dark_mode(move |enabled| {
                ui_state_for_dark.borrow_mut().dark_mode = enabled;
                app_context_for_dark.borrow_mut().theme =
                    if enabled { Theme::dark() } else { Theme::light() };
                *dirty_for_dark.borrow_mut() = true;
            })
            .with_on_unlock_vault(move |passphrase| {
                if let Err(e) = state_for_vault_unlock.unlock_vault(passphrase) {
                    log::error!("failed to unlock vault: {}", e);
                }
                *dirty_for_vault_unlock.borrow_mut() = true;
            })
            .with_on_add_vault_secret(move |key, value| {
                if let Err(e) = state_for_vault_add.set_vault_secret(&key, &value) {
                    log::error!("failed to add vault secret: {}", e);
                }
                *dirty_for_vault_add.borrow_mut() = true;
            })
            .with_on_create_cluster(move |name, passphrase| {
                if let Err(e) = state_for_cluster_create.create_cluster(&name, &passphrase) {
                    log::error!("failed to create cluster: {}", e);
                }
                *dirty_for_cluster_create.borrow_mut() = true;
            })
            .with_on_unlock_cluster(move |passphrase| {
                if let Err(e) = state_for_cluster_unlock.unlock_cluster_identity(&passphrase) {
                    log::error!("failed to unlock cluster identity: {}", e);
                }
                *dirty_for_cluster_unlock.borrow_mut() = true;
            });

        let content = Container::new(settings.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(goble_ui::elements::EdgeInsets::uniform(padding))
            .finish();

        Self { content }
    }
}

impl Element for SettingsViewPanel {
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
