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
                vec2f(1182.0, 667.0),
                false,
                SettingsCategory::General,
            ),
            (
                "spotlight-scope",
                vec2f(1182.0, 667.0),
                false,
                SettingsCategory::General,
            ),
            (
                "spotlight-options",
                vec2f(1182.0, 667.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-files",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "sidebar-no-files",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-question",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-add-tab",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "files-explorer",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-task-output",
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
                "sidebar-collapsed",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "tool-inline",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "side-tabs-reordered",
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
                "composer-narrow",
                vec2f(820.0, 900.0),
                false,
                SettingsCategory::General,
            ),
            (
                "composer-model-open",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "composer-permissions-open",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "sidebar-no-conversations",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "sidebar-conversations",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "sidebar-conversations-menu",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "composer-project-menu",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "composer-project-typed",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "chat-scroll-rail-hover",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "chat-no-model",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "automations-popover",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "automations-empty",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "settings-select-open",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::General,
            ),
            (
                "settings-integrations",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Providers,
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
                "settings-long",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Advanced,
            ),
            (
                "settings-shortcuts-record",
                vec2f(1600.0, 1000.0),
                true,
                SettingsCategory::Shortcuts,
            ),
            (
                "side-tab-new",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
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
            (
                "stacked-split",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "floating-file",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "floating-file-left",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "floating-file-right",
                vec2f(1600.0, 1000.0),
                false,
                SettingsCategory::General,
            ),
            (
                "floating-file-small",
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
                    if !name.ends_with("narrow") {
                        // The reference's wrapped inline diff.
                        s.toggle_diff_wrap(pane);
                    }
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
                if name == "stacked-split" {
                    let space_index = s.active_space;
                    let pane = s.active_pane_id;
                    let mut next = s.next_pane_id;
                    if let Some(space) = s.spaces.get_mut(space_index) {
                        if let Some(second) = space.split_with_kind(
                            pane,
                            goble_app::ui::SplitDir::Vertical,
                            &mut next,
                            goble_app::state::PaneKind::Terminal,
                        ) {
                            s.next_pane_id = next;
                            s.terminal.borrow_mut().sessions.insert(
                                second,
                                goble_app::terminal::TerminalSession::preview_screen(
                                    "user@goble ~ % echo lower\r\nlower\r\nuser@goble ~ % ",
                                ),
                            );
                        }
                    }
                }
                if name == "floating-file-right" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Overview];
                    s.active_side_tab = 0;
                }
                if name.starts_with("floating-file") {
                    let file = repo_path.join("src/lib.rs");
                    let file = file.to_string_lossy().to_string();
                    if s.open_file_in_floating(file, None) {
                        let id = s.active_pane_id;
                        let rect = if name == "floating-file-small" {
                            goble_app::ui::PaneRect::new(0.18, 0.22, 0.34, 0.38)
                        } else if name == "floating-file-left" {
                            goble_app::ui::PaneRect::new(0.01, 0.14, 0.34, 0.58)
                        } else if name == "floating-file-right" {
                            goble_app::ui::PaneRect::new(0.63, 0.14, 0.34, 0.58)
                        } else {
                            goble_app::ui::PaneRect::new(0.16, 0.18, 0.46, 0.50)
                        };
                        let space_index = s.active_space;
                        if let Some(space) = s.spaces.get_mut(space_index) {
                            space.set_floating_rect(id, rect);
                        }
                    }
                }
                if name == "side-panel" {
                    // Empty panel: the Start surface, no chip, with "+".
                    s.right_sidebar_open = true;
                    s.side_tabs = Vec::new();
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
                            "user@goble ~ % cargo test -p goble-app --lib\r\n   Compiling goble-app\r\nuser@goble ~ % ",
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
                            "user@goble ~ % git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\nuser@goble ~ % ",
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
                            "user@goble ~ % git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\nuser@goble ~ % ",
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
                    // The reference has exactly two conversations in the
                    // spotlight fixture; the shared seed carries a third plus
                    // an archived row, so trim the preview data to match the
                    // shot while leaving every other scene untouched.
                    s.conversations
                        .retain(|entry| entry.id == "spot-1" || entry.id == "spot-2");
                    s.spotlight.open = true;
                    // The reference menus were opened from the scope pill /
                    // options button, which leaves the query line unfocused;
                    // the caret is not in the shot.
                    s.spotlight.focused =
                        !(name == "spotlight-scope" || name == "spotlight-options");
                    if name == "spotlight-scope" {
                        *s.spotlight.scope_menu_open.borrow_mut() = true;
                    }
                    if name == "spotlight-options" {
                        *s.spotlight.options_menu_open.borrow_mut() = true;
                    }
                }
                if name == "side-tabs-reordered" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Files,
                        goble_app::state::SideTabKind::Agent,
                        goble_app::state::SideTabKind::Terminal,
                    ];
                    s.active_side_tab = 0;
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
                if name == "side-files" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Files];
                    s.active_side_tab = 0;
                    s.side_file = Some(repo_path.join("src/lib.rs").to_string_lossy().to_string());
                }
                if name == "sidebar-no-files" {
                    s.right_sidebar_open = false;
                    s.sidebar_visible = true;
                }
                if name == "side-agent" || name == "side-question" {
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
                            "user@goble ~ % git status\r\nOn branch feat/desktop-redesign\r\nnothing to commit\r\nuser@goble ~ % ",
                        ),
                    );
                }
                if name == "composer-narrow" {
                    s.right_sidebar_open = true;
                    s.side_split_ratio = 0.48;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Overview];
                    s.active_side_tab = 0;
                }
                if name == "composer-model-open" {
                    use goble_desktop_service::ModelGroup;
                    use goble_harness_types::HarnessModel;
                    s.model_groups = vec![
                        ModelGroup {
                            harness: "anthropic".to_string(),
                            label: "Anthropic".to_string(),
                            models: vec![
                                HarnessModel::new("claude-opus-4", "Claude Opus 4").with_thinking(
                                    vec!["low".into(), "medium".into(), "high".into()],
                                ),
                                HarnessModel::new("claude-sonnet-4", "Claude Sonnet 4")
                                    .with_thinking(vec![
                                        "low".into(),
                                        "medium".into(),
                                        "high".into(),
                                    ]),
                            ],
                        },
                        ModelGroup {
                            harness: "openai".to_string(),
                            label: "OpenAI".to_string(),
                            models: vec![HarnessModel::new("gpt-5", "GPT-5").with_thinking(vec![
                                "minimal".into(),
                                "low".into(),
                                "high".into(),
                            ])],
                        },
                        ModelGroup {
                            harness: "google".to_string(),
                            label: "Google".to_string(),
                            models: vec![HarnessModel::new("gemini-2.5-pro", "Gemini 2.5 Pro")],
                        },
                    ];
                    let pane = s.active_pane_id;
                    s.selected_model = "claude-sonnet-4".to_string();
                    let controls = s.pane_controls_mut(pane);
                    controls.model = "claude-sonnet-4".to_string();
                    controls.thinking = "high".to_string();
                    *controls.model_menu_open.borrow_mut() = true;
                    *controls.model_tray_open.borrow_mut() = true;
                }
                if name == "composer-permissions-open" {
                    let pane = s.active_pane_id;
                    *s.pane_controls_mut(pane).approval_menu_open.borrow_mut() = true;
                }
                if name == "settings-shortcuts-record" {
                    s.settings.shortcuts_recording = Some("Command palette".to_string());
                    s.settings.shortcuts_recorded = vec!["⌘".to_string(), "K".to_string()];
                }
                if name == "side-tab-new" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![
                        goble_app::state::SideTabKind::Overview,
                        goble_app::state::SideTabKind::NewTab,
                    ];
                    s.active_side_tab = 1;
                }
                if name == "settings-select-open" {
                    let (open, _) = s.settings.select_menu("app.approval_mode");
                    *open.borrow_mut() = true;
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
                    s.global_search_focused = true;
                    let active = s.active_pane_id;
                    let runtime = s.pane_runtime.entry(active).or_default();
                    for (index, command) in [
                        "wait for GHCR build to finish",
                        "watch Release v1.1.0 run",
                        "watch CI run",
                        "watch Release workflow run 44804857294",
                        "watch background job 5",
                        "watch background job 6",
                        "watch background job 7",
                        "watch background job 8",
                        "watch background job 9",
                        "watch background job 10",
                        "watch background job 11",
                        "watch background job 12",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let id = format!("preview-task-{index:02}");
                        runtime.in_flight_tools.insert(
                            id.clone(),
                            goble_ui::ToolCall {
                                id,
                                name: "Shell".to_string(),
                                arguments: format!("{{\"command\":\"{command}\"}}"),
                                status: goble_core::harness::ToolCallStatus::Running,
                                result: None,
                                diff: None,
                            },
                        );
                    }
                }
                if name == "tool-inline" {
                    let mut message = ChatMessage::new(
                        ChatRole::Assistant,
                        vec![ChatFragment::text("Let me inspect the change.")],
                    );
                    message.tool_calls.push(goble_ui::ToolCall {
                        id: "preview-tool-inline".into(),
                        name: "Read".into(),
                        arguments: r#"{"path":"src/lib.rs"}"#.into(),
                        status: goble_core::harness::ToolCallStatus::Finished,
                        result: Some("pub fn build() {}".into()),
                        diff: None,
                    });
                    let active = s.active_pane_id;
                    s.pane_runtime
                        .entry(active)
                        .or_default()
                        .messages
                        .push(message);
                }
                if name == "chat-no-model" {
                    s.show_llm_key_banner = true;
                    s.settings_llm_api_key = "preview-key".to_string();
                    s.settings_llm_provider.clear();
                }
                if name == "automations-popover" {
                    s.automations_open = true;
                    s.crons = vec![
                        goble_app::ui::CronEntry::new(
                            "preview-nightly",
                            "Nightly dependency audit",
                            "Every day at 02:00",
                            "2h ago",
                        )
                        .with_enabled(true),
                        goble_app::ui::CronEntry::new(
                            "preview-release",
                            "Release checklist",
                            "Every Friday at 16:00",
                            "3d ago",
                        ),
                    ];
                }
                if name == "automations-empty" {
                    s.automations_open = true;
                    s.crons.clear();
                }
                if name == "sidebar-no-conversations" {
                    s.conversations.clear();
                }
                if name == "sidebar-conversations"
                    || name == "sidebar-conversations-menu"
                    || name == "composer-project-menu"
                {
                    s.conversations = vec![ConversationEntry::new(
                        "loose-preview",
                        "Loose conversation",
                        "A conversation outside any project",
                        "10:42",
                    )
                    .with_folder("Conversations")];
                }
                if name == "sidebar-conversations-menu" {
                    *s.conversations_add_menu_open.borrow_mut() = true;
                }
                if name == "composer-project-menu" || name == "composer-project-typed" {
                    s.set_active_pane_path(String::new());
                    let active = s.active_pane_id;
                    let runtime = s.pane_runtime.entry(active).or_default();
                    runtime.messages.clear();
                    runtime.pending_ask = None;
                    runtime.pending_command = None;
                    if name == "composer-project-menu" {
                        s.composer_draft.clear();
                        *s.project_menu_open.borrow_mut() = true;
                    } else {
                        s.composer_draft = "Hello".to_string();
                        s.pane_sessions.entry(active).or_default().draft = "Hello".to_string();
                    }
                }
                if name == "sidebar-collapsed" {
                    s.sidebar_visible = false;
                    s.right_sidebar_open = true;
                    s.side_split_ratio = 0.42;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Terminal];
                    s.active_side_tab = 0;
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
                if name == "side-add-tab" {
                    s.right_sidebar_open = true;
                    *s.side_tab_add_menu_open.borrow_mut() = true;
                }
                if name == "files-explorer" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![goble_app::state::SideTabKind::Files];
                    s.active_side_tab = 0;
                    s.side_file = Some(repo_path.join("src/lib.rs").to_string_lossy().to_string());
                    let space_index = s.active_space;
                    let pane = s.active_pane_id;
                    let mut next = s.next_pane_id;
                    if let Some(space) = s.spaces.get_mut(space_index) {
                        if let Some(id) = space.split_with_kind(
                            pane,
                            goble_app::ui::SplitDir::Horizontal,
                            &mut next,
                            goble_app::state::PaneKind::Explorer,
                        ) {
                            s.next_pane_id = next;
                            s.focus_pane(id);
                            s.pane_sessions.entry(id).or_default().path =
                                repo_path.to_string_lossy().to_string();
                        }
                    }
                }
                if name == "side-task-output" {
                    s.right_sidebar_open = true;
                    s.side_tabs = vec![goble_app::state::SideTabKind::TaskOutput];
                    s.active_side_tab = 0;
                }
                if name == "side-terminal" || name == "side-task-output" {
                    let active = s.active_pane_id;
                    let runtime = s.pane_runtime.entry(active).or_default();
                    for (index, command) in [
                        "job finished build",
                        "job failed tests",
                        "job running deploy",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let id = format!("rail-task-{index:02}");
                        runtime.in_flight_tools.insert(
                            id.clone(),
                            goble_ui::ToolCall {
                                id,
                                name: "Shell".to_string(),
                                arguments: format!("{{\"command\":\"{command}\"}}"),
                                status: goble_core::harness::ToolCallStatus::Running,
                                result: None,
                                diff: None,
                            },
                        );
                    }
                }
                if name == "terminal-dock" {
                    let pane = s.active_pane_id;
                    if s.toggle_terminal_dock(pane) {
                        s.set_terminal_dock_height(pane, 260.0);
                        s.terminal.borrow_mut().sessions.insert(
                            pane,
                            goble_app::terminal::TerminalSession::preview_screen(
                                "user@goble ~ % echo tty-ok\r\ntty-ok\r\nuser@goble ~ % ",
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
            if name == "chat-scroll-rail-hover" {
                let _ = raster::render_element_to_png_at(
                    &assets,
                    &app,
                    &mut root,
                    size.x,
                    size.y,
                    1.0,
                    vec2f(270.0, 500.0),
                    None,
                    &path,
                );
            }
            if name == "settings-wide" || name == "settings-long" {
                let _ = raster::render_element_to_png_at(
                    &assets,
                    &app,
                    &mut root,
                    size.x,
                    size.y,
                    1.0,
                    vec2f(60.0, 300.0),
                    None,
                    &path,
                );
            }
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
            if name == "overview" {
                // Z: capture the Browser row's dark hover band, as in the
                // reference launcher.
                let commands = raster::render_commands(&mut root, size, &app);
                if let Some(origin) = commands.iter().find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. } if text == "Browser" => {
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
                        origin + vec2f(20.0, 4.0),
                        None,
                        &path,
                    );
                }
            }
            if name.starts_with("floating-file") {
                let _ = raster::render_element_to_png_at(
                    &assets,
                    &app,
                    &mut root,
                    size.x,
                    size.y,
                    1.0,
                    vec2f(1082.0, 678.0),
                    None,
                    &path,
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
