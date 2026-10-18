//! A small Model Context Protocol server over stdio for the Goble app.
//!
//! The server speaks newline-delimited JSON-RPC 2.0, supports the three methods
//! an MCP client needs (`initialize`, `tools/list`, `tools/call`), and turns
//! every tool call into one typed [`goble_control::Command`] on the app's
//! control socket. No MCP SDK is involved: the wire surface is JSON values.
//!
//! Layers:
//! * [`jsonrpc`] — the JSON-RPC envelope helpers and standard error codes.
//! * [`tools`] — the tool names, their input schemas, and argument decoding.
//! * [`server`] — the protocol state machine, generic over [`server::Control`].
//!
//! `goble-mcp [--socket PATH]` runs the server on stdin/stdout. The socket
//! defaults to the control client's default (`$GOBLE_CONTROL_SOCKET`, else
//! `$HOME/.goble/control.sock`).

#![cfg(unix)]
#![warn(missing_debug_implementations)]

pub mod jsonrpc;
pub mod server;
pub mod tools;

pub use server::{serve, Control, McpServer, SocketControl};
pub use tools::{tool_definitions, TOOL_NAMES};

/// The newest MCP protocol revision this server implements.
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// The protocol revisions [`McpServer`] can answer.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2024-11-05", "2025-03-26", "2025-06-18"];

/// The package version reported in `initialize`.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
