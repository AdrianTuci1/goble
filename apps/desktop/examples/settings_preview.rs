//! Headless raster previews of the settings tab: every page of the DSH IA,
//! rendered from a real temp `config.toml` through the real app tree.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real `RootView` over an in-memory store whose config path
//! is pinned to a temp directory holding a seeded `config.toml`,
//! `environment.toml` and `hosts.toml`, opens the settings tab, points it at
//! each page and rasterizes the window. It is the visual verification tool for
//! the settings rework; the PNGs are inspected against the T3/DSH parity target.
//!
//! Usage: cargo run -p goble-app --example settings_preview
//! Output: /tmp/goble-work/out/settings-<page>.png

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_app::ui::SettingsCategory;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use goble_ui::geometry::Vector2F;
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::vec2f;

const OUT_DIR: &str = "/tmp/goble-work/out";
const SCALE: f32 = 2.0;

const CONFIG: &str = r#"# goble configuration, in grok's own layout: one [model.<slug>] table per model,
# the model new sessions start on under [models] default, and MCP servers under
# [mcp_servers.<name>].

[app]
friendly_name = "Ada"
telemetry = true

[theme]
dark = true
preset = "goble-night"

[models]
default = "deepseek"

[model.deepseek]
model = "deepseek-flash"
name = "DeepSeek V4 Flash"
base_url = "https://api.deepseek.com"
api_key = "sk-preview"

[model.local]
model = "qwen3"
name = "Local Qwen"
base_url = "http://localhost:8080/v1"

[mcp_servers.penpot]
url = "http://localhost:4400/mcp"
enabled = true

[remote]
mode = "gateway"
gateway_url = "https://gateway.example.com"
"#;

const ENVIRONMENT: &str = r#"# goble environment groups.

[groups.production]
LOG_LEVEL = "info"
API_BASE = "https://api.example.com"
"#;

const HOSTS: &str = r#"# saved hosts.

[[hosts]]
name = "staging"
host = "staging.example.com"
user = "deploy"
port = 22
"#;

fn seed(dir: &Path) -> anyhow::Result<PathBuf> {
    let config = dir.join("config.toml");
    std::fs::write(&config, CONFIG)?;
    std::fs::write(dir.join("environment.toml"), ENVIRONMENT)?;
    std::fs::write(dir.join("hosts.toml"), HOSTS)?;
    Ok(config)
}

fn paint_frame(
    app: &AppContext,
    root: &mut Box<dyn Element>,
    size: Vector2F,
) -> Vec<RenderCommand> {
    let _ = root.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// Run one pass so the root writes its live theme into `app_context`, then
/// return a snapshot of the context that pass produced. The root applies the
/// theme while it lays out, so a context cloned before that pass would paint
/// the previous theme.
fn prepare(
    root: &mut Box<dyn Element>,
    app_context: &Rc<RefCell<AppContext>>,
    size: Vector2F,
) -> AppContext {
    // Take the snapshot out of the RefCell first: a guard held across the
    // frame would collide with the root's own borrow while it applies the
    // theme.
    let snapshot = app_context.borrow().clone();
    let _ = paint_frame(&snapshot, root, size);
    app_context.borrow().clone()
}

fn main() -> anyhow::Result<()> {
    std::fs::create_dir_all(OUT_DIR)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;
    let dir = tempfile::tempdir()?;
    let config = seed(dir.path())?;
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory()?,
        ThreadStore::new(dir.path())?,
    ));
    desktop.set_config_path(config);
    let size = vec2f(1440.0, 900.0);

    for page in SettingsCategory::ALL {
        // A fresh AppContext per page: the root writes the live theme into it
        // each frame, so the light pass actually reaches the elements.
        let app_context = Rc::new(RefCell::new(AppContext::default()));
        let view = RootView::new(&app_context.borrow(), &desktop, None)
            .with_app_context(Rc::clone(&app_context));
        let state: Rc<RefCell<UiState>> = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.sidebar_visible = false;
            s.crons_open = false;
            s.open_settings_tab(Some(&desktop));
            s.settings_set_page(*page);
            s.settings.region = goble_app::ui::settings::SettingsRegion::Rail;
            s.refresh_settings(&desktop);
        }
        let mut root: Box<dyn Element> = Box::new(view);
        // One frame settles the pane sizes, the snapshot and the live theme;
        // the second paints.
        let app = prepare(&mut root, &app_context, size);
        let commands = paint_frame(&app, &mut root, size);
        let texts = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::DrawText { .. }))
            .count();
        let strokes = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::StrokeRect { .. }))
            .count();
        let out = Path::new(OUT_DIR).join(format!("settings-{:?}.png", page).to_lowercase());
        raster::render_element_to_png(&assets, &app, &mut root, size.x, size.y, SCALE, None, &out)?;
        println!(
            "{} · {} text runs · {} strokes · {}",
            page.label(),
            texts,
            strokes,
            out.display()
        );
    }
    // One light-theme pass over three pages: the elements resolve every colour
    // through the theme, so the light scheme is the proof the tokens are real
    // and not baked into a palette.
    for page in [
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Providers,
    ] {
        // A fresh AppContext per page: the root writes the live theme into it
        // each frame, so the light pass actually reaches the elements.
        let app_context = Rc::new(RefCell::new(AppContext::default()));
        let view = RootView::new(&app_context.borrow(), &desktop, None)
            .with_app_context(Rc::clone(&app_context));
        let state: Rc<RefCell<UiState>> = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.sidebar_visible = false;
            s.open_settings_tab(Some(&desktop));
            s.settings_set_page(page);
            s.settings.region = goble_app::ui::settings::SettingsRegion::Rail;
            s.refresh_settings(&desktop);
            // After the refresh, so the light choice is the one rendered: the
            // refresh re-reads the dark base from the seeded config.
            s.settings_dark_mode = false;
            s.theme_preset = Some("goble-day".to_string());
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let app = prepare(&mut root, &app_context, size);
        let _ = paint_frame(&app, &mut root, size);
        let out = Path::new(OUT_DIR).join(format!("settings-light-{:?}.png", page).to_lowercase());
        raster::render_element_to_png(&assets, &app, &mut root, size.x, size.y, SCALE, None, &out)?;
        println!("light {} · {}", page.label(), out.display());
    }
    Ok(())
}
