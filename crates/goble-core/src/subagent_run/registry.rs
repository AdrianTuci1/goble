use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::harness::HarnessEvent;
use crate::llm::TokenUsage;
use crate::subagent::{SubAgentId, SubAgentRecord};

use super::cancel::ChildCancel;
use super::events::{subagent_finished, subagent_progress, subagent_spawned, subagent_token_usage};
use super::host::ChildDeps;

/// One child as the registry holds it: S1's record, the handle that stops the
/// task writing it, and what the child has spent on the model.
struct ChildEntry {
    record: SubAgentRecord,
    cancel: ChildCancel,
    usage: ChildUsage,
}

/// A child's token spend, summed over its model calls, and the part of it a
/// turn has already been told about. A child keeps calling the model after the
/// turn that spawned it has ended — a background spawn returns at once — and
/// that spend belongs to the parent conversation like any other, so the
/// unreported remainder waits here for the next turn that attaches.
#[derive(Clone, Copy, Default)]
struct ChildUsage {
    total: UsageBuckets,
    reported: UsageBuckets,
}

/// One call's tokens, or a sum of them. `cached` stays absent until a call
/// reports cache accounting, so a provider without one never contributes a zero.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct UsageBuckets {
    input: u64,
    cached: Option<u64>,
    output: u64,
}

impl UsageBuckets {
    fn of(usage: &TokenUsage) -> Self {
        Self {
            input: usage.input,
            cached: usage.cached,
            output: usage.output,
        }
    }

    fn fold(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        if let Some(cached) = other.cached {
            self.cached = Some(self.cached.unwrap_or(0).saturating_add(cached));
        }
    }

    /// What `self` holds beyond `other`: the spend a turn has not heard yet.
    fn since(&self, other: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input),
            cached: match (self.cached, other.cached) {
                (Some(mine), Some(theirs)) => Some(mine.saturating_sub(theirs)),
                (mine, _) => mine,
            },
            output: self.output.saturating_sub(other.output),
        }
    }

    fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cached.unwrap_or(0) == 0
    }
}

impl From<UsageBuckets> for TokenUsage {
    fn from(buckets: UsageBuckets) -> Self {
        Self {
            input: buckets.input,
            cached: buckets.cached,
            output: buckets.output,
        }
    }
}

/// The children one `Harness` has spawned, in spawn order, keyed by
/// `SubAgentId`. Shared by every turn of the harness and cloned into every
/// child, so a nested spawn lands in the same registry and the renderer (S5–S7)
/// and the wire events (S4) have one source to read.
#[derive(Clone, Default)]
pub(crate) struct SubAgentRegistry {
    children: Arc<Mutex<Vec<ChildEntry>>>,
    /// The live turn's S4 sink (S3's `publish`/`register`/`cancel` transitions
    /// push the `SubAgent*` lifecycle events here). `None` while no turn is
    /// attached — a transition then updates the record only, never blocks.
    events: Arc<Mutex<Option<UnboundedSender<HarnessEvent>>>>,
}

impl SubAgentRegistry {
    /// Hand the turn that is starting a receiver for the lifecycle events its
    /// children emit. Attaching retires the previous sink, so events a child
    /// emits after its turn has ended go nowhere — the record stays the source.
    ///
    /// A turn attaching is also when spend a child accumulated with no turn
    /// listening is handed over: the child's calls that happened after its
    /// spawner's turn ended are the parent conversation's spend too.
    pub(crate) fn attach_event_sink(&self) -> UnboundedReceiver<HarnessEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut children = self.children.lock();
            for entry in children.iter_mut() {
                let owed = entry.usage.total.since(&entry.usage.reported);
                if owed.is_zero() {
                    continue;
                }
                entry.usage.reported = entry.usage.total;
                let _ = sender.send(subagent_token_usage(
                    &entry.record.spec.parent_chat_id,
                    &owed.into(),
                ));
            }
        }
        *self.events.lock() = Some(sender);
        receiver
    }

    /// Push one lifecycle event to the attached turn. Unbounded and never
    /// blocking, so it is safe to call from a registry transition.
    fn emit(&self, event: HarnessEvent) {
        if let Some(sender) = self.events.lock().as_ref() {
            let _ = sender.send(event);
        }
    }

    /// Note one of a child's model calls. It joins the child's running spend
    /// and, while a turn is attached, crosses that turn's stream at once,
    /// attributed to the conversation that spawned the child. A call no turn was
    /// there to hear waits on the entry for the next
    /// [`Self::attach_event_sink`] to hand over.
    pub(crate) fn note_child_usage(&self, child_id: &str, parent_chat_id: &str, usage: &TokenUsage) {
        let call = UsageBuckets::of(usage);
        let sender = self.events.lock().clone();
        let mut children = self.children.lock();
        if let Some(entry) = children
            .iter_mut()
            .find(|entry| entry.record.spec.id.0 == child_id)
        {
            entry.usage.total.fold(&call);
            if sender.is_some() {
                entry.usage.reported.fold(&call);
            }
        }
        if let Some(sender) = sender {
            let _ = sender.send(subagent_token_usage(parent_chat_id, usage));
        }
    }

    /// Add a live child before its task runs, so the caller of a background
    /// spawn sees a `Running` record the moment the spawn is accepted. The
    /// push is the `SubAgentSpawned` transition (S4).
    pub(super) fn register(&self, record: SubAgentRecord, cancel: ChildCancel) {
        {
            let mut children = self.children.lock();
            if children
                .iter()
                .any(|entry| entry.record.spec.id == record.spec.id)
            {
                return;
            }
            children.push(ChildEntry {
                record: record.clone(),
                cancel,
                usage: ChildUsage::default(),
            });
        }
        self.emit(subagent_spawned(&record));
    }

    /// Every record, in spawn order.
    pub(crate) fn records(&self) -> Vec<SubAgentRecord> {
        self.children
            .lock()
            .iter()
            .map(|entry| entry.record.clone())
            .collect()
    }

    /// One record as it stands right now.
    pub(crate) fn snapshot(&self, id: &SubAgentId) -> Option<SubAgentRecord> {
        self.children
            .lock()
            .iter()
            .find(|entry| entry.record.spec.id == *id)
            .map(|entry| entry.record.clone())
    }

    /// Write a child's own view of its record. A record that is already
    /// terminal in the registry keeps that outcome: a kill the user saw first
    /// is not overwritten by the loop finishing on its way out. An applied
    /// write is a lifecycle transition (S4): `SubAgentFinished` when the
    /// status is terminal, `SubAgentProgress` while the child is live.
    pub(super) fn publish(&self, record: &SubAgentRecord) {
        let applied = {
            let mut children = self.children.lock();
            match children
                .iter_mut()
                .find(|entry| entry.record.spec.id == record.spec.id)
            {
                Some(entry) if entry.record.status.is_terminal() => false,
                Some(entry) => {
                    entry.record = record.clone();
                    true
                }
                None => false,
            }
        };
        if applied {
            self.emit(if record.status.is_terminal() {
                subagent_finished(record)
            } else {
                subagent_progress(record)
            });
        }
    }

    /// Stop one child and everything it spawned, leaving its siblings alone.
    /// `false` when the id is unknown or the child already ended, so a second
    /// kill cannot report a cancellation that never happened.
    pub(crate) fn cancel(&self, id: &SubAgentId, reason: &str) -> bool {
        let mut cancelled = Vec::new();
        let hit = cancel_entry(&mut self.children.lock(), id, reason, &mut cancelled);
        for record in &cancelled {
            self.emit(subagent_finished(record));
        }
        hit
    }

    /// Stop every live child — the parent's `Harness::cancel`. Each record goes
    /// `Cancelled` with `reason` and each loop stops at its next check.
    pub(crate) fn cancel_all(&self, reason: &str) {
        let mut cancelled = Vec::new();
        {
            let mut children = self.children.lock();
            let ids: Vec<SubAgentId> = children
                .iter()
                .map(|entry| entry.record.spec.id.clone())
                .collect();
            for id in ids {
                cancel_entry(&mut children, &id, reason, &mut cancelled);
            }
        }
        for record in &cancelled {
            self.emit(subagent_finished(record));
        }
    }
}

/// Move one record to `Cancelled`, fire its stop bit, then walk into the
/// children it spawned. Descending only from a record this call just
/// terminalised is what makes the walk finite: a record ends once. Each record
/// this terminalises is pushed to `cancelled` so the caller can emit its
/// `SubAgentFinished` outside the registry lock.
fn cancel_entry(
    children: &mut Vec<ChildEntry>,
    id: &SubAgentId,
    reason: &str,
    cancelled: &mut Vec<SubAgentRecord>,
) -> bool {
    let mut ended = false;
    if let Some(entry) = children
        .iter_mut()
        .find(|entry| entry.record.spec.id == *id)
    {
        if !entry.record.status.is_terminal() {
            entry.record.cancel(reason);
            entry.cancel.fire(reason);
            ended = true;
            cancelled.push(entry.record.clone());
        }
    }
    if ended {
        // A child's conversation is its spawner's child id, so the link to walk
        // is `parent_chat_id` pointing at the record being cancelled.
        let nested: Vec<SubAgentId> = children
            .iter()
            .filter(|entry| entry.record.spec.parent_chat_id == id.0)
            .map(|entry| entry.record.spec.id.clone())
            .collect();
        for nested_id in nested {
            if cancel_entry(children, &nested_id, reason, cancelled) {
                ended = true;
            }
        }
    }
    ended
}

/// Show the registry what the child has reached. A record already terminal
/// there keeps that outcome — the shared copy follows S1's
/// first-terminal-wins rule, so a kill the user saw is never overwritten by the
/// loop finishing on its way out.
pub(super) fn publish(record: &SubAgentRecord, deps: &ChildDeps) {
    deps.host.registry.publish(record);
}

/// The stop check, run wherever the loop is about to spend another action.
/// `true` means the child's bit had fired: the record is now terminal
/// `Cancelled` with the reason, published, and the run ends here.
pub(super) fn stop_if_cancelled(record: &mut SubAgentRecord, deps: &ChildDeps) -> bool {
    let Some(reason) = deps.cancel.signal() else {
        return false;
    };
    record.cancel(reason);
    publish(record, deps);
    true
}
