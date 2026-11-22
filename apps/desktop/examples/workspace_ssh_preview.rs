//! Headless previews of the Workspaces "+" menu and the "Open SSH Host" overlay
//! over the real app tree, so the two reference screenshots can be compared
//! without a native window.
//!
//! Usage: cargo run -p goble-app --example workspace_ssh_preview
//! Output: /tmp/goble-work/out/workspace-menu-{dark,light}.png and
//!         /tmp/goble-work/out/ssh-host-{dark,light}.png

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element};
use goble_ui::theme::Theme;
use goble_ui::vec2f;

fn fixture_config(dir: &Path) -> PathBuf {
    let path = dir.join("ssh_config");
    std::fs::write(
        &path,
        "Host controlpanel\n    HostName 10.0.0.4\n    User deploy\n    Port 2222\n\n\
         Host worker2\n    HostName worker2.internal\n\n\
         Host myvps\n    HostName vps.example.com\n    User root\n\n\
         Host github.com\n    HostName github.com\n    User git\n\n\
         Host *\n    ServerAliveInterval 30\n\n\
         Host !bad *.wild\n    User ignored\n",
    )
    .unwrap();
    path
}

fn seed(state: &mut UiState, desktop: &DesktopState, config: &Path, mode: &str) {
    state.show_workspace_choice = false;
    state.show_llm_key_banner = false;
    state.show_onboarding_tip = false;
    state.right_sidebar_open = false;
    state.crons_open = false;
    state.conversations_expanded = true;
    state.add_workspace("/tmp/goble-work/alpha", Some(desktop));
    state.add_ssh_workspace("controlpanel", "controlpanel", Some(desktop));
    state.add_ssh_workspace("worker2", "worker2", Some(desktop));
    if mode == "menu" {
        *state.sidebar_add_menu_open.borrow_mut() = true;
    } else {
        state.ssh_config_path = config.to_string_lossy().to_string();
        state.ssh_target_draft = "controlpanel".to_string();
        state.open_ssh_target();
    }
}

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from("/tmp/goble-work/out");
    std::fs::create_dir_all(&out)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;
    let repo = tempfile::tempdir()?;
    let config = fixture_config(repo.path());
    for (label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for mode in ["menu", "ssh"] {
            let app = AppContext {
                theme: theme.clone(),
                ..AppContext::default()
            };
            let desktop = Arc::new(DesktopState::new(
                Store::open_in_memory()?,
                ThreadStore::new(repo.path())?,
            ));
            let view = RootView::new(&app, &desktop, None);
            let state = view.state_rc();
            seed(&mut state.borrow_mut(), &desktop, &config, mode);
            let mut root: Box<dyn Element> = Box::new(view);
            let size = vec2f(1024.0, 768.0);
            let path = out.join(format!("workspace-{mode}-{label}.png"));
            raster::render_element_to_png(
                &assets, &app, &mut root, size.x, size.y, 2.0, None, &path,
            )?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
