use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::oneshot;

use crate::harness::{CommandRunner, WebSearchConfig};
use crate::llm::LlmProvider;
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::subagent::SubAgentRecord;
use crate::worker::WorkerId;

use super::cancel::ChildCancel;
use super::registry::SubAgentRegistry;
use super::run::run_child_task;

/// Everything a child run needs beyond its own spec: the harness handles the
/// parent's turn is executing with, carried through rather than rebuilt, plus
/// this child's stop bit. Owned, because the child runs on a task of its own.
#[derive(Clone)]
pub(crate) struct ChildDeps {
    pub host: SubAgentHost,
    pub provider: String,
    pub model: String,
    pub web_search: WebSearchConfig,
    pub cancel: ChildCancel,
}

/// The harness side of a child run: the same handles the parent's turn uses,
/// the registry the child's record lives in, the parent's cancel bit and the
/// foreground budget. Cheap to clone; built per turn by the `Harness`, which is
/// where those handles live.
#[derive(Clone)]
pub(crate) struct SubAgentHost {
    pub(crate) store: Store,
    pub(crate) runner: Arc<dyn CommandRunner>,
    pub(crate) llm: Arc<dyn LlmProvider>,
    pub(crate) deploy_sender:
        Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
    pub(crate) mcp_manager: McpManager,
    pub(crate) docs_dir: PathBuf,
    pub(crate) registry: SubAgentRegistry,
    pub(crate) parent_cancel: Arc<AtomicBool>,
    pub(crate) foreground_wait: Duration,
}

impl SubAgentHost {
    /// The wall-clock budget a foreground spawn awaits its child for.
    pub(crate) fn foreground_wait(&self) -> Duration {
        self.foreground_wait
    }

    /// Register the child, put its loop on a task and hand back the terminal
    /// record. The receiver is what a foreground awaiter waits on; a background
    /// spawn drops it and the child keeps running to its terminal status.
    pub(crate) fn spawn_child(
        &self,
        mut record: SubAgentRecord,
        provider: &str,
        model: &str,
        web_search: &WebSearchConfig,
    ) -> oneshot::Receiver<SubAgentRecord> {
        record.begin_running();
        let cancel = ChildCancel::new(Arc::clone(&self.parent_cancel));
        self.registry.register(record.clone(), cancel.clone());
        let deps = ChildDeps {
            host: self.clone(),
            provider: provider.to_string(),
            model: model.to_string(),
            web_search: web_search.clone(),
            cancel,
        };
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(run_child_task(record, deps, sender));
        receiver
    }
}
