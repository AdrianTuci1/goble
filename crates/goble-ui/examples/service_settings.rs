use goble_core::store::Store;
use goble_core::user::UserProfile;
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{AppContext, LayoutContext, SizeConstraint};
use goble_ui::geometry::vec2f;
use goble_ui::{Element, SettingsPage, SettingsView};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let bus = CollectingEventBus::default();
    let state = DesktopState::new(
        Store::open_in_memory()?,
        goble_desktop_service::ThreadStore::new(PathBuf::from("/tmp/goble-ui-settings-example"))?,
    );
    state.set_event_bus(Arc::new(bus.clone()));

    let profile = UserProfile::new(
        goble_core::principal::PrincipalId("u1".to_string()),
        "Ada",
        "ada@example.com",
    );
    state.thread_store().set_profile(profile)?;

    let model = state
        .get_llm_setting("openai")
        .map(|s| s.model)
        .unwrap_or_else(|| "gpt-4o".to_string());

    println!("Loaded profile and LLM defaults.");

    let app = AppContext::default();
    let mut view = SettingsView::new(SettingsPage::Profile)
        .with_profile("Ada", "ada@example.com")
        .with_llm("openai", &model)
        .with_on_save_profile(|name, email| println!("save profile: {} <{}>", name, email))
        .with_on_save_llm(|provider, model| println!("save llm: {} / {}", provider, model))
        .with_on_toggle_dark_mode(|enabled| println!("dark mode: {}", enabled));

    let size = view.layout(
        SizeConstraint::loose(vec2f(800.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );

    println!("SettingsView layout size: {}x{}", size.x, size.y);
    println!("Service events emitted: {}", bus.events().len());

    Ok(())
}
