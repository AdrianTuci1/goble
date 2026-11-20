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
use goble_ui::geometry::{rectf, RectF};
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
    state.chat_messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Make the shell DSH-like.")]),
        ChatMessage::from_markdown(
            ChatRole::Assistant,
            "## Shell\n\nThe global bar is gone. The panel owns the top edge.\n\n- Sidebar first\n- Panel title\n- Settings overlay\n",
        ),
    ];
    state.refresh_settings(desktop);
}

/// The painted extent of one command, used to decide which commands a menu's
/// surface owns. Commands that only change clip state have no extent of their
/// own and are treated as part of the run they sit in.
fn command_bounds(command: &RenderCommand) -> Option<RectF> {
    match command {
        RenderCommand::FillRect { rect, .. }
        | RenderCommand::FillRectFadeRight { rect, .. }
        | RenderCommand::StrokeRect { rect, .. }
        | RenderCommand::DrawImage { rect, .. }
        | RenderCommand::ClipRect(rect) => Some(*rect),
        RenderCommand::PopClip => None,
        RenderCommand::DrawText {
            origin,
            text,
            font_size,
            max_width,
            line_height,
            ..
        } => {
            // `max_width` is the layout budget, not the glyph extent: the
            // shortcut cells pass a budget wider than the panel, so using it
            // would end the menu's run inside its first row. Estimate the
            // painted width instead, capped by the budget when one is set.
            let budget = if max_width.is_finite() && *max_width > 0.0 {
                *max_width
            } else {
                f32::INFINITY
            };
            let width = (font_size * 0.7 * text.chars().count() as f32).min(budget);
            let height = font_size * line_height.max(1.0);
            Some(rectf(origin.x, origin.y, width.max(1.0), height.max(1.0)))
        }
        RenderCommand::DrawIcon { origin, size, .. } => {
            Some(rectf(origin.x, origin.y, *size, *size))
        }
    }
}

/// Paints the open More menu above the chrome/body that otherwise cover it.
///
/// A popup's panel is painted in tree order, inside the topbar, so later
/// commands (the pane body, the sidebar, the window border) paint over it. The
/// open-menu registry already knows the rectangle the menu's own last-layer
/// paint reported; collect the contiguous run of commands painted inside that
/// surface, starting after the trigger, and move just that run to the end of
/// the command list before rasterizing. Everything else keeps its paint order,
/// so only the menu is lifted.
fn lift_open_menu_commands(
    commands: Vec<RenderCommand>,
    surfaces: &[RectF],
    trigger: Option<usize>,
) -> Vec<RenderCommand> {
    if surfaces.is_empty() {
        return commands;
    }
    let inside = |command: &RenderCommand| match command_bounds(command) {
        Some(bounds) => surfaces
            .iter()
            .any(|surface| surface.contains_rect(&bounds)),
        None => true,
    };

    // The menu's commands are a contiguous run; the popup paints its panel
    // immediately after its trigger. Prefer that run, falling back to the
    // longest run if the trigger index moved.
    let mut runs = Vec::new();
    let mut index = 0;
    while index < commands.len() {
        if inside(&commands[index]) {
            let start = index;
            while index < commands.len() && inside(&commands[index]) {
                index += 1;
            }
            if index - start >= 2 {
                runs.push((start, index));
            }
        } else {
            index += 1;
        }
    }
    let run = trigger
        .and_then(|trigger| runs.iter().copied().find(|(start, _)| *start > trigger))
        .or_else(|| runs.iter().copied().max_by_key(|(start, end)| end - start));
    let Some((start, end)) = run else {
        return commands;
    };

    let mut lifted = Vec::with_capacity(commands.len());
    lifted.extend_from_slice(&commands[..start]);
    lifted.extend_from_slice(&commands[end..]);
    lifted.extend_from_slice(&commands[start..end]);
    lifted
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
            ("side-panel-max", vec2f(1600.0, 1000.0), false, SettingsCategory::General),
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
                if name == "side-panel" {
                    s.right_sidebar_open = true;
                }
                if name == "overview" {
                    s.right_sidebar_open = true;
                }
                if name == "side-panel-max" {
                    s.right_sidebar_open = true;
                    s.side_panel_maximized = true;
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
                let commands = raster::render_commands(&mut root, size, &app);
                let surfaces = app.open_menus.surfaces();
                let commands = lift_open_menu_commands(commands, &surfaces, trigger);
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
