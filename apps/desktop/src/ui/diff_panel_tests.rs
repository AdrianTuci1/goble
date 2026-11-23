//! The review pane over a real repository: what it draws, and what its topbar,
//! file list and cards do to the view.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};

use crate::root_view::RootView;
use crate::state::{PaneSession, UiState};
use crate::ui::{Pane, PaneKind, Space, SplitDir};

fn window() -> Vector2F {
    vec2f(1024.0, 768.0)
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one commit and the `review` branch checked out.
fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    std::fs::write(dir.path().join("tracked.txt"), "alpha\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "initial"]);
    git(dir.path(), &["checkout", "-q", "-b", "review"]);
    dir
}

/// The real window with one review pane over `cwd` as its only pane.
fn review_root(
    cwd: &str,
) -> (
    RootView,
    Rc<RefCell<UiState>>,
    Arc<DesktopState>,
    tempfile::TempDir,
    AppContext,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Diff {
                cwd: cwd.to_string(),
            },
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
    }
    (view, state, desktop, dir, app)
}

/// One frame of the real tree: layout and paint.
fn frame(root: &mut RootView, app: &AppContext) -> Vec<RenderCommand> {
    let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
    let mut paint_ctx = PaintContext::new(Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// Frames until `ready` holds. The review colours files on a worker thread, so
/// a test that reads the drawn runs has to let a few frames pass, the way the
/// window's idle redraw does.
fn frame_until(
    root: &mut RootView,
    app: &AppContext,
    ready: impl Fn(&[RenderCommand]) -> bool,
) -> Vec<RenderCommand> {
    let start = std::time::Instant::now();
    loop {
        let commands = frame(root, app);
        if ready(&commands) {
            return commands;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "the worker never coloured the file"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// A real click: press, one redraw, release.
fn click(root: &mut RootView, app: &AppContext, position: Vector2F) {
    let mut ctx = EventContext;
    let down = DispatchedEvent::MouseDown {
        position,
        button: 0,
    };
    let _ = root.dispatch_event(&down, &mut ctx, app);
    let _ = frame(root, app);
    let up = DispatchedEvent::MouseUp {
        position,
        button: 0,
    };
    let _ = root.dispatch_event(&up, &mut ctx, app);
}

/// Every text drawn, in paint order.
fn texts(cmds: &[RenderCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn times(cmds: &[RenderCommand], text: &str) -> usize {
    texts(cmds).iter().filter(|drawn| *drawn == text).count()
}

/// The first time `text` is drawn, and where. The sidebar draws a path before
/// the diff list does, so a path's first origin is the list row.
fn text_origin(cmds: &[RenderCommand], text: &str) -> Vector2F {
    cmds.iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text: drawn,
                ..
            } if drawn == text => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the pane draws {text:?}: {:?}", texts(cmds)))
}

/// Click the row that draws `text`: a few points into the glyph run, which is
/// inside the row's own box.
fn click_text(root: &mut RootView, app: &AppContext, text: &str) {
    let commands = frame(root, app);
    let origin = text_origin(&commands, text);
    click(root, app, origin + vec2f(4.0, 4.0));
}

/// Whether a drawing at `y` is on the pane's own topbar row.
fn in_bar_band(y: f32, bar_y: f32) -> bool {
    (y - bar_y).abs() < 12.0
}

/// The y of the pane's own topbar: where its close control was drawn. The app's
/// topbar draws above it, and the review's cards below.
fn pane_bar_y(cmds: &[RenderCommand]) -> f32 {
    cmds.iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "x-close" && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(origin.y)
            }
            _ => None,
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .expect("the pane draws its topbar's close control")
}

/// The first time `text` is drawn on the pane's topbar row. The cards are
/// painted before the bar, so a string drawn in both places (`-1`, a path) has
/// to be read by where it is, not by draw order.
fn bar_text_origin(cmds: &[RenderCommand], text: &str, bar_y: f32) -> Vector2F {
    cmds.iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text: drawn,
                ..
            } if drawn == text && in_bar_band(origin.y, bar_y) => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the pane's topbar draws {text:?}: {:?}", texts(cmds)))
}

/// The y of the review's second topbar: the row the file-list toggle and the
/// commit split share, under the pane's own header.
fn toolbar_y(cmds: &[RenderCommand]) -> f32 {
    cmds.iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "layers-three-01" && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(origin.y)
            }
            _ => None,
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .expect("the review draws its controls bar")
}

/// Whether a drawing at `y` is on the review's second topbar.
fn in_toolbar_band(y: f32, toolbar_y: f32) -> bool {
    (y - toolbar_y).abs() < 14.0
}

/// Where the review's second topbar draws the first `name` icon. The pane's
/// own header is above it and the cards below, so the bar's band tells the
/// review's controls from the ones a card carries.
fn toolbar_icon_origin(cmds: &[RenderCommand], name: &str) -> Vector2F {
    // The bar draws after the cards now, and a card's header has a
    // `chevron-down` of its own: the band is what tells the two apart.
    let bar_y = toolbar_y(cmds);
    // The controls bar can draw the same glyph twice (the ReviewTab selector
    // and the commit split both carry a chevron): the trailing one is the
    // control the bar ends with, which is what these tests press.
    cmds.iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon {
                origin,
                name: drawn,
                ..
            } if drawn == name && in_toolbar_band(origin.y, bar_y) => Some(*origin),
            _ => None,
        })
        .last()
        .unwrap_or_else(|| panic!("the review's controls bar draws the {name:?} icon"))
}

/// The first time `text` is drawn on the review's controls bar.
fn toolbar_text_origin(cmds: &[RenderCommand], text: &str) -> Vector2F {
    let bar_y = toolbar_y(cmds);
    cmds.iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text: drawn,
                ..
            } if drawn == text && in_toolbar_band(origin.y, bar_y) => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "the review's controls bar draws {text:?}: {:?}",
                texts(cmds)
            )
        })
}

/// A press on one of the review's controls-bar icons.
fn click_toolbar_icon(root: &mut RootView, app: &AppContext, name: &str) {
    let commands = frame(root, app);
    click(
        root,
        app,
        toolbar_icon_origin(&commands, name) + vec2f(6.0, 6.0),
    );
}

#[test]
fn the_topbar_draws_the_tree_totals_and_its_controls_above_the_cards() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "beta\ndelta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();

    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);
    let commands = frame(&mut root, &app);
    let drawn = texts(&commands);

    // The pane's topbar describes the tree: directory, branch and totals. The
    // label it used to lead with is gone.
    let bar_y = text_origin(&commands, "review").y;
    for expected in ["review", "2 files", "+3", "-1"] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the topbar draws {expected:?}: {drawn:?}"
        );
        bar_text_origin(&commands, expected, bar_y);
    }
    // The working directory, shortened the way the app labels a path.
    let working_directory = crate::state::display_path(&cwd);
    assert!(
        commands.iter().any(|command| match command {
            RenderCommand::DrawText { origin, text, .. } =>
                in_bar_band(origin.y, bar_y)
                    && (text == &working_directory
                        || (text.starts_with('…')
                            && working_directory.ends_with(text.trim_start_matches('…')))),
            _ => false,
        }),
        "the topbar names the working directory {working_directory:?}: {drawn:?}"
    );
    assert_eq!(times(&commands, "Working tree"), 0);
    assert!(
        !drawn.iter().any(|text| text == "Options"),
        "the options tray is gone: {drawn:?}"
    );

    // The controls are their own bar under the topbar, above the cards.
    let toolbar = toolbar_y(&commands);
    assert!(
        toolbar > bar_y + 12.0,
        "the commit and the file-list toggle sit under the topbar: {bar_y} then {toolbar}"
    );
    assert_eq!(
        times(&commands, "tracked.txt"),
        2,
        "the Files Changed rail row and the diff header name the second file"
    );
    let pane_x = text_origin(&commands, "review").x;
    let first_content = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, .. }
                if origin.x > pane_x - 8.0 && origin.y > toolbar + 14.0 =>
            {
                Some(origin.y)
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min);
    assert!(
        first_content > toolbar + 20.0,
        "the review's content starts under its controls bar: {toolbar} then {first_content}"
    );
    for once in ["review", "2 files", "+3", "Commit"] {
        assert_eq!(
            times(&commands, once),
            1,
            "one row draws {once:?}: {drawn:?}"
        );
    }

    // The commit is a split control: the check leads the label inside the
    // accent fill, and the chevron is a fill of its own beside it.
    let accent = app.theme.color(goble_ui::theme::ColorToken::Accent);
    let raised = app.theme.color(goble_ui::theme::ColorToken::SurfaceRaised);
    let bar_fill = |cmds: &[RenderCommand], color: goble_ui::color::ColorU| {
        cmds.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect, color: drawn, ..
            } if *drawn == color
                && in_toolbar_band(rect.min_y() + rect.height() / 2.0, toolbar) =>
            {
                Some(*rect)
            }
            _ => None,
        })
    };
    let primary = bar_fill(&commands, accent).expect("the commit draws its accent fill");
    assert!(
        primary.height() <= 32.0 && primary.height() > 16.0,
        "the primary fits the controls bar: {primary:?}"
    );
    let check = toolbar_icon_origin(&commands, "check");
    let label = toolbar_text_origin(&commands, "Commit");
    assert!(
        check.x < label.x,
        "the check leads the label: {check:?} against {label:?}"
    );
    let chevron = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == raised
                    && rect.min_x() > primary.max_x()
                    && in_toolbar_band(rect.min_y() + rect.height() / 2.0, toolbar) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the chevron draws its own fill beside the primary");
    assert!(
        chevron.min_x() > primary.max_x(),
        "the two halves are separate controls: {primary:?} then {chevron:?}"
    );

    assert_eq!(
        times(&commands, "new.txt"),
        3,
        "the rail, the ReviewTab selector and the new file's header all name it"
    );
    for expected in ["beta", "gamma"] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the diff draws {expected:?}: {drawn:?}"
        );
    }
}

#[test]
fn clicking_a_review_file_opens_the_side_files_tab() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);
    {
        let mut s = state.borrow_mut();
        s.right_sidebar_open = true;
        s.side_panel_maximized = true;
        s.side_split_ratio = 0.75;
        s.side_tabs = vec![crate::state::SideTabKind::Review];
        s.active_side_tab = 0;
        s.select_diff_file(7, None);
    }
    let pane_before = state.borrow().active_pane_id;
    let max_before = state.borrow().spaces[0].root.max_id();

    let commands = frame(&mut root, &app);
    let target = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "new.txt" => Some(*origin),
            _ => None,
        })
        .max_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
        .expect("the Files Changed rail row");
    click(&mut root, &app, target + vec2f(6.0, 4.0));

    let s = state.borrow();
    assert_eq!(
        s.active_side_tab_kind(),
        crate::state::SideTabKind::Files,
        "the Files tab is activated"
    );
    assert!(s.right_sidebar_open);
    assert!(
        s.side_file
            .as_deref()
            .is_some_and(|path| path.ends_with("new.txt")),
        "the tab names the clicked file: {:?}",
        s.side_file
    );
    assert_eq!(s.active_pane_id, pane_before);
    assert_eq!(s.spaces[0].root.max_id(), max_before, "no pane is split");
}

#[test]
fn a_clean_tree_says_there_are_no_open_changes() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "No open changes"),
        "the empty state is the pane's own: {drawn:?}"
    );
    assert!(drawn.iter().any(|text| text == "0 files"));
}

/// Y: selecting a file in the rail expands its inline diff but leaves every
/// file row in the list; there is no "All changes" restoration row.
#[test]
fn selecting_a_file_expands_it_but_keeps_the_whole_list() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);

    {
        let mut s = state.borrow_mut();
        let active = s.active_pane_id;
        s.select_diff_file(active, Some("new.txt".to_string()));
    }
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "gamma"),
        "the new file's diff"
    );
    assert!(
        drawn.iter().any(|text| text == "beta"),
        "the other file row and its diff stay in the list: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|text| text == "All changes"),
        "the rail has no All changes row: {drawn:?}"
    );
    assert_eq!(
        state.borrow().pane_controls[&state.borrow().active_pane_id]
            .diff_selected
            .borrow()
            .as_deref(),
        Some("new.txt")
    );
}

#[test]
fn the_file_list_icon_shows_and_hides_the_rail() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    // R: the Files Changed rail is visible by default.
    let commands = frame(&mut root, &app);
    let drawn = texts(&commands);
    assert_eq!(
        times(&commands, "new.txt"),
        3,
        "the rail row, the ReviewTab selector and the diff header all name it: {drawn:?}"
    );

    click_toolbar_icon(&mut root, &app, "layers-three-01");
    let commands = frame(&mut root, &app);
    assert_eq!(
        times(&commands, "new.txt"),
        2,
        "hiding leaves the path in the ReviewTab selector and diff header: {:?}",
        texts(&commands)
    );

    click_toolbar_icon(&mut root, &app, "layers-three-01");
    let commands = frame(&mut root, &app);
    assert_eq!(
        times(&commands, "new.txt"),
        3,
        "showing it again brings the rail row back: {:?}",
        texts(&commands)
    );
}

/// A file past the row cap opens folded, and its own header opens it. The fold
/// state is set from what the row wants, so one click on a default-folded file
/// is enough.
#[test]
fn a_large_file_starts_folded_and_its_header_opens_it() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let body: String = (1..=700).map(|index| format!("line-{index}\n")).collect();
    std::fs::write(repo.path().join("big.txt"), body).unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame(&mut root, &app);
    let drawn = texts(&commands);
    assert_eq!(
        times(&commands, "big.txt"),
        3,
        "the rail, the ReviewTab selector and the diff header all name it"
    );
    assert!(
        !drawn.iter().any(|text| text == "line-1"),
        "a long diff starts folded: {drawn:?}"
    );

    let header = text_origin(&commands, "big.txt");
    click(&mut root, &app, header + vec2f(4.0, 4.0));
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "line-1"),
        "one click on the folded header opens the diff"
    );

    click(&mut root, &app, header + vec2f(4.0, 4.0));
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        !drawn.iter().any(|text| text == "line-1"),
        "and another folds it again"
    );
}

/// A directory that is no longer in a work tree says so instead of waiting on a
/// read that will never arrive.
#[test]
fn a_directory_outside_a_repository_says_there_is_no_working_tree() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cwd = dir.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "No working tree here"),
        "the pane names the state: {drawn:?}"
    );
    assert!(!drawn.iter().any(|text| text == "Reading the changes…"));
}

/// Y: a file row expands its diff directly underneath; no second header.
#[test]
fn a_file_shows_its_diff_under_its_row() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let body: String = (1..=10).map(|index| format!("line-{index}\n")).collect();
    std::fs::write(repo.path().join("tracked.txt"), &body).unwrap();
    git(repo.path(), &["add", "tracked.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "body"]);
    std::fs::write(
        repo.path().join("tracked.txt"),
        body.replace("line-10", "changed"),
    )
    .unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame(&mut root, &app);
    let diff = texts(&commands);
    assert!(
        diff.iter().any(|text| text == "changed"),
        "the changed line is drawn without a press: {diff:?}"
    );
    assert!(
        !diff.iter().any(|text| text == "line-1"),
        "a unified diff leaves the untouched head of the file out: {diff:?}"
    );
    assert!(
        !diff.iter().any(|text| text == "File"),
        "the row has no toolbar: {diff:?}"
    );
    assert!(
        !diff.iter().any(|text| text.starts_with("@@")),
        "the inline diff has no second hunk header: {diff:?}"
    );
}

#[test]
fn a_file_card_paints_the_added_and_removed_bands_by_default() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "beta\ndelta\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let insert = app.theme.color(goble_ui::theme::ColorToken::DiffInsertBg);
    let delete = app.theme.color(goble_ui::theme::ColorToken::DiffDeleteBg);
    let commands = frame_until(&mut root, &app, |commands| {
        let bands: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { color, .. } if *color == insert || *color == delete => {
                    Some(*color)
                }
                _ => None,
            })
            .collect();
        bands.contains(&insert) && bands.contains(&delete)
    });
    assert!(
        texts(&commands).iter().any(|text| text == "beta"),
        "the card draws the changed text: {:?}",
        texts(&commands)
    );
    // `FileDiffView` keeps the change bands from the theme's diff colours and
    // draws the text in the comparison's own ink; the gutter markers carry the
    // success/error pair. The band check above is the contract this test pins.
    let ink = app.theme.color(goble_ui::theme::ColorToken::Text);
    let coloured = |text: &str, color: goble_ui::color::ColorU| {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::DrawText { text: drawn, color: drawn_color, .. }
                if drawn == text && *drawn_color == color)
        })
    };
    assert!(
        coloured("beta", ink),
        "the added line is drawn in the comparison's text ink: {:?}",
        texts(&commands)
    );
    assert!(
        coloured("alpha", ink),
        "the removed line is drawn in the comparison's text ink: {:?}",
        texts(&commands)
    );
    // The green/red marker at the gutter is what tells the two apart now that
    // the body text shares one ink.
    let success = app.theme.color(goble_ui::theme::ColorToken::Success);
    let error = app.theme.color(goble_ui::theme::ColorToken::Error);
    let marker = |color: goble_ui::color::ColorU| {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::FillRect { color: drawn, .. } if *drawn == color)
        })
    };
    assert!(marker(success), "the added line keeps its success marker");
    assert!(marker(error), "the removed line keeps its error marker");
    let toolbar = toolbar_y(&commands);
    let gutters: Vec<Vector2F> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if origin.y > toolbar + 14.0 && (text == "1" || text == "2") =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        gutters.len(),
        3,
        "the card numbers its old and new lines in the left gutters: {gutters:?}"
    );
    assert!(
        gutters.iter().all(|origin| origin.x > 300.0),
        "the numbers sit in the card, right of the sidebar, not at the window's left edge: {gutters:?}"
    );
    // DSH's two tiled 3.5em gutters: old number then new number.
    let mut xs: Vec<f32> = gutters.iter().map(|origin| origin.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(
        xs.len(),
        2,
        "old and new line numbers use two gutter columns: {gutters:?}"
    );
}

/// A diff card's changed lines are coloured by the review's worker: the card
/// opens flat and the colour lands a frame or two later, off the paint.
#[test]
fn a_diff_card_is_coloured_beside_the_frame() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(
        repo.path().join("main.rs"),
        "fn main() {\n    let x = 1;\n}\n",
    )
    .unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let highlighted = goble_ui::syntax::highlight("fn main() {", "main.rs")
        .expect("Rust resolves through the internal highlighter");
    let keyword = highlighted[0]
        .iter()
        .find(|span| span.text == "fn")
        .map(|span| span.color)
        .expect("the highlighter styles the keyword");
    let commands = frame_until(&mut root, &app, |commands| {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::DrawText { text, color, .. } if text == "fn" && *color == keyword)
        })
    });
    assert!(
        texts(&commands).iter().any(|text| text == "fn"),
        "the card draws the keyword as its own run: {:?}",
        texts(&commands)
    );
}

#[test]
fn scrolling_through_an_editor_continues_into_the_next_file() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let alpha: String = (1..=100).map(|index| format!("alpha-{index}\n")).collect();
    let beta: String = (1..=100).map(|index| format!("beta-{index}\n")).collect();
    std::fs::write(repo.path().join("alpha.txt"), alpha).unwrap();
    std::fs::write(repo.path().join("beta.txt"), beta).unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);
    {
        let mut s = state.borrow_mut();
        let controls = s.pane_controls_mut(7);
        controls
            .diff_editing
            .borrow_mut()
            .insert("alpha.txt".to_string(), true);
        controls
            .diff_editing
            .borrow_mut()
            .insert("beta.txt".to_string(), true);
    }

    // The first frame measures the region and reads both files; the second is
    // the one that can cull the file below the viewport.
    let _ = frame(&mut root, &app);
    let top = texts(&frame(&mut root, &app));
    assert!(
        top.iter().any(|text| text == "alpha-1"),
        "the first file's text opens the list: {top:?}"
    );
    assert!(
        !top.iter().any(|text| text == "beta-40"),
        "the second file's middle is below the window: {top:?}"
    );

    // Each editor is its text's rows (the 100 newline-terminated lines plus
    // the empty last one) and the body's padding. The second file's line 40
    // sits below the first file's whole block.
    let editor_height = 12.0 + 101.0 * 17.0;
    let beta_body_top = 10.0 + 30.0 + editor_height + 8.0 + 30.0;
    let into_beta = beta_body_top + 39.0 * 17.0;
    {
        let s = state.borrow();
        s.pane_controls(7)
            .diff_scroll
            .borrow_mut()
            .scroll_by(into_beta);
    }
    let scrolled = texts(&frame(&mut root, &app));
    assert!(
        scrolled.iter().any(|text| text == "beta-40"),
        "the scroll continues into the next file: {scrolled:?}"
    );
    assert!(
        !scrolled.iter().any(|text| text == "alpha-1"),
        "and the file above it draws none of its rows off screen: {scrolled:?}"
    );
}

/// A wheel over the review glides: the event aims the column at its target and
/// the following frames travel there, instead of the list landing in the
/// wheel's own step. The glide holds the frame clock open so those frames come
/// at the active rate rather than on the idle heartbeat.
#[test]
fn a_wheel_over_the_review_glides_instead_of_jumping() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let alpha: String = (1..=100).map(|index| format!("alpha-{index}\n")).collect();
    std::fs::write(repo.path().join("alpha.txt"), alpha).unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);
    // Two frames: the first reads and measures, the second paints the boxes
    // the wheel is hit-tested against.
    let _ = frame(&mut root, &app);
    let _ = frame(&mut root, &app);

    let mut ctx = EventContext;
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseMove {
            position: vec2f(700.0, 400.0),
        },
        &mut ctx,
        &app,
    );
    let consumed = root.dispatch_event(&DispatchedEvent::scroll(vec2f(0.0, -40.0)), &mut ctx, &app);
    assert!(consumed, "the wheel lands on the review column");
    let scroll = state.borrow().pane_controls(7).diff_scroll.clone();
    assert_eq!(
        scroll.borrow().offset(),
        0.0,
        "the event aims the glide; it does not land in its own step"
    );
    assert!(
        root.wants_animation(),
        "the glide keeps the frame clock at the active rate"
    );

    let mut frames = 0;
    while root.wants_animation() {
        let _ = frame(&mut root, &app);
        frames += 1;
        assert!(frames < 64, "the glide settles");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert_eq!(
        scroll.borrow().offset(),
        40.0,
        "the column arrived at the wheel's target"
    );
    assert!(frames > 1, "it travelled over more than one frame");
}

/// A long file's editor lays out a window, not every line: the rows far below
/// the viewport are spacers, which is what keeps a review of large files cheap.
#[test]
fn a_large_inline_editor_draws_only_the_rows_in_view() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let body: String = (1..=800).map(|index| format!("row-{index}\n")).collect();
    std::fs::write(repo.path().join("long.txt"), body).unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);
    {
        let mut s = state.borrow_mut();
        let controls = s.pane_controls_mut(7);
        controls
            .diff_editing
            .borrow_mut()
            .insert("long.txt".to_string(), true);
        controls
            .diff_expanded
            .borrow_mut()
            .insert("long.txt".to_string(), true);
    }
    let top = frame(&mut root, &app);
    let drawn = texts(&top);
    assert!(
        drawn.iter().any(|text| text == "row-1"),
        "the first row is drawn: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|text| text == "row-700"),
        "an off-screen row is not part of the frame: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|text| text == "Binary file"),
        "the file was read as text"
    );

    // The editor's own top is the list padding, the file header and the body's
    // top padding; row 700 is that plus 699 line boxes.
    let row_700 = 10.0 + 30.0 + 4.0 + 699.0 * 17.0;
    {
        let mut s = state.borrow_mut();
        s.pane_controls_mut(7)
            .diff_scroll
            .borrow_mut()
            .scroll_by(row_700);
    }
    let scrolled = texts(&frame(&mut root, &app));
    assert!(
        scrolled.iter().any(|text| text == "row-700"),
        "the window follows the scroll: {scrolled:?}"
    );
    assert!(
        !scrolled.iter().any(|text| text == "row-1"),
        "and the rows above it are left out: {scrolled:?}"
    );
}

/// Y: changed files are flat full-width rows with one thin divider under each,
/// not rounded cards or bordered boxes.
#[test]
fn every_file_card_has_a_rounded_border_at_the_column_width() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _desktop, _state, _dir, app) = review_root(&cwd);
    let commands = frame(&mut root, &app);
    let border = app.theme.color(goble_ui::theme::ColorToken::Border);
    let cards: Vec<goble_ui::geometry::RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                color,
                corner_radius,
                ..
            } if *color == border
                && (*corner_radius - 8.0).abs() < 0.01
                && rect.height() > 30.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert!(cards.len() >= 2, "both files wear a card border: {cards:?}");
    let width = cards[0].width();
    assert!(
        cards.iter().all(|card| (card.width() - width).abs() < 0.6),
        "every card spans the same full list column: {cards:?}"
    );
    assert!(
        width > window().x * 0.35,
        "the file column is not a narrow fixed rail: {width}"
    );
}

#[test]
fn a_long_line_swipes_sideways_inside_its_file_box() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let long = format!("/*{}*/", "x".repeat(400));
    let body = format!("{long}\nlet tail = 1;\n");
    std::fs::write(repo.path().join("wide.rs"), body).unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame_until(&mut root, &app, |commands| {
        texts(commands).iter().any(|text| text == "let")
    });
    let before = text_origin(&commands, "let");
    let mut ctx = EventContext;
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseMove {
            position: before + vec2f(2.0, 4.0),
        },
        &mut ctx,
        &app,
    );
    let _ = root.dispatch_event(&DispatchedEvent::scroll(vec2f(-40.0, 0.0)), &mut ctx, &app);
    // The swipe eases over the frames that follow, the way the window runs
    // them; the box is where it lands, not where the event left it.
    let mut frames = 0;
    while root.wants_animation() {
        let _ = frame(&mut root, &app);
        frames += 1;
        assert!(frames < 64, "the sideways glide settles");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let after = text_origin(&frame(&mut root, &app), "let");
    assert!(
        after.x < before.x - 20.0,
        "the file's own box followed the sideways swipe: {before:?} -> {after:?}"
    );
}

/// Type into whatever field holds the keyboard. The tree is rebuilt after each
/// key, the way the window runs, so the field's value and the app's copy stay
/// in step.
fn type_text(root: &mut RootView, app: &AppContext, text: &str) {
    let mut ctx = EventContext;
    for character in text.chars() {
        let _ = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: character.to_string(),
                modifiers: ModifiersState::default(),
            },
            &mut ctx,
            app,
        );
        let _ = frame(root, app);
    }
}

fn press_enter(root: &mut RootView, app: &AppContext) {
    let mut ctx = EventContext;
    let _ = root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState::default(),
        },
        &mut ctx,
        app,
    );
}

fn git_status(state: &Rc<RefCell<UiState>>) -> Option<(String, bool)> {
    state
        .borrow()
        .pane_controls(7)
        .diff_git_status
        .borrow()
        .clone()
}

/// The topbar's Commit button opens the message bar when there is no message,
/// the focused field takes the message, and Enter commits the working tree.
#[test]
fn the_commit_button_takes_a_message_and_commits_the_working_tree() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);

    // A press with no message opens the bar instead of committing nothing.
    click_text(&mut root, &app, "Commit");
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "Commit message"),
        "the bar opens on its message field: {drawn:?}"
    );

    type_text(&mut root, &app, "add beta");
    press_enter(&mut root, &app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut status = git_status(&state);
    while !matches!(status.as_ref(), Some((text, _)) if text == "Committed")
        && std::time::Instant::now() < deadline
    {
        let _ = frame(&mut root, &app);
        status = git_status(&state);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        status,
        Some(("Committed".to_string(), true)),
        "the commit's word reaches the pane"
    );

    let output = std::process::Command::new("git")
        .current_dir(repo.path())
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .expect("git runs");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "add beta",
        "the message became the commit"
    );
}

/// The split menu's Push runs `git push` off the frame, sets the upstream on a
/// branch that has none, and reports the branch arriving at its remote.
#[test]
fn the_split_menu_pushes_the_branch_and_sets_its_upstream() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let remote = tempfile::tempdir().expect("remote dir");
    git(remote.path(), &["init", "-q", "--bare"]);
    let remote_path = remote.path().to_string_lossy().to_string();
    git(
        repo.path(),
        &["remote", "add", "origin", remote_path.as_str()],
    );
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);

    // The menu belongs to the pane's controls, which the first frame creates.
    let _ = frame(&mut root, &app);
    *state
        .borrow()
        .pane_controls(7)
        .diff_commit_menu_open
        .borrow_mut() = true;
    click_text(&mut root, &app, "Push");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut status = git_status(&state);
    while !matches!(status.as_ref(), Some((text, _)) if text == "Pushed")
        && std::time::Instant::now() < deadline
    {
        let _ = frame(&mut root, &app);
        status = git_status(&state);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        status,
        Some(("Pushed".to_string(), true)),
        "the push's word reaches the pane"
    );

    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "--verify", "review"])
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "the branch arrived at the remote: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The commit split button's chevron opens the menu with the two jobs that
/// leave the machine, below the control, and a press outside it closes the
/// menu again.
#[test]
fn the_commit_chevron_opens_the_push_and_pull_request_menu() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    // The first frame creates the pane's controls, which the menu's open flag
    // belongs to.
    let _ = frame(&mut root, &app);
    click_toolbar_icon(&mut root, &app, "chevron-down");
    let commands = frame(&mut root, &app);
    let drawn = texts(&commands);
    for expected in ["Commit", "Push", "Create pull request"] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the split control draws {expected:?}: {drawn:?}"
        );
    }
    let chevron = toolbar_icon_origin(&commands, "chevron-down");
    assert!(
        text_origin(&commands, "Push").y > chevron.y,
        "the menu opens below the control: {chevron:?}"
    );
    // The panel is part of the pane's chrome, painted after the cards: the
    // file list must not cover it.
    let push_at = commands
        .iter()
        .position(
            |command| matches!(command, RenderCommand::DrawText { text, .. } if text == "Push"),
        )
        .expect("the menu draws Push");
    let card_at = commands
        .iter()
        .position(|command| {
            matches!(command, RenderCommand::DrawText { text, .. } if text == "tracked.txt")
        })
        .expect("the card names its file");
    assert!(
        push_at > card_at,
        "the menu is painted above the cards: {push_at} against {card_at}"
    );

    click(&mut root, &app, vec2f(500.0, 700.0));
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        !drawn.iter().any(|text| text == "Push"),
        "a press outside the menu closes it: {drawn:?}"
    );
}

/// A review builds only the boxes the viewport can show: a long list of files
/// does not lay out — or read — the ones far below the window, and scrolling
/// brings them in.
#[test]
fn only_the_files_in_view_are_built() {
    if !git_available() {
        return;
    }
    let repo = repository();
    for index in 1..=24 {
        let body: String = (1..=40)
            .map(|line| format!("file-{index:02}-line-{line}\n"))
            .collect();
        std::fs::write(repo.path().join(format!("file-{index:02}.txt")), body).unwrap();
    }
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);

    let top = texts(&frame(&mut root, &app));
    assert!(
        top.iter().any(|text| text == "file-01.txt"),
        "the first file opens the list: {top:?}"
    );
    assert!(
        !top.iter().any(|text| text == "file-24-line-1"),
        "a file far below the window has no diff body built: {top:?}"
    );
    // The Files Changed rail is a text list and names every file; only a
    // windowful of DIFF BODIES is built.
    let bodies = top
        .iter()
        .filter(|text| text.starts_with("file-") && text.ends_with("-line-1"))
        .count();
    assert!(
        bodies <= 8,
        "only a windowful of diff bodies is built, got {bodies}: {top:?}"
    );

    {
        let s = state.borrow();
        s.pane_controls(7)
            .diff_scroll
            .borrow_mut()
            .scroll_by(100_000.0);
    }
    let bottom = texts(&frame(&mut root, &app));
    assert!(
        bottom.iter().any(|text| text == "file-24-line-40"),
        "scrolling brings the last file's body in: {bottom:?}"
    );
    assert!(
        !bottom.iter().any(|text| text == "file-01-line-1"),
        "and leaves the first file's body out: {bottom:?}"
    );
}
