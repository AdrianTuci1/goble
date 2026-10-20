//! The control protocol served by the Goble app.
//!
//! The app owns UI state on its winit thread, so the control server runs on a
//! background thread and the app's [`Handler`] forwards UI verbs to that thread
//! with a reply channel. This crate holds only the transport and the typed
//! vocabulary: one newline-delimited JSON message per request and response over
//! a Unix socket, structured errors for everything that fails, and a client
//! that reports a clear connection error when the app is not running.
//!
//! Layers:
//! * [`protocol`] — [`Command`], [`Payload`], [`ControlError`] and the wire
//!   [`Request`] / [`Response`].
//! * [`codec`] — bounded newline framing.
//! * [`server`] — the Unix socket listener, generic over [`Handler`].
//! * [`client`] — connect, [`Command`] in, [`Payload`] out.
//!
//! The control socket is Unix-only, like the PTY daemon it routes `pty.*` to.

#![cfg(unix)]
#![warn(missing_debug_implementations)]

pub mod client;
pub mod codec;
pub mod protocol;
pub mod server;

pub use client::{resolve_socket_path, Client};
pub use codec::{write_message, MessageReader, ProtocolError};
pub use protocol::{
    is_known_verb, AckResult, AgentStateInfo, Command, ControlError, ErrorCode, InboxInfo,
    LayoutInfo, LayoutKind, LayoutNode, PaneInfo, PaneKind, Payload, PromptResult,
    PtySessionInfo, Request, Response, SendResult, SpaceInfo, SplitAxis, MAX_MESSAGE_BYTES, VERBS,
};
pub use server::{Handler, Server};
