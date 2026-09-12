//! Harness tests, one module per surface of the harness.

use std::sync::Arc;

use chrono::Utc;

use crate::harness::Harness;
use crate::llm::{CompletionResponse, LlmToolCall, MockProvider};
use crate::store::Store;

mod agents;
mod commands;
mod credentials;
mod definitions;
mod entities;
mod files;
mod guide;
mod handoff;
mod mcp;
mod presentation;
mod runner;
mod web;
mod workflows;

fn chat(store: &Store) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store
        .insert_chat(&id, "test", None, None, &now, &now)
        .unwrap();
    id
}

fn harness_with_tool(name: &str, arguments: serde_json::Value) -> (Store, String, Harness) {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc1".to_string(),
                name: name.to_string(),
                arguments,
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    (store, chat_id, harness)
}
