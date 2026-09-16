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

/// Where the frame draws `text`: the origin of the one command that paints it.
fn drawn_origin(commands: &[RenderCommand], text: &str) -> Option<goble_ui::Vector2F> {
    commands.iter().find_map(|command| match command {
        RenderCommand::DrawText {
            origin,
            text: drawn,
            ..
        } if drawn == text => Some(*origin),
        _ => None,
    })
}

/// One key press into the tree the last frame built.
fn press(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    key: &str,
    modifiers: goble_ui::event::ModifiersState,
) -> bool {
    use goble_ui::elements::EventContext;
    use goble_ui::event::DispatchedEvent;

    let mut ctx = EventContext::default();
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers,
        },
        &mut ctx,
        app,
    )
}

/// A file the read took whole is editable in its pane: a press in the body
/// focuses the pane's editor — the beam is drawn where the press landed, in the
/// focus blue — typing lands in the pane's own text, the pane says the buffer is
/// unsaved, and Cmd+S writes it back to the file.
#[test]
fn a_file_pane_is_typed_into_and_saved() {
    use goble_ui::elements::caret::CARET_WIDTH;
    use goble_ui::event::ModifiersState;
    use goble_ui::theme::ColorToken;

    let (desktop, _dir) = common::desktop_state();
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("main.rs");
    let original = "fn main() {}\nlet x = 1;\n";
    std::fs::write(&path, original).expect("write the file");
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
    let commands = frame(&mut root, &app);

    // The second line's number is the row's own left edge, so the press below
    // lands in that line of the buffer. (The pane's press is a down; the release
    // that follows is nobody's — the beam the next frame draws is the proof the
    // editor took it.)
    let row = drawn_origin(&commands, "    2 ").expect("the second line's number is drawn");
    let _ = click(&mut root, &app, vec2f(row.x + 400.0, row.y + 4.0));

    // The frame after the press draws the field focused, with the beam as the
    // focus-blue slot the composer's own field uses.
    let commands = frame(&mut root, &app);
    let focus = app.theme.color(ColorToken::Focus);
    let line_box = (12.0 * 1.35f32).ceil();
    let beam = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. } if *color == focus => Some(*rect),
        _ => None,
    });
    let beam = beam.unwrap_or_else(|| panic!("the focused pane draws its beam: {commands:?}"));
    assert!(
        (beam.width() - CARET_WIDTH).abs() < 0.5 && (beam.height() - line_box).abs() < 0.5,
        "the beam is the editor's own slot: {beam:?}"
    );
    assert!(
        beam.min_y() >= row.y - 1.0 && beam.min_y() < row.y + line_box,
        "and it sits on the line the press landed on: {beam:?} against {row:?}"
    );

    // A character typed while the pane is focused reaches the buffer, and the
    // pane says the buffer is not what the file holds.
    assert!(
        press(&mut root, &app, "!", ModifiersState::default()),
        "the focused editor takes the character"
    );
    let commands = frame(&mut root, &app);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, RenderCommand::DrawText { text, .. } if text == "let x = 1;!")),
        "the typed character is in the line the beam was on: {:?}",
        commands
    );
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, RenderCommand::DrawText { text, .. } if text.contains("unsaved changes"))),
        "and the pane marks the buffer unsaved: {commands:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        original,
        "typing alone writes nothing to the file"
    );

    // Cmd+S saves it: the file holds the buffer's bytes and the pane says so.
    let save = ModifiersState {
        command: true,
        ..Default::default()
    };
    assert!(
        press(&mut root, &app, "s", save),
        "the focused pane answers Cmd+S"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "fn main() {}\nlet x = 1;!\n",
        "the save writes the buffer's bytes, whole"
    );
    let commands = frame(&mut root, &app);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, RenderCommand::DrawText { text, .. } if text.contains("saved"))),
        "the pane reports the save: {commands:?}"
    );
    assert!(
        !commands.iter().any(
            |command| matches!(command, RenderCommand::DrawText { text, .. } if text.contains("unsaved changes"))
        ),
        "and the unsaved mark is gone"
    );
}

/// A file the read did not take whole is read-only in its pane: the pane says
/// why, and Cmd+S leaves the file exactly as it is — the refusal is the state
/// machine's, and the pane shows it rather than a file that was written over.
#[test]
fn a_file_too_long_to_edit_says_so_and_saves_nothing() {
    use goble_ui::event::ModifiersState;
    use goble_ui::theme::ColorToken;

    let (desktop, _dir) = common::desktop_state();
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("long.txt");
    let text: String = (0..2_025).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&path, &text).expect("write the file");
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
    let commands = frame(&mut root, &app);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text.contains("too long to edit")
        )),
        "the pane says why the file is not editable: {commands:?}"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text.contains("showing the first")
        )),
        "and draws the part of the file it read, as it always did: {commands:?}"
    );

    let save = ModifiersState {
        command: true,
        ..Default::default()
    };
    assert!(press(&mut root, &app, "s", save), "the pane answers Cmd+S");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        text,
        "a refused save writes nothing"
    );
    let commands = frame(&mut root, &app);
    let error = app.theme.color(ColorToken::Error);
    let refusal = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } if text.contains("Not saved") => {
                Some((text.clone(), *color))
            }
            _ => None,
        });
    assert_eq!(
        refusal.as_ref().map(|(text, _)| text),
        Some(&"Not saved: this file is too long to edit.".to_string()),
        "the refusal is said in the pane: {commands:?}"
    );
    assert_eq!(
        refusal.map(|(_, color)| color),
        Some(error),
        "and it is drawn as the pane's error, not as ordinary detail"
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
        // The first tab holds an agent with no conversation subject, so its
        // label would be derived; name it explicitly — what is under test here
        // is that every workspace is drawn as its own chip.
        state.spaces[0].name = "Primary".to_string();
        state.spaces[0].named = true;
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
        text_index(&commands, "Primary").is_some(),
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
        text_index(&commands, "New Agent").is_none(),
        "the label is swapped out while renaming"
    );
    assert_eq!(
        topbar_surface(&commands, &app).height(),
        goble_app::ui::shell::TOPBAR_HEIGHT,
        "the bar keeps its height while the rename field is shown"
    );
}

/// R72: a second click on a tab opens its inline rename field, and the field is
/// still there on the next frame — the tab is editable by double click, as it
/// was before.
#[test]
fn a_double_click_on_a_tab_opens_its_rename_field() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    let mut root: Box<dyn Element> = Box::new(view);

    let tab = active_tab_rect(&frame(&mut root, &app), &app);
    let at = vec2f(tab.min_x() + 4.0, tab.min_y() + tab.height() / 2.0);
    click(&mut root, &app, at);
    // The frame the app paints between the two presses, which is where a tab
    // the first click renamed would move out from under the pointer.
    let tab = active_tab_rect(&frame(&mut root, &app), &app);
    let at = vec2f(at.x.max(tab.min_x() + 4.0), at.y);
    click(&mut root, &app, at);

    assert!(
        state.borrow().space_rename_editing,
        "the second click starts the rename of the clicked tab"
    );
    let commands = frame(&mut root, &app);
    assert!(state.borrow().space_rename_editing, "the field stays open");
    // The field is the live one, not the tab's own label: a focused field
    // paints its insertion beam, and the beam sits in the tab's slot.
    let caret = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Focus)
                    && (rect.height() - 16.0).abs() < 0.5 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "the rename field draws its caret in the tab: {:?}",
                commands
                    .iter()
                    .filter_map(|c| match c {
                        RenderCommand::DrawText { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        caret.min_x() >= tab.min_x() - 1.0 && caret.max_x() <= tab.max_x() + 1.0,
        "the field sits in the tab {tab:?}, got {caret:?}"
    );

    // The field is editable: a key typed while it is open reaches it, not the
    // composer behind it.
    let before = state.borrow().space_rename_draft.clone();
    send(
        &mut root,
        &app,
        goble_ui::event::DispatchedEvent::KeyDown {
            key: "x".to_string(),
            modifiers: goble_ui::event::ModifiersState::default(),
        },
    );
    assert_ne!(
        state.borrow().space_rename_draft,
        before,
        "typing while the tab is being renamed edits the tab's name"
    );
}

/// R72: the gesture is counted on the press, which is warp-new's own rule
/// (`MULTI_CLICK_INTERVAL`, counted press-to-press). A second press inside the
/// window opens the field however long that press is held afterwards — the
/// case a release-to-release reading of the same two clicks misses, because
/// the two releases stand further apart than the window.
#[test]
fn a_held_second_press_still_renames_the_tab() {
    use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY};

    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    let mut root: Box<dyn Element> = Box::new(view);

    let tab = active_tab_rect(&frame(&mut root, &app), &app);
    let at = vec2f(tab.min_x() + 4.0, tab.min_y() + tab.height() / 2.0);

    let started = std::time::Instant::now();
    let down = DispatchedEvent::MouseDown {
        position: at,
        button: BUTTON_PRIMARY,
    };
    let up = DispatchedEvent::MouseUp {
        position: at,
        button: BUTTON_PRIMARY,
    };
    send(&mut root, &app, down.clone());
    send(&mut root, &app, up.clone());
    // The second press lands half a window after the first, and is held for
    // three quarters of one: the presses are 200 ms apart and the releases
    // 500 ms, so a reading that timed the releases would see no gesture.
    let window = goble_app::state::MULTI_CLICK_INTERVAL_MS;
    std::thread::sleep(std::time::Duration::from_millis((window / 2) as u64));
    send(&mut root, &app, down);
    std::thread::sleep(std::time::Duration::from_millis((window * 3 / 4) as u64));
    send(&mut root, &app, up);
    let span = started.elapsed().as_millis();

    assert!(
        span > window,
        "the two releases are {span} ms apart, past the {window} ms window — \
         only the presses are inside it"
    );
    assert!(
        state.borrow().space_rename_editing,
        "the second press opens the field, whatever the release that follows it does"
    );
}

/// The origin the topbar draws `label` at, so a test can aim a click at that
/// tab rather than at the window.
fn tab_label_origin(commands: &[RenderCommand], label: &str) -> goble_ui::Vector2F {
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the strip draws a {label:?} tab"))
}

/// R72: two clicks at the *same* pointer position land on the same tab even
/// when the first one changes which tab is active — the reported case, where a
/// tab could no longer be renamed by double click.
#[test]
fn a_double_click_renames_a_tab_that_is_not_the_active_one() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_onboarding_tip = false;
        s.show_llm_key_banner = false;
        s.show_workspace_choice = false;
        s.spaces[0].name = "Primary".to_string();
        s.spaces[0].named = true;
        let id = s.next_pane_id;
        s.next_pane_id += 1;
        s.spaces.push(goble_app::ui::Space::new(
            "Second",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
        s.active_space = 0;
    }
    let mut root: Box<dyn Element> = Box::new(view);

    // The second tab is the one the user clicks: the pointer never moves, so
    // both presses land on the same absolute point — the whole point of the
    // case, since the first press makes that tab the active one.
    let commands = frame(&mut root, &app);
    let label = tab_label_origin(&commands, "Second");
    let at = vec2f(label.x + 2.0, goble_app::ui::shell::TOPBAR_HEIGHT / 2.0);
    click(&mut root, &app, at);
    let _ = frame(&mut root, &app);
    click(&mut root, &app, at);

    assert_eq!(state.borrow().active_space, 1, "the first click selected it");
    assert!(
        state.borrow().space_rename_editing,
        "the second click opens its rename field"
    );
    let commands = frame(&mut root, &app);
    assert_eq!(
        state.borrow().space_rename_focused,
        true,
        "the field takes the keyboard"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Focus)
                    && (rect.height() - 16.0).abs() < 0.5
        )),
        "the open field draws its caret: {:?}",
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}

/// A right click at `at`: the down/up pair the window's own loop sends for the
/// secondary button — the one a tab's menu opens on.
fn right_click(root: &mut Box<dyn Element>, app: &AppContext, at: goble_ui::Vector2F) {
    use goble_ui::event::DispatchedEvent;
    send(
        root,
        app,
        DispatchedEvent::MouseDown {
            position: at,
            button: 1,
        },
    );
    send(
        root,
        app,
        DispatchedEvent::MouseUp {
            position: at,
            button: 1,
        },
    );
}

/// The origin the menu draws `label` at, or a panic naming the rows that were
/// drawn instead — so a missing row reads as the list it was in.
fn row_origin(commands: &[RenderCommand], label: &str) -> goble_ui::Vector2F {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "the menu draws {label:?}, got {:?}",
                commands
                    .iter()
                    .filter_map(|command| match command {
                        RenderCommand::DrawText { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            )
        })
}

/// Open the menu of the tab whose label is `label`, and paint the frame that
/// carries it — the panel is built from the state the right click leaves.
fn open_tab_menu(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    label: &str,
) -> Vec<RenderCommand> {
    let commands = frame(root, app);
    let at = tab_label_origin(&commands, label);
    right_click(
        root,
        app,
        vec2f(
            at.x + 2.0,
            goble_app::ui::shell::TOPBAR_HEIGHT / 2.0,
        ),
    );
    frame(root, app)
}

/// Choose one row of the menu that is already open: the row's own label is
/// drawn inside the row's bounds, so a click on it lands on the row.
fn choose_row(root: &mut Box<dyn Element>, app: &AppContext, row: &str) -> Vec<RenderCommand> {
    let commands = frame(root, app);
    let at = row_origin(&commands, row);
    click(root, app, vec2f(at.x + 1.0, at.y + 1.0));
    frame(root, app)
}

/// The menu's own panel: the one rounded surface the frame draws at the theme's
/// raised colour. It is asserted unique, so it cannot silently become some
/// other card of the shell.
fn menu_panel(commands: &[RenderCommand], app: &AppContext) -> goble_ui::geometry::RectF {
    let panels: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - 6.0).abs() < 0.001 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(panels.len(), 1, "the menu draws one panel: {panels:?}");
    panels[0]
}

/// The rectangle of the red dot of the menu's colour row, in the frame that is
/// open: the one circle of that colour drawn as a rounded fill.
fn red_dot(commands: &[RenderCommand]) -> goble_ui::geometry::RectF {
    let red = goble_ui::theme::TabColor::Red.color();
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == red && (*corner_radius - 8.0).abs() < 0.001 => Some(*rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the menu's colour row draws a red dot: {commands:?}"))
}

/// A view of two tabs — an agent tab that derives its own label and a terminal
/// tab the user named — with the flags a first run would raise turned off.
/// Returns the state handle and the frame's element, so a test can drive the
/// strip.
#[allow(clippy::type_complexity)]
fn two_tab_view(
    desktop: &Arc<DesktopState>,
    app: &AppContext,
) -> (Rc<RefCell<UiState>>, Box<dyn Element>) {
    let view = RootView::new(app, desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_onboarding_tip = false;
        s.show_llm_key_banner = false;
        s.show_workspace_choice = false;
        // The first tab is nobody's name: it reads what it holds, which is the
        // agent label while its conversation has no subject.
        s.spaces[0].name.clear();
        s.spaces[0].named = false;
        let id = s.next_pane_id;
        s.next_pane_id += 1;
        s.spaces.push(goble_app::ui::Space::new(
            "Second",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
        s.active_space = 0;
    }
    (state, Box::new(view))
}

/// R73: a right click on a tab opens its menu hung from the pointer itself —
/// warp-new's `TabContextMenuAnchor::Pointer` — drawn over the window rather
/// than clipped to the bar it hangs from.
#[test]
fn a_right_click_hangs_the_tab_menu_from_the_pointer_over_the_window() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    // Well inside the second tab, so a panel hung from that tab's own corner
    // would be drawn tens of pixels to the left of where the click landed.
    let at = vec2f(
        tab_label_x(&mut root, &app, "Second") + 40.0,
        goble_app::ui::shell::TOPBAR_HEIGHT / 2.0,
    );
    right_click(&mut root, &app, at);
    let commands = frame(&mut root, &app);

    assert_eq!(
        state.borrow().space_menu.borrow().map(|menu| menu.index),
        Some(1),
        "the tab the click landed on is the one whose menu is open"
    );
    let rename = row_origin(&commands, "Rename tab");
    assert!(
        rename.x >= at.x && rename.x - at.x <= 30.0,
        "the panel's first row starts at the pointer ({at:?}), not at the tab's own corner: {rename:?}"
    );
    assert!(rename.y >= at.y, "the panel opens below the pointer");

    // Drawn over the window, not inside the bar: the rows paint after the pane
    // behind them, and the colour row — the panel's last — is drawn below the
    // toolbar band entirely.
    let pane = text_index(&commands, "Ask anything to get started")
        .expect("the pane's empty state");
    assert!(
        text_index(&commands, "Rename tab").unwrap() > pane,
        "the menu paints above the pane surface"
    );
    let dot = red_dot(&commands);
    assert!(
        dot.min_y() > goble_app::ui::shell::TOPBAR_HEIGHT,
        "the menu's last row is drawn over the body, below the bar: {dot:?}"
    );
}

/// R73: only the rows that apply to the tab are drawn — warp-new's own gates on
/// "Reset tab name" (a name the user typed), "Move Tab Left" (a slot to move
/// into) and "Close other tabs" (a second tab to close).
#[test]
fn the_tab_menus_rows_are_the_ones_that_apply_to_that_tab() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (_state, mut root) = two_tab_view(&desktop, &app);

    // The first tab: it has nowhere to move left and no typed name to reset,
    // and there is a second tab for "Close other tabs" to close.
    let commands = open_tab_menu(&mut root, &app, "New Agent");
    assert!(text_index(&commands, "Rename tab").is_some());
    assert!(text_index(&commands, "Close tab").is_some());
    assert!(
        text_index(&commands, "Move Tab Left").is_none(),
        "the first tab has nowhere to move left"
    );
    assert!(
        text_index(&commands, "Reset tab name").is_none(),
        "a tab that derived its own label has no typed name to give back"
    );
    assert!(
        text_index(&commands, "Close other tabs").is_some(),
        "and a second tab is there to close"
    );

    // The same right click on the other tab, which has the rows the first one
    // lacked: a right click belongs to the tab it lands on.
    let commands = open_tab_menu(&mut root, &app, "Second");
    assert!(text_index(&commands, "Reset tab name").is_some(), "a typed name can be reset");
    assert!(text_index(&commands, "Move Tab Left").is_some(), "the second tab can move left");
    assert!(text_index(&commands, "Close other tabs").is_some());
}

/// R73: "Rename tab" opens the tab's inline rename field, seeded with the name
/// that tab draws — the same field a double click opens.
#[test]
fn the_tab_menus_rename_row_opens_the_inline_field() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    open_tab_menu(&mut root, &app, "Second");
    choose_row(&mut root, &app, "Rename tab");

    let state = state.borrow();
    assert!(state.space_rename_editing, "the field is open");
    assert_eq!(state.active_space, 1, "on the tab the menu belonged to");
    assert_eq!(state.space_rename_draft, "Second", "seeded with the name it draws");
}

/// R73: "Reset tab name" gives the typed name up, and the label derived from
/// what the tab holds takes over again.
#[test]
fn the_tab_menus_reset_row_gives_the_typed_name_back() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    open_tab_menu(&mut root, &app, "Second");
    let commands = choose_row(&mut root, &app, "Reset tab name");

    assert!(!state.borrow().spaces[1].named, "the name is no longer the user's");
    assert!(
        text_index(&commands, "Second").is_none(),
        "the tab reads what it holds again: {:?}",
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}

/// R73: "Move Tab Left" swaps the tab with its left neighbour — in the state
/// and in the strip that is drawn from it.
#[test]
fn the_tab_menus_move_left_row_swaps_the_tab_with_its_neighbour() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    open_tab_menu(&mut root, &app, "Second");
    let commands = choose_row(&mut root, &app, "Move Tab Left");

    assert_eq!(
        state.borrow().spaces[0].name,
        "Second",
        "the tab that moved is the first one now"
    );
    assert!(
        tab_label_origin(&commands, "Second").x < tab_label_origin(&commands, "New Agent").x,
        "and it is drawn to the left of the tab it passed"
    );
}

/// R73: "Close tab" closes the tab the menu was opened on.
#[test]
fn the_tab_menus_close_row_closes_that_tab() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    open_tab_menu(&mut root, &app, "Second");
    let commands = choose_row(&mut root, &app, "Close tab");

    assert_eq!(state.borrow().spaces.len(), 1, "one tab is left");
    assert_eq!(state.borrow().spaces[0].name, "New Agent", "the other one");
    assert!(text_index(&commands, "Second").is_none(), "and it is off the strip");
}

/// R73: "Close other tabs" leaves the tab the menu was opened on and nothing
/// else, however many tabs stand on either side of it.
#[test]
fn the_tab_menus_close_others_row_leaves_only_that_tab() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);
    {
        // A third tab, so there is one on either side of the one being kept.
        let mut s = state.borrow_mut();
        let id = s.next_pane_id;
        s.next_pane_id += 1;
        s.spaces.push(goble_app::ui::Space::new(
            "Third",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
    }

    open_tab_menu(&mut root, &app, "Second");
    let commands = choose_row(&mut root, &app, "Close other tabs");

    let state = state.borrow();
    assert_eq!(state.spaces.len(), 1, "only the tab the menu belonged to is left");
    assert_eq!(state.spaces[0].name, "Second");
    assert_eq!(state.active_space, 0, "and it is the tab on screen");
    assert!(text_index(&commands, "New Agent").is_none());
    assert!(text_index(&commands, "Third").is_none());
}

/// R73: a colour dot tints the tab, and the dot of the colour the tab already
/// carries clears it again — the toggle warp-new's own dots have.
#[test]
fn a_colour_dot_tints_the_tab_and_the_same_dot_clears_it() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);
    let red = goble_ui::theme::TabColor::Red;

    let commands = open_tab_menu(&mut root, &app, "Second");
    let dot = red_dot(&commands);
    click(&mut root, &app, vec2f(dot.min_x() + 8.0, dot.min_y() + 8.0));
    let commands = frame(&mut root, &app);

    assert_eq!(
        state.borrow().spaces[1].color,
        Some(red),
        "the dot's own colour lands on the tab"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. }
                if *color
                    == red.tint_over(
                        app.theme.color(ColorToken::Surface),
                        goble_ui::theme::TabTint::Resting,
                    )
        )),
        "and the resting tab is filled with it"
    );

    // The same dot again: the tab's colour is given up.
    let commands = open_tab_menu(&mut root, &app, "Second");
    let dot = red_dot(&commands);
    click(&mut root, &app, vec2f(dot.min_x() + 8.0, dot.min_y() + 8.0));
    frame(&mut root, &app);
    assert_eq!(state.borrow().spaces[1].color, None, "the dot toggles it off");
}

/// R73: a press on the menu's own surface never reaches the tab under it —
/// without that, a press in the panel's padding would be counted as a click on
/// the tab the panel hangs over, and two of them would open its rename field.
#[test]
fn a_press_on_the_menus_own_surface_never_reaches_the_tab_under_it() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    let commands = open_tab_menu(&mut root, &app, "Second");
    // Inside the panel and on no row of it — the padding above its first row —
    // and still over the tab the panel hangs from: the panel's top edge is a
    // few pixels below the pointer, which is inside the bar.
    let panel = menu_panel(&commands, &app);
    let slack = vec2f(panel.min_x() + 4.0, panel.min_y() + 3.0);
    assert!(
        (slack.y - goble_app::ui::shell::TOPBAR_HEIGHT).abs() < 12.0,
        "the point is on the tab that opened the menu: {slack:?} against the bar's {}",
        goble_app::ui::shell::TOPBAR_HEIGHT
    );
    for _ in 0..2 {
        click(&mut root, &app, slack);
        frame(&mut root, &app);
    }

    let state = state.borrow();
    assert!(
        !state.space_rename_editing,
        "two presses on the menu's own surface are not a double click on the tab"
    );
    assert!(
        state.space_menu.borrow().is_some(),
        "and they do not close the menu either — they are presses on it"
    );
}

/// R73: a press outside the panel closes the menu, on the frame that follows —
/// the app's own open flag is what the next frame is built from, so a menu that
/// only closed itself would open again.
#[test]
fn a_press_outside_the_tab_menu_closes_it() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    open_tab_menu(&mut root, &app, "Second");
    assert!(
        state.borrow().space_menu.borrow().is_some(),
        "the menu is open"
    );

    send(
        &mut root,
        &app,
        goble_ui::event::DispatchedEvent::MouseDown {
            position: vec2f(600.0, 600.0),
            button: 0,
        },
    );
    let commands = frame(&mut root, &app);

    assert!(
        state.borrow().space_menu.borrow().is_none(),
        "the press outside closed it"
    );
    assert!(
        text_index(&commands, "Rename tab").is_none(),
        "and the next frame draws no panel"
    );
}

/// R73: a second right click on the same tab closes its menu — the toggle
/// warp-new's `ToggleTabRightClickMenu` is.
#[test]
fn a_second_right_click_on_the_same_tab_closes_its_menu() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let (state, mut root) = two_tab_view(&desktop, &app);

    let commands = open_tab_menu(&mut root, &app, "Second");
    let _ = row_origin(&commands, "Rename tab");
    let again = vec2f(
        tab_label_x(&mut root, &app, "Second") + 40.0,
        goble_app::ui::shell::TOPBAR_HEIGHT / 2.0,
    );
    right_click(&mut root, &app, again);
    let commands = frame(&mut root, &app);

    assert!(
        state.borrow().space_menu.borrow().is_none(),
        "the second right click closed it"
    );
    assert!(text_index(&commands, "Rename tab").is_none(), "and it is off the frame");
}

/// The x the tab whose label is `label` draws that label at, on the frame that
/// is already showing.
fn tab_label_x(root: &mut Box<dyn Element>, app: &AppContext, label: &str) -> f32 {
    let commands = frame(root, app);
    tab_label_origin(&commands, label).x
}

/// The Settings→Appearance color wheel paints at its own layout origin (the
/// settings pane's content column), not at the window origin where it would
/// smear over the sidebar and the toolbar. The settings tab is a pane of the
/// main column, so "inside the pane" means the band right of the sidebar.
#[test]
fn settings_color_wheel_paints_inside_the_panel() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    {
        let mut state = state_rc.borrow_mut();
        state.show_workspace_choice = false;
        state.show_llm_key_banner = false;
        state.open_settings_tab(Some(&desktop));
        state.settings_set_page(goble_app::ui::SettingsCategory::Appearance);
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let window = vec2f(1024.0, 768.0);
    let commands = render_element(&mut root, window, &app);
    let pane_left = goble_app::ui::SIDEBAR_WIDTH;
    let pane_right = window.x;

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
            x > pane_left,
            "wheel painted at x={x}, left of the settings pane"
        );
        assert!(
            x < pane_right,
            "wheel painted at x={x}, right of the settings pane"
        );
    }

    // The hue ring is one image fill, also inside the pane.
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
        ring.min_x() >= pane_left && ring.max_x() <= pane_right && ring.max_y() <= window.y,
        "the ring stays inside the pane, got {ring:?}"
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

    let chip = text_origin(&commands, "New Agent").expect("workspace chip");
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

/// A fresh app comes up on the real store with nothing seeded in it — no
/// conversation rows, no transcript — so the pane shows the empty state, and
/// it draws that state centered inside its frame.
#[test]
fn a_fresh_app_shows_the_empty_state_inside_its_frame() {
    let (desktop, _dir) = common::desktop_state();
    assert!(
        desktop.list_chats().is_empty(),
        "the app ships no conversation of its own"
    );

    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, &desktop, None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    let painted = |needle: &str| {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
            _ => None,
        })
    };
    let invitation = painted("Ask anything to get started.").expect("the empty state's invitation");
    let frame = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                corner_radius,
                ..
            } if *corner_radius > 0.0
                && invitation.x >= rect.min_x()
                && invitation.x <= rect.max_x()
                && invitation.y >= rect.min_y()
                && invitation.y <= rect.max_y() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the pane frames its empty state");

    let title = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. }
                if text == "New conversation"
                    && origin.x >= frame.min_x()
                    && origin.x <= frame.max_x()
                    && origin.y >= frame.min_y()
                    && origin.y <= frame.max_y() =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the pane draws the empty state's title inside its frame");
    assert!(
        title.y > frame.min_y(),
        "the title sits below the frame's top edge: {title:?} in {frame:?}"
    );
    assert!(
        frame.min_x() >= 1024.0 / 4.0,
        "the frame belongs to the pane, not the sidebar: {frame:?}"
    );
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

/// One pointer event through the tree, the way the window's own loop sends it.
/// A frame is rendered between events (see [`frame`]), which is when the tree is
/// rebuilt from the state the previous event left behind.
fn send(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    event: goble_ui::event::DispatchedEvent,
) -> bool {
    let mut ctx = goble_ui::elements::EventContext::default();
    root.dispatch_event(&event, &mut ctx, app)
}

/// The fill of the active workspace tab: the one raised, full-height surface in
/// the toolbar band, so a drop aimed at it lands inside the tab strip.
fn active_tab_rect(
    commands: &[RenderCommand],
    app: &AppContext,
) -> goble_ui::geometry::RectF {
    let raised = app.theme.color(ColorToken::SurfaceRaised);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. }
                if *color == raised
                    && rect.min_y() == 0.0
                    && (rect.height() - topbar_height).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the active workspace tab is the filled tab of the strip")
}

/// Mount the app on one space holding a two-pane split, name the tab, and give
/// the pane that will leave a session of its own. Returns the root, the state,
/// the pane that stays and the pane that moves.
fn split_workspace(
    app: &AppContext,
) -> (
    Box<dyn Element>,
    Rc<RefCell<UiState>>,
    Arc<DesktopState>,
    tempfile::TempDir,
    u64,
    u64,
) {
    let (desktop, dir) = common::desktop_state();
    let view = RootView::new(app, &desktop, None);
    let state_rc = view.state_rc();
    let (kept, moved) = {
        let mut state = state_rc.borrow_mut();
        // No first-run band may sit between the toolbar and the pane header the
        // drag starts on.
        state.show_onboarding_tip = false;
        state.show_llm_key_banner = false;
        state.show_workspace_choice = false;
        // A name of the user's own, so the label is not derived from the panes:
        // exactly this one tab is drawn in the strip.
        state.spaces[0].name = "Primary".to_string();
        state.spaces[0].named = true;
        let moved = state.active_pane_id;
        // Split needs its own borrow of the id counter: spaces is borrowed too.
        let mut next_pane_id = state.next_pane_id;
        let kept = state.spaces[0]
            .split(moved, goble_app::ui::SplitDir::Horizontal, &mut next_pane_id)
            .expect("the active pane splits in two");
        state.next_pane_id = next_pane_id;
        state.active_pane_id = moved;
        state.ensure_pane_sessions(None);
        // The pane that leaves owns a conversation: the move must carry it.
        state
            .pane_sessions
            .get_mut(&moved)
            .expect("the pane has a session")
            .conversation_id = "conv-moved".to_string();
        (kept, moved)
    };
    (Box::new(view), state_rc, desktop, dir, kept, moved)
}

/// R33: a pane dragged by its header onto the tab strip becomes a tab of its
/// own there — carrying the pane itself, so its session follows it — while the
/// split it left collapses onto the pane that stayed.
#[test]
fn a_pane_dragged_onto_the_tab_strip_becomes_its_own_tab() {
    use goble_ui::event::DispatchedEvent;

    let app = AppContext::default();
    let (mut root, state_rc, _desktop, _dir, kept, moved) = split_workspace(&app);
    let commands = frame(&mut root, &app);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;

    // The handle is the pane's own header: the top band of the pane, where the
    // header's controls sit — never the body, whose press belongs to the editor.
    let (control_x, header_y) =
        topmost_icon_center(&commands, "maximize-01").expect("the pane header's expand control");
    assert!(
        header_y > topbar_height,
        "the handle is the pane's header, below the toolbar (y={header_y})"
    );
    let press_at = vec2f(control_x - 60.0, header_y);
    assert!(
        send(
            &mut root,
            &app,
            DispatchedEvent::MouseDown {
                position: press_at,
                button: 0,
            },
        ),
        "the pane body takes the press on its header"
    );
    {
        let state = state_rc.borrow();
        let drag = state.pane_drag.as_ref().expect("the pane is lifted");
        assert_eq!(drag.pane_id, moved, "the pressed pane is the lifted one");
        assert_eq!(drag.source_space, 0, "and it comes out of the space on screen");
        assert_eq!(drag.title, "New Agent", "the ghost is titled after the pane");
    }

    // The pointer carries it onto the strip: past the center of the only tab
    // drawn, so the pane lands at the end of the strip.
    let commands = frame(&mut root, &app);
    let tab = active_tab_rect(&commands, &app);
    let drop_at = vec2f(tab.max_x() - 2.0, topbar_height / 2.0);
    assert!(send(
        &mut root,
        &app,
        DispatchedEvent::MouseMove { position: drop_at },
    ));
    let commands = frame(&mut root, &app);

    let title = {
        let state = state_rc.borrow();
        let drag = state.pane_drag.as_ref().expect("the pane is still lifted");
        assert_eq!(
            drag.drop_index,
            Some(1),
            "the insertion point is past the only drawn tab"
        );
        drag.title.clone()
    };

    // The drag is drawn while it is in flight: the pane's title on the card that
    // follows the pointer, and the marker where the pane would land.
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawText { origin, text, .. }
                if text == &title
                    && (origin.x - drop_at.x).abs() < 40.0
                    && (origin.y - drop_at.y).abs() < 40.0
        )),
        "the ghost card draws the pane's title at the pointer"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Accent)
                    && (rect.width() - 2.0).abs() < 0.01
                    && rect.min_y() == 0.0
                    && (rect.height() - topbar_height).abs() < 0.01
        )),
        "the strip marks the boundary the pane lands on"
    );

    // Releasing over the strip performs the move.
    assert!(
        send(
            &mut root,
            &app,
            DispatchedEvent::MouseUp {
                position: drop_at,
                button: 0,
            },
        ),
        "the strip takes the release"
    );

    let state = state_rc.borrow();
    assert_eq!(state.spaces.len(), 2, "the pane became a tab of its own");
    assert!(state.pane_drag.is_none(), "the drag is over");
    assert_eq!(state.spaces[0].name, "Primary", "the source tab kept its name");
    assert!(
        !state.spaces[0].root.contains_leaf(moved),
        "the moved pane left its old space"
    );
    assert_eq!(
        state.spaces[0].root.leaf_kind(kept),
        Some(&goble_app::ui::PaneKind::Chat),
        "the split collapsed onto the pane that stayed"
    );
    assert_eq!(
        state.spaces[1].root.leaf_kind(moved),
        Some(&goble_app::ui::PaneKind::Chat),
        "the new tab holds the moved pane itself, by id"
    );
    assert_eq!(
        state
            .pane_sessions
            .get(&moved)
            .map(|s| s.conversation_id.as_str()),
        Some("conv-moved"),
        "the pane's own conversation travelled with it"
    );
    assert_eq!(state.active_space, 1, "the moved pane's tab is on screen");
    assert_eq!(state.active_pane_id, moved, "and the moved pane holds the focus");
}

/// R33: a pane lifted onto the strip and released anywhere else cancels: the
/// workspace is exactly as it was.
#[test]
fn a_pane_lifted_onto_the_strip_is_dropped_off_it_with_no_change() {
    use goble_ui::event::DispatchedEvent;

    let app = AppContext::default();
    let (mut root, state_rc, _desktop, _dir, kept, moved) = split_workspace(&app);
    let commands = frame(&mut root, &app);
    let (control_x, header_y) =
        topmost_icon_center(&commands, "maximize-01").expect("the pane header's expand control");
    let press_at = vec2f(control_x - 60.0, header_y);
    send(
        &mut root,
        &app,
        DispatchedEvent::MouseDown {
            position: press_at,
            button: 0,
        },
    );

    // The pointer goes onto the strip and then off it, into the pane body, where
    // the release happens: no target, no move.
    let commands = frame(&mut root, &app);
    let tab = active_tab_rect(&commands, &app);
    let over_strip = vec2f(tab.max_x() - 2.0, 8.0);
    send(
        &mut root,
        &app,
        DispatchedEvent::MouseMove {
            position: over_strip,
        },
    );
    let _ = frame(&mut root, &app);
    let away = vec2f(control_x - 60.0, header_y + 200.0);
    send(&mut root, &app, DispatchedEvent::MouseMove { position: away });
    let _ = frame(&mut root, &app);
    assert!(send(
        &mut root,
        &app,
        DispatchedEvent::MouseUp {
            position: away,
            button: 0,
        },
    ));

    let state = state_rc.borrow();
    assert!(state.pane_drag.is_none(), "the drag is over");
    assert_eq!(state.spaces.len(), 1, "no tab was added");
    assert_eq!(state.spaces[0].name, "Primary");
    assert!(
        state.spaces[0].root.contains_leaf(moved) && state.spaces[0].root.contains_leaf(kept),
        "both panes are still in the split they were in"
    );
    assert_eq!(state.active_space, 0, "nothing was switched");
}
