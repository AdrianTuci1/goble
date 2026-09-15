use super::*;
use goble_terminal::hooks::{InitShellValue, PrecmdValue};

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

    /// The working directory a hook reports is the pane's shell saying where it
    /// is: `cd` prints nothing, so this channel and OSC 7 are the only places a
    /// prompt that moved shows up. A hook that says nothing about it — or carries
    /// a blank path — leaves the last report standing.
    #[test]
    fn a_hook_reports_the_working_directory_its_shell_is_in() {
        let mut session = detached();
        assert_eq!(session.reported_cwd(), None, "a shell that has said nothing");

        run_hook(
            &mut session,
            HookEvent::InitShell(InitShellValue {
                cwd: Some("/Users/me".to_string()),
                ..Default::default()
            }),
        );
        assert_eq!(session.reported_cwd(), Some("/Users/me"));

        run_hook(
            &mut session,
            HookEvent::Precmd(PrecmdValue {
                pwd: Some("/Users/me/Projects".to_string()),
                ..Default::default()
            }),
        );
        assert_eq!(
            session.reported_cwd(),
            Some("/Users/me/Projects"),
            "the prompt that followed a `cd` reports the new directory"
        );

        run_hook(&mut session, preexec("ls"));
        run_hook(
            &mut session,
            HookEvent::Precmd(PrecmdValue {
                pwd: Some("   ".to_string()),
                ..Default::default()
            }),
        );
        assert_eq!(
            session.reported_cwd(),
            Some("/Users/me/Projects"),
            "a blank report never blanks the directory the pane knows"
        );
    }

    /// A shell is started in a directory — one the app named, or one it picked
    /// for itself when the app named none — so where it starts is where it was
    /// put, never a move. The session still knows that directory; it just never
    /// offers it to the app as a place the pane went. A prompt report is a move.
    #[test]
    fn a_shells_start_is_recorded_but_is_not_a_move() {
        let mut session = TerminalSession::started(Emulator::new(80, 24));

        run_hook(
            &mut session,
            HookEvent::InitShell(InitShellValue {
                cwd: Some("/Users/me".to_string()),
                ..Default::default()
            }),
        );
        assert_eq!(session.reported_cwd(), Some("/Users/me"));
        assert_eq!(session.take_cwd_move(), None, "a start is not a move");

        run_hook(
            &mut session,
            HookEvent::Precmd(PrecmdValue {
                pwd: Some("/Users/me/Projects".to_string()),
                ..Default::default()
            }),
        );
        assert_eq!(
            session.take_cwd_move(),
            Some("/Users/me/Projects".to_string()),
            "the prompt that followed a `cd` reports the move"
        );
        assert_eq!(
            session.take_cwd_move(),
            None,
            "a report the app has been offered is not offered again"
        );

        // The app set the pane's directory itself, and told the shell to move:
        // a report the shell made before that is not where the pane is going.
        run_hook(
            &mut session,
            HookEvent::Precmd(PrecmdValue {
                pwd: Some("/tmp".to_string()),
                ..Default::default()
            }),
        );
        session.retire_reported_cwds();
        assert_eq!(session.take_cwd_move(), None);
    }
