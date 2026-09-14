use alacritty_terminal::vte::ansi::NamedColor;

use super::*;

fn screen(columns: usize, lines: usize, history: usize) -> Screen {
    Screen::new(
        ScreenSize::new(columns, lines),
        ScreenConfig::scrolling(history),
    )
}

#[test]
fn prints_text() {
    let mut s = screen(20, 3, 0);
    s.feed(b"hello");
    assert_eq!(s.text(), "hello");
    assert_eq!(s.cursor().column, 5);
    assert_eq!(s.cursor().line, 0);
    assert!(s.cursor().visible);
}

#[test]
fn interprets_cursor_addressing() {
    let mut s = screen(20, 5, 0);
    s.feed(b"one\x1b[3;5Htwo");
    let lines: Vec<String> = s.lines().iter().map(|l| l.text()).collect();
    assert_eq!(lines[0], "one");
    assert_eq!(lines[2], "    two");
}

#[test]
fn interprets_sgr_colour_and_attributes() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b[1;31mred\x1b[0m");
    let line = s.line(0).unwrap();
    assert_eq!(line.cells[0].ch, 'r');
    assert!(line.cells[0].attrs.bold);
    // `SGR 31` is the palette's red (index 1), which the palette names.
    assert_eq!(line.cells[0].fg, ScreenColor::Named(NamedColor::Red));
    // After the reset the attributes are gone.
    s.feed(b"plain");
    let line = s.line(0).unwrap();
    assert_eq!(line.cells[3].ch, 'p');
    assert!(!line.cells[3].attrs.bold);
    assert_eq!(line.cells[3].fg, ScreenColor::Named(NamedColor::Foreground));
}

#[test]
fn indexed_colour_survives_as_an_index() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b[38;5;208mx");
    assert_eq!(s.line(0).unwrap().cells[0].fg, ScreenColor::Indexed(208));
}

#[test]
fn truecolor_is_reported_verbatim() {
    let mut s = screen(10, 2, 0);
    s.feed(b"\x1b[38;2;18;52;86mx");
    assert_eq!(s.line(0).unwrap().cells[0].fg, ScreenColor::Rgb(18, 52, 86));
}

#[test]
fn wide_characters_occupy_two_cells() {
    let mut s = screen(10, 2, 0);
    s.feed("日本".as_bytes());
    let line = s.line(0).unwrap();
    assert_eq!(line.cells[0].ch, '日');
    assert!(line.cells[0].attrs.wide);
    assert!(line.cells[1].attrs.wide_spacer);
    assert_eq!(line.cells[2].ch, '本');
    // The spacer is not part of the text.
    assert_eq!(line.text(), "日本");
}

#[test]
fn combining_marks_stay_with_their_cell() {
    let mut s = screen(10, 2, 0);
    s.feed("e\u{0301}".as_bytes());
    let line = s.line(0).unwrap();
    assert_eq!(line.cells[0].ch, 'e');
    assert_eq!(line.cells[0].zerowidth, vec!['\u{0301}']);
    assert_eq!(line.text(), "e\u{0301}");
}

#[test]
fn alt_screen_swaps_and_restores() {
    let mut s = screen(20, 3, 0);
    s.feed(b"primary");
    s.feed(b"\x1b[?1049h");
    assert!(s.is_alt_screen());
    assert_eq!(s.text(), "", "the alt screen starts blank");
    // Full-screen apps clear and home the cursor themselves, as vim does.
    s.feed(b"\x1b[2J\x1b[Hfullscreen");
    assert_eq!(s.text(), "fullscreen");
    s.feed(b"\x1b[?1049l");
    assert!(!s.is_alt_screen());
    assert_eq!(s.text(), "primary");
}

#[test]
fn alt_screen_has_no_scrollback() {
    let mut s = screen(10, 2, 0);
    s.feed(b"\x1b[?1049h");
    for i in 0..10 {
        s.feed(format!("line{i}\r\n").as_bytes());
    }
    assert_eq!(s.history_size(), 0);
}

#[test]
fn scrollback_grows_when_enabled() {
    let mut s = screen(20, 2, 100);
    for i in 0..10 {
        s.feed(format!("line{i}\r\n").as_bytes());
    }
    assert!(s.history_size() > 0);
    s.scroll_display(1);
    assert_eq!(s.display_offset(), 1);
    s.scroll_to_bottom();
    assert_eq!(s.display_offset(), 0);
}

#[test]
fn bracketed_paste_mode_is_visible_to_the_input_layer() {
    let mut s = screen(10, 2, 0);
    assert!(!s.is_bracketed_paste());
    s.feed(b"\x1b[?2004h");
    assert!(s.is_bracketed_paste());
}

#[test]
fn application_cursor_mode_is_reported_for_key_encoding() {
    let mut s = screen(10, 2, 0);
    assert!(!s.input_mode().app_cursor);
    s.feed(b"\x1b[?1h");
    assert!(s.input_mode().app_cursor);
}

#[test]
fn mouse_and_focus_modes_are_reported_for_input() {
    let mut s = screen(10, 2, 0);
    assert!(!s.input_mode().mouse_reported());

    s.feed(b"\x1b[?1000h\x1b[?1006h");
    let mode = s.input_mode();
    assert!(mode.mouse_click && mode.sgr_mouse);
    assert!(!mode.mouse_drag && !mode.mouse_motion);

    s.feed(b"\x1b[?1000l\x1b[?1002h");
    let mode = s.input_mode();
    assert!(!mode.mouse_click && mode.mouse_drag && mode.sgr_mouse);

    s.feed(b"\x1b[?1004h");
    assert!(s.input_mode().focus_reporting);
    s.feed(b"\x1b[?1004l");
    assert!(!s.input_mode().focus_reporting);
}

#[test]
fn the_deferred_wrap_flag_is_visible() {
    let mut s = screen(4, 2, 0);
    s.feed(b"abcd");
    assert!(s.wrap_pending(), "the last column was written");
    // A cursor move cancels it, which is what keeps a right-hand border
    // lined up in a full-screen application.
    s.feed(b"\x1b[1;1H");
    assert!(!s.wrap_pending());
}

#[test]
fn a_mode_query_is_answered_from_the_grid_state() {
    let mut s = screen(10, 2, 0);
    s.feed(b"\x1b[?1006h");
    s.drain_events();
    s.feed(b"\x1b[?1006$p");
    let replies: Vec<String> = s
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            ScreenEvent::PtyWrite(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(replies, vec!["\x1b[?1006;1$y"], "set, so state 1");

    s.feed(b"\x1b[?1006l\x1b[?1006$p");
    let replies: Vec<String> = s
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            ScreenEvent::PtyWrite(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(replies, vec!["\x1b[?1006;2$y"], "reset, so state 2");
}

#[test]
fn an_unimplemented_mode_query_reports_not_recognised() {
    let mut s = screen(10, 2, 0);
    // 89 is a DEC private mode we do not implement; 0 means "not
    // recognised", which is how an application learns to stop asking.
    s.feed(b"\x1b[?89$p");
    let replies: Vec<String> = s
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            ScreenEvent::PtyWrite(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(replies, vec!["\x1b[?89;0$y"]);
}

#[test]
fn resize_reflows_and_keeps_content() {
    let mut s = screen(20, 6, 100);
    s.feed(b"the quick brown fox jumps");
    assert_eq!(s.text(), "the quick brown fox\njumps");
    s.resize(ScreenSize::new(10, 6));
    let text = s.text();
    assert!(text.contains("brown fox"), "{text:?}");
    assert!(text.contains("jumps"), "{text:?}");
}

#[test]
fn osc_8_hyperlinks_are_exposed() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");
    let cell = &s.line(0).unwrap().cells[0];
    assert_eq!(cell.ch, 'l');
    assert_eq!(cell.hyperlink.as_deref(), Some("https://example.com"));
}

#[test]
fn events_reach_the_listener() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b]0;a title\x07");
    s.feed(b"\x07");
    let events = s.drain_events();
    assert!(
        events.contains(&ScreenEvent::Title("a title".into())),
        "{events:?}"
    );
    assert!(events.contains(&ScreenEvent::Bell), "{events:?}");
    assert!(s.drain_events().is_empty(), "events are drained once");
}

#[test]
fn a_frozen_screen_ignores_further_output() {
    let mut s = screen(20, 5, 0);
    s.feed(b"result");
    s.freeze();
    s.feed(b"\r\nnext prompt");
    assert_eq!(s.text(), "result");
    assert!(s.is_frozen());
    assert!(s.dropped_bytes() > 0);
}

#[test]
fn freezing_twice_is_harmless() {
    let mut s = screen(20, 5, 0);
    s.feed(b"a\r\nb");
    s.freeze();
    s.freeze();
    assert_eq!(s.text(), "a\nb");
}

#[test]
fn a_block_screen_keeps_output_that_scrolled_past_the_viewport() {
    let mut s = Screen::new(ScreenSize::new(20, 3), ScreenConfig::block());
    for index in 0..10 {
        s.feed(format!("line{index}\r\n").as_bytes());
    }
    // The viewport shows the tail...
    assert!(!s.text().contains("line0"), "{:?}", s.text());
    // ...but the block draws its whole grid.
    let content: Vec<String> = s.content_lines().iter().map(ScreenLine::text).collect();
    assert_eq!(content[0], "line0");
    assert_eq!(content[9], "line9");
    assert_eq!(
        s.history_size(),
        content.len() - 3,
        "history and screen are contiguous"
    );
}

#[test]
fn content_survives_a_shrink_that_needs_more_rows() {
    let mut s = Screen::new(ScreenSize::new(20, 3), ScreenConfig::block());
    s.feed(b"a rather long line that has to wrap once the screen is narrow");
    s.resize(ScreenSize::new(10, 3));
    // Rows are hard-wrapped, so joining them back recovers the whole line.
    let joined: String = s.content_lines().iter().map(ScreenLine::text).collect();
    assert_eq!(
        joined,
        "a rather long line that has to wrap once the screen is narrow"
    );
}

#[test]
fn clipboard_read_keeps_the_formatter() {
    let mut s = Screen::new(
        ScreenSize::new(20, 2),
        ScreenConfig::scrolling(0).allow_clipboard_read(),
    );
    // OSC 52 with an empty payload and `?` asks for the clipboard.
    s.feed(b"\x1b]52;c;?\x07");
    let queries = s.drain_queries();
    assert_eq!(queries.len(), 1, "{queries:?}");
    let reply = queries[0]
        .clipboard_reply("copied")
        .expect("a clipboard query");
    assert!(
        reply.contains("Y29waWVk"),
        "base64 of the selection: {reply:?}"
    );
    assert_eq!(s.query_count(), 0, "queries are drained once");
}

#[test]
fn colour_report_keeps_the_formatter_and_the_index() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b]4;1;?\x07");
    let queries = s.drain_queries();
    let query = queries.first().expect("a colour query");
    assert_eq!(query.color_index(), Some(1));
    assert!(
        query.color_reply(2, (1, 2, 3)).is_none(),
        "the wrong index is not answered"
    );
    let reply = query
        .color_reply(1, (255, 0, 0))
        .expect("the requested index");
    assert!(reply.contains("ffff/0000/0000"), "{reply:?}");
}

#[test]
fn query_pending_is_announced_as_an_event() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b]4;1;?\x07");
    assert!(s.drain_events().contains(&ScreenEvent::QueryPending));
}

#[test]
fn clipboard_read_is_denied_unless_enabled() {
    let mut s = screen(20, 2, 0);
    s.feed(b"\x1b]52;c;?\x07");
    assert!(s.drain_queries().is_empty());
    assert!(!s.drain_events().contains(&ScreenEvent::QueryPending));
}

#[test]
fn reset_blanks_the_grid_and_restores_modes() {
    let mut s = screen(20, 3, 0);
    s.feed(b"\x1b[?1h\x1b[31mred text");
    assert!(s.input_mode().app_cursor);
    s.reset();
    assert_eq!(s.text(), "");
    assert!(!s.input_mode().app_cursor);
    assert_eq!(s.cursor().line, 0);
    assert_eq!(s.cursor().column, 0);
}
