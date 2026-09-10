//! The shell-integration hook channel.
//!
//! Shell integration needs to tell us things the VT stream cannot express: that
//! a command started, what it was, and that it finished with an exit code. That
//! travels as our own envelope embedded in the PTY stream:
//!
//! ```text
//! ESC P $ d <hex(JSON)> ESC \
//! ```
//!
//! [`HookTap`] sits between the PTY and the VT parser. It lifts those envelopes
//! out of the stream, decodes them into [`HookEvent`]s, and passes every other
//! byte through untouched — including foreign `DCS` sequences, which the parser
//! still needs to see. Bytes belonging to a partially received envelope are held
//! until the next chunk, so a hook split across two PTY reads still decodes.
//!
//! This is deliberately the *only* envelope we recognise: one channel, one
//! decoder. A second channel is a second thing to keep in sync with the shell
//! scripts.

use serde::{Deserialize, Serialize};

const PREFIX: &[u8] = b"\x1bP$d";
const STRING_TERMINATOR: &[u8] = b"\x1b\\";
const ESC: u8 = 0x1b;
/// Upper bound on a single hook payload; beyond this the sequence is treated as
/// malformed and dropped rather than buffered without limit.
const MAX_BODY: usize = 64 * 1024;

/// A decoded shell-integration hook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "hook", content = "value", rename_all = "PascalCase")]
pub enum HookEvent {
    /// Shell integration is installed and the session is usable.
    InitShell(InitShellValue),
    /// The shell finished sourcing its startup files.
    Bootstrapped(BootstrappedValue),
    /// A prompt is about to be drawn; carries the block metadata.
    Precmd(PrecmdValue),
    /// A command is about to run.
    Preexec(PreexecValue),
    /// The command finished; this closes the block and opens the next one.
    CommandFinished(CommandFinishedValue),
    /// The shell's line editor reports the current input buffer.
    InputBuffer(InputBufferValue),
    /// The `clear` builtin was invoked.
    Clear,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InitShellValue {
    pub session_id: Option<String>,
    pub shell: Option<String>,
    pub host: Option<String>,
    pub cwd: Option<String>,
    pub honor_ps1: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BootstrappedValue {
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrecmdValue {
    pub pwd: Option<String>,
    pub git_branch: Option<String>,
    pub rprompt: Option<String>,
    pub session_id: Option<String>,
    pub virtual_env: Option<String>,
    pub conda_env: Option<String>,
    pub node_version: Option<String>,
    pub honor_ps1: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PreexecValue {
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandFinishedValue {
    /// Shell exit status. Absent is read as success.
    pub exit_code: i32,
    pub next_block_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InputBufferValue {
    pub buffer: String,
    pub cursor: Option<usize>,
}

/// A hook that could not be decoded. The bytes are dropped and counted; the
/// session never fails because of a malformed hook.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookError {
    #[error("hook payload is not valid hex")]
    BadHex,
    #[error("hook payload is not valid JSON: {0}")]
    BadJson(String),
    #[error("hook payload is larger than {MAX_BODY} bytes")]
    TooLarge,
}

/// Encode a hook into its wire form. Used by tests and by tooling that generates
/// envelopes on the shell side.
pub fn encode_hook(event: &HookEvent) -> Vec<u8> {
    let json = serde_json::to_vec(event).expect("hook payloads are serializable");
    let mut out = Vec::with_capacity(PREFIX.len() + json.len() * 2 + 2);
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(hex::encode(json).as_bytes());
    out.extend_from_slice(STRING_TERMINATOR);
    out
}

/// Decode a hook from the hex body of an envelope (between `ESC P $ d` and the
/// string terminator).
pub fn decode_hook(body: &[u8]) -> Result<HookEvent, HookError> {
    if body.len() > MAX_BODY {
        return Err(HookError::TooLarge);
    }
    let json = hex::decode(body).map_err(|_| HookError::BadHex)?;
    serde_json::from_slice(&json).map_err(|e| HookError::BadJson(e.to_string()))
}

#[derive(Debug)]
enum State {
    /// Passing bytes through.
    Ground,
    /// Bytes matched against a possible envelope start, always starting with ESC.
    Prefix(Vec<u8>),
    /// Collecting the hex body of a confirmed envelope.
    Body(Vec<u8>),
}

/// Splits the PTY byte stream into hook events and pass-through bytes.
#[derive(Debug)]
pub struct HookTap {
    state: State,
    dropped: usize,
}

impl Default for HookTap {
    fn default() -> Self {
        Self::new()
    }
}

impl HookTap {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            dropped: 0,
        }
    }

    /// Number of envelopes dropped as malformed, oversized or undecodable.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Split `input` into pass-through bytes and hook events.
    ///
    /// Bytes of a partial envelope are retained until more input arrives; they
    /// are released by a later call or by [`HookTap::flush`].
    pub fn tap(&mut self, input: &[u8], passthrough: &mut Vec<u8>) -> Vec<HookEvent> {
        let mut hooks = Vec::new();

        for &byte in input {
            match &mut self.state {
                State::Ground => {
                    if byte == ESC {
                        self.state = State::Prefix(vec![ESC]);
                    } else {
                        passthrough.push(byte);
                    }
                }
                State::Prefix(prefix) => {
                    prefix.push(byte);
                    if !PREFIX.starts_with(prefix.as_slice()) {
                        // Not our envelope: release the bytes we held back.
                        passthrough.extend_from_slice(prefix);
                        self.state = State::Ground;
                    } else if prefix.len() == PREFIX.len() {
                        self.state = State::Body(Vec::new());
                    }
                }
                State::Body(body) => {
                    // An ESC inside the payload is only legal as the start of
                    // the terminator, so the byte after it decides.
                    let after_escape = body.last() == Some(&ESC);
                    body.push(byte);
                    if body.ends_with(STRING_TERMINATOR) {
                        let payload = &body[..body.len() - STRING_TERMINATOR.len()];
                        match decode_hook(payload) {
                            Ok(event) => hooks.push(event),
                            Err(err) => {
                                log::warn!("dropping shell hook: {err}");
                                self.dropped += 1;
                            }
                        }
                        self.state = State::Ground;
                    } else if after_escape && byte != b'\\' {
                        log::warn!("dropping malformed shell hook: unescaped ESC in payload");
                        self.dropped += 1;
                        self.state = State::Ground;
                    } else if body.len() > MAX_BODY {
                        log::warn!("dropping oversized shell hook");
                        self.dropped += 1;
                        self.state = State::Ground;
                    }
                }
            }
        }

        hooks
    }

    /// Release any bytes held back at the end of a session. Returns pass-through
    /// bytes that were buffered waiting for a continuation that never came, and
    /// any hook that completed at the very end of the stream.
    pub fn flush(&mut self, passthrough: &mut Vec<u8>) -> Vec<HookEvent> {
        let mut hooks = Vec::new();
        match std::mem::replace(&mut self.state, State::Ground) {
            State::Ground => {}
            State::Prefix(prefix) => passthrough.extend_from_slice(&prefix),
            State::Body(body) => {
                // The body was complete up to the terminator, which never arrived.
                if body.ends_with(STRING_TERMINATOR) {
                    let payload = &body[..body.len() - STRING_TERMINATOR.len()];
                    match decode_hook(payload) {
                        Ok(event) => hooks.push(event),
                        Err(err) => {
                            log::warn!("dropping shell hook: {err}");
                            self.dropped += 1;
                        }
                    }
                } else if !body.is_empty() {
                    self.dropped += 1;
                }
            }
        }
        hooks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(exit_code: i32) -> HookEvent {
        HookEvent::CommandFinished(CommandFinishedValue {
            exit_code,
            next_block_id: Some("b2".into()),
        })
    }

    #[test]
    fn round_trips_a_hook() {
        let wire = encode_hook(&finished(7));
        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let hooks = tap.tap(&wire, &mut out);
        assert_eq!(hooks, vec![finished(7)]);
        assert!(out.is_empty(), "the envelope must not reach the parser");
        assert_eq!(tap.dropped(), 0);
    }

    #[test]
    fn passes_surrounding_bytes_through_verbatim() {
        let mut wire = b"hello \x1b[31mred".to_vec();
        wire.extend_from_slice(&encode_hook(&finished(0)));
        wire.extend_from_slice(b"world\x1b]0;title\x07");

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let hooks = tap.tap(&wire, &mut out);

        assert_eq!(hooks, vec![finished(0)]);
        assert_eq!(out, b"hello \x1b[31mredworld\x1b]0;title\x07");
    }

    #[test]
    fn reassembles_a_hook_split_across_reads() {
        let wire = encode_hook(&finished(3));
        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let mut hooks = Vec::new();

        for chunk in wire.chunks(3) {
            hooks.extend(tap.tap(chunk, &mut out));
        }

        assert_eq!(hooks, vec![finished(3)]);
        assert!(out.is_empty());
    }

    #[test]
    fn passes_foreign_dcs_through() {
        // A DCS that is not ours must reach the parser intact.
        let foreign = b"\x1bP>|goble 1.0\x1b\\";
        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let hooks = tap.tap(foreign, &mut out);
        assert!(hooks.is_empty());
        assert_eq!(out, foreign);
    }

    #[test]
    fn releases_a_prefix_that_turns_out_to_be_foreign() {
        let wire = b"\x1b[0m";
        let mut tap = HookTap::new();
        let mut out = Vec::new();
        // Feed the escape alone first: it is held back.
        tap.tap(b"\x1b", &mut out);
        assert!(out.is_empty());
        // The rest proves it was a CSI, so the whole thing is released.
        tap.tap(b"[0m", &mut out);
        assert_eq!(out, wire);
    }

    #[test]
    fn flush_releases_a_trailing_escape() {
        let mut tap = HookTap::new();
        let mut out = Vec::new();
        tap.tap(b"text\x1b", &mut out);
        assert_eq!(out, b"text");
        tap.flush(&mut out);
        assert_eq!(out, b"text\x1b");
    }

    #[test]
    fn drops_bad_hex_without_panicking() {
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(b"zzzz");
        wire.extend_from_slice(STRING_TERMINATOR);

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let hooks = tap.tap(&wire, &mut out);
        assert!(hooks.is_empty());
        assert!(out.is_empty());
        assert_eq!(tap.dropped(), 1);
    }

    #[test]
    fn drops_unknown_hook_names_without_panicking() {
        let payload = hex::encode(br#"{"hook":"NotAThing"}"#);
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(payload.as_bytes());
        wire.extend_from_slice(STRING_TERMINATOR);

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        assert!(tap.tap(&wire, &mut out).is_empty());
        assert_eq!(tap.dropped(), 1);
    }

    #[test]
    fn drop_does_not_swallow_the_following_bytes() {
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(b"zz");
        wire.extend_from_slice(STRING_TERMINATOR);
        wire.extend_from_slice(b"after");

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        tap.tap(&wire, &mut out);
        assert_eq!(out, b"after");
    }

    #[test]
    fn decodes_every_hook_variant() {
        let events = vec![
            HookEvent::InitShell(InitShellValue {
                session_id: Some("s1".into()),
                shell: Some("zsh".into()),
                host: Some("mac".into()),
                cwd: Some("/tmp".into()),
                honor_ps1: Some(false),
            }),
            HookEvent::Bootstrapped(BootstrappedValue {
                version: Some("1".into()),
            }),
            HookEvent::Precmd(PrecmdValue {
                pwd: Some("/tmp".into()),
                git_branch: Some("main".into()),
                rprompt: None,
                session_id: Some("s1".into()),
                virtual_env: None,
                conda_env: None,
                node_version: None,
                honor_ps1: Some(false),
            }),
            HookEvent::Preexec(PreexecValue {
                command: Some("ls -la".into()),
            }),
            finished(0),
            HookEvent::InputBuffer(InputBufferValue {
                buffer: "ls".into(),
                cursor: Some(2),
            }),
            HookEvent::Clear,
        ];

        let mut wire = Vec::new();
        for event in &events {
            wire.extend_from_slice(&encode_hook(event));
        }

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        assert_eq!(tap.tap(&wire, &mut out), events);
        assert!(out.is_empty());
    }

    #[test]
    fn missing_optional_fields_are_tolerated() {
        let payload = hex::encode(br#"{"hook":"Precmd","value":{"pwd":"/w"}}"#);
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(payload.as_bytes());
        wire.extend_from_slice(STRING_TERMINATOR);

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        let hooks = tap.tap(&wire, &mut out);
        assert_eq!(hooks.len(), 1);
        match &hooks[0] {
            HookEvent::Precmd(v) => {
                assert_eq!(v.pwd.as_deref(), Some("/w"));
                assert!(v.git_branch.is_none());
            }
            other => panic!("unexpected hook: {other:?}"),
        }
    }

    #[test]
    fn command_finished_defaults_to_success_when_exit_code_is_absent() {
        let payload = hex::encode(br#"{"hook":"CommandFinished","value":{}}"#);
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(payload.as_bytes());
        wire.extend_from_slice(STRING_TERMINATOR);

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        assert_eq!(
            tap.tap(&wire, &mut out),
            vec![HookEvent::CommandFinished(CommandFinishedValue {
                exit_code: 0,
                next_block_id: None,
            })]
        );
    }

    #[test]
    fn a_hook_without_its_value_object_is_dropped() {
        // The envelope is adjacently tagged: the payload has to be there even
        // when every field in it is optional.
        let payload = hex::encode(br#"{"hook":"CommandFinished"}"#);
        let mut wire = PREFIX.to_vec();
        wire.extend_from_slice(payload.as_bytes());
        wire.extend_from_slice(STRING_TERMINATOR);

        let mut tap = HookTap::new();
        let mut out = Vec::new();
        assert!(tap.tap(&wire, &mut out).is_empty());
        assert_eq!(tap.dropped(), 1);
    }
}
