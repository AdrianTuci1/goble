//! Copy mode, mouse selection and the scrollback find bar, exercised through
//! the pane registry a real pane uses.

use super::*;
use crate::terminal::{Cell, CopyMotion};

/// A session whose screen the test drives directly. The emulator keeps the
/// pane's own scrollback, so rows fed past the viewport become history.
fn session(lines: &str, columns: usize, rows: usize) -> TerminalSession {
    let mut emulator = Emulator::new(columns, rows);
    emulator.feed(lines.as_bytes());
    TerminalSession::with_emulator(emulator)
}

#[test]
fn copy_mode_moves_and_yanks_a_visual_line() {
    let mut registry = TerminalRegistry::default();
    registry
        .sessions
        .insert(1, session("one\r\ntwo", 20, 4));

    registry.enter_copy_mode(1);
    assert!(registry.copy_active(1));
    // The cursor starts where the shell left it, on the second row.
    assert_eq!(registry.pane_ui(1).unwrap().copy.cursor.line, 1);

    registry.move_copy(1, CopyMotion::Top);
    registry.move_copy(1, CopyMotion::LineStart);
    registry.toggle_copy_visual(1);
    registry.move_copy(1, CopyMotion::LineEnd);
    assert_eq!(
        registry.copy_mode_yank_text(1).as_deref(),
        Some("one"),
        "the visual selection is the whole first line"
    );

    registry.finish_copy_yank(1);
    assert!(!registry.copy_active(1), "y leaves copy mode");
    assert!(registry.pane_ui(1).unwrap().selection.is_none());
}

#[test]
fn a_mouse_selection_copies_the_cells_it_covers() {
    let mut registry = TerminalRegistry::default();
    registry
        .sessions
        .insert(1, session("hello world", 20, 4));
    let now = std::time::Instant::now();
    registry.begin_selection(1, Cell::new(0, 0), now);
    registry.extend_selection(1, Cell::new(0, 4));
    assert_eq!(
        registry.selection_text(1).as_deref(),
        Some("hello"),
        "a drag covers both ends inclusively"
    );
    // A reverse drag over the same cells is the same text.
    registry.begin_selection(1, Cell::new(0, 4), now);
    registry.extend_selection(1, Cell::new(0, 0));
    assert_eq!(registry.selection_text(1).as_deref(), Some("hello"));
}

#[test]
fn a_find_searches_history_and_the_screen_and_reports_a_bad_regex() {
    let mut registry = TerminalRegistry::default();
    let mut emulator = Emulator::new(20, 2);
    for i in 0..8 {
        emulator.feed(format!("row{i}\r\n").as_bytes());
    }
    registry
        .sessions
        .insert(1, TerminalSession::with_emulator(emulator));

    registry.open_find(1);
    assert!(registry.find_open(1));
    registry.set_find_query(1, "row1".to_string());
    let find = registry.pane_ui(1).unwrap().find.clone();
    assert_eq!(find.matches.len(), 1, "{:?}", find.matches);
    assert_eq!(find.count_label(), "1/1");
    let history = registry.sessions.get(&1).unwrap().history_len();
    assert!(find.matches[0].line < history, "the match is in history");
    // The find auto-scrolled to the match.
    assert!(
        registry.sessions.get(&1).unwrap().view().display_offset > 0,
        "the viewport walked back to the match"
    );

    registry.find_next(1);
    assert_eq!(
        registry.pane_ui(1).unwrap().find.count_label(),
        "1/1",
        "the single match wraps to itself"
    );

    registry.toggle_find_regex(1);
    registry.set_find_query(1, "(".to_string());
    let find = registry.pane_ui(1).unwrap().find.clone();
    assert!(find.matches.is_empty());
    assert!(
        find.error.as_deref().is_some_and(|e| e.contains("invalid regex")),
        "{:?}",
        find.error
    );

    registry.close_find(1);
    assert!(!registry.find_open(1), "Esc closes the find bar");
    assert!(registry.pane_ui(1).unwrap().find.matches.is_empty());
}

#[test]
fn a_copy_mode_selection_yields_to_the_mouse_selection_for_cmd_c() {
    let mut registry = TerminalRegistry::default();
    registry
        .sessions
        .insert(1, session("alpha beta", 20, 4));
    registry.begin_selection(1, Cell::new(0, 6), std::time::Instant::now());
    registry.extend_selection(1, Cell::new(0, 9));
    assert_eq!(registry.copy_text(1).as_deref(), Some("beta"));
}
