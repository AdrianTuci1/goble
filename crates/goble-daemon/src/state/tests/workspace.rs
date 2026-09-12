use super::*;

// --- per-session agent workspace ---

/// A harness that supports a per-run workspace: it receives the session's
/// workspace dir through [`HarnessRuntime::set_workspace_dir`] and, when
/// running, writes a marker file into that directory. Used to verify that a
/// configured `workspace_root` materializes a per-session agent workspace.
struct WorkspaceHarness {
    id: HarnessId,
    workspace_dir: Arc<Mutex<Option<PathBuf>>>,
}

impl WorkspaceHarness {
    fn new(id: HarnessId) -> Self {
        Self {
            id,
            workspace_dir: Arc::new(Mutex::new(None)),
        }
    }
}

impl HarnessRuntime for WorkspaceHarness {
    fn id(&self) -> HarnessId {
        self.id.clone()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::internal()
    }
    fn set_workspace_dir(&self, dir: &std::path::Path) -> bool {
        *self.workspace_dir.lock().unwrap() = Some(dir.to_path_buf());
        true
    }
    fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
        let dir = self
            .workspace_dir
            .lock()
            .unwrap()
            .clone()
            .expect("the daemon sets the session workspace before run");
        std::fs::write(dir.join("agent.txt"), "agent workspace").unwrap();
        let session_id = turn.session_id;
        HarnessRun {
            events: Box::pin(futures::stream::iter(vec![
                HarnessServerEvent::AssistantDelta {
                    session_id: session_id.clone(),
                    delta: "worked".to_string(),
                },
                HarnessServerEvent::Done { session_id },
            ])),
        }
    }
}

/// With a `workspace_root` set, running a session materializes
/// `<workspace_root>/harness/<trace_id>` and a harness that supports a
/// per-run workspace writes its files there.
#[tokio::test]
async fn workspace_root_materializes_the_session_agent_workspace() {
    let root = std::env::temp_dir().join(format!("goble-daemon-ws-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let registry = HarnessRegistry::new();
    registry.register(Arc::new(WorkspaceHarness::new(HarnessId::new("mock"))));
    let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink))
        .with_workspace_root(root.clone());

    // Session "s1" maps to trace id "trace-s1", so its workspace is
    // `<root>/harness/trace-s1`.
    daemon.run(turn("s1")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let trace_dir = root.join("harness").join("trace-s1");
    assert!(
        trace_dir.is_dir(),
        "running a session must create its per-session agent workspace"
    );
    assert!(
        trace_dir.join("agent.txt").is_file(),
        "a harness-backed file must land inside the session workspace"
    );

    let _ = std::fs::remove_dir_all(&root);
}
