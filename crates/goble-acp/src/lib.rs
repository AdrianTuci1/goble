//! `goble-acp`: a stdio [Agent Client Protocol](https://agentclientprotocol.com)
//! server that proxies prompts to the Goble gateway's remote-execution API.
//!
//! An ACP client (T3 Code web/desktop/mobile, or any other ACP client) launches
//! this binary as a "local ACP command". The bridge has no model and no tools of
//! its own: it speaks newline-delimited JSON-RPC 2.0 on stdin/stdout and turns
//! each `session/prompt` into a gateway run, translating the run's SSE events
//! back into ACP `session/update` notifications. The sandbox behind the gateway
//! is the only place code runs; this process never touches the client's
//! filesystem or terminals.
//!
//! Layering:
//!
//! - [`config`] — environment configuration (`GOBLE_GATEWAY_*`, `GOBLE_ACP_*`).
//! - [`gateway`] — HTTP/SSE client for `/api/v1/sessions`, `/api/v1/runs`, with
//!   bearer/PIN auth and `from_index` stream reconnect.
//! - [`mapping`] — gateway run events → ACP `session/update` frames and terminal
//!   stop reasons.
//! - [`server`] — stdio JSON-RPC loop, ACP method dispatch, session registry.
//! - [`prompt`] — text extraction from ACP prompt content blocks.

pub mod config;
pub mod gateway;
pub mod mapping;
pub mod prompt;
pub mod server;

pub use config::AcpConfig;
pub use server::run;
