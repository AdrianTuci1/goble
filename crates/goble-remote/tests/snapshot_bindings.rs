//! Durable-session bindings: the snapshot half of [`SandboxProvider`] and the
//! adapters that implement it.
//!
//! Three layers are covered:
//!
//! - the **trait defaults**: a provider written before snapshots existed keeps
//!   its old behaviour — no snapshot, `create_from_snapshot` is an honest
//!   `Unsupported`, and `release` destroys it;
//! - the **binding**: `SandboxBinding` + `SnapshotRef` round-trip through JSON,
//!   and a snapshot is only offered to the provider kind that minted it;
//! - the **adapters**: Modal through its bridge (fake runner), E2B through a
//!   local axum mock of its resume/pause routes, Docker/SSH workspace
//!   persistence, and Cloud Run's documented "no snapshot".

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use goble_remote::{
    CloudRunProvider, CommandOutput, CommandRunner, DockerProvider, E2BProvider, ModalProvider,
    ReleaseOutcome, SandboxBinding, SandboxError, SandboxHandle, SandboxProvider, SandboxRequest,
    SandboxStatus, SnapshotRef, SnapshotSupport, SshProvider,
};
use serde_json::{json, Value};

// --- shared fixtures -------------------------------------------------------

fn request() -> SandboxRequest {
    SandboxRequest {
        project_id: "project-1".to_string(),
        sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
        workspace_path: "/workspace/goble".to_string(),
        image: "ghcr.io/goble/worker:latest".to_string(),
        command: Vec::new(),
        env: BTreeMap::from([("GOBLE_WORKER_ID".to_string(), "worker-1".to_string())]),
        timeout_secs: 600,
        metadata: BTreeMap::new(),
    }
}

fn handle(provider: &str, sandbox_id: &str) -> SandboxHandle {
    SandboxHandle {
        provider: provider.to_string(),
        sandbox_id: sandbox_id.to_string(),
        sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
        workspace_path: "/workspace/goble".to_string(),
    }
}

fn binding(provider: &str) -> SandboxBinding {
    SandboxBinding::new(provider, "goble-goble-2fbfef8fa780", "/workspace/goble")
}

/// A provider that implements only the three original methods: every snapshot
/// hook must fall back to its documented default.
struct PlainProvider {
    calls: Mutex<Vec<&'static str>>,
}

impl PlainProvider {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.lock().unwrap().push(call);
    }
}

#[async_trait]
impl SandboxProvider for PlainProvider {
    fn kind(&self) -> &'static str {
        "plain"
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        self.record("create");
        Ok(SandboxHandle::from_request("plain", "sb-1", request))
    }

    async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.record("destroy");
        Ok(())
    }

    async fn status(&self, _handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        Ok(SandboxStatus::Running)
    }
}

// --- trait defaults --------------------------------------------------------

#[tokio::test]
async fn a_provider_without_snapshots_keeps_the_old_behaviour() {
    let provider = PlainProvider::new();

    // The capability is None and no snapshot is taken.
    assert_eq!(provider.snapshot_support(), SnapshotSupport::None);
    assert_eq!(
        provider.snapshot(&handle("plain", "sb-1")).await.unwrap(),
        None
    );

    // Restoring a snapshot it never takes is an honest error, not a silent
    // fresh sandbox.
    let error = provider
        .create_from_snapshot(&request(), &SnapshotRef::new("plain", "im-1"))
        .await
        .expect_err("a plain provider cannot restore a snapshot");
    assert!(matches!(error, SandboxError::Unsupported { .. }), "{error}");

    // Resume without a snapshot is a plain create.
    let resumed = provider
        .resume(&request(), &binding("plain"))
        .await
        .unwrap();
    assert_eq!(resumed.sandbox_id, "sb-1");

    // Resume with a snapshot cannot be honoured, and says so.
    let mut with_snapshot = binding("plain");
    with_snapshot.snapshot = Some(SnapshotRef::new("plain", "im-1"));
    let error = provider
        .resume(&request(), &with_snapshot)
        .await
        .expect_err("the snapshot cannot be restored");
    assert!(matches!(error, SandboxError::Unsupported { .. }));

    // Release snaps (nothing) then destroys, and reports it destroyed.
    let released = provider.release(&handle("plain", "sb-1")).await.unwrap();
    assert_eq!(released, ReleaseOutcome::destroyed(None));
    assert!(!released.sandbox_kept);

    assert_eq!(provider.calls(), vec!["create", "destroy"]);
}

/// A provider with real filesystem snapshots, used to pin the *default*
/// release/resume order a provider does not override.
struct SnapshotProvider {
    calls: Mutex<Vec<&'static str>>,
}

impl SnapshotProvider {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.lock().unwrap().push(call);
    }
}

#[async_trait]
impl SandboxProvider for SnapshotProvider {
    fn kind(&self) -> &'static str {
        "snapshot"
    }

    fn snapshot_support(&self) -> SnapshotSupport {
        SnapshotSupport::Filesystem
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        self.record("create");
        Ok(SandboxHandle::from_request(
            "snapshot",
            "sb-created",
            request,
        ))
    }

    async fn create_from_snapshot(
        &self,
        request: &SandboxRequest,
        _snapshot: &SnapshotRef,
    ) -> Result<SandboxHandle, SandboxError> {
        self.record("create_from_snapshot");
        Ok(SandboxHandle::from_request(
            "snapshot",
            "sb-restored",
            request,
        ))
    }

    async fn snapshot(&self, _handle: &SandboxHandle) -> Result<Option<SnapshotRef>, SandboxError> {
        self.record("snapshot");
        Ok(Some(SnapshotRef::new("snapshot", "im-1")))
    }

    async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.record("destroy");
        Ok(())
    }

    async fn status(&self, _handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        Ok(SandboxStatus::Running)
    }
}

#[tokio::test]
async fn the_default_release_snapshots_before_it_destroys() {
    let provider = SnapshotProvider::new();

    let released = provider.release(&handle("snapshot", "sb-1")).await.unwrap();

    assert_eq!(
        provider.calls(),
        vec!["snapshot", "destroy"],
        "the snapshot must happen while the sandbox still exists"
    );
    assert_eq!(
        released.snapshot,
        Some(SnapshotRef::new("snapshot", "im-1"))
    );
    assert!(!released.sandbox_kept);
}

#[tokio::test]
async fn the_default_resume_restores_the_bindings_snapshot() {
    let provider = SnapshotProvider::new();
    let mut bound = binding("snapshot");
    bound.snapshot = Some(SnapshotRef::new("snapshot", "im-1"));

    let resumed = provider.resume(&request(), &bound).await.unwrap();

    assert_eq!(resumed.sandbox_id, "sb-restored");
    assert_eq!(provider.calls(), vec!["create_from_snapshot"]);
}

// --- the binding -----------------------------------------------------------

#[test]
fn a_binding_roundtrips_through_json() {
    let mut bound = binding("modal");
    bound.sandbox_id = Some("sb-1".to_string());
    bound.snapshot = Some(SnapshotRef::new("modal", "im-1"));
    bound.native_session_id = Some("conversation-1".to_string());

    let json = serde_json::to_string(&bound).expect("serialize the binding");
    let decoded: SandboxBinding = serde_json::from_str(&json).expect("deserialize the binding");
    assert_eq!(decoded, bound);
    assert!(decoded.is_resumable());
}

#[test]
fn a_snapshot_is_only_offered_to_the_provider_that_minted_it() {
    let mut bound = binding("modal");
    bound.snapshot = Some(SnapshotRef::new("modal", "im-1"));

    assert_eq!(
        bound
            .snapshot_for("modal")
            .map(|snapshot| snapshot.snapshot_id.as_str()),
        Some("im-1")
    );
    assert!(
        bound.snapshot_for("e2b").is_none(),
        "another provider cannot restore a Modal image id"
    );
    assert!(bound.is_resumable());

    // A binding with nothing to restore is not resumable.
    let empty = binding("modal");
    assert!(!empty.is_resumable());

    // A blank snapshot id is not a snapshot.
    let mut blank = binding("modal");
    blank.snapshot = Some(SnapshotRef::new("modal", "  "));
    assert!(!blank.is_resumable());
}

// --- the honest per-provider matrix ---------------------------------------

#[test]
fn the_provider_matrix_is_what_the_docs_say() {
    // Modal really snapshots the filesystem.
    assert_eq!(
        ModalProvider::new("id", "secret", Arc::new(NoopRunner)).snapshot_support(),
        SnapshotSupport::Filesystem
    );
    // E2B keeps the sandbox paused; there is no separate snapshot object.
    assert_eq!(
        E2BProvider::new("key", "https://e2b.example").snapshot_support(),
        SnapshotSupport::PauseResume
    );
    // Docker and SSH keep a workspace, not a snapshot.
    assert_eq!(
        DockerProvider::new().snapshot_support(),
        SnapshotSupport::Workspace
    );
    assert_eq!(
        SshProvider::new("user@host").unwrap().snapshot_support(),
        SnapshotSupport::Workspace
    );
    // Cloud Run has nothing durable on its side and does not pretend.
    let config = cloud_run_config("https://run.example");
    assert_eq!(
        CloudRunProvider::with_runner(&config, Arc::new(NoopRunner))
            .expect("build the adapter")
            .snapshot_support(),
        SnapshotSupport::None
    );

    assert_eq!(SnapshotSupport::Filesystem.as_str(), "filesystem");
    assert_eq!(SnapshotSupport::PauseResume.as_str(), "pause_resume");
    assert_eq!(SnapshotSupport::Workspace.as_str(), "workspace");
    assert_eq!(SnapshotSupport::None.as_str(), "none");
    assert!(SnapshotSupport::Filesystem.is_durable());
    assert!(SnapshotSupport::Workspace.is_durable());
    assert!(!SnapshotSupport::None.is_durable());
}

fn cloud_run_config(api_url: &str) -> goble_core::config::RemoteAgentConfig {
    goble_core::config::RemoteAgentConfig {
        provider: goble_core::config::RemoteAgentProvider::CloudRun,
        enabled: true,
        api_key: Some("bearer".to_string()),
        token_id: None,
        token_secret: None,
        api_url: Some(api_url.to_string()),
        image: Some("worker:latest".to_string()),
        region: Some("europe-west1".to_string()),
        environment: Some("gcp-project".to_string()),
    }
}

// --- command runner double -------------------------------------------------

/// Records every `CommandRunner` call and answers with one fixed output.
struct RecordingRunner {
    output: CommandOutput,
    calls: Mutex<Vec<RecordedCall>>,
}

#[derive(Debug, Clone)]
struct RecordedCall {
    program: String,
    args: Vec<String>,
    stdin: Option<Value>,
    env: BTreeMap<String, String>,
}

impl RecordingRunner {
    fn ok(stdout: impl Into<String>) -> Arc<Self> {
        Self::new(CommandOutput {
            status: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    fn failing(stderr: impl Into<String>) -> Arc<Self> {
        Self::new(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: stderr.into(),
        })
    }

    fn new(output: CommandOutput) -> Arc<Self> {
        Arc::new(Self {
            output,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    fn only_call(&self) -> RecordedCall {
        let calls = self.calls();
        assert_eq!(calls.len(), 1, "expected one command, got {calls:?}");
        calls.into_iter().next().expect("the recorded call")
    }
}

#[async_trait]
impl CommandRunner for RecordingRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError> {
        self.calls.lock().unwrap().push(RecordedCall {
            program: program.to_string(),
            args: args.to_vec(),
            stdin: stdin.map(|text| serde_json::from_str(text).expect("stdin JSON")),
            env: env.clone(),
        });
        Ok(self.output.clone())
    }
}

/// A runner that panics if it is ever asked to run anything.
struct NoopRunner;

#[async_trait]
impl CommandRunner for NoopRunner {
    async fn run(
        &self,
        program: &str,
        _args: &[String],
        _stdin: Option<&str>,
        _env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError> {
        panic!("{program} was not supposed to run");
    }
}

// --- Modal: bridge payloads ------------------------------------------------

fn modal(runner: Arc<RecordingRunner>) -> ModalProvider {
    ModalProvider::new("token-id", "token-secret", runner)
        .with_python("python3")
        .with_bridge("/bridge/modal_sandbox.py")
}

#[tokio::test]
async fn modal_snapshot_calls_the_snapshot_action() {
    let runner = RecordingRunner::ok(r#"{"snapshot_id":"im-modal-1"}"#);
    let provider = modal(runner.clone());

    let snapshot = provider
        .snapshot(&handle("modal", "sb-modal-1"))
        .await
        .expect("snapshot");

    assert_eq!(snapshot, Some(SnapshotRef::new("modal", "im-modal-1")));
    let call = runner.only_call();
    assert_eq!(call.program, "python3");
    assert_eq!(call.args[1], "snapshot");
    assert_eq!(call.stdin, Some(json!({"sandbox_id": "sb-modal-1"})));
    assert_eq!(
        call.env.get("MODAL_TOKEN_ID").map(String::as_str),
        Some("token-id")
    );
}

#[tokio::test]
async fn modal_create_from_snapshot_sends_the_snapshot_id() {
    let runner = RecordingRunner::ok(r#"{"sandbox_id":"sb-modal-2"}"#);
    let provider = modal(runner.clone());

    let created = provider
        .create_from_snapshot(&request(), &SnapshotRef::new("modal", "im-modal-1"))
        .await
        .expect("create from snapshot");

    assert_eq!(created.sandbox_id, "sb-modal-2");
    let call = runner.only_call();
    assert_eq!(call.args[1], "create");
    let stdin = call.stdin.expect("the payload");
    assert_eq!(stdin["snapshot_id"], "im-modal-1");
    assert_eq!(stdin["image"], "ghcr.io/goble/worker:latest");
}

#[tokio::test]
async fn modal_resume_restores_the_bindings_snapshot_and_plain_create_omits_it() {
    // A binding with a snapshot restores it.
    let runner = RecordingRunner::ok(r#"{"sandbox_id":"sb-modal-2"}"#);
    let provider = modal(runner.clone());
    let mut bound = binding("modal");
    bound.snapshot = Some(SnapshotRef::new("modal", "im-modal-1"));
    let resumed = provider.resume(&request(), &bound).await.expect("resume");
    assert_eq!(resumed.sandbox_id, "sb-modal-2");
    assert_eq!(
        runner.only_call().stdin.expect("payload")["snapshot_id"],
        "im-modal-1"
    );

    // A binding without one creates from the image, with no snapshot field.
    let runner = RecordingRunner::ok(r#"{"sandbox_id":"sb-modal-3"}"#);
    let provider = modal(runner.clone());
    provider
        .resume(&request(), &binding("modal"))
        .await
        .expect("create");
    let stdin = runner.only_call().stdin.expect("payload");
    assert!(
        stdin.get("snapshot_id").is_none(),
        "a create must not carry a snapshot it does not have: {stdin}"
    );
}

#[tokio::test]
async fn modal_release_snapshots_and_terminates_in_one_bridge_call() {
    let runner = RecordingRunner::ok(r#"{"snapshot_id":"im-modal-2"}"#);
    let provider = modal(runner.clone());

    let released = provider
        .release(&handle("modal", "sb-modal-1"))
        .await
        .expect("release");

    assert_eq!(
        released.snapshot,
        Some(SnapshotRef::new("modal", "im-modal-2"))
    );
    assert!(!released.sandbox_kept);
    let call = runner.only_call();
    assert_eq!(
        call.args[1], "snapshot_and_terminate",
        "a teardown is one SDK process, not a snapshot followed by a destroy"
    );
}

#[tokio::test]
async fn modal_release_of_an_already_gone_sandbox_is_not_an_error() {
    let runner = RecordingRunner::ok(r#"{"snapshot_id":null}"#);
    let provider = modal(runner);

    let released = provider
        .release(&handle("modal", "sb-missing"))
        .await
        .expect("release");

    assert_eq!(released, ReleaseOutcome::destroyed(None));
}

#[tokio::test]
async fn modal_release_does_not_invent_a_snapshot_when_the_bridge_fails() {
    let runner = RecordingRunner::failing("modal: snapshot failed");
    let provider = modal(runner);

    let error = provider
        .release(&handle("modal", "sb-modal-1"))
        .await
        .expect_err("a failed teardown is an error");

    match error {
        SandboxError::Command { command, message } => {
            assert_eq!(command, "modal snapshot_and_terminate");
            assert!(message.contains("snapshot failed"), "{message}");
        }
        other => panic!("expected a command error, got {other}"),
    }
}

// --- E2B: pause/resume through a local mock --------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    method: Method,
    path: String,
}

#[derive(Clone, Default)]
struct Recorder {
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Recorder {
    fn record(&self, method: Method, path: String) {
        self.requests.lock().unwrap().push(Request { method, path });
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

#[derive(Clone)]
struct E2BMock {
    recorder: Recorder,
    resume_status: StatusCode,
    pause_status: StatusCode,
}

impl E2BMock {
    fn new(resume_status: StatusCode, pause_status: StatusCode) -> Self {
        Self {
            recorder: Recorder::default(),
            resume_status,
            pause_status,
        }
    }
}

async fn e2b_create(State(state): State<E2BMock>, Json(_body): Json<Value>) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, "/sandboxes".to_string());
    (StatusCode::OK, r#"{"sandboxID":"sb-new"}"#.to_string())
}

async fn e2b_resume(
    State(state): State<E2BMock>,
    Path(sandbox_id): Path<String>,
) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, format!("/sandboxes/{sandbox_id}/resume"));
    let body = if state.resume_status == StatusCode::NOT_FOUND {
        String::new()
    } else {
        format!(r#"{{"sandboxID":"{sandbox_id}"}}"#)
    };
    (state.resume_status, body)
}

async fn e2b_pause(
    State(state): State<E2BMock>,
    Path(sandbox_id): Path<String>,
) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, format!("/sandboxes/{sandbox_id}/pause"));
    (state.pause_status, String::new())
}

async fn e2b_provider(mock: E2BMock) -> (E2BProvider, Recorder) {
    let app = Router::new()
        .route("/sandboxes", post(e2b_create))
        .route("/sandboxes/{sandbox_id}/resume", post(e2b_resume))
        .route("/sandboxes/{sandbox_id}/pause", post(e2b_pause))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the mock");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve the mock");
    });
    let recorder = mock.recorder.clone();
    (
        E2BProvider::new("e2b-key", format!("http://{address}")),
        recorder,
    )
}

#[tokio::test]
async fn e2b_resume_wakes_the_recorded_sandbox() {
    let (provider, recorder) = e2b_provider(E2BMock::new(StatusCode::OK, StatusCode::OK)).await;
    let mut bound = binding("e2b");
    bound.sandbox_id = Some("sb-old".to_string());

    let resumed = provider.resume(&request(), &bound).await.expect("resume");

    assert_eq!(
        resumed.sandbox_id, "sb-old",
        "the paused sandbox comes back with its id"
    );
    assert_eq!(
        recorder.requests(),
        vec![Request {
            method: Method::POST,
            path: "/sandboxes/sb-old/resume".to_string(),
        }]
    );
}

#[tokio::test]
async fn e2b_resume_of_a_sandbox_that_is_gone_creates_a_new_one() {
    let (provider, recorder) =
        e2b_provider(E2BMock::new(StatusCode::NOT_FOUND, StatusCode::OK)).await;
    let mut bound = binding("e2b");
    bound.sandbox_id = Some("sb-gone".to_string());

    let resumed = provider.resume(&request(), &bound).await.expect("resume");

    assert_eq!(resumed.sandbox_id, "sb-new");
    assert_eq!(
        recorder
            .requests()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/sandboxes/sb-gone/resume", "/sandboxes"]
    );
}

#[tokio::test]
async fn e2b_release_pauses_instead_of_destroying() {
    let (provider, recorder) = e2b_provider(E2BMock::new(StatusCode::OK, StatusCode::OK)).await;

    let released = provider
        .release(&handle("e2b", "sb-old"))
        .await
        .expect("release");

    assert!(released.sandbox_kept, "the paused sandbox is kept");
    assert_eq!(released.snapshot, None);
    assert_eq!(
        recorder.requests(),
        vec![Request {
            method: Method::POST,
            path: "/sandboxes/sb-old/pause".to_string(),
        }],
        "a pause keeps the filesystem and memory; a delete would not"
    );
}

#[tokio::test]
async fn e2b_release_of_a_missing_sandbox_keeps_nothing() {
    let (provider, _recorder) =
        e2b_provider(E2BMock::new(StatusCode::OK, StatusCode::NOT_FOUND)).await;

    let released = provider
        .release(&handle("e2b", "sb-gone"))
        .await
        .expect("release");

    assert_eq!(released, ReleaseOutcome::destroyed(None));
}

// --- Docker / SSH: workspace persistence ----------------------------------

#[tokio::test]
async fn docker_mounts_a_stable_workspace_volume_on_every_create() {
    let request = request();
    assert_eq!(
        goble_remote::docker::workspace_volume(&request.sandbox_name),
        "goble-ws-goble-goble-2fbfef8fa780"
    );
    // Resuming mounts the same volume, so the recreated container sees the
    // previous one's files. `create` is what `resume` calls for Docker.
    let runner = RecordingRunner::ok("container-2\n");
    let provider = DockerProvider::with_runner(runner.clone());
    let resumed = provider
        .resume(&request, &binding("docker"))
        .await
        .expect("resume");

    assert_eq!(resumed.sandbox_id, "container-2");
    assert!(runner
        .only_call()
        .args
        .iter()
        .any(|arg| arg
            == "type=volume,source=goble-ws-goble-goble-2fbfef8fa780,target=/workspace/goble"));
}

#[tokio::test]
async fn ssh_release_stops_the_worker_and_keeps_the_directory() {
    let runner = RecordingRunner::ok("stopped\n");
    let provider = SshProvider::with_runner("ssh://deploy@example.com", runner.clone())
        .expect("a valid target");
    let handle = handle("ssh", "ssh://deploy@example.com/opt/goble/worker");

    let released = provider.release(&handle).await.expect("release");

    assert!(released.sandbox_kept, "the install directory survives");
    assert_eq!(released.snapshot, None);
    let call = runner.only_call();
    assert_eq!(call.program, "ssh");
    let script = call.args.last().expect("the remote script");
    assert!(
        script.contains("echo stopped"),
        "the worker is stopped: {script}"
    );
    assert!(
        !script.contains("rm -rf"),
        "an idle release keeps the directory: {script}"
    );
}

#[tokio::test]
async fn ssh_destroy_still_removes_the_directory() {
    let runner = RecordingRunner::ok("done\n");
    let provider = SshProvider::with_runner("ssh://deploy@example.com", runner.clone())
        .expect("a valid target");
    let handle = handle("ssh", "ssh://deploy@example.com/opt/goble/worker");

    provider.destroy(&handle).await.expect("destroy");

    let script = runner.only_call().args.last().cloned().expect("script");
    assert!(
        script.contains("rm -rf"),
        "an explicit destroy is a full teardown: {script}"
    );
}
