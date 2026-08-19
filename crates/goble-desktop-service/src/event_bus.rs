use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

/// Abstraction over event emission so the service layer stays independent of
/// the UI framework (Tauri, native wgpu, tests, etc.).
pub trait EventBus: Send + Sync {
    /// Emit an event with a JSON-serializable payload.
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Event bus that drops every event. Useful for native UI binaries that do not
/// need Tauri-style broadcasts yet.
#[derive(Default, Clone)]
pub struct NoOpEventBus;

impl EventBus for NoOpEventBus {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
}

/// Event bus that collects emitted events in memory. Useful for tests and for
/// native UIs that poll for changes.
#[derive(Default, Clone)]
pub struct CollectingEventBus {
    events: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl CollectingEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_events(&self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut *self.events.lock())
    }

    pub fn has_event(&self, event: &str) -> bool {
        self.events.lock().iter().any(|(e, _)| e == event)
    }

    pub fn events(&self) -> Vec<(String, serde_json::Value)> {
        self.events.lock().clone()
    }
}

impl EventBus for CollectingEventBus {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        self.events.lock().push((event.to_string(), payload));
    }
}

/// Helper to emit a typed payload that implements [`Serialize`].
pub fn emit_value(bus: &dyn EventBus, event: &str, payload: impl Serialize) {
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    bus.emit(event, value);
}

/// Helper to emit an empty payload.
pub fn emit_empty(bus: &dyn EventBus, event: &str) {
    bus.emit(event, serde_json::Value::Null);
}
