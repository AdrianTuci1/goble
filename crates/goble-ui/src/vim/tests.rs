use super::*;

/// Press one key with no modifiers, as the composer does.
fn press(state: &mut VimState, buffer: &mut VimBuffer, key: &str) -> VimOutcome {
    state.handle(key, &ModifiersState::none(), buffer, None)
}

/// Press each key in turn.
fn press_all(state: &mut VimState, buffer: &mut VimBuffer, keys: &[&str]) -> VimOutcome {
    let mut last = VimOutcome::Consumed;
    for key in keys {
        last = press(state, buffer, key);
    }
    last
}

/// Type `text` one character at a time, as insert mode does.
fn type_text(state: &mut VimState, buffer: &mut VimBuffer, text: &str) -> VimOutcome {
    let mut last = VimOutcome::Consumed;
    for c in text.chars() {
        last = press(state, buffer, &c.to_string());
    }
    last
}

fn buffer(text: &str, caret: usize) -> VimBuffer {
    VimBuffer::new(text, caret)
}

/// The reference's own word-motion fixture, so the ported motions can be
/// checked against the expectations its test suite records.
const LINE: &str = "impl<'a, T: TextBuffer + ?Sized + 'a>   Iterator for WordBoundariesVim";

#[test]
fn w_walks_the_words_of_the_line() {
    let chars: Vec<char> = LINE.chars().collect();
    let mut at = 0usize;
    let mut seen = Vec::new();
    for _ in 0..17 {
        at = words::word_start_forward(&chars, at, WordKind::Word);
        seen.push(at);
    }
    assert_eq!(
        seen,
        vec![4, 6, 7, 9, 10, 12, 23, 25, 26, 32, 34, 35, 36, 40, 49, 53, 70],
        "w stops at each run of punctuation and each word"
    );
    assert_eq!(
        words::word_start_forward(&chars, 15, WordKind::Word),
        23,
        "w from inside whitespace skips to the next word"
    );
    assert_eq!(
        words::word_start_forward(&chars, 38, WordKind::Word),
        40,
        "w steps over the punctuation run one character at a time"
    );
}

#[test]
fn capital_w_steps_over_symbols() {
    let chars: Vec<char> = LINE.chars().collect();
    assert_eq!(
        words::word_start_forward(&chars, 0, WordKind::BigWord),
        9,
        "W takes the whole symbol run with the word"
    );
    assert_eq!(
        words::word_start_forward(&chars, 15, WordKind::BigWord),
        23,
        "W skips whitespace the same way"
    );
}

#[test]
fn word_ends_stop_on_the_last_character() {
    let chars: Vec<char> = "foo(bar) baz".chars().collect();
    assert_eq!(
        words::word_end_forward(&chars, 0, WordKind::Word),
        2,
        "e stops on the last character of the word"
    );
    assert_eq!(
        words::word_end_forward(&chars, 4, WordKind::Word),
        6,
        "e from inside a word reaches its end"
    );
    assert_eq!(
        words::word_start_backward(&chars, 9, WordKind::Word),
        7,
        "b steps onto the symbol run before the word"
    );
    assert_eq!(
        words::word_end_backward(&chars, 9, WordKind::Word),
        7,
        "ge steps onto the end of that symbol run"
    );
}

/// The motions against vim 9.1 itself: `b`/`ge` were checked by running
/// `normal!` in a headless vim, so these are its answers, not a reading of them.
#[test]
fn backward_motions_match_vim() {
    let chars: Vec<char> = "aaa bbb ccc".chars().collect();
    assert_eq!(words::word_start_backward(&chars, 8, WordKind::Word), 4);
    assert_eq!(
        words::word_start_backward(&chars, 5, WordKind::Word),
        4,
        "b from inside a word reaches that word's start"
    );
    assert_eq!(
        words::word_start_backward(&chars, 4, WordKind::Word),
        0,
        "b from a word's start reaches the word before"
    );
    assert_eq!(words::word_start_forward(&chars, 0, WordKind::Word), 4);
    assert_eq!(words::word_end_forward(&chars, 0, WordKind::Word), 2);
    assert_eq!(
        words::word_end_backward(&chars, 4, WordKind::Word),
        2,
        "ge from a word's start reaches the end of the word before"
    );
    assert_eq!(
        words::word_end_backward(&chars, 5, WordKind::Word),
        2,
        "ge from inside a word reaches the end of the word before that one"
    );
    assert_eq!(words::word_end_backward(&chars, 8, WordKind::Word), 6);
}

#[test]
fn h_and_l_move_the_beam_without_touching_the_text() {
    let mut state = VimState::new();
    let mut buf = buffer("hello", 0);
    press_all(&mut state, &mut buf, &["Escape"]);
    assert_eq!(state.mode(), VimMode::Normal);
    assert_eq!(press(&mut state, &mut buf, "l"), VimOutcome::Moved);
    assert_eq!(buf.caret, 1);
    assert_eq!(
        press(&mut state, &mut buf, "h"),
        VimOutcome::Moved,
        "h reports a move, not an edit"
    );
    assert_eq!(buf.caret, 0);
    assert_eq!(buf.text, "hello");
    assert_eq!(
        press(&mut state, &mut buf, "h"),
        VimOutcome::Moved,
        "h at the start stays put"
    );
    assert_eq!(buf.caret, 0);
}

#[test]
fn counts_repeat_a_motion_and_an_operator() {
    let mut state = VimState::new();
    let mut buf = buffer("one two three four", 0);
    press_all(&mut state, &mut buf, &["Escape"]);
    press_all(&mut state, &mut buf, &["2", "w"]);
    assert_eq!(buf.caret, 8, "2w lands on the third word");

    // `d2w` deletes two words from the cursor.
    let mut state = VimState::new();
    let mut buf = buffer("one two three four", 0);
    press_all(&mut state, &mut buf, &["Escape", "d", "2", "w"]);
    assert_eq!(buf.text, "three four");
}

#[test]
fn operators_delete_change_and_yank_a_range() {
    let mut state = VimState::new();
    let mut buf = buffer("let value = 42;", 4);
    press_all(&mut state, &mut buf, &["Escape", "d", "w"]);
    assert_eq!(buf.text, "let = 42;", "dw deletes to the next word");
    assert_eq!(buf.caret, 4);

    let mut state = VimState::new();
    let mut buf = buffer("let value = 42;", 4);
    press_all(&mut state, &mut buf, &["Escape", "c", "w"]);
    assert_eq!(
        buf.text, "let  = 42;",
        "cw takes the word only, like ce, and leaves the space"
    );
    assert_eq!(state.mode(), VimMode::Insert, "cw opens insert mode");
    type_text(&mut state, &mut buf, "count");
    press(&mut state, &mut buf, "Escape");
    assert_eq!(buf.text, "let count = 42;");
    assert_eq!(state.mode(), VimMode::Normal);
}

#[test]
fn doubled_operators_take_the_whole_line() {
    let mut state = VimState::new();
    let mut buf = buffer("a whole line", 3);
    press_all(&mut state, &mut buf, &["Escape", "d", "d"]);
    assert_eq!(buf.text, "", "dd clears the one line the input has");

    let mut state = VimState::new();
    let mut buf = buffer("a whole line", 3);
    press_all(&mut state, &mut buf, &["Escape", "c", "c"]);
    assert_eq!(buf.text, "");
    assert_eq!(state.mode(), VimMode::Insert);
    type_text(&mut state, &mut buf, "next");
    press(&mut state, &mut buf, "Escape");
    assert_eq!(buf.text, "next");

    let mut state = VimState::new();
    let mut buf = buffer("keep me", 1);
    press_all(&mut state, &mut buf, &["Escape", "y", "y"]);
    assert_eq!(buf.text, "keep me", "yy leaves the text alone");
}

#[test]
fn x_deletes_and_u_brings_it_back() {
    let mut state = VimState::new();
    let mut buf = buffer("abc", 1);
    press_all(&mut state, &mut buf, &["Escape", "x"]);
    assert_eq!(buf.text, "ac");
    press(&mut state, &mut buf, "u");
    assert_eq!(buf.text, "abc", "u undoes the delete");
    press(&mut state, &mut buf, "u");
    assert_eq!(buf.text, "abc", "undo at the start of history does nothing");
}

#[test]
fn dollar_deletes_to_the_end_and_zero_to_the_start() {
    let mut state = VimState::new();
    let mut buf = buffer("delete this tail", 7);
    press_all(&mut state, &mut buf, &["Escape", "D"]);
    assert_eq!(buf.text, "delete ");

    let mut state = VimState::new();
    let mut buf = buffer("trim the head", 8);
    press_all(&mut state, &mut buf, &["Escape", "d", "0"]);
    assert_eq!(buf.text, " head", "d0 takes everything before the beam");
}

#[test]
fn text_objects_select_words_quotes_and_brackets() {
    let mut state = VimState::new();
    let mut buf = buffer("run cargo build now", 6);
    press_all(&mut state, &mut buf, &["Escape", "d", "i", "w"]);
    assert_eq!(buf.text, "run  build now", "diw takes the word only");

    let mut state = VimState::new();
    let mut buf = buffer("run cargo build now", 6);
    press_all(&mut state, &mut buf, &["Escape", "d", "a", "w"]);
    assert_eq!(buf.text, "run build now", "daw takes the space after it");

    let mut state = VimState::new();
    let mut buf = buffer("say \"hello there\" twice", 8);
    press_all(&mut state, &mut buf, &["Escape", "d", "i", "\""]);
    assert_eq!(buf.text, "say \"\" twice", "di\" keeps the quotes");

    let mut state = VimState::new();
    let mut buf = buffer("call(a, b) again", 6);
    press_all(&mut state, &mut buf, &["Escape", "d", "i", "("]);
    assert_eq!(buf.text, "call() again", "di( keeps the parentheses");

    let mut state = VimState::new();
    let mut buf = buffer("call(a, b) again", 6);
    press_all(&mut state, &mut buf, &["Escape", "d", "a", "("]);
    assert_eq!(buf.text, "call again", "da( takes them");
}

#[test]
fn a_paragraph_object_covers_the_line_the_input_has() {
    let mut state = VimState::new();
    let mut buf = buffer("the only line", 5);
    press_all(&mut state, &mut buf, &["Escape", "d", "i", "p"]);
    assert_eq!(buf.text, "", "dip takes the line");
}

#[test]
fn find_motions_reach_the_character_and_stop_before_it() {
    let mut state = VimState::new();
    let mut buf = buffer("a,b,c", 0);
    press_all(&mut state, &mut buf, &["Escape", "f", ","]);
    assert_eq!(buf.caret, 1, "f lands on the character");

    let mut state = VimState::new();
    let mut buf = buffer("a,b,c", 0);
    press_all(&mut state, &mut buf, &["Escape", "t", ","]);
    assert_eq!(
        press(&mut state, &mut buf, "l"),
        VimOutcome::Moved,
        "the beam still moves over a character"
    );

    let mut state = VimState::new();
    let mut buf = buffer("a,b,c", 0);
    press_all(&mut state, &mut buf, &["Escape", "t", ","]);
    assert_eq!(buf.caret, 0, "t stops one short of the character");

    // `dt,` deletes up to but not including the comma.
    let mut state = VimState::new();
    let mut buf = buffer("abc,def", 0);
    press_all(&mut state, &mut buf, &["Escape", "d", "t", ","]);
    assert_eq!(buf.text, ",def");

    // `;` repeats the search.
    let mut state = VimState::new();
    let mut buf = buffer("a,b,c", 0);
    press_all(&mut state, &mut buf, &["Escape", "f", ",", ";"]);
    assert_eq!(buf.caret, 3, "; finds the next one");
}

#[test]
fn registers_carry_yanked_text_to_a_paste() {
    let mut state = VimState::new();
    let mut buf = buffer("copy me", 0);
    press_all(
        &mut state,
        &mut buf,
        &["Escape", "\"", "a", "y", "y", "P"],
    );
    assert_eq!(
        buf.text, "copy mecopy me",
        "yy yanks into register a, which also fills the unnamed one P pastes from"
    );

    // The unnamed register holds a delete, and `p` pastes it after the beam.
    let mut state = VimState::new();
    let mut buf = buffer("abc", 0);
    press_all(&mut state, &mut buf, &["Escape", "x", "p"]);
    assert_eq!(buf.text, "bac", "p pastes the deleted character after");
}

#[test]
fn visual_mode_selects_a_range_an_operator_takes() {
    let mut state = VimState::new();
    let mut buf = buffer("hello world", 0);
    press_all(&mut state, &mut buf, &["Escape", "v", "l", "l"]);
    assert_eq!(state.mode(), VimMode::Visual(MotionType::Charwise));
    assert_eq!(
        state.selection(buf.caret, buf.text.chars().count()),
        Some(0..3),
        "the selection covers the beam and where it started"
    );
    press(&mut state, &mut buf, "d");
    assert_eq!(buf.text, "lo world");
    assert_eq!(state.mode(), VimMode::Normal);
}

#[test]
fn visual_text_objects_select_the_object() {
    let mut state = VimState::new();
    let mut buf = buffer("run cargo build", 6);
    press_all(&mut state, &mut buf, &["Escape", "v", "i", "w"]);
    assert_eq!(
        state.selection(buf.caret, buf.text.chars().count()),
        Some(4..9),
        "viw selects the word"
    );
    press(&mut state, &mut buf, "c");
    assert_eq!(buf.text, "run  build");
    assert_eq!(state.mode(), VimMode::Insert);
}

#[test]
fn dot_repeats_the_last_change_with_its_typed_text() {
    let mut state = VimState::new();
    let mut buf = buffer("one two three", 0);
    press_all(&mut state, &mut buf, &["Escape", "d", "w"]);
    assert_eq!(buf.text, "two three");
    press(&mut state, &mut buf, ".");
    assert_eq!(buf.text, "three", ". repeats the delete");

    // A change with typed text repeats the text too.
    let mut state = VimState::new();
    let mut buf = buffer("abc", 0);
    press_all(&mut state, &mut buf, &["Escape", "A"]);
    type_text(&mut state, &mut buf, "XY");
    press(&mut state, &mut buf, "Escape");
    assert_eq!(buf.text, "abcXY");
    press(&mut state, &mut buf, ".");
    assert_eq!(buf.text, "abcXYXY", ". appends the same text again");
}

#[test]
fn insert_mode_types_at_the_beam_and_leaves_enter_to_the_host() {
    let mut state = VimState::new();
    let mut buf = buffer("bc", 0);
    press(&mut state, &mut buf, "Escape");
    assert_eq!(
        press(&mut state, &mut buf, "a"),
        VimOutcome::Moved,
        "a only moves the beam, it does not edit"
    );
    type_text(&mut state, &mut buf, "X");
    assert_eq!(buf.text, "bXc");
    assert_eq!(
        press(&mut state, &mut buf, "Enter"),
        VimOutcome::Passthrough,
        "Enter stays the host's, so a command still submits"
    );
    assert_eq!(
        press(&mut state, &mut buf, "ArrowLeft"),
        VimOutcome::Passthrough,
        "the arrows stay the host's too"
    );
    assert_eq!(state.mode(), VimMode::Insert);
    press(&mut state, &mut buf, "Escape");
    assert_eq!(state.mode(), VimMode::Normal);
}

#[test]
fn capital_i_and_a_open_the_ends_of_the_line() {
    let mut state = VimState::new();
    let mut buf = buffer("one two", 4);
    press_all(&mut state, &mut buf, &["Escape", "I"]);
    assert_eq!(buf.caret, 0, "I opens at the first non-blank");
    assert_eq!(state.mode(), VimMode::Insert);

    let mut state = VimState::new();
    let mut buf = buffer("one two", 4);
    press_all(&mut state, &mut buf, &["Escape", "A"]);
    assert_eq!(buf.caret, 7, "A opens after the last character");
    assert_eq!(state.mode(), VimMode::Insert);
}

#[test]
fn escape_hands_the_key_back_only_when_nothing_is_pending() {
    let mut state = VimState::new();
    let mut buf = buffer("text", 0);
    press(&mut state, &mut buf, "Escape");
    assert_eq!(
        press(&mut state, &mut buf, "Escape"),
        VimOutcome::Passthrough,
        "in normal mode with nothing pending the app's Escape runs"
    );
    // A half-typed command swallows it instead.
    press(&mut state, &mut buf, "d");
    assert!(state.is_pending());
    assert_eq!(press(&mut state, &mut buf, "Escape"), VimOutcome::Consumed);
    assert!(!state.is_pending());
}

#[test]
fn cmd_shortcuts_pass_through_and_ctrl_r_redoes() {
    let mut state = VimState::new();
    let mut buf = buffer("abc", 0);
    press_all(&mut state, &mut buf, &["Escape", "x", "u"]);
    assert_eq!(buf.text, "abc", "u puts the character back");
    let mut redo = ModifiersState::none();
    redo.ctrl = true;
    state.handle("r", &redo, &mut buf, None);
    assert_eq!(buf.text, "bc", "Ctrl-R redoes the delete");

    let mut command = ModifiersState::none();
    command.command = true;
    assert_eq!(
        state.handle("k", &command, &mut buf, None),
        VimOutcome::Passthrough,
        "Cmd shortcuts stay the app's"
    );
}

#[test]
fn showcmd_and_selection_track_the_half_typed_command() {
    let mut state = VimState::new();
    let mut buf = buffer("one two", 0);
    press_all(&mut state, &mut buf, &["Escape"]);
    assert_eq!(state.showcmd(), "");
    press_all(&mut state, &mut buf, &["2", "d"]);
    assert_eq!(state.showcmd(), "2d");
    assert!(state.is_pending());
    press(&mut state, &mut buf, "w");
    assert_eq!(state.showcmd(), "");
    assert!(!state.is_pending());
    assert_eq!(state.mode(), VimMode::Normal);
}

#[test]
fn a_register_name_survives_a_pending_operator() {
    let mut state = VimState::new();
    let mut buf = buffer("hello", 0);
    press_all(&mut state, &mut buf, &["Escape", "\"", "_", "x"]);
    assert_eq!(buf.text, "ello", "the black hole register still deletes");
    press(&mut state, &mut buf, "p");
    assert_eq!(
        buf.text, "ello",
        "nothing comes back from the black hole register"
    );
}

#[test]
fn reset_returns_the_input_to_insert_mode() {
    let mut state = VimState::new();
    let mut buf = buffer("text", 2);
    press_all(&mut state, &mut buf, &["Escape", "2", "d"]);
    assert!(state.is_pending());
    state.reset();
    assert_eq!(state.mode(), VimMode::Insert);
    assert!(!state.is_pending());
    assert_eq!(state.showcmd(), "");
}
