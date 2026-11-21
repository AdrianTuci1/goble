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
use goble_ui::elements::{AppContext, Element, EventContext};
use goble_ui::event::DispatchedEvent;
use goble_ui::render::RenderCommand;
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
    let messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Make the shell DSH-like.")]),
        ChatMessage::from_markdown(
            ChatRole::Assistant,
            "## Shell\n\nThe global bar is gone. The panel owns the top edge.\n\n- Sidebar first\n- Panel title\n- Settings overlay\n",
        ),
    ];
    state.chat_messages = messages.clone();
    // The pane's own transcript is what the chat leaf draws: keep the mock
    // messages on its runtime so the shell preview shows a real conversation
    // rather than the empty state (no store chat is created here).
    state.pane_runtime.entry(pane).or_default().messages = messages;
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
            ("overview-popover", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("side-panel", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("side-tabs-overflow", vec2f(1200.0, 900.0), false, SettingsCategory::General),
            ("side-agent", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("side-terminal", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("overview", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("side-panel-max", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("terminal-dock", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
            ("settings-wide", vec2f(1600.0, 1000.0), true, SettingsCategory::General),
            ("settings-wide-appearance", vec2f(1600.0, 1000.0), true, SettingsCategory::Appearance),
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
                if name == "side-panel" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Files,
                    ];
                    s.active_side_tab = 0;
                }
                if name == "side-tabs-overflow" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Files,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Files,
                        goble_app::state::SideTabKind::Agent,
                    ];
                    s.active_side_tab = 0;
                }
                if name == "side-agent" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                    ];
                    s.active_side_tab = 1;
                    s.ensure_pane_controls();
                    s.pane_sessions
                        .entry(goble_app::state::SIDE_AGENT_PANE_ID)
                        .or_default()
                        .draft = "Why did the wrap fix drop spaces?".to_string();
                    s.pane_runtime
                        .entry(goble_app::state::SIDE_AGENT_PANE_ID)
                        .or_default()
                        .messages = vec![
                        ChatMessage::new(
                            ChatRole::User,
                            vec![ChatFragment::text("What changed in the diff panel?")],
                        ),
                        ChatMessage::from_markdown(
                            ChatRole::Assistant,
                            "The wrapped runs now come from source prefixes, so the \
                             highlighted rows concatenate back to the source exactly.",
                        ),
                    ];
                }
                if name == "side-terminal" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                    ];
                    s.active_side_tab = 2;
                    let pane = s.active_pane_id;
                    s.terminal.borrow_mut().sessions.insert(
                        pane,
                        goble_app::terminal::TerminalSession::preview_screen(
                            "$ git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\n$ ",
                        ),
                    );
                }
                if name == "overview-popover" {
                    s.right_sidebar_open = true;
                    *s.chrome_overview_open.borrow_mut() = true;
                }
                if name == "overview" {
                    s.right_sidebar_open = true;
                }
                if name == "side-panel-max" {
                    s.right_sidebar_open = true;
                    s.side_panel_maximized = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Files,
                    ];
                    s.active_side_tab = 0;
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
            if name == "panel-more" {
                let closed = raster::render_commands(&mut root, size, &app);
                let trigger = closed.iter().position(|command| {
                    matches!(
                        command,
                        RenderCommand::DrawIcon { name, .. } if name == "ellipsis"
                    )
                });
                if let Some(trigger) = trigger.and_then(|index| match &closed[index] {
                    RenderCommand::DrawIcon { origin, size, .. } => {
                        Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                    }
                    _ => None,
                }) {
                    let mut ctx = EventContext::default();
                    for event in [
                        DispatchedEvent::MouseDown { position: trigger, button: 0 },
                        DispatchedEvent::MouseUp { position: trigger, button: 0 },
                    ] {
                        let _ = root.dispatch_event(&event, &mut ctx, &app);
                    }
                }
                // A failed synthetic click still opens the product menu; the
                // point is the frame, not the pointer.
                if !*state.borrow().chrome_more_open.borrow() {
                    *state.borrow().chrome_more_open.borrow_mut() = true;
                }
                // The chrome menus paint themselves in the window's last
                // layer, so the command list is already in the right order:
                // the open panel is after every pane and overlay.
                let commands = raster::render_commands(&mut root, size, &app);
                raster::rasterize_to_png(
                    &assets, &app, &commands, size, 2.0, None, &path,
                )?;
            } else {
                raster::render_element_to_png(
                    &assets, &app, &mut root, size.x, size.y, 2.0, None, &path,
                )?;
            }
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
