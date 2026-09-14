use goble_ui::event::ModifiersState;

use super::*;

    #[test]
    fn parser_defaults_everything_to_terminal_command() {
        // Terminal mode (no active agent conversation): even a multi-word
        // input runs as a shell command — `ls` / `git status` are commands.
        assert_eq!(
            classify_input("ls", false),
            Some(InputClass::TerminalCommand("ls".to_string()))
        );
        assert_eq!(
            classify_input("git status", false),
            Some(InputClass::TerminalCommand("git status".to_string()))
        );
    }

    #[test]
    fn parser_is_agent_prompt_in_agent_mode() {
        // Agent mode (an active agent conversation): free text is a prompt.
        assert_eq!(
            classify_input("hello world", true),
            Some(InputClass::AgentPrompt("hello world".to_string()))
        );
    }

    #[test]
    fn parser_bang_forces_terminal_command_in_agent_mode() {
        // Agent mode + leading `!` => a terminal command with the `!` stripped.
        assert_eq!(
            classify_input("!foo", true),
            Some(InputClass::TerminalCommand("foo".to_string()))
        );
        assert_eq!(
            classify_input("! git status", true),
            Some(InputClass::TerminalCommand("git status".to_string()))
        );
    }

    #[test]
    fn parser_rejects_blank_input() {
        assert_eq!(classify_input("", false), None);
        assert_eq!(classify_input("   ", true), None);
        // A bare `!` in agent mode has no command after it.
        assert_eq!(classify_input("!", true), None);
    }

    #[test]
    fn input_mirror_tracks_appends_backspace_and_clear() {
        let mut mirror = String::new();
        update_input_mirror(&mut mirror, "l", ModifiersState::none());
        update_input_mirror(&mut mirror, "s", ModifiersState::none());
        assert_eq!(mirror, "ls");
        update_input_mirror(&mut mirror, "Backspace", ModifiersState::none());
        assert_eq!(mirror, "l");
        update_input_mirror(&mut mirror, "Enter", ModifiersState::none());
        assert_eq!(mirror, "");
    }

    /// Only typing is mirrored: a key that changes the shell's line in a way
    /// this mirror cannot follow must not invent text for the agent.
    #[test]
    fn the_input_mirror_ignores_keys_it_cannot_follow() {
        let mut mirror = "ls".to_string();
        update_input_mirror(&mut mirror, "ArrowUp", ModifiersState::none());
        update_input_mirror(&mut mirror, "Escape", ModifiersState::none());
        update_input_mirror(&mut mirror, "Home", ModifiersState::none());
        update_input_mirror(&mut mirror, "PageUp", ModifiersState::none());
        assert_eq!(mirror, "ls");
        // Ctrl+C is a keystroke, but not a character: the shell's line is not
        // what the mirror holds any more.
        update_input_mirror(
            &mut mirror,
            "c",
            ModifiersState {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(mirror, "ls");
        // A paste arrives as text and is mirrored whole.
        update_input_mirror(&mut mirror, "hi", ModifiersState::none());
        assert_eq!(mirror, "lshi");
    }
