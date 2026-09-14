//! Threads, messages, users and authorized keys, persisted as JSON under one
//! base directory.
//!
//! One module per surface of the store: thread CRUD plus participants
//! ([`threads`]), messages and reactions ([`messages`]), the user profile and
//! authorized keys ([`users`]), the JSON persistence layer ([`persist`]) and the
//! one-shot legacy-chat import ([`migrate`]). What the surfaces share — the
//! [`ThreadStore`] handle and the in-memory maps it locks — lives here.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use goble_core::thread::{Thread, ThreadMessage};
use goble_core::user::{AuthorizedKey, UserProfile};
use parking_lot::Mutex;

mod messages;
mod migrate;
mod persist;
mod threads;
mod users;

#[cfg(test)]
mod tests;

pub use migrate::{LegacyChat, LegacyChatMessage};

/// In-memory store for threads, messages, user profile, and authorized keys.
/// Persists to JSON files under the provided base directory.
pub struct ThreadStore {
    threads: Mutex<Vec<Thread>>,
    messages: Mutex<HashMap<String, Vec<ThreadMessage>>>,
    last_read_at: Mutex<HashMap<String, chrono::DateTime<Utc>>>,
    profile: Mutex<Option<UserProfile>>,
    keys: Mutex<Vec<AuthorizedKey>>,
    base_path: PathBuf,
}
