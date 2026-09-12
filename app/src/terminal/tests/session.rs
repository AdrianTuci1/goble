use super::*;

    #[test]
    fn a_snapshot_drops_the_blank_rows_below_the_prompt() {
        let session = detached();
        assert!(
            session.snapshot(48).lines.is_empty(),
            "an idle screen contributes no block"
        );
    }

    #[test]
    fn a_snapshot_keeps_the_newest_lines_up_to_the_limit() {
        let session = detached();
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"one\r\ntwo\r\nthree\r\nfour");
        }
        let lines = session.snapshot(3).lines;
        assert_eq!(lines, vec!["two", "three", "four"]);
    }

    #[test]
    fn a_snapshot_keeps_blank_rows_that_separate_content() {
        let session = detached();
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"top\r\n\r\nbottom");
        }
        assert_eq!(session.snapshot(48).lines, vec!["top", "", "bottom"]);
    }

    #[test]
    fn a_view_reports_the_cursor_and_the_modes() {
        let session = detached();
        assert!(!session.view().has_content);
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"\x1b[3;4Hhi\x1b[?25h\x1b[?1h");
        }
        let view = session.view();
        assert!(view.has_content);
        assert_eq!((view.cursor.line, view.cursor.column), (2, 5));
        assert!(view.cursor.visible);
        assert!(view.mode.app_cursor);
    }

    #[test]
    fn resizing_a_session_resizes_its_screen_and_remembers_the_metrics() {
        let mut session = detached();
        session.set_size(30, 100, 8, 16);
        assert_eq!(session.size(), (30, 100));
        assert_eq!(session.view().rows.len(), 30);
        assert!(session.view().rows.iter().all(|row| row.cells.len() == 100));
    }

    #[test]
    fn resizing_to_the_same_size_keeps_the_screen() {
        let mut session = detached();
        session.set_size(30, 100, 8, 16);
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"kept");
        }
        session.set_size(30, 100, 8, 16);
        assert_eq!(session.snapshot(48).lines, vec!["kept"]);
    }

    #[test]
    fn a_failed_session_renders_its_error_instead_of_panicking() {
        let session = TerminalSession::failed("no pty".to_string());
        let lines = session.snapshot(48).lines;
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("no pty"), "{lines:?}");
    }
