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
use goble_ui::ConversationEntry;
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
    state.conversations = vec![
        ConversationEntry::new(
            "spot-1",
            "Logic Pro MCP Control and Deep House",
            "Let's wire the transport controls.",
            "2d",
        )
        .with_folder("Music"),
        ConversationEntry::new(
            "spot-2",
            "Grammar Check For GitHub Description",
            "Here is a tighter description.",
            "2d",
        )
        .with_folder("Writing"),
        ConversationEntry::new(
            "spot-3",
            "Automate the release notes",
            "The scheduled task is ready.",
            "5d",
        )
        .with_folder("Automations"),
        ConversationEntry::new("spot-4", "Old archived thread", "Archived work.", "3w")
            .with_archived(true),
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
    // A deterministic git working tree for the split Review previews: one
    // committed file changed on disk, so the panel shows real git output.
    let review_repo = tempfile::tempdir()?;
    let repo_path = review_repo.path().to_path_buf();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo_path)
        .status()?;
    std::process::Command::new("git")
        .args(["config", "user.email", "preview@goble.local"])
        .current_dir(&repo_path)
        .status()?;
    std::process::Command::new("git")
        .args(["config", "user.name", "Goble Preview"])
        .current_dir(&repo_path)
        .status()?;
    std::fs::create_dir_all(repo_path.join("src"))?;
    std::fs::write(
        repo_path.join("src/lib.rs"),
        "pub fn build() -> usize {\n    1\n}\n\npub fn layout() {\n    // old\n}\n",
    )?;
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .status()?;
    std::process::Command::new("git")
        .args(["commit", "-qm", "seed"])
        .current_dir(&repo_path)
        .status()?;
    std::fs::write(
        repo_path.join("src/lib.rs"),
        "pub fn build() -> usize {\n    let panes = 2;\n    panes + 1\n}\n\npub fn layout() {\n    // new\n    render();\n}\n",
    )?;
    std::fs::write(repo_path.join("README.md"), "# Preview repo\n")?;
    for (label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (name, size, settings, page) in [
            (
                "shell-wide",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "shell-narrow",
                vec2f(760.0, 900.0),
                false,
                SettingsCategory::General,
            ),
            (
                "panel-more",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "panel-more-split",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "panel-more-copy",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "overview-popover",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-panel",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-panel-resize",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-tabs-multi",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-tabs-overflow",
                vec2f(1200.0, 900.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-tabs-hover",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-tabs-add",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "conversation-title-pill",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "conversation-title-hover",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "conversation-title-edit",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "spotlight",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "spotlight-scope",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "spotlight-options",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-agent",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-terminal",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "overview",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-panel-max",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "terminal-dock",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "settings-wide",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::General,
            ),
            (
                "settings-wide-appearance",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Appearance,
            ),
            (
                "settings-wide-appearance-palette",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Appearance,
            ),
            (
                "settings-wide-shortcuts",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Shortcuts,
            ),
            (
                "settings-narrow",
                vec2f(700.0, 900.0),
                true,
                SettingsCategory::General,
            ),
            (
                "split-review",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "split-review-narrow",
                vec2f(1200.0, 900.0),
                false,
                SettingsCategory::General,
            ),
            (
                "split-file-viewer",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
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
                if name.starts_with("split-") {
                    // Q/O: Review is a tab in panel 2; the main pane keeps its
                    // chat (or a file viewer), and the panel shows exactly one
                    // file rail plus the diff.
                    s.right_sidebar_open = true;
                    s.side_split_ratio = if name.ends_with("narrow") { 0.58 } else { 0.52 };
                    s.side_tabs = vec![goble_app::state::SideTabKind::Review];
                    s.active_side_tab = 0;
                    let pane = s.active_pane_id;
                    let root = repo_path.to_string_lossy().to_string();
                    s.pane_sessions.entry(pane).or_default().path = root.clone();
                    s.side_review = Some((pane, root));
                    s.select_diff_file(pane, None);
                    if name == "split-file-viewer" {
                        let file = repo_path.join("src/lib.rs");
                        let file = file.to_string_lossy().to_string();
                        let _ = s.spaces[0].set_leaf_kind(
                            pane,
                            goble_app::state::PaneKind::File { path: file.clone() },
                        );
                        s.attach_pane_file(pane, file);
                    }
                }
                if name == "side-panel" {
                    // Empty panel: the Overview home surface, no chip and no +.
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Overview];
                    s.active_side_tab = 0;
                }
                if name == "side-panel-resize" {
                    // Mid-drag: a wider side panel and the hairline boundary
                    // between the two flat panels.
                    s.right_sidebar_open = true;
                    s.side_split_ratio = 0.62;
                    s.side_split_dragging = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Terminal,
                    ];
                    s.active_side_tab = 1;
                    let pane = s.active_pane_id;
                    s.terminal.borrow_mut().sessions.insert(
                        pane,
                        goble_app::terminal::TerminalSession::preview_screen(
                            "$ cargo test -p goble-app --lib\r\n   Compiling goble-app\r\n$ ",
                        ),
                    );
                }
                if name == "side-tabs-multi" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Files,
                    ];
                    s.active_side_tab = 1;
                    let pane = s.active_pane_id;
                    s.terminal.borrow_mut().sessions.insert(
                        pane,
                        goble_app::terminal::TerminalSession::preview_screen(
                            "$ git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\n$ ",
                        ),
                    );
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
                    s.active_side_tab = 2;
                    let pane = s.active_pane_id;
                    s.terminal.borrow_mut().sessions.insert(
                        pane,
                        goble_app::terminal::TerminalSession::preview_screen(
                            "$ git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\n$ ",
                        ),
                    );
                }
                if name.starts_with("conversation-title") {
                    s.selected_id = Some("spot-1".to_string());
                    if name == "conversation-title-edit" {
                        s.begin_inline_rename("spot-1");
                    }
                }
                if name.starts_with("spotlight") {
                    s.spotlight.open = true;
                    s.spotlight.focused = true;
                    if name == "spotlight-scope" {
                        *s.spotlight.scope_menu_open.borrow_mut() = true;
                    }
                    if name == "spotlight-options" {
                        *s.spotlight.options_menu_open.borrow_mut() = true;
                    }
                }
                if name == "side-tabs-hover" || name == "side-tabs-add" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Terminal,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Files,
                    ];
                    s.active_side_tab = 0;
                    if name == "side-tabs-add" {
                        *s.side_tab_add_menu_open.borrow_mut() = true;
                    }
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
                        goble_app::state::SideTabKind::Terminal,
                    ];
                    s.active_side_tab = 1;
                    let pane = s.active_pane_id;
                    s.terminal.borrow_mut().sessions.insert(
                        pane,
                        goble_app::terminal::TerminalSession::preview_screen(
                            "$ git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\n$ ",
                        ),
                    );
                }
                if name == "settings-wide-appearance-palette" {
                    // The in-field swatch's palette popover, opened on the
                    // accent channel so the square and the anchored grid are
                    // both in the shot.
                    *s.settings.color_palette_menu.borrow_mut() = Some(0);
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
            if name == "side-tabs-hover" {
                // Paint once with the pointer on the Side Question chip, so the
                // hover close X is in the shot.
                let commands = raster::render_commands(&mut root, size, &app);
                if let Some(origin) = commands.iter().find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text == "Side Question" && origin.y < 44.0 =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                }) {
                    let _ = raster::render_element_to_png_at(
                        &assets,
                        &app,
                        &mut root,
                        size.x,
                        size.y,
                        1.0,
                        origin + vec2f(20.0, 6.0),
                        None,
                        &path,
                    );
                }
            }
            if name == "conversation-title-hover" {
                let commands = raster::render_commands(&mut root, size, &app);
                if let Some(origin) = commands.iter().find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text.starts_with("Logic Pro") && origin.y < 44.0 =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                }) {
                    let _ = raster::render_element_to_png_at(
                        &assets,
                        &app,
                        &mut root,
                        size.x,
                        size.y,
                        1.0,
                        origin + vec2f(16.0, 8.0),
                        None,
                        &path,
                    );
                }
            }
            if name == "side-tabs-add" {
                // The add menu is anchored on its trigger; settle one frame.
                let _ = raster::render_element_to_png(
                    &assets, &app, &mut root, size.x, size.y, 1.0, None, &path,
                );
            }
            if name == "conversation-title-hover" {
                let commands = raster::render_commands(&mut root, size, &app);
                if let Some(origin) = commands.iter().find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text.starts_with("Logic Pro") && origin.y < 44.0 =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                }) {
                    let _ = raster::render_element_to_png_at(
                        &assets,
                        &app,
                        &mut root,
                        size.x,
                        size.y,
                        1.0,
                        origin + vec2f(16.0, 8.0),
                        None,
                        &path,
                    );
                }
            }
            if name == "spotlight-scope" || name == "spotlight-options" {
                // The anchored menus lay out on the first frame and settle on
                // the next.
                let _ = raster::render_element_to_png(
                    &assets, &app, &mut root, size.x, size.y, 1.0, None, &path,
                );
            }
            if name == "settings-wide-appearance-palette" {
                // The palette is anchored to the swatch painted on the first
                // frame; draw the settled frame before the final capture.
                let _ = raster::render_element_to_png(
                    &assets, &app, &mut root, size.x, size.y, 1.0, None, &path,
                );
            }
            if name.starts_with("panel-more") {
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
                        DispatchedEvent::MouseDown {
                            position: trigger,
                            button: 0,
                        },
                        DispatchedEvent::MouseUp {
                            position: trigger,
                            button: 0,
                        },
                    ] {
                        let _ = root.dispatch_event(&event, &mut ctx, &app);
                    }
                }
                // A failed synthetic click still opens the product menu; the
                // point is the frame, not the pointer.
                if !*state.borrow().chrome_more_open.borrow() {
                    *state.borrow().chrome_more_open.borrow_mut() = true;
                }
                // The flyout variants put the keyboard on Split (row 3) or
                // Copy (row 4) in the open menu, then open its flyout with
                // Right — the same keys the app handles.
                if name != "panel-more" {
                    // One frame with the menu open: its rebuild puts the
                    // keyboard on the first row, so the Down count below lands
                    // on Split/Copy rather than just seeding the highlight.
                    let _ = raster::render_commands(&mut root, size, &app);
                    let mut ctx = EventContext::default();
                    let steps = if name == "panel-more-copy" { 4 } else { 3 };
                    for _ in 0..steps {
                        let _ = root.dispatch_event(
                            &DispatchedEvent::KeyDown {
                                key: "ArrowDown".to_string(),
                                modifiers: Default::default(),
                            },
                            &mut ctx,
                            &app,
                        );
                    }
                    let _ = root.dispatch_event(
                        &DispatchedEvent::KeyDown {
                            key: "ArrowRight".to_string(),
                            modifiers: Default::default(),
                        },
                        &mut ctx,
                        &app,
                    );
                }
                // A flyout's left/right placement is decided from the menu's
                // last-painted origin (layout has no window origin), so the
                // first frame with the flyout measures it and the next one
                // places it. Render that settling frame before the final one.
                if name != "panel-more" {
                    let _ = raster::render_commands(&mut root, size, &app);
                }
                // The chrome menus paint themselves in the window's last
                // layer, so the command list is already in the right order:
                // the open panel is after every pane and overlay.
                let commands = raster::render_commands(&mut root, size, &app);
                raster::rasterize_to_png(&assets, &app, &commands, size, 2.0, None, &path)?;
            } else if name != "side-tabs-hover" && name != "conversation-title-hover" {
                // The hover shot already carries its pointer; a final
                // pointer-free pass would overwrite the revealed X.
                raster::render_element_to_png(
                    &assets, &app, &mut root, size.x, size.y, 2.0, None, &path,
                )?;
            }
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
