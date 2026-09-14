//! Smoke tests for the whole app shell: mounting the real [`RootView`] over a
//! live [`DesktopState`] and laying it out + painting it headlessly must not
//! panic and must emit render commands. This exercises the seam where the
//! backend data flows into the app-owned element tree (`build_ui`).

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::media::MediaState;
use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_app::ui::AppTab;
use goble_app::ui::SidebarView;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::AppContext;
use goble_ui::platform::WindowControl;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::{command_counts, render_element, RenderCommandCounts};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, Element, SettingsPage};

fn render(desktop: &Arc<DesktopState>) -> RenderCommandCounts {
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, desktop, None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    command_counts(&commands)
}

/// Render the whole shell with a given first-run flag forced on, to exercise
/// the modal overlay path headlessly (no browser for this native app).
fn render_with_flag(
    desktop: &Arc<DesktopState>,
    set: impl FnOnce(&mut UiState),
) -> RenderCommandCounts {
    let app = AppContext::default();
    let view = RootView::new(&app, desktop, None);
    set(&mut view.state_rc().borrow_mut());
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    command_counts(&commands)
}

#[test]
fn full_app_renders_from_empty_backend() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render(&desktop);
    assert!(counts.fill_rect > 0, "shell should paint backgrounds");
    assert!(counts.draw_text > 0, "shell should paint text");
}

/// R28: the files tab's tree is drawn the reference tool's way. Every row takes
/// one colour for its chevron, its icon and its name — the muted one at rest,
/// the main text colour while the pointer is over that row — the hovered row's
/// band is rounded, and a directory is drawn from the same file icon set as a
/// file (the one folder glyph; the chevron says whether it is open).
///
/// Hover is read from the frame's cursor while painting, so each pointer
/// position costs two frames: the first reports the row under the pointer into
/// the app's own cell, the second draws what that styled.
#[test]
fn the_files_tab_draws_directories_from_the_file_icon_set() {
    use goble_ui::elements::{LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::theme::SpacingToken;
    use goble_ui::{ColorU, RectF, Vector2F};

    /// Lay the shell out and paint it, with the pointer at `cursor` when given.
    fn paint_frame(
        root: &mut Box<dyn Element>,
        cursor: Option<Vector2F>,
        app: &AppContext,
    ) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(position) = cursor {
            ctx.cursor_inside = true;
            ctx.cursor_position = position;
        }
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The row the frame drew for `name`: the origin and colour of its 14 pt
    /// label.
    fn label(commands: &[RenderCommand], name: &str) -> (Vector2F, ColorU) {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    origin,
                    text,
                    font_size,
                    color,
                    ..
                } if text == name && *font_size == 14.0 => Some((*origin, *color)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the tree draws a {name:?} row"))
    }

    /// The `size` pt icon `name` drawn on the row whose label starts at `label`
    /// (a neighbouring row is ~25 pt away, well outside this row), with the
    /// colour it is drawn in.
    fn icon_at(
        commands: &[RenderCommand],
        name: &str,
        size: f32,
        label: Vector2F,
    ) -> (Vector2F, ColorU) {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name: drawn,
                    size: drawn_size,
                    color,
                } if drawn == name && *drawn_size == size && (origin.y - label.y).abs() < 12.0 => {
                    Some((*origin, *color))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the row at {label:?} draws its {name:?} icon"))
    }

    /// The band drawn on the row whose label starts at `label`, if any: a fill
    /// in the hover colour covering that label. The test keys on the row's own
    /// rect because the dark theme's hover grey is `SurfaceRaised` by value.
    fn row_band(
        commands: &[RenderCommand],
        app: &AppContext,
        label: Vector2F,
    ) -> Option<(RectF, f32)> {
        let hover = app.theme.color(ColorToken::Hover);
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == hover
                && rect.min_x() <= label.x
                && rect.max_x() >= label.x
                && rect.min_y() <= label.y
                && rect.max_y() >= label.y =>
            {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
    }

    let (desktop, _dir) = common::desktop_state();
    let tree = tempfile::tempdir().expect("temp tree");
    std::fs::create_dir(tree.path().join("src")).expect("create directory");
    std::fs::write(tree.path().join("src").join("lib.rs"), "pub fn lib() {}\n")
        .expect("write file");
    std::fs::write(tree.path().join("main.rs"), "fn main() {}").expect("write file");
    let root_path = tree.path().to_string_lossy().to_string();
    let src_path = tree.path().join("src").to_string_lossy().to_string();

    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_llm_key_banner = false;
        s.show_workspace_choice = false;
        s.show_onboarding_tip = false;
        s.sidebar_view = SidebarView::Explorer;
        // The tree is the active pane's working directory, with `src` open, so
        // the frame draws two levels to measure the indent against.
        let active = s.active_pane_id;
        if let Some(session) = s.pane_sessions.get_mut(&active) {
            session.path = root_path.clone();
        }
        s.explorer_expanded.insert(src_path);
    }
    let mut root: Box<dyn Element> = Box::new(view);

    let muted = app.theme.color(ColorToken::Muted);
    let text = app.theme.color(ColorToken::Text);

    // Frame 1: no pointer in the window, so every row is drawn at rest.
    let rest = paint_frame(&mut root, None, &app);
    let (src_label, src_color) = label(&rest, "src");
    assert_eq!(src_color, muted, "a directory's name is muted at rest");
    assert_eq!(
        label(&rest, "lib.rs").1,
        muted,
        "a file one level in is muted at rest"
    );
    let (main_label, main_color) = label(&rest, "main.rs");
    assert_eq!(main_color, muted, "a file's name is muted at rest");
    let (src_icon, src_icon_color) = icon_at(&rest, "folder", 16.0, src_label);
    assert_eq!(
        src_icon_color, muted,
        "the directory's icon is the file set's folder glyph, muted like the row's name"
    );
    assert_eq!(
        icon_at(&rest, "chevron-down", 12.0, src_label).1,
        muted,
        "and so is the chevron beside it"
    );
    let (main_icon, main_icon_color) = icon_at(&rest, "file-rust", 16.0, main_label);
    assert_eq!(
        main_icon_color, muted,
        "the file's own type icon is muted at rest too"
    );
    assert!(
        row_band(&rest, &app, main_label).is_none(),
        "no row is under the pointer, so no row draws a band"
    );

    // The columns: a file's icon sits in a directory's, one level of the tree is
    // one 16 pt column, and the name follows the icon's 16 pt slot and the 8 pt
    // gap after it.
    assert_eq!(
        main_icon.x, src_icon.x,
        "a file's icon is in the same column as a directory's"
    );
    assert_eq!(
        label(&rest, "lib.rs").0.x - main_label.x,
        16.0,
        "one level of the tree indents by 16 pt"
    );
    assert_eq!(
        main_label.x - main_icon.x,
        24.0,
        "the name starts one 16 pt icon slot and its 8 pt gap after the icon"
    );

    // Frame 2 puts the pointer over the `main.rs` row, which reports itself into
    // the app's cell while painting; frame 3 is the first frame that can draw
    // the row's own colour from it. The cursor sits on the label, inside the row.
    let cursor = vec2f(main_label.x, main_label.y);
    let _ = paint_frame(&mut root, Some(cursor), &app);
    let hovered = paint_frame(&mut root, Some(cursor), &app);

    let (hover_main, hover_main_color) = label(&hovered, "main.rs");
    assert_eq!(
        hover_main, main_label,
        "the rows do not move between frames"
    );
    assert_eq!(
        hover_main_color, text,
        "the row under the pointer draws its name in the main colour"
    );
    assert_eq!(
        icon_at(&hovered, "file-rust", 16.0, hover_main).1,
        text,
        "its icon takes the same colour as its name"
    );
    let (band, radius) = row_band(&hovered, &app, hover_main)
        .unwrap_or_else(|| panic!("the row under the pointer draws its band"));
    assert_eq!(radius, 4.0, "the hovered row's band is rounded");
    // The band is the row's whole width, not just its content: the row stretches
    // to the tree's column (the reference wraps each item in a full-width
    // container and highlights that), which is the sidebar's content box here.
    let inset = app.theme.spacing_px(SpacingToken::Xs);
    assert_eq!(
        band.width(),
        state.borrow().sidebar_width - inset * 2.0,
        "the band spans the list, not the row's own content: {band:?}"
    );
    assert_eq!(band.min_x(), inset, "and starts at the panel's own inset");
    // The row is the recipe's 4 + 16 + 4, grown by the label's own line box:
    // 14 pt at the framework's 1.2 line-height ratio is 16.8, a little taller
    // than the 16 pt icon slot. (warp-new's inline text uses the same 1.2 ratio,
    // so its rows are this tall too — 24 is the slot-and-padding sum, not the
    // drawn height.)
    let label_line_box = 14.0 * 1.2;
    assert!(
        (band.height() - (4.0 + label_line_box + 4.0)).abs() < 0.01,
        "the row is its icon slot plus the 4 pt padding either side, or the label's \
         line box when that is taller: {}",
        band.height()
    );
    assert!(
        row_band(&hovered, &app, src_label).is_none(),
        "the row under the pointer is the only one with a band"
    );
    assert_eq!(
        label(&hovered, "src").1,
        muted,
        "the sibling directory stays at rest"
    );
    assert_eq!(
        label(&hovered, "lib.rs").1,
        muted,
        "and so does the sibling file"
    );
    assert_eq!(
        icon_at(&hovered, "folder", 16.0, src_label).1,
        muted,
        "the sibling's icon stays at rest"
    );

    // The next two frames move the pointer to the directory: its chevron, its
    // folder icon and its name all take the one colour, and the row left behind
    // goes back to muted — the cell holds the row the pointer is on, not every
    // row it passed.
    let cursor = vec2f(src_label.x, src_label.y);
    let _ = paint_frame(&mut root, Some(cursor), &app);
    let dir_hovered = paint_frame(&mut root, Some(cursor), &app);

    assert_eq!(
        label(&dir_hovered, "src").1,
        text,
        "the directory's name under the pointer is in the main colour"
    );
    assert_eq!(
        icon_at(&dir_hovered, "chevron-down", 12.0, src_label).1,
        text,
        "and so is its chevron"
    );
    assert_eq!(
        icon_at(&dir_hovered, "folder", 16.0, src_label).1,
        text,
        "and so is the folder glyph"
    );
    assert_eq!(
        label(&dir_hovered, "main.rs").1,
        muted,
        "the row the pointer left goes back to muted"
    );
}

#[test]
fn full_app_renders_with_chat_data() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop
        .create_chat("Demo", None, None)
        .expect("create chat");
    desktop
        .add_chat_message(&chat_id, "user", "Salut!")
        .expect("add user message");
    desktop
        .add_chat_message(&chat_id, "assistant", "Bine ai venit!")
        .expect("add assistant message");

    let counts = render(&desktop);
    assert!(counts.fill_rect > 0, "shell should paint backgrounds");
    assert!(counts.draw_text > 0, "shell should paint text");
}

/// A file pane draws the file it was opened on: the file's own lines and its
/// name, read once while the frame is built.
#[test]
fn a_file_view_pane_draws_the_file_it_was_opened_on() {
    let (desktop, _dir) = common::desktop_state();
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() { println!(\"hi\"); }\n").expect("write the file");
    let path = path.to_string_lossy().to_string();

    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    {
        let state = view.state_rc();
        let mut state = state.borrow_mut();
        let (space, pane_id) = (state.active_space, state.active_pane_id);
        state.spaces[space].set_leaf_kind(
            pane_id,
            goble_app::ui::PaneKind::File { path: path.clone() },
        );
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    // The line is drawn as the runs its own type (Rust, from the path) resolves
    // to. It used to be one command holding the whole line; the runs together
    // are still exactly that line, which is the stronger reading of "the file's
    // own line is drawn".
    let line = "fn main() { println!(\"hi\"); }";
    let drawn: String = drawn_runs(&commands, line)
        .iter()
        .map(|(run, _)| run.as_str())
        .collect();
    assert_eq!(drawn, line, "the file's own line is drawn: {texts:?}");
    assert!(
        texts.iter().any(|text| text.contains("main.rs")),
        "the view names the file it shows: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("1 lines")),
        "the view says how much of the file it shows: {texts:?}"
    );
}

#[test]
fn full_app_renders_inline_key_error() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_llm_key_banner = true);
    assert!(
        counts.fill_rect > 0,
        "the inline key error should paint its panel"
    );
    assert!(
        counts.draw_text > 0,
        "the inline key error should paint its label"
    );
}

#[test]
fn full_app_renders_workspace_choice_overlay() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_workspace_choice = true);
    assert!(
        counts.fill_rect > 0,
        "workspace choice overlay should paint"
    );
    assert!(
        counts.draw_text > 0,
        "workspace choice overlay should paint its label"
    );
}

#[test]
fn toggle_right_sidebar_action_flips_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert!(!state.borrow().right_sidebar_open, "sidebar starts hidden");
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(
        state.borrow().right_sidebar_open,
        "toggle opens the sidebar"
    );
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(
        !state.borrow().right_sidebar_open,
        "toggling again hides the sidebar"
    );
}

#[test]
fn settings_navigate_and_back_flip_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert_eq!(state.borrow().current_tab, AppTab::Chat);
    (actions.on_settings_navigate.borrow_mut())(SettingsPage::Llm);
    assert_eq!(state.borrow().settings_page, SettingsPage::Llm);
    (actions.on_settings_back.borrow_mut())();
    assert_eq!(
        state.borrow().current_tab,
        AppTab::Chat,
        "back returns to chat"
    );
    assert_eq!(
        state.borrow().settings_page,
        SettingsPage::Llm,
        "back keeps the last selected settings page"
    );
}

#[test]
fn pane_layout_persists_across_restart() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store_path = dir.path().join("store.sqlite");

    // First run: split the active pane to the right, then "quit" the app. The
    // on_split_right action persists the pane layout via the DesktopState store.
    {
        let desktop = Arc::new(DesktopState::new(
            Store::open(&store_path).expect("open store"),
            ThreadStore::new(dir.path().join("threads")).expect("open thread store"),
        ));
        let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            Rc::new(RefCell::new(MediaState::mock())),
            WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        (actions.on_split_right.borrow_mut())();

        let split = state.borrow();
        assert_eq!(split.spaces[0].root.max_id(), 3, "split allocates a new leaf");
        assert_eq!(split.active_pane_id, 3, "new pane becomes active");
    }

    // Reopen the same sqlite store: the restored layout must match what was
    // persisted on the first run.
    {
        let desktop = Arc::new(DesktopState::new(
            Store::open(&store_path).expect("reopen store"),
            ThreadStore::new(dir.path().join("threads")).expect("reopen thread store"),
        ));
        let state = UiState::from_desktop(&desktop);
        assert_eq!(state.spaces.len(), 1, "one space restored");
        assert_eq!(state.spaces[0].root.max_id(), 3, "split tree restored");
        assert!(
            state.spaces[0].root.contains_leaf(1) && state.spaces[0].root.contains_leaf(3),
            "both leaves restored"
        );
        assert_eq!(state.active_pane_id, 3, "active pane restored");
        assert!(state.next_pane_id > 3, "id counter recovered above max id");
    }
}

#[test]
fn toggle_dark_mode_updates_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert!(!state.borrow().settings_dark_mode);
    (actions.on_toggle_dark_mode.borrow_mut())(true);
    assert!(state.borrow().settings_dark_mode);
}

/// Index of the first `DrawText` command whose text contains `needle`.
fn text_index(commands: &[goble_ui::render::RenderCommand], needle: &str) -> Option<usize> {
    commands.iter().position(|c| {
        matches!(c, goble_ui::render::RenderCommand::DrawText { text, .. } if text.contains(needle))
    })
}

/// Every workspace in `state.spaces` is drawn as its own chip in the toolbar,
/// so adding a workspace shows it *next to* the current one.
#[test]
fn topbar_lists_every_workspace() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    {
        let mut state = state_rc.borrow_mut();
        let id = state.next_pane_id;
        state.next_pane_id += 1;
        state.spaces.push(goble_app::ui::Space::new(
            "Workspace Two",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        text_index(&commands, "Space 1").is_some(),
        "the current workspace chip is drawn"
    );
    assert!(
        text_index(&commands, "Workspace Two").is_some(),
        "the added workspace is drawn next to the current one"
    );
}

/// The full-width fill the toolbar paints at the very top of the window.
fn topbar_surface(commands: &[RenderCommand], app: &AppContext) -> goble_ui::geometry::RectF {
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Surface)
                    && rect.min_y() == 0.0
                    && rect.width() == 1024.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the toolbar paints its own surface")
}

/// Center of the top-most icon drawn under `atlas_name` (smallest y), so a
/// glyph the shell reuses lower down still resolves to the toolbar control.
fn topmost_icon_center(commands: &[RenderCommand], atlas_name: &str) -> Option<(f32, f32)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon {
                origin, name, size, ..
            } if name == atlas_name => Some((origin.x + size / 2.0, origin.y + size / 2.0)),
            _ => None,
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// R2: the workspace tabs read as browser-style tabs. Each tab is drawn at the
/// full height of the toolbar, and a strip of N tabs draws exactly N-1 vertical
/// rules — one per boundary, never a doubled line.
#[test]
fn workspace_tabs_fill_the_toolbar_and_draw_one_rule_per_boundary() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    let space_count = {
        let mut state = state_rc.borrow_mut();
        let id = state.next_pane_id;
        state.next_pane_id += 1;
        state.spaces.push(goble_app::ui::Space::new(
            "Workspace Two",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
        state.spaces.len()
    };
    assert_eq!(space_count, 2, "two spaces to separate");
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;

    assert_eq!(
        topbar_surface(&commands, &app).height(),
        topbar_height,
        "the bar is exactly TOPBAR_HEIGHT tall"
    );

    let border = app.theme.color(ColorToken::Border);
    let rules: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. }
                if *color == border && rect.width() == 1.0 && rect.min_y() == 0.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        rules.len(),
        space_count - 1,
        "one rule per boundary, not one per tab: {rules:?}"
    );
    for rect in rules {
        assert_eq!(
            rect.height(),
            topbar_height,
            "a rule spans the full tab height: {rect:?}"
        );
    }

    let raised = app.theme.color(ColorToken::SurfaceRaised);
    let active: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == raised
                && rect.min_y() == 0.0
                && rect.height() == topbar_height
                && rect.width() > 1.0 =>
            {
                Some(*corner_radius)
            }
            _ => None,
        })
        .collect();
    assert_eq!(active.len(), 1, "exactly one tab is the active, filled one");
    assert_eq!(active[0], 0.0, "the active tab has square corners");

    // The +/▾ control and the Settings icon keep their place and size: both
    // stay centered in the bar.
    for icon in ["chevron-down", "settings"] {
        let (_, y) = topmost_icon_center(&commands, icon).unwrap_or_else(|| panic!("{icon} drawn"));
        assert_eq!(y, topbar_height / 2.0, "{icon} stays centered in the bar");
    }
}

/// R2: while the active space is being renamed, its tab still renders the inline
/// name field, and the bar keeps its height.
#[test]
fn the_active_workspace_tab_becomes_the_rename_field() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    view.state_rc().borrow_mut().space_rename_editing = true;
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    assert!(
        text_index(&commands, "Workspace name").is_some(),
        "the inline rename field replaces the label"
    );
    assert!(
        text_index(&commands, "Space 1").is_none(),
        "the label is swapped out while renaming"
    );
    assert_eq!(
        topbar_surface(&commands, &app).height(),
        goble_app::ui::shell::TOPBAR_HEIGHT,
        "the bar keeps its height while the rename field is shown"
    );
}

/// The Settings→Appearance color wheel paints at its own layout origin (the
/// panel's content column), not at the window origin where it would smear over
/// the sidebar and the toolbar. The panel itself is the window minus a small
/// inset, so "inside the panel" means the whole viewport minus that margin.
#[test]
fn settings_color_wheel_paints_inside_the_panel() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    {
        let mut state = state_rc.borrow_mut();
        state.settings_overlay_open = true;
        state.settings_category = goble_app::ui::SettingsCategory::Appearance;
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    // The saturation/value square is drawn as a column of fade-right rows.
    let rows: Vec<f32> = commands
        .iter()
        .filter_map(|c| match c {
            goble_ui::render::RenderCommand::FillRectFadeRight { rect, .. } => Some(rect.min_x()),
            _ => None,
        })
        .collect();
    assert!(!rows.is_empty(), "the appearance pane draws its color wheel");
    for x in rows {
        assert!(
            x > goble_app::ui::SETTINGS_OVERLAY_INSET,
            "wheel painted at x={x}, left of the settings panel"
        );
        assert!(
            x < 1024.0,
            "wheel painted at x={x}, outside the window"
        );
    }

    // The hue ring is one image fill, also inside the inset panel.
    let ring = commands.iter().find_map(|c| match c {
        goble_ui::render::RenderCommand::DrawImage { rect, source, .. }
            if source.contains("ring") =>
        {
            Some(*rect)
        }
        _ => None,
    });
    let ring = ring.expect("the hue ring is drawn as an image");
    assert!(
        ring.min_x() >= goble_app::ui::SETTINGS_OVERLAY_INSET
            && ring.max_y() <= 768.0 - goble_app::ui::SETTINGS_OVERLAY_INSET,
        "the ring stays inside the inset panel, got {ring:?}"
    );
}

/// Origin of the first `DrawText` command whose text contains `needle`.
fn text_origin(
    commands: &[goble_ui::render::RenderCommand],
    needle: &str,
) -> Option<goble_ui::Vector2F> {
    commands.iter().find_map(|c| match c {
        goble_ui::render::RenderCommand::DrawText { origin, text, .. } if text.contains(needle) => {
            Some(*origin)
        }
        _ => None,
    })
}

/// The toolbar is layered above the shell body so its trays paint over the
/// sidebar and the pane surface, but it still reserves its height in the body
/// column: every body surface must start below the toolbar band, or the
/// toolbar would cover the sidebar's search field.
#[test]
fn shell_body_paints_below_the_toolbar() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, &desktop, None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;

    let chip = text_origin(&commands, "Space 1").expect("workspace chip");
    assert!(
        chip.y < topbar_height,
        "workspace chips paint inside the toolbar (y={})",
        chip.y
    );

    for needle in ["Search", "Ask anything to get started"] {
        let y = text_origin(&commands, needle)
            .unwrap_or_else(|| panic!("{needle} is drawn"))
            .y;
        assert!(
            y >= topbar_height,
            "{needle} paints at y={y}, inside the toolbar band (height {topbar_height})"
        );
    }
}

/// The toolbar's trays paint above the shell surface: the "+ ▾" environment
/// menu is drawn after the sidebar and the pane content, so nothing covers it.
#[test]
fn environment_menu_paints_above_the_shell_surface() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    *state_rc.borrow().env_selector_open.borrow_mut() = true;
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    let sidebar = text_index(&commands, "Search").expect("sidebar search field");
    let pane = text_index(&commands, "Ask anything to get started")
        .expect("pane empty state");
    let menu = text_index(&commands, "New environment").expect("environment menu item");
    assert!(
        menu > sidebar,
        "the environment tray ({menu}) must paint above the sidebar ({sidebar})"
    );
    assert!(
        menu > pane,
        "the environment tray ({menu}) must paint above the pane surface ({pane})"
    );
}

/// Lay the whole app out and paint one frame, returning its render commands.
/// A frame has to happen before a dispatch: an element learns its own origin
/// while painting, which is where a click is resolved.
fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
    use goble_ui::elements::{LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::render::Renderer;

    let _ = root.layout(
        SizeConstraint::loose(vec2f(1024.0, 768.0)),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// A click at `at`: the down/up pair an element's own hit test reads. Buttons
/// fire on the release, so both land on the same frame's tree.
fn click(root: &mut Box<dyn Element>, app: &AppContext, at: goble_ui::Vector2F) -> bool {
    use goble_ui::elements::EventContext;
    use goble_ui::event::DispatchedEvent;

    let mut ctx = EventContext::default();
    let down = root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
        &mut ctx,
        app,
    );
    let up = root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        },
        &mut ctx,
        app,
    );
    down && up
}

/// The commands that draw `line` in the frame, in paint order: the one command
/// a plain line is drawn as, or the runs a highlighted line is split into.
fn drawn_runs(commands: &[RenderCommand], line: &str) -> Vec<(String, goble_ui::ColorU)> {
    let mut runs: Vec<(String, goble_ui::ColorU)> = Vec::new();
    for command in commands {
        let RenderCommand::DrawText { text, color, .. } = command else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let drawn: usize = runs.iter().map(|(run, _)| run.len()).sum();
        if line[drawn..].starts_with(text.as_str()) {
            runs.push((text.clone(), *color));
            if drawn + text.len() == line.len() {
                return runs;
            }
        } else {
            runs.clear();
            if line.starts_with(text.as_str()) {
                runs.push((text.clone(), *color));
                if text.len() == line.len() {
                    return runs;
                }
            }
        }
    }
    Vec::new()
}

/// A file pane colours its lines by the file's own type: a `config.toml` is
/// drawn as its TOML runs — more than one command per line, in more than one
/// colour, together spelling the line — while a file whose type does not
/// resolve stays one plain command per line.
#[test]
fn a_file_pane_draws_a_toml_files_lines_as_highlighted_runs() {
    const LINE: &str = "api_key = \"sk-test\"";

    let (desktop, _dir) = common::desktop_state();
    let dir = tempfile::tempdir().expect("temp dir");
    let toml = dir.path().join("config.toml");
    std::fs::write(&toml, format!("{LINE}\n")).expect("write the config");
    let plain = dir.path().join("notes.zzz");
    std::fs::write(&plain, format!("{LINE}\n")).expect("write the plain file");

    /// The frame of the real app with `path` open in the active pane.
    fn pane_frame(
        desktop: &Arc<DesktopState>,
        path: &std::path::Path,
        app: &AppContext,
    ) -> Vec<RenderCommand> {
        let view = RootView::new(app, desktop, None);
        {
            let state = view.state_rc();
            let mut state = state.borrow_mut();
            let (space, pane_id) = (state.active_space, state.active_pane_id);
            state.spaces[space].set_leaf_kind(
                pane_id,
                goble_app::ui::PaneKind::File {
                    path: path.to_string_lossy().to_string(),
                },
            );
        }
        frame(&mut (Box::new(view) as Box<dyn Element>), app)
    }

    let app = AppContext::default();

    let highlighted = pane_frame(&desktop, &toml, &app);
    let runs = drawn_runs(&highlighted, LINE);
    assert!(
        runs.len() > 1,
        "a TOML line is drawn as its runs, not as one string: {runs:?}"
    );
    assert_eq!(
        runs.iter().map(|(run, _)| run.as_str()).collect::<String>(),
        LINE,
        "the runs together are the line"
    );
    let colours: std::collections::HashSet<goble_ui::ColorU> =
        runs.iter().map(|(_, color)| *color).collect();
    assert!(
        colours.len() > 1,
        "a key and its string are not one colour: {colours:?}"
    );

    let plain = pane_frame(&desktop, &plain, &app);
    let runs = drawn_runs(&plain, LINE);
    assert_eq!(
        runs.len(),
        1,
        "a type that does not resolve draws the line plain: {runs:?}"
    );
    assert_eq!(runs[0].0, LINE, "and the plain line is the whole line");
}

/// The notice band's "Edit API keys" opens `~/.goble/config.toml` — where the
/// keys and the models live — in a pane of its own, and does **not** open the
/// model-provider overlay.
#[test]
fn the_notice_bands_edit_api_keys_opens_the_config_file_in_a_pane() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    let (space, active_before) = {
        let mut s = state.borrow_mut();
        s.show_llm_key_banner = true;
        s.show_workspace_choice = false;
        s.show_onboarding_tip = false;
        (s.active_space, s.active_pane_id)
    };
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = frame(&mut root, &app);
    let button = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "Edit API keys" => {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the band draws its button");
    assert!(
        click(&mut root, &app, button + vec2f(2.0, 2.0)),
        "the click lands on the band's button"
    );

    let expected = goble_core::app_home::GobleHome::locate()
        .expect("the app's own home")
        .config_path()
        .to_string_lossy()
        .to_string();
    assert!(
        expected.ends_with("config.toml"),
        "the app's config file is config.toml: {expected}"
    );
    let s = state.borrow();
    assert!(
        !s.llm_dialog_open,
        "the model-provider overlay must not open"
    );
    assert_ne!(
        s.active_pane_id, active_before,
        "the config file opens in a pane of its own"
    );
    assert_eq!(
        s.spaces[space].leaf_kind(s.active_pane_id),
        Some(&goble_app::ui::PaneKind::File { path: expected }),
        "the pane is a file view of the config file"
    );
}

/// A click on an explorer **row** opens that file in a pane to the right of the
/// pane that was active, focused, and headed by the file's name and its type
/// icon.
#[test]
fn an_explorer_row_click_opens_the_file_in_a_pane_on_the_right_headed_by_its_name() {
    use goble_app::ui::{Pane, PaneKind, SplitDir};

    let (desktop, _dir) = common::desktop_state();
    let tree = tempfile::tempdir().expect("temp tree");
    std::fs::write(tree.path().join("main.rs"), "fn main() {}\n").expect("write the file");
    let root_path = tree.path().to_string_lossy().to_string();
    let file_path = tree.path().join("main.rs").to_string_lossy().to_string();

    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    let (space, active_before, sidebar_width) = {
        let mut s = state.borrow_mut();
        s.show_llm_key_banner = false;
        s.show_workspace_choice = false;
        s.show_onboarding_tip = false;
        s.sidebar_view = SidebarView::Explorer;
        let active = s.active_pane_id;
        if let Some(session) = s.pane_sessions.get_mut(&active) {
            session.path = root_path.clone();
        }
        (s.active_space, active, s.sidebar_width)
    };
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = frame(&mut root, &app);
    // The tree's own row label is the 14 pt one, the same as R28's test reads.
    let row = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                font_size,
                ..
            } if text == "main.rs" && *font_size == 14.0 => Some(*origin),
            _ => None,
        })
        .expect("the tree draws the file's row");
    assert!(
        click(&mut root, &app, row + vec2f(2.0, 2.0)),
        "the click lands on the tree's file row"
    );

    let file_pane = {
        let s = state.borrow();
        assert_ne!(
            s.active_pane_id, active_before,
            "the clicked file opens as its own pane"
        );
        assert_eq!(
            s.spaces[space].leaf_kind(s.active_pane_id),
            Some(&PaneKind::File {
                path: file_path.clone()
            }),
            "the pane shows the file that was clicked"
        );
        match &s.spaces[space].root {
            Pane::Split {
                dir, first, second, ..
            } => {
                assert_eq!(
                    *dir,
                    SplitDir::Horizontal,
                    "the file view opens beside the pane, not over it"
                );
                assert!(
                    matches!(&**first, Pane::Leaf { id, .. } if *id == active_before),
                    "the pane that was active keeps the left half"
                );
                assert!(
                    matches!(&**second, Pane::Leaf { id, .. } if *id == s.active_pane_id),
                    "the file view is the right half, and it is the focused one"
                );
            }
            other => panic!("expected one split, got {other:?}"),
        }
        s.active_pane_id
    };

    // The pane's own header names the file, with the file's type icon beside
    // it — the title row of the file view, drawn above the lines it shows.
    let commands = frame(&mut root, &app);
    let header_name = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                font_size,
                ..
            } if text == "main.rs" && *font_size == 13.0 => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the file pane's header names the file"));
    let icon = commands.iter().find_map(|command| match command {
        RenderCommand::DrawIcon {
            origin, name, size, ..
        } if name == "file-rust"
            && *size == 16.0
            && origin.x > sidebar_width
            && (origin.y - header_name.y).abs() < 12.0 =>
        {
            Some(*origin)
        }
        _ => None,
    });
    assert!(
        icon.is_some(),
        "the pane headed by main.rs at {header_name:?} draws its own type icon \
         (pane {file_pane})"
    );
}
