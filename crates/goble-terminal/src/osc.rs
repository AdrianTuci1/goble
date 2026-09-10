//! In-band `OSC` observation: the working directory and the prompt markers.
//!
//! Two `OSC` numbers carry product signal but are not implemented by the
//! vendored parser, which discards every `OSC` it does not know:
//!
//! * `OSC 7` — the shell's working directory, as a `file://` URI.
//! * `OSC 133` — semantic prompt markers: `A` prompt start, `B` command start,
//!   `C` output start, `D` command end with its exit code.
//!
//! [`OscTap`] sits next to the PTY, in front of the parser, and *observes*
//! these without consuming them: every byte it is given is also given back, so
//! the parser sees exactly the stream it saw before. That is the difference
//! from [`crate::hooks::HookTap`], which must hide our own `DCS` envelope from
//! the parser; here nothing is hidden, so the tap cannot change how a program
//! renders.
//!
//! A sequence split across two PTY reads is reassembled, because a partial
//! `OSC` is held in the state machine while its bytes are passed through.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Upper bound on one `OSC` body. Past this the sequence is abandoned rather
/// than buffered without limit — `OSC 52` clipboard payloads can be megabytes,
/// and none of the numbers we read come close to this.
const MAX_BODY: usize = 8 * 1024;

/// A prompt marker from `OSC 133`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMarker {
    /// `133;A` — the shell is about to draw its prompt.
    PromptStart,
    /// `133;B` — the command line begins: the prompt is done.
    CommandStart,
    /// `133;C` — the command is running; its output follows.
    OutputStart,
    /// `133;D` — the command finished, with its exit code when the shell sent
    /// one.
    CommandEnd { exit_code: Option<i32> },
}

/// Something the tap observed in the byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    /// `OSC 7` — the shell's working directory.
    WorkingDirectory(String),
    /// `OSC 133` — a prompt marker.
    Prompt(PromptMarker),
}

#[derive(Debug)]
enum State {
    Ground,
    /// Saw `ESC`; the next byte decides whether this is an `OSC`.
    Escape,
    /// Inside an `OSC` body, collecting until `BEL` or `ESC \`.
    Body {
        body: Vec<u8>,
        esc: bool,
    },
}

/// Observes `OSC 7` and `OSC 133` in the PTY byte stream.
#[derive(Debug)]
pub struct OscTap {
    state: State,
    dropped: usize,
}

impl Default for OscTap {
    fn default() -> Self {
        Self::new()
    }
}

impl OscTap {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            dropped: 0,
        }
    }

    /// Number of sequences abandoned as malformed or oversized. Observation is
    /// best effort: a bad sequence never fails the session.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Observe `input`, appending every byte to `passthrough` and returning
    /// what was recognised.
    pub fn tap(&mut self, input: &[u8], passthrough: &mut Vec<u8>) -> Vec<OscEvent> {
        let mut events = Vec::new();

        for &byte in input {
            passthrough.push(byte);

            match &mut self.state {
                State::Ground => {
                    if byte == ESC {
                        self.state = State::Escape;
                    }
                }
                State::Escape => match byte {
                    b']' => {
                        self.state = State::Body {
                            body: Vec::new(),
                            esc: false,
                        }
                    }
                    // `ESC ESC` still promises an introducer: stay waiting.
                    ESC => {}
                    _ => self.state = State::Ground,
                },
                State::Body { body, esc } => {
                    if *esc {
                        // Inside a body an `ESC` only means something when it
                        // is followed by the string terminator.
                        if byte == b'\\' {
                            finish(body, &mut events);
                            self.state = State::Ground;
                        } else if byte == ESC {
                            // A second `ESC` keeps the terminator pending.
                        } else {
                            self.dropped += 1;
                            self.state = State::Ground;
                        }
                        continue;
                    }

                    if byte == BEL {
                        finish(body, &mut events);
                        self.state = State::Ground;
                    } else if byte == ESC {
                        *esc = true;
                    } else if body.len() >= MAX_BODY {
                        self.dropped += 1;
                        self.state = State::Ground;
                    } else {
                        body.push(byte);
                    }
                }
            }
        }

        events
    }

    /// Abandon a sequence still in flight at the end of the stream.
    pub fn flush(&mut self) {
        if !matches!(self.state, State::Ground) {
            if let State::Body { .. } = self.state {
                self.dropped += 1;
            }
            self.state = State::Ground;
        }
    }
}

/// Decode a completed body and keep it when it is one of ours.
fn finish(body: &[u8], events: &mut Vec<OscEvent>) {
    if let Some(event) = decode(body) {
        events.push(event);
    }
    // Every other OSC belongs to somebody else, and the parser has it too.
}

/// Decode the numbers we care about, ignoring the rest.
fn decode(body: &[u8]) -> Option<OscEvent> {
    let (number, rest) = split_once(body, b';')?;
    match number {
        b"7" => {
            let uri = std::str::from_utf8(rest).ok()?;
            Some(OscEvent::WorkingDirectory(path_from_uri(uri)?))
        }
        b"133" => {
            let marker = match rest.first()? {
                b'A' => PromptMarker::PromptStart,
                b'B' => PromptMarker::CommandStart,
                b'C' => PromptMarker::OutputStart,
                b'D' => {
                    let code = split_once(rest, b';')
                        .map(|(_, value)| value)
                        .filter(|value| !value.is_empty())
                        .and_then(|value| std::str::from_utf8(value).ok())
                        .and_then(|value| value.trim().parse::<i32>().ok());
                    PromptMarker::CommandEnd { exit_code: code }
                }
                _ => return None,
            };
            Some(OscEvent::Prompt(marker))
        }
        _ => None,
    }
}

fn split_once(input: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    let index = input.iter().position(|&byte| byte == separator)?;
    Some((&input[..index], &input[index + 1..]))
}

/// Turn an `OSC 7` value into a filesystem path.
///
/// The value is a `file://` URI: `file://host/path` or `file:///path`. The
/// authority is dropped (a remote host in a URI we will read locally would be a
/// lie) and percent escapes are decoded, because a shell escapes the spaces and
/// non-ASCII bytes in a path.
pub fn path_from_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // Only a leading empty authority (`file:///path`) or a path is kept; a
    // named host is discarded along with the slash that follows it.
    let path = match rest.find('/') {
        Some(index) => &rest[index..],
        None => "",
    };
    if path.is_empty() {
        return None;
    }
    Some(percent_decode(path))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
            {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observe(input: &[u8]) -> (Vec<OscEvent>, Vec<u8>) {
        let mut tap = OscTap::new();
        let mut out = Vec::new();
        let events = tap.tap(input, &mut out);
        (events, out)
    }

    #[test]
    fn every_byte_is_passed_through() {
        let input = b"\x1b]7;file://host/tmp\x07text";
        let (events, out) = observe(input);
        assert_eq!(out, input, "the parser must see the same stream");
        assert_eq!(events, vec![OscEvent::WorkingDirectory("/tmp".into())]);
    }

    #[test]
    fn the_working_directory_keeps_escaped_characters() {
        let (events, _) = observe(b"\x1b]7;file:///Users/me/My%20Code\x1b\\");
        assert_eq!(
            events,
            vec![OscEvent::WorkingDirectory("/Users/me/My Code".into())]
        );
    }

    #[test]
    fn a_uri_without_a_scheme_is_ignored() {
        let (events, out) = observe(b"\x1b]7;/tmp\x07");
        assert!(events.is_empty());
        assert_eq!(out, b"\x1b]7;/tmp\x07");
    }

    #[test]
    fn prompt_markers_are_decoded() {
        let (events, _) = observe(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07");
        assert_eq!(
            events,
            vec![
                OscEvent::Prompt(PromptMarker::PromptStart),
                OscEvent::Prompt(PromptMarker::CommandStart),
                OscEvent::Prompt(PromptMarker::OutputStart),
            ]
        );
    }

    #[test]
    fn command_end_carries_its_exit_code() {
        let (events, _) = observe(b"\x1b]133;D;130\x07");
        assert_eq!(
            events,
            vec![OscEvent::Prompt(PromptMarker::CommandEnd {
                exit_code: Some(130)
            })]
        );
        let (events, _) = observe(b"\x1b]133;D\x07");
        assert_eq!(
            events,
            vec![OscEvent::Prompt(PromptMarker::CommandEnd {
                exit_code: None
            })]
        );
    }

    #[test]
    fn a_sequence_split_across_reads_is_reassembled() {
        let mut tap = OscTap::new();
        let mut out = Vec::new();
        assert!(tap.tap(b"\x1b]13", &mut out).is_empty());
        assert_eq!(
            tap.tap(b"3;A\x07", &mut out),
            vec![OscEvent::Prompt(PromptMarker::PromptStart)]
        );
        assert_eq!(out, b"\x1b]133;A\x07");
    }

    #[test]
    fn the_bell_terminator_and_the_string_terminator_both_end_a_body() {
        let (events, _) = observe(b"\x1b]133;A\x07\x1b]133;B\x1b\\");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn foreign_osc_numbers_are_ignored_but_passed_on() {
        let input = b"\x1b]0;a title\x07\x1b]52;c;aGk=\x07";
        let (events, out) = observe(input);
        assert!(events.is_empty());
        assert_eq!(out, input);
    }

    #[test]
    fn a_stray_escape_inside_a_body_abandons_it() {
        let mut tap = OscTap::new();
        let mut out = Vec::new();
        tap.tap(b"\x1b]133;A\x1bZ", &mut out);
        assert_eq!(tap.dropped(), 1);
        assert_eq!(out, b"\x1b]133;A\x1bZ", "the bytes still reach the parser");
    }

    #[test]
    fn a_second_escape_keeps_the_terminator_pending() {
        let (events, out) = observe(b"\x1b]133;A\x1b\x1b\\");
        assert_eq!(events.len(), 1);
        assert_eq!(out, b"\x1b]133;A\x1b\x1b\\");
    }

    #[test]
    fn an_oversized_body_is_abandoned() {
        let mut tap = OscTap::new();
        let mut out = Vec::new();
        let mut input = b"\x1b]133;A".to_vec();
        input.extend(std::iter::repeat_n(b'x', MAX_BODY + 10));
        tap.tap(&input, &mut out);
        assert_eq!(tap.dropped(), 1);
    }

    #[test]
    fn a_truncated_sequence_is_counted_at_flush() {
        let mut tap = OscTap::new();
        let mut out = Vec::new();
        tap.tap(b"\x1b]133;", &mut out);
        tap.flush();
        assert_eq!(tap.dropped(), 1);
        assert_eq!(out, b"\x1b]133;", "held bytes are never withheld");
    }

    #[test]
    fn an_escape_that_is_not_an_osc_leaves_the_tap_inert() {
        let (events, out) = observe(b"\x1b[31mred\x1bP>|x\x1b\\");
        assert!(events.is_empty());
        assert_eq!(out, b"\x1b[31mred\x1bP>|x\x1b\\");
        assert_eq!(OscTap::new().dropped(), 0);
    }
}
