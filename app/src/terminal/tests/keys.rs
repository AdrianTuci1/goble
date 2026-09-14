use goble_terminal::{MouseAction, TermMode};
use goble_ui::event::ModifiersState;

use super::*;

    #[test]
    fn enter_is_forwarded_plain_and_routed_on_cmd_or_ctrl() {
        use ModifiersState;
        assert_eq!(
            classify_key("Enter", ModifiersState::none(), TermMode::NONE),
            TerminalKeyAction::Forward(b"\r".to_vec())
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    command: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::RouteToAgent
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::RouteToAgent
        );
        // Shift+Enter is still a newline for the shell.
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    shift: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::Forward(b"\r".to_vec())
        );
    }

    #[test]
    fn agent_enter_requires_command_or_ctrl_without_shift() {
        use ModifiersState;
        assert!(!is_agent_enter("Enter", ModifiersState::default()));
        assert!(is_agent_enter(
            "Enter",
            ModifiersState {
                command: true,
                ..Default::default()
            }
        ));
        assert!(is_agent_enter(
            "Enter",
            ModifiersState {
                ctrl: true,
                ..Default::default()
            }
        ));
        assert!(!is_agent_enter(
            "Enter",
            ModifiersState {
                command: true,
                shift: true,
                ..Default::default()
            }
        ));
        assert!(!is_agent_enter("Enter", ModifiersState::none()));
        assert!(!is_agent_enter("a", ModifiersState::default()));
    }

    /// Command belongs to the platform: everything bound to it except the
    /// submit-to-agent key must reach neither the shell nor a menu.
    #[test]
    fn command_combinations_other_than_enter_are_not_shell_input() {
        use ModifiersState;
        let command_c = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert_eq!(
            classify_key("c", command_c, TermMode::NONE),
            TerminalKeyAction::Ignore
        );
        assert_eq!(
            classify_key("Backspace", command_c, TermMode::NONE),
            TerminalKeyAction::Ignore
        );
        // Ctrl+C by contrast is a keystroke: it interrupts the program.
        assert_eq!(
            classify_key(
                "c",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::Forward(vec![0x03])
        );
    }

    #[test]
    fn printable_keys_are_forwarded_as_bytes() {
        assert_eq!(
            classify_key("a", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b"a".to_vec()
        );
        assert_eq!(
            classify_key("A", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b"A".to_vec()
        );
        assert_eq!(
            classify_key("Backspace", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            vec![0x7f]
        );
        assert_eq!(
            classify_key(" ", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b" ".to_vec()
        );
        // A multi-character key arrives from an input method or a paste.
        assert_eq!(
            classify_key("日本", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            "日本".as_bytes().to_vec()
        );
    }

    #[test]
    fn ctrl_letter_sends_ascii_control_code() {
        use ModifiersState;
        assert_eq!(
            classify_key(
                "c",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            vec![0x03],
            "Ctrl+C should interrupt the shell, not type 'c'"
        );
        assert_eq!(
            classify_key(
                "l",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            vec![0x0c],
            "Ctrl+L clears the screen"
        );
    }

    #[test]
    fn alt_prefixes_a_character_with_escape() {
        assert_eq!(
            classify_key(
                "x",
                ModifiersState {
                    alt: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            b"\x1bx".to_vec()
        );
    }

    /// The named keys are encoded for the program, not dropped: a shell reads
    /// arrows, Home/End, the paging keys and the function keys.
    #[test]
    fn named_keys_are_encoded_for_the_program() {
        let key =
            |key: &str| classify_key(key, ModifiersState::none(), TermMode::NONE).unwrap_forward();
        assert_eq!(key("ArrowUp"), b"\x1b[A".to_vec());
        assert_eq!(key("ArrowLeft"), b"\x1b[D".to_vec());
        assert_eq!(key("Home"), b"\x1b[H".to_vec());
        assert_eq!(key("End"), b"\x1b[F".to_vec());
        assert_eq!(key("Delete"), b"\x1b[3~".to_vec());
        assert_eq!(key("PageUp"), b"\x1b[5~".to_vec());
        assert_eq!(key("PageDown"), b"\x1b[6~".to_vec());
        assert_eq!(key("Insert"), b"\x1b[2~".to_vec());
        assert_eq!(key("F1"), b"\x1bOP".to_vec());
        assert_eq!(key("Tab"), b"\t".to_vec());
        assert_eq!(key("Escape"), b"\x1b".to_vec());
    }

    /// An application that took the cursor keys over gets the `SS3` form, which
    /// is the whole reason the mode is negotiated.
    #[test]
    fn application_cursor_mode_changes_the_arrow_encoding() {
        let app_cursor = TermMode {
            app_cursor: true,
            ..TermMode::NONE
        };
        assert_eq!(
            classify_key("ArrowUp", ModifiersState::none(), app_cursor).unwrap_forward(),
            b"\x1bOA".to_vec()
        );
        // With a modifier the parameterised form is used either way.
        assert_eq!(
            classify_key(
                "ArrowUp",
                ModifiersState {
                    shift: true,
                    ..Default::default()
                },
                app_cursor
            )
            .unwrap_forward(),
            b"\x1b[1;2A".to_vec()
        );
    }

    #[test]
    fn pointer_reports_follow_the_negotiated_mode() {
        use goble_terminal::MouseButton;
        use MouseAction;
        // No mouse mode: the application is not listening.
        assert!(mouse_report(
            MouseAction::Press(MouseButton::Left),
            (0, 0),
            TermMode::NONE
        )
        .is_none());

        let clicking = TermMode {
            mouse_click: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert_eq!(
            mouse_report(MouseAction::Press(MouseButton::Left), (4, 7), clicking).unwrap(),
            b"\x1b[<0;8;5M".to_vec(),
            "SGR reports one-based column then line"
        );
        assert_eq!(
            mouse_report(MouseAction::Release(MouseButton::Left), (4, 7), clicking).unwrap(),
            b"\x1b[<0;8;5m".to_vec()
        );
        // Motion is only reported when the application asked for it.
        assert!(mouse_report(MouseAction::Move, (1, 1), clicking).is_none());
        let dragging = TermMode {
            mouse_drag: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert_eq!(
            mouse_report(MouseAction::Drag(MouseButton::Left), (1, 1), dragging).unwrap(),
            b"\x1b[<32;2;2M".to_vec()
        );
        // A wheel is a press with no button, and needs no motion mode.
        assert_eq!(
            mouse_report(MouseAction::WheelUp, (0, 0), clicking).unwrap(),
            b"\x1b[<64;1;1M".to_vec()
        );
    }

    impl TerminalKeyAction {
        fn unwrap_forward(&self) -> Vec<u8> {
            match self {
                TerminalKeyAction::Forward(b) => b.clone(),
                other => panic!("expected Forward, got {other:?}"),
            }
        }
    }
