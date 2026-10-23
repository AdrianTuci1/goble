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
                if name == "layers-three-01"
                    && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
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
    cmds.iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                origin,
                name: drawn,
                ..
            } if drawn == name && in_toolbar_band(origin.y, bar_y) => Some(*origin),
            _ => None,
        })
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
        .unwrap_or_else(|| panic!("the review's controls bar draws {text:?}: {:?}", texts(cmds)))
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
        1,
        "the file list starts collapsed, so only the diff header names it"
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
            } if *drawn == color && in_toolbar_band(rect.min_y() + rect.height() / 2.0, toolbar) => {
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

    assert_eq!(times(&commands, "new.txt"), 1);
    for expected in ["beta", "gamma"] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the diff draws {expected:?}: {drawn:?}"
        );
    }
}

/// The topbar reads the split it is drawn in: a narrow review keeps the
/// shortened directory and gives up the branch and the totals, while the
/// controls stay on their own bar under it.
#[test]
fn a_narrow_review_keeps_only_the_directory_on_its_topbar() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "beta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Split {
            id: 90,
            dir: SplitDir::Horizontal,
            ratio: 0.34,
            first: Box::new(Pane::Leaf {
                id: 9,
                kind: PaneKind::Diff { cwd: cwd.clone() },
            }),
            second: Box::new(Pane::Leaf {
                id: 8,
                kind: PaneKind::Chat,
            }),
        })];
        s.active_space = 0;
        s.active_pane_id = 9;
        s.pane_sessions.insert(
            8,
            PaneSession {
                conversation_id: String::new(),
                draft: String::new(),
                path: String::new(),
                daemon_session_id: String::new(),
            },
        );
    }

    // The first frame records the split's widths; the second builds the review
    // at the width the pane actually has.
    let _ = frame(&mut root, &app);
    let commands = frame(&mut root, &app);
    let bar_y = pane_bar_y(&commands);
    let bar_texts: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if in_bar_band(origin.y, bar_y) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert!(
        !bar_texts.iter().any(|text| text == "review"),
        "the branch gives way in a narrow pane: {bar_texts:?}"
    );
    assert!(
        !bar_texts.iter().any(|text| text == "2 files"),
        "the totals give way too: {bar_texts:?}"
    );
    assert!(
        bar_texts
            .iter()
            .any(|text| text.starts_with('…') || text.contains("Projects")),
        "the directory is still named: {bar_texts:?}"
    );
    let toolbar = toolbar_y(&commands);
    assert!(
        toolbar > bar_y + 12.0,
        "the controls sit on their own bar under the topbar: {bar_y} then {toolbar}"
    );
    assert!(
        toolbar_icon_origin(&commands, "check").y > bar_y + 12.0,
        "the commit moved with them"
    );
}

/// The header's path keeps inside the room the copy control leaves: a deep
/// path is cut there and fades into the card, instead of painting under the
/// control.
#[test]
fn a_long_path_stops_at_the_copy_control_and_fades() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let nested = "alpha-one/beta-two/gamma-three/delta-four/epsilon-five/zeta-six/eta-seven/theta-eight/iota-nine/kappa-ten";
    let dir = repo.path().join(nested);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a_really_long_file_name_for_the_review.txt"),
        "changed\n",
    )
    .unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame(&mut root, &app);
    let copy = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon { origin, name, .. } if name == "copy" => Some(*origin),
            _ => None,
        })
        .expect("the card header draws its copy control");
    let fade = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRectFadeRight { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("the cut path fades into the card");
    assert!(
        fade.max_x() <= copy.x,
        "the path's fade ends before the copy control: {fade:?} against {copy:?}"
    );
    assert!(
        fade.width() > 10.0,
        "the fade is the label's cut edge, not a sliver: {fade:?}"
    );
    let path = format!("{nested}/a_really_long_file_name_for_the_review.txt");
    let start = text_origin(&commands, &path);
    assert!(
        start.x < fade.min_x(),
        "the path starts left of its fade: {start:?} against {fade:?}"
    );
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

#[test]
fn selecting_a_file_narrows_the_view_and_all_changes_restores_it() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    // The list starts collapsed; it is where one file is picked from.
    click_toolbar_icon(&mut root, &app, "layers-three-01");
    click_text(&mut root, &app, "new.txt");
    let drawn = texts(&frame(&mut root, &app));
    assert!(
        drawn.iter().any(|text| text == "gamma"),
        "the file's diff stays"
    );
    assert!(
        !drawn.iter().any(|text| text == "beta"),
        "the other file's diff is out of the view: {drawn:?}"
    );
    assert!(drawn.iter().any(|text| text == "All changes"));

    click_text(&mut root, &app, "All changes");
    let drawn = texts(&frame(&mut root, &app));
    assert!(drawn.iter().any(|text| text == "beta"));
    assert!(drawn.iter().any(|text| text == "gamma"));
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

    click_toolbar_icon(&mut root, &app, "layers-three-01");
    let commands = frame(&mut root, &app);
    let drawn = texts(&commands);
    assert_eq!(
        times(&commands, "new.txt"),
        2,
        "the list row and the diff header both name it: {drawn:?}"
    );

    click_toolbar_icon(&mut root, &app, "layers-three-01");
    let commands = frame(&mut root, &app);
    assert_eq!(
        times(&commands, "new.txt"),
        1,
        "hiding leaves the path in the diff header only: {:?}",
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
        1,
        "the diff header names it, since the file list starts collapsed"
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

/// A file's body is its diff; the header's "File" swaps in the whole text —
/// the lines a unified diff leaves out included — through the same parser and
/// highlighter the file pane uses, and "Diff" puts the diff back.
#[test]
fn a_file_shows_its_diff_and_the_header_opens_the_whole_text() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let body: String = (1..=10).map(|index| format!("line-{index}\n")).collect();
    // Commit the body, then change only its last line: the unified diff then
    // carries three lines of context instead of every line of the file.
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

    let diff = texts(&frame(&mut root, &app));
    assert!(
        diff.iter().any(|text| text == "changed"),
        "the changed line is drawn without a press: {diff:?}"
    );
    assert!(
        !diff.iter().any(|text| text == "line-1"),
        "a unified diff leaves the untouched head of the file out: {diff:?}"
    );
    assert!(
        diff.iter().any(|text| text == "File"),
        "the header offers the file's text: {diff:?}"
    );

    click_text(&mut root, &app, "File");
    let edited = texts(&frame(&mut root, &app));
    assert!(
        edited.iter().any(|text| text == "line-1"),
        "the whole file is parsed and drawn: {edited:?}"
    );
    assert!(
        edited.iter().any(|text| text == "changed"),
        "and its last line too: {edited:?}"
    );

    click_text(&mut root, &app, "Diff");
    let restored = texts(&frame(&mut root, &app));
    assert!(
        !restored.iter().any(|text| text == "line-1"),
        "the diff is back: {restored:?}"
    );
}

/// A file card paints the change itself: the added row's insert band and the
/// removed row's delete band, both from the theme's diff colours.
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
    let success = app.theme.color(goble_ui::theme::ColorToken::Success);
    let error = app.theme.color(goble_ui::theme::ColorToken::Error);
    let coloured = |text: &str, color: goble_ui::color::ColorU| {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::DrawText { text: drawn, color: drawn_color, .. }
                if drawn == text && *drawn_color == color)
        })
    };
    assert!(
        coloured("beta", success),
        "the added line is drawn in the success colour: {:?}",
        texts(&commands)
    );
    assert!(
        coloured("alpha", error),
        "the removed line is drawn in the error colour: {:?}",
        texts(&commands)
    );
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
    let first_x = gutters[0].x;
    assert!(
        gutters.iter().all(|origin| (origin.x - first_x).abs() < 0.5),
        "one gutter column, not one number per side: {gutters:?}"
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

/// Typing in the in-place editor and pressing Cmd+S writes the file through the
/// file pane's own save, exact bytes and all.
#[test]
fn the_inline_editor_saves_the_file_with_cmd_s() {
    if !git_available() {
        return;
    }
    let repo = repository();
    let body: String = (1..=10).map(|index| format!("line-{index}\n")).collect();
    std::fs::write(repo.path().join("tracked.txt"), &body).unwrap();
    let path = repo.path().join("tracked.txt");
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    // The body is the diff; the header's toggle swaps in the file's text.
    click_text(&mut root, &app, "File");
    let commands = frame(&mut root, &app);
    let line = text_origin(&commands, "line-1");
    click(&mut root, &app, line + vec2f(1.0, 4.0));

    let mut ctx = EventContext;
    let typed = DispatchedEvent::KeyDown {
        key: "Z".to_string(),
        modifiers: ModifiersState::none(),
    };
    let _ = root.dispatch_event(&typed, &mut ctx, &app);
    let save = DispatchedEvent::KeyDown {
        key: "s".to_string(),
        modifiers: ModifiersState {
            command: true,
            ..ModifiersState::none()
        },
    };
    let _ = root.dispatch_event(&save, &mut ctx, &app);

    let written = std::fs::read_to_string(&path).expect("the file is written");
    assert!(
        written.contains('Z'),
        "the typed character reached the file: {written:?}"
    );
    assert_ne!(written, body, "and the file differs from what it held");
}

/// The review list is one continuous document: the editor of an open file
/// reads the shared scroll offset against its own top, and scrolling past the
/// end of one file carries into the next.
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
    let consumed = root.dispatch_event(
        &DispatchedEvent::scroll(vec2f(0.0, -40.0)),
        &mut ctx,
        &app,
    );
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

/// The code inside an editor is painted in the internal highlighter's own
/// colours: the file pane's parser and `goble_ui::syntax` do the colouring, and
/// the field draws the runs rather than one flat text colour.
#[test]
fn the_inline_editor_paints_code_in_the_highlighters_colours() {
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

    // The body is the diff; the header's toggle puts the file's text in it.
    click_text(&mut root, &app, "File");
    let commands = frame_until(&mut root, &app, |commands| {
        commands
            .iter()
            .any(|command| matches!(command, RenderCommand::DrawText { text, .. } if text == "fn"))
    });
    let highlighted = goble_ui::syntax::highlight("fn main() {", "main.rs")
        .expect("Rust resolves through the internal highlighter");
    let keyword = highlighted[0]
        .iter()
        .find(|span| span.text == "fn")
        .map(|span| span.color)
        .expect("the highlighter styles the keyword");
    let name = highlighted[0]
        .iter()
        .find(|span| span.text == "main")
        .map(|span| span.color)
        .expect("the highlighter styles the name");
    let drawn: Vec<(String, goble_ui::color::ColorU)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    assert!(
        drawn
            .iter()
            .any(|(text, color)| text == "fn" && *color == keyword),
        "the keyword is drawn in the highlighter's colour: {drawn:?}"
    );
    assert!(
        drawn
            .iter()
            .any(|(text, color)| text == "main" && *color == name),
        "and the name in its own: {drawn:?}"
    );
    assert_ne!(keyword, name, "the palette is doing more than one colour");
}

/// Each changed file is its own bordered box, square-cornered like the rest of
/// the pane.
#[test]
fn every_file_is_drawn_inside_its_own_border() {
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
    let boxes: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } if *color == border && *width == 1.0 => Some((*rect, *corner_radius)),
            _ => None,
        })
        .collect();
    assert!(
        boxes.len() >= 2,
        "both files wear a border: {:?}",
        texts(&commands)
    );
    let file_boxes: Vec<_> = boxes
        .into_iter()
        .filter(|(rect, _)| rect.height() > 30.0)
        .collect();
    assert!(
        file_boxes.len() >= 2
            && file_boxes
                .iter()
                .all(|(rect, radius)| *radius == 8.0 && rect.width() > 200.0),
        "the cards are rounded and enclose a header and a body: {file_boxes:?}"
    );
}

/// The first time `name` is drawn as an icon, and where.
fn icon_origin(cmds: &[RenderCommand], name: &str) -> Vector2F {
    cmds.iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                origin,
                name: drawn,
                ..
            } if drawn == name => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the pane draws the {name:?} icon"))
}

/// A card's header carries warp-new's controls: the fold chevron, copy, link,
/// revert and open-in-editor, in every file's row.
#[test]
fn every_card_header_wears_warp_news_controls() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, _state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame(&mut root, &app);
    let drawn: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    for control in ["copy", "link", "undo", "maximize-01"] {
        assert!(
            drawn.iter().filter(|name| *name == control).count() >= 2,
            "both cards draw {control:?}: {drawn:?}"
        );
    }
    assert!(
        drawn.iter().any(|name| name == "chevron-down"),
        "an open card draws its fold chevron: {drawn:?}"
    );
}

/// Revert asks in the header before it discards a change, then restores the
/// file to `HEAD` off the frame.
#[test]
fn reverting_a_file_asks_first_and_restores_its_head_version() {
    if !git_available() {
        return;
    }
    let repo = repository();
    std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
    let cwd = repo.path().to_string_lossy().to_string();
    let (mut root, state, _desktop, _dir, app) = review_root(&cwd);

    let commands = frame(&mut root, &app);
    let undo = icon_origin(&commands, "undo");
    click(&mut root, &app, undo + vec2f(6.0, 6.0));
    let asked = texts(&frame(&mut root, &app));
    assert!(
        asked.iter().any(|text| text == "Discard changes?"),
        "the header asks before it reverts: {asked:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "alpha\nbeta\n",
        "the first press changes nothing"
    );

    click_text(&mut root, &app, "Revert");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut status = git_status(&state);
    while !matches!(status.as_ref(), Some((text, _)) if text.starts_with("Reverted"))
        && std::time::Instant::now() < deadline
    {
        let _ = frame(&mut root, &app);
        status = git_status(&state);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        status,
        Some(("Reverted tracked.txt".to_string(), true)),
        "the revert's word reaches the pane"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "alpha\n",
        "the file is back to its committed text"
    );
}

/// A box whose lines are wider than the pane swipes sideways on its own: the
/// horizontal offset moves the code, while the list keeps its vertical place.
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
        .position(|command| matches!(command, RenderCommand::DrawText { text, .. } if text == "Push"))
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
        !top.iter().any(|text| text == "file-24.txt"),
        "a file far below the window is not built: {top:?}"
    );
    let headers = top
        .iter()
        .filter(|text| text.starts_with("file-") && text.ends_with(".txt"))
        .count();
    assert!(
        headers <= 8,
        "only a windowful of boxes is built, got {headers}: {top:?}"
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
        bottom.iter().any(|text| text == "file-24.txt"),
        "scrolling brings the last file in: {bottom:?}"
    );
    assert!(
        !bottom.iter().any(|text| text == "file-01.txt"),
        "and leaves the first out: {bottom:?}"
    );
}
