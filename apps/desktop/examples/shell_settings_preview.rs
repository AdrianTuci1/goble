//! Headless previews of the topbar-less shell and the floating Settings sheet.
//!
//! Usage: cargo run -p goble-app --example shell_settings_preview
//! Output: /tmp/goble-work/out/shell-*.png, settings-*.png

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_app::ui::SettingsCategory;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element};
use goble_ui::theme::Theme;
use goble_ui::{vec2f, ChatFragment, ChatMessage, ChatRole};

fn seed(state: &mut UiState, desktop: &DesktopState) {
    state.show_workspace_choice = false;
    state.show_llm_key_banner = false;
    state.show_onboarding_tip = false;
    state.right_sidebar_open = false;
    state.crons_open = false;
    state.sidebar_visible = true;
    state.sidebar_width = 250.0;
    state.add_workspace("/tmp/goble-work/alpha", Some(desktop));
    state.add_workspace("/tmp/goble-work/beta", Some(desktop));
    let pane = state.active_pane_id;
    state.pane_sessions.entry(pane).or_default();
    state.chat_messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Make the shell DSH-like.")]),
        ChatMessage::from_markdown(
            ChatRole::Assistant,
            "## Shell\n\nThe global bar is gone. The panel owns the top edge.\n\n- Sidebar first\n- Panel title\n- Settings overlay\n",
        ),
    ];
    state.refresh_settings(desktop);
}

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from("/tmp/goble-work/out");
    std::fs::create_dir_all(&dir)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;
    let repo = tempfile::tempdir()?;
    for (label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (name, size, settings, page) in [
            ("shell-wide", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("shell-narrow", vec2f(760.0, 900.0), false, SettingsCategory::General),
            ("panel-more", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("side-panel", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("overview", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("terminal-dock", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("settings-wide", vec2f(1600.0, 1000.0), true, SettingsCategory::General),
            ("settings-wide-shortcuts", vec2f(1600.0, 1000.0), true, SettingsCategory::Shortcuts),
            ("settings-narrow", vec2f(700.0, 900.0), true, SettingsCategory::General),
        ] {
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
            {
                let mut s = state.borrow_mut();
                seed(&mut s, &desktop);
                s.settings_overlay_open = settings;
                s.settings.page = page;
                s.settings_overlay_wide = size.x >= 900.0;
                if name == "panel-more" {
                    *s.chrome_more_open.borrow_mut() = true;
                }
                if name == "side-panel" {
                    s.right_sidebar_open = true;
                }
                if name == "overview" {
                    *s.chrome_overview_open.borrow_mut() = true;
                }
                if name == "terminal-dock" {
                    let pane = s.active_pane_id;
                    if s.toggle_terminal_dock(pane) {
                        s.set_terminal_dock_height(pane, 260.0);
                        s.terminal.borrow_mut().sessions.insert(
                            pane,
                            goble_app::terminal::TerminalSession::preview_screen(
                                "$ echo tty-ok\r\ntty-ok\r\n$ ",
                            ),
                        );
                    }
                }
            }
            let mut root: Box<dyn Element> = Box::new(view);
            let path = dir.join(format!("{name}-{label}.png"));
            // Two frames: the snapshot's window size settles on the first.
            let _ = raster::render_element_to_png(
                &assets, &app, &mut root, size.x, size.y, 1.0, None, &path,
            );
            raster::render_element_to_png(
                &assets, &app, &mut root, size.x, size.y, 2.0, None, &path,
            )?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
