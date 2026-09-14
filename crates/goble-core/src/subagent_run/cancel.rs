use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

/// Why a child stopped when the parent's cancel bit was flipped without going
/// through `Harness::cancel` (the bit is an `Arc` the host holds too).
pub(crate) const PARENT_CANCEL_REASON: &str = "cancelled by the parent agent";

/// The stop signal of one child: the parent's bit, shared by the whole tree
/// (`Harness::cancel`, or the host that owns the same `Arc`), and this child's
/// own bit with the reason a kill gave it. Cloned into the child's task; the
/// registry fires it, the loop reads it.
#[derive(Clone)]
pub(crate) struct ChildCancel {
    parent: Arc<AtomicBool>,
    own: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
}

impl ChildCancel {
    pub(super) fn new(parent: Arc<AtomicBool>) -> Self {
        Self {
            parent,
            own: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    /// Stop this child and remember why. Idempotent: the first reason wins, the
    /// same way the first terminal outcome wins.
    pub(super) fn fire(&self, reason: &str) {
        if !self.own.swap(true, Ordering::Relaxed) {
            *self.reason.lock() = Some(reason.to_string());
        }
    }

    /// `Some(reason)` while the child must not spend another action. The kill
    /// of this child carries its own reason; the parent's bit is the generic one.
    pub(super) fn signal(&self) -> Option<String> {
        if self.own.load(Ordering::Relaxed) {
            return Some(
                self.reason
                    .lock()
                    .clone()
                    .unwrap_or_else(|| PARENT_CANCEL_REASON.to_string()),
            );
        }
        self.parent
            .load(Ordering::Relaxed)
            .then(|| PARENT_CANCEL_REASON.to_string())
    }
}
