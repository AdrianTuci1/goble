use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goble_core::harness::Harness;
use goble_desktop_service::DesktopState;

static HARNESS_CANCEL: once_cell::sync::Lazy<
    std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn list_harness_tools() -> anyhow::Result<Vec<goble_core::harness::ToolSchema>> {
    let harness = Harness::new(goble_core::store::Store::open_in_memory()?);
    Ok(harness.list_tools())
}

pub struct RunHarnessRequest {
    pub chat_id: String,
    pub prompt: String,
    pub provider: String,
    pub model: String,
}

pub fn run_harness(state: &Arc<DesktopState>, req: RunHarnessRequest) -> anyhow::Result<()> {
    use futures::StreamExt;
    use goble_core::harness::HarnessEvent;
    use goble_core::harness::{harness_sandbox, SandboxedCommandRunner};

    let (llm, model_name) = state.resolve_llm_provider(&req.provider, &req.model);
    let provider_name = if req.provider.is_empty() {
        "openai"
    } else {
        &req.provider
    };
    let deploy_state = Arc::clone(state);
    let cancel = Arc::new(AtomicBool::new(false));
    HARNESS_CANCEL
        .lock()
        .unwrap()
        .insert(req.chat_id.clone(), cancel.clone());
    let harness = Harness::new(state.store_clone())
        .with_llm(llm)
        .with_runner(Arc::new(
            SandboxedCommandRunner::default_tools().with_sandbox(harness_sandbox()),
        ))
        .with_deploy_sender(move |worker_id, msg| deploy_state.send_to_worker(worker_id, msg))
        .with_cancel(cancel.clone());
    let chat_id_for_cleanup = req.chat_id.clone();
    let mut stream = harness.run_turn(&req.chat_id, &req.prompt, provider_name, &model_name);
    let state_clone = Arc::clone(state);
    let chat_id = req.chat_id.clone();
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))?;
    handle.spawn(async move {
        while let Some(event) = stream.next().await {
            let payload = serde_json::json!({
                "chat_id": &chat_id,
                "event": event,
            });
            state_clone.emit("harness:event", payload.clone());
            if let HarnessEvent::Error(e) = &event {
                state_clone.add_log(format!("harness error: {e}"));
            }
        }
        HARNESS_CANCEL.lock().unwrap().remove(&chat_id_for_cleanup);
    });
    Ok(())
}

pub fn cancel_harness(chat_id: &str) {
    if let Some(cancel) = HARNESS_CANCEL.lock().unwrap().get(chat_id) {
        cancel.store(true, Ordering::Relaxed);
    }
}
