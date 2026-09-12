//! Goble's command-line surface.
//!
//! One module per surface: the clap definitions ([`cli`]), the dispatch that
//! runs a parsed [`Command`] ([`commands`]) and the worker-facing helpers it
//! calls ([`worker`]). Every item that used to live in this file is re-exported
//! here, so `goble_cli::…` names what it always did.

mod cli;
mod commands;
mod worker;

pub use cli::{
    Args, ClusterAction, Command, DeviceAction, IdentityAction, ScheduleAction, SecretAction,
    SnapshotAction,
};
pub use commands::{async_main, main};
