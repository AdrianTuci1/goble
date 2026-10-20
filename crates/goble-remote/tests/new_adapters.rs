use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use goble_core::config::{RemoteAgentConfig, RemoteAgentProvider};
use goble_remote::{
    provider_for, BoxProvider, CommandOutput, CommandRunner, DaytonaProvider, SandboxError,
    SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus, SshProvider,
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
        env: BTreeMap::from([
            (
                "GOBLE_GATEWAY_URL".to_string(),
                "wss://gateway.example/ws/worker".to_string(),
            ),
            ("GOBLE_WORKER_ID".to_string(), "worker-1".to_string()),
        ]),
        timeout_secs: 600,
        metadata: BTreeMap::from([("goble.project".to_string(), "project-1".to_string())]),
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

fn agent(provider: RemoteAgentProvider) -> RemoteAgentConfig {
    RemoteAgentConfig {
        provider,
        enabled: true,
        api_key: None,
        token_id: None,
        token_secret: None,
        api_url: None,
        image: None,
        region: None,
        environment: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    program: String,
    args: Vec<String>,
    stdin: Option<String>,
    env: BTreeMap<String, String>,
}

struct MockRunner {
    output: CommandOutput,
    calls: Mutex<Vec<Call>>,
}

impl MockRunner {
    fn new(output: CommandOutput) -> Arc<Self> {
        Arc::new(Self {
            output,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn ok(stdout: impl Into<String>) -> Arc<Self> {
        Self::new(CommandOutput {
            status: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    fn failing(status: i32, stderr: impl Into<String>) -> Arc<Self> {
        Self::new(CommandOutput {
            status: Some(status),
            stdout: String::new(),
            stderr: stderr.into(),
        })
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("mock runner lock").clone()
    }

    fn only_call(&self) -> Call {
        let calls = self.calls();
        assert_eq!(calls.len(), 1, "expected one command, got {calls:?}");
        calls.into_iter().next().expect("the recorded call")
    }
}

#[async_trait]
impl CommandRunner for MockRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError> {
        self.calls.lock().expect("mock runner lock").push(Call {
            program: program.to_string(),
            args: args.to_vec(),
            stdin: stdin.map(str::to_string),
            env: env.clone(),
        });
        Ok(self.output.clone())
    }
}

// --- HTTP recorder and mocks -----------------------------------------------

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: Method,
    path: String,
    authorization: Option<String>,
    confirm_delete: Option<String>,
    body: Value,
}

#[derive(Debug, Clone, Default)]
struct Recorder {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl Recorder {
    fn record(&self, method: Method, path: String, headers: &HeaderMap, body: Value) {
        self.requests
            .lock()
            .expect("recorder lock")
            .push(RecordedRequest {
                method,
                path,
                authorization: headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string),
                confirm_delete: headers
                    .get("x-ascii-confirm-delete")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string),
                body,
            });
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("recorder lock").clone()
    }
}

#[derive(Clone)]
struct DaytonaState {
    recorder: Recorder,
    create_status: StatusCode,
    create_body: String,
}

impl DaytonaState {
    fn new(create_status: StatusCode, create_body: impl Into<String>) -> Self {
        Self {
            recorder: Recorder::default(),
            create_status,
            create_body: create_body.into(),
        }
    }
}

async fn daytona_create(
    State(state): State<DaytonaState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, "/sandbox".to_string(), &headers, body);
    (state.create_status, state.create_body.clone())
}

async fn daytona_get(
    State(state): State<DaytonaState>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::GET,
        format!("/sandbox/{sandbox_id}"),
        &headers,
        Value::Null,
    );
    match sandbox_id.as_str() {
        "sb-missing" => (StatusCode::NOT_FOUND, String::new()),
        "sb-stopped" => (
            StatusCode::OK,
            r#"{"id":"sb-stopped","state":"stopped"}"#.to_string(),
        ),
        "sb-paused" => (
            StatusCode::OK,
            r#"{"id":"sb-paused","state":"paused"}"#.to_string(),
        ),
        "sb-archived" => (
            StatusCode::OK,
            r#"{"id":"sb-archived","state":"archived"}"#.to_string(),
        ),
        other => (
            StatusCode::OK,
            format!(r#"{{"id":"{other}","state":"running"}}"#),
        ),
    }
}

async fn daytona_delete(
    State(state): State<DaytonaState>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::DELETE,
        format!("/sandbox/{sandbox_id}"),
        &headers,
        Value::Null,
    );
    if sandbox_id == "sb-missing" {
        (StatusCode::NOT_FOUND, String::new())
    } else {
        (StatusCode::NO_CONTENT, String::new())
    }
}

async fn start_daytona_mock(state: DaytonaState) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/sandbox", post(daytona_create))
        .route(
            "/sandbox/{sandbox_id}",
            get(daytona_get).delete(daytona_delete),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the Daytona mock");
    let address = listener
        .local_addr()
        .expect("read the Daytona mock address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve the Daytona mock");
    });
    (format!("http://{address}"), server)
}

#[derive(Clone)]
struct BoxState {
    recorder: Recorder,
    create_status: StatusCode,
    create_body: String,
}

impl BoxState {
    fn new(create_status: StatusCode, create_body: impl Into<String>) -> Self {
        Self {
            recorder: Recorder::default(),
            create_status,
            create_body: create_body.into(),
        }
    }
}

async fn box_create(
    State(state): State<BoxState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, "/sandboxes".to_string(), &headers, body);
    (state.create_status, state.create_body.clone())
}

async fn box_get(
    State(state): State<BoxState>,
    Path(box_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::GET,
        format!("/sandboxes/{box_id}"),
        &headers,
        Value::Null,
    );
    match box_id.as_str() {
        "bx-missing" => (StatusCode::NOT_FOUND, String::new()),
        "bx-archived" => (
            StatusCode::OK,
            r#"{"ok":true,"sandbox":{"id":"bx-archived","name":"old","state":"archived"}}"#
                .to_string(),
        ),
        "bx-provisioning" => (
            StatusCode::OK,
            r#"{"ok":true,"sandbox":{"id":"bx-provisioning","name":"new","state":"provisioning"}}"#
                .to_string(),
        ),
        other => (
            StatusCode::OK,
            format!(r#"{{"ok":true,"sandbox":{{"id":"{other}","name":"box","state":"ready"}}}}"#),
        ),
    }
}

async fn box_patch(
    State(state): State<BoxState>,
    Path(box_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.recorder.record(
        Method::PATCH,
        format!("/sandboxes/{box_id}"),
        &headers,
        body,
    );
    (
        StatusCode::OK,
        format!(
            r#"{{"ok":true,"sandbox":{{"id":"{box_id}","name":"renamed","state":"provisioning"}}}}"#
        ),
    )
}

async fn box_stop(
    State(state): State<BoxState>,
    Path(box_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.recorder.record(
        Method::POST,
        format!("/sandboxes/{box_id}/stop"),
        &headers,
        body,
    );
    if box_id == "bx-missing" {
        (StatusCode::NOT_FOUND, String::new())
    } else {
        (StatusCode::OK, r#"{"ok":true}"#.to_string())
    }
}

async fn box_delete(
    State(state): State<BoxState>,
    Path(box_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::DELETE,
        format!("/sandboxes/{box_id}"),
        &headers,
        Value::Null,
    );
    (StatusCode::OK, r#"{"ok":true}"#.to_string())
}

async fn start_box_mock(state: BoxState) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/sandboxes", post(box_create))
        .route(
            "/sandboxes/{box_id}",
            get(box_get).patch(box_patch).delete(box_delete),
        )
        .route("/sandboxes/{box_id}/stop", post(box_stop))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the Box mock");
    let address = listener.local_addr().expect("read the Box mock address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve the Box mock");
    });
    (format!("http://{address}"), server)
}

// --- Daytona ---------------------------------------------------------------

mod daytona {
    use super::*;

    fn snapshot_request() -> SandboxRequest {
        let mut request = request();
        request.image = "goble-worker-snapshot".to_string();
        request
    }

    #[tokio::test]
    async fn create_posts_an_image_build_labels_and_the_region() {
        let state = DaytonaState::new(
            StatusCode::OK,
            r#"{"id":"sb-123","name":"goble-goble-2fbfef8fa780","state":"started"}"#,
        );
        let recorder = state.recorder.clone();
        let (base_url, server) = start_daytona_mock(state).await;
        let provider = DaytonaProvider::new("daytona-key", format!("{base_url}/"))
            .with_region(Some("us".to_string()));

        let created = provider.create(&request()).await.expect("create");

        server.abort();
        assert_eq!(
            created,
            SandboxHandle {
                provider: "daytona".to_string(),
                sandbox_id: "sb-123".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let requests = recorder.requests();
        assert_eq!(requests.len(), 1);
        let sent = &requests[0];
        assert_eq!(sent.method, Method::POST);
        assert_eq!(sent.path, "/sandbox");
        assert_eq!(sent.authorization.as_deref(), Some("Bearer daytona-key"));
        assert_eq!(
            sent.body,
            json!({
                "name": "goble-goble-2fbfef8fa780",
                "buildInfo": {"dockerfileContent": "FROM ghcr.io/goble/worker:latest\n"},
                "env": {
                    "GOBLE_GATEWAY_URL": "wss://gateway.example/ws/worker",
                    "GOBLE_WORKER_ID": "worker-1",
                },
                "labels": {"goble.project": "project-1"},
                "target": "us",
                "autoStopInterval": 10,
                "autoDeleteInterval": 0,
            })
        );
    }

    #[tokio::test]
    async fn create_uses_a_non_registry_image_as_a_snapshot() {
        let state = DaytonaState::new(StatusCode::OK, r#"{"id":"sb-123"}"#);
        let recorder = state.recorder.clone();
        let (base_url, server) = start_daytona_mock(state).await;
        let provider = DaytonaProvider::new("daytona-key", &base_url);

        provider.create(&snapshot_request()).await.expect("create");

        server.abort();
        assert_eq!(
            recorder.requests()[0].body,
            json!({
                "name": "goble-goble-2fbfef8fa780",
                "snapshot": "goble-worker-snapshot",
                "env": {
                    "GOBLE_GATEWAY_URL": "wss://gateway.example/ws/worker",
                    "GOBLE_WORKER_ID": "worker-1",
                },
                "labels": {"goble.project": "project-1"},
                "autoStopInterval": 10,
                "autoDeleteInterval": 0,
            })
        );
    }

    #[tokio::test]
    async fn destroy_and_status_address_the_sandbox_id() {
        let state = DaytonaState::new(StatusCode::OK, "{}");
        let recorder = state.recorder.clone();
        let (base_url, server) = start_daytona_mock(state).await;
        let provider = DaytonaProvider::new("daytona-key", &base_url);

        provider
            .destroy(&handle("daytona", "sb-123"))
            .await
            .expect("destroy");
        provider
            .destroy(&handle("daytona", "sb-missing"))
            .await
            .expect("destroying a missing sandbox is a no-op");

        assert_eq!(
            provider
                .status(&handle("daytona", "sb-123"))
                .await
                .expect("running status"),
            SandboxStatus::Running
        );
        assert_eq!(
            provider
                .status(&handle("daytona", "sb-stopped"))
                .await
                .expect("stopped status"),
            SandboxStatus::Paused
        );
        assert_eq!(
            provider
                .status(&handle("daytona", "sb-paused"))
                .await
                .expect("paused status"),
            SandboxStatus::Paused
        );
        assert_eq!(
            provider
                .status(&handle("daytona", "sb-missing"))
                .await
                .expect("missing status"),
            SandboxStatus::Missing
        );
        assert_eq!(
            provider
                .status(&handle("daytona", "sb-archived"))
                .await
                .expect("unknown status"),
            SandboxStatus::Unknown
        );

        server.abort();
        let requests = recorder.requests();
        assert_eq!(requests.len(), 7);
        assert_eq!(requests[0].method, Method::DELETE);
        assert_eq!(requests[0].path, "/sandbox/sb-123");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer daytona-key")
        );
        assert_eq!(requests[1].method, Method::DELETE);
        assert_eq!(requests[1].path, "/sandbox/sb-missing");
        for (index, id) in [
            "sb-123",
            "sb-stopped",
            "sb-paused",
            "sb-missing",
            "sb-archived",
        ]
        .iter()
        .enumerate()
        {
            let request = &requests[index + 2];
            assert_eq!(request.method, Method::GET);
            assert_eq!(request.path, format!("/sandbox/{id}"));
            assert_eq!(request.authorization.as_deref(), Some("Bearer daytona-key"));
        }
    }

    #[tokio::test]
    async fn a_rejected_create_carries_the_http_status_and_body() {
        let state = DaytonaState::new(StatusCode::UNAUTHORIZED, "bad daytona key");
        let (base_url, server) = start_daytona_mock(state).await;
        let provider = DaytonaProvider::new("wrong-key", &base_url);

        let error = provider
            .create(&request())
            .await
            .expect_err("a 401 must fail");

        server.abort();
        match error {
            SandboxError::Http {
                provider,
                status,
                body,
            } => {
                assert_eq!(provider, "daytona");
                assert_eq!(status, 401);
                assert_eq!(body, "bad daytona key");
            }
            other => panic!("expected an HTTP error, got {other}"),
        }
    }
}

// --- Box -------------------------------------------------------------------

mod box_provider {
    use super::*;

    fn snapshot_request() -> SandboxRequest {
        let mut request = request();
        request.image = "goble-worker-snapshot".to_string();
        request
    }

    #[tokio::test]
    async fn create_posts_the_environment_then_renames_the_box() {
        let state = BoxState::new(
            StatusCode::OK,
            r#"{"ok":true,"sandbox":{"id":"bx-123","name":"box-abc","state":"provisioning"}}"#,
        );
        let recorder = state.recorder.clone();
        let (base_url, server) = start_box_mock(state).await;
        let provider = BoxProvider::new("box-key", format!("{base_url}/"))
            .with_environment(Some("dev".to_string()));

        let created = provider.create(&snapshot_request()).await.expect("create");

        server.abort();
        assert_eq!(
            created,
            SandboxHandle {
                provider: "box".to_string(),
                sandbox_id: "bx-123".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let requests = recorder.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].path, "/sandboxes");
        assert_eq!(requests[0].authorization.as_deref(), Some("Bearer box-key"));
        assert_eq!(
            requests[0].body,
            json!({
                "ttlSeconds": 600,
                "noEnv": true,
                "env": {
                    "GOBLE_GATEWAY_URL": "wss://gateway.example/ws/worker",
                    "GOBLE_WORKER_ID": "worker-1",
                },
                "from": "goble-worker-snapshot",
                "environment": "dev",
            })
        );
        assert_eq!(requests[1].method, Method::PATCH);
        assert_eq!(requests[1].path, "/sandboxes/bx-123");
        assert_eq!(requests[1].authorization.as_deref(), Some("Bearer box-key"));
        assert_eq!(
            requests[1].body,
            json!({"name": "goble-goble-2fbfef8fa780"})
        );
    }

    #[tokio::test]
    async fn destroy_stops_then_deletes_the_box() {
        let state = BoxState::new(StatusCode::OK, "{}");
        let recorder = state.recorder.clone();
        let (base_url, server) = start_box_mock(state).await;
        let provider = BoxProvider::new("box-key", &base_url);

        provider
            .destroy(&handle("box", "bx-123"))
            .await
            .expect("destroy");
        provider
            .destroy(&handle("box", "bx-missing"))
            .await
            .expect("destroying a missing box is a no-op");

        server.abort();
        let requests = recorder.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].path, "/sandboxes/bx-123/stop");
        assert_eq!(requests[0].authorization.as_deref(), Some("Bearer box-key"));
        assert_eq!(requests[0].body, json!({"force": true}));
        assert_eq!(requests[1].method, Method::DELETE);
        assert_eq!(requests[1].path, "/sandboxes/bx-123");
        assert_eq!(requests[1].confirm_delete.as_deref(), Some("bx-123"));
        assert_eq!(requests[2].method, Method::POST);
        assert_eq!(requests[2].path, "/sandboxes/bx-missing/stop");
    }

    #[tokio::test]
    async fn status_maps_the_box_state() {
        let state = BoxState::new(StatusCode::OK, "{}");
        let (base_url, server) = start_box_mock(state).await;
        let provider = BoxProvider::new("box-key", &base_url);

        assert_eq!(
            provider
                .status(&handle("box", "bx-123"))
                .await
                .expect("ready status"),
            SandboxStatus::Running
        );
        assert_eq!(
            provider
                .status(&handle("box", "bx-archived"))
                .await
                .expect("archived status"),
            SandboxStatus::Paused
        );
        assert_eq!(
            provider
                .status(&handle("box", "bx-provisioning"))
                .await
                .expect("provisioning status"),
            SandboxStatus::Unknown
        );
        assert_eq!(
            provider
                .status(&handle("box", "bx-missing"))
                .await
                .expect("missing status"),
            SandboxStatus::Missing
        );

        server.abort();
    }

    #[tokio::test]
    async fn a_rejected_create_carries_the_http_status_and_body() {
        let state = BoxState::new(StatusCode::UNAUTHORIZED, "bad box key");
        let (base_url, server) = start_box_mock(state).await;
        let provider = BoxProvider::new("wrong-key", &base_url);

        let error = provider
            .create(&snapshot_request())
            .await
            .expect_err("a 401 must fail");

        server.abort();
        match error {
            SandboxError::Http {
                provider,
                status,
                body,
            } => {
                assert_eq!(provider, "box");
                assert_eq!(status, 401);
                assert_eq!(body, "bad box key");
            }
            other => panic!("expected an HTTP error, got {other}"),
        }
    }
}

// --- SSH -------------------------------------------------------------------

mod ssh {
    use super::*;

    const TARGET: &str = "ssh://deploy@example.com:2222";
    const DIR: &str = "/opt/goble/worker";

    fn provider(runner: Arc<MockRunner>) -> SshProvider {
        SshProvider::with_runner(TARGET, runner).expect("a valid target")
    }

    fn plain_provider(target: &str, runner: Arc<MockRunner>) -> SshProvider {
        SshProvider::with_runner(target, runner).expect("a valid target")
    }

    fn ssh_request() -> SandboxRequest {
        let mut request = request();
        request.image = DIR.to_string();
        request.env = BTreeMap::from([
            (
                "GOBLE_GATEWAY_URL".to_string(),
                "wss://gateway.example/ws/worker?a=1&b=2".to_string(),
            ),
            ("GOBLE_WORKER_ID".to_string(), "worker-1".to_string()),
            (
                "GOBLE_WORKER_TOKEN".to_string(),
                "tok'en with space".to_string(),
            ),
            (
                "GOBLE_WORKSPACE_PATH".to_string(),
                "/workspace/my repo".to_string(),
            ),
        ]);
        request
    }

    fn ssh_argv(script: &str) -> Vec<String> {
        vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
            "-p".to_string(),
            "2222".to_string(),
            "deploy@example.com".to_string(),
            script.to_string(),
        ]
    }

    fn start_script(dir: &str) -> String {
        format!(
            "set -e\ncd '{dir}'\ntest -x '{dir}/goblin'\nnohup env 'GOBLE_GATEWAY_URL=wss://gateway.example/ws/worker?a=1&b=2' 'GOBLE_WORKER_ID=worker-1' 'GOBLE_WORKER_TOKEN=tok'\\''en with space' 'GOBLE_WORKSPACE_PATH=/workspace/my repo' '{dir}/goblin' >> '{dir}/goble-worker.log' 2>&1 &\necho $! > '{dir}/goble-worker.pid'\necho started"
        )
    }

    fn destroy_script(dir: &str) -> String {
        format!(
            "set -e\npid_file='{dir}/goble-worker.pid'\nif [ -f \"$pid_file\" ]; then\n  pid=$(cat \"$pid_file\")\n  if [ -n \"$pid\" ]; then\n    kill \"$pid\" 2>/dev/null || true\n    n=0\n    while kill -0 \"$pid\" 2>/dev/null && [ \"$n\" -lt 50 ]; do\n      sleep 0.1\n      n=$((n + 1))\n    done\n    kill -9 \"$pid\" 2>/dev/null || true\n  fi\nfi\nif pgrep -f -- \"$dir/goblin\" >/dev/null 2>&1; then\n  pkill -f -- \"$dir/goblin\" 2>/dev/null || true\n  sleep 0.2\n  pkill -9 -f -- \"$dir/goblin\" 2>/dev/null || true\nfi\nrm -rf '{dir}'"
        )
    }

    fn status_script(dir: &str) -> String {
        format!(
            "dir='{dir}'\npid_file=\"$dir/goble-worker.pid\"\nif [ ! -d \"$dir\" ]; then\n  echo missing\n  exit 0\nfi\nif [ -f \"$pid_file\" ]; then\n  pid=$(cat \"$pid_file\")\n  if [ -n \"$pid\" ] && kill -0 \"$pid\" 2>/dev/null; then\n    echo running\n    exit 0\n  fi\nfi\nif pgrep -f -- \"$dir/goblin\" >/dev/null 2>&1; then\n  echo running\nelse\n  echo paused\nfi"
        )
    }

    #[tokio::test]
    async fn create_installs_the_directory_and_starts_the_worker() {
        let runner = MockRunner::ok("");
        let provider = provider(runner.clone());

        let created = provider.create(&ssh_request()).await.expect("create");

        assert_eq!(
            created,
            SandboxHandle {
                provider: "ssh".to_string(),
                sandbox_id: "ssh://deploy@example.com:2222/opt/goble/worker".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].program, "ssh");
        assert_eq!(calls[0].args, ssh_argv("mkdir -p '/opt/goble/worker'"));
        assert_eq!(calls[0].stdin, None);
        assert!(calls[0].env.is_empty());
        assert_eq!(calls[1].program, "ssh");
        assert_eq!(calls[1].args, ssh_argv(&start_script(DIR)));
        assert_eq!(calls[1].stdin, None);
        assert!(calls[1].env.is_empty());
    }

    #[tokio::test]
    async fn create_downloads_a_worker_binary_url_into_a_default_directory() {
        let runner = MockRunner::ok("");
        let provider = plain_provider("deploy@example.com", runner.clone());
        let mut request = ssh_request();
        request.image = "https://example.test/goblin".to_string();
        let dir = "/tmp/goble-goble-goble-2fbfef8fa780";

        let created = provider.create(&request).await.expect("create");

        assert_eq!(
            created.sandbox_id,
            "ssh://deploy@example.com/tmp/goble-goble-goble-2fbfef8fa780"
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0].args.last().expect("script"),
            &format!("mkdir -p '{dir}'")
        );
        assert_eq!(
            calls[1].args,
            vec![
                "-o".to_string(),
                "BatchMode=yes".to_string(),
                "-o".to_string(),
                "StrictHostKeyChecking=accept-new".to_string(),
                "-o".to_string(),
                "ConnectTimeout=10".to_string(),
                "deploy@example.com".to_string(),
                format!(
                    "set -e\ncurl -fsSL 'https://example.test/goblin' -o '{dir}/goblin'\nchmod +x '{dir}/goblin'"
                ),
            ]
        );
        assert_eq!(calls[2].args.last().expect("script"), &start_script(dir));
    }

    #[tokio::test]
    async fn create_copies_a_local_worker_with_scp() {
        let local = std::env::temp_dir().join(format!(
            "goble-ssh-worker-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos()
        ));
        std::fs::write(&local, b"worker").expect("write the local worker");
        let runner = MockRunner::ok("");
        let provider = provider(runner.clone());
        let mut request = ssh_request();
        request.image = local.display().to_string();
        let dir = "/tmp/goble-goble-goble-2fbfef8fa780";

        let created = provider.create(&request).await.expect("create");

        std::fs::remove_file(&local).expect("clean up the local worker");
        assert_eq!(
            created.sandbox_id,
            "ssh://deploy@example.com:2222/tmp/goble-goble-goble-2fbfef8fa780"
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1].program, "scp");
        assert_eq!(
            calls[1].args,
            vec![
                "-o".to_string(),
                "BatchMode=yes".to_string(),
                "-o".to_string(),
                "StrictHostKeyChecking=accept-new".to_string(),
                "-o".to_string(),
                "ConnectTimeout=10".to_string(),
                "-P".to_string(),
                "2222".to_string(),
                local.display().to_string(),
                format!("deploy@example.com:{dir}/goblin"),
            ]
        );
        assert_eq!(calls[2].args.last().expect("script"), &start_script(dir));
    }

    #[tokio::test]
    async fn destroy_stops_the_worker_and_removes_the_directory() {
        let runner = MockRunner::ok("");
        let provider = provider(runner.clone());

        provider
            .destroy(&handle(
                "ssh",
                "ssh://deploy@example.com:2222/opt/goble/worker",
            ))
            .await
            .expect("destroy");

        let call = runner.only_call();
        assert_eq!(call.program, "ssh");
        assert_eq!(call.args, ssh_argv(&destroy_script(DIR)));
        assert_eq!(call.stdin, None);
        assert!(call.env.is_empty());
    }

    #[tokio::test]
    async fn status_maps_the_remote_check() {
        async fn status_for(stdout: &str) -> SandboxStatus {
            let runner = MockRunner::ok(stdout);
            let provider = provider(runner.clone());
            let status = provider
                .status(&handle(
                    "ssh",
                    "ssh://deploy@example.com:2222/opt/goble/worker",
                ))
                .await
                .expect("status");
            let call = runner.only_call();
            assert_eq!(call.args, ssh_argv(&status_script(DIR)));
            status
        }

        assert_eq!(status_for("running\n").await, SandboxStatus::Running);
        assert_eq!(status_for("paused\n").await, SandboxStatus::Paused);
        assert_eq!(status_for("missing\n").await, SandboxStatus::Missing);
        assert_eq!(status_for("something else\n").await, SandboxStatus::Unknown);
    }

    #[tokio::test]
    async fn a_failed_ssh_is_a_command_error() {
        let runner = MockRunner::failing(255, "ssh: connect to host example.com port 22: refused");
        let failing = provider(runner);

        let error = failing
            .create(&ssh_request())
            .await
            .expect_err("ssh failed");

        match error {
            SandboxError::Command { command, message } => {
                assert_eq!(command, "ssh mkdir -p /opt/goble/worker");
                assert!(message.contains("refused"), "{message}");
            }
            other => panic!("expected a command error, got {other}"),
        }

        let runner = MockRunner::failing(255, "ssh: connect to host example.com port 22: refused");
        let failing = provider(runner);
        let error = failing
            .status(&handle(
                "ssh",
                "ssh://deploy@example.com:2222/opt/goble/worker",
            ))
            .await
            .expect_err("status ssh failed");
        assert!(matches!(error, SandboxError::Command { .. }), "{error}");
    }

    #[tokio::test]
    async fn create_rejects_an_image_that_is_not_a_directory_or_url() {
        let runner = MockRunner::ok("");
        let provider = provider(runner.clone());
        let mut request = ssh_request();
        request.image = "ghcr.io/goble/worker:latest".to_string();

        let error = provider.create(&request).await.expect_err("no directory");

        assert!(
            matches!(error, SandboxError::InvalidRequest { .. }),
            "{error}"
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn a_target_must_be_a_destination() {
        let runner = MockRunner::ok("");
        for target in ["", "deploy@", "@example.com", "user@host/path", "ssh://"] {
            let error = SshProvider::with_runner(target, runner.clone())
                .err()
                .expect("invalid target");
            assert!(
                matches!(error, SandboxError::InvalidRequest { .. }),
                "{error}"
            );
        }
    }
}

// --- factory ---------------------------------------------------------------

mod factory {
    use super::*;

    #[test]
    fn daytona_and_box_require_an_api_key() {
        for (provider, key) in [
            (RemoteAgentProvider::Daytona, "DAYTONA_API_KEY"),
            (RemoteAgentProvider::Box, "BOX_API_KEY"),
        ] {
            let error = provider_for(&agent(provider)).err().expect("missing key");
            assert!(
                matches!(
                    error,
                    SandboxError::MissingCredential {
                        key: missing,
                        ..
                    } if missing == key
                ),
                "{error}"
            );
        }

        let mut daytona = agent(RemoteAgentProvider::Daytona);
        daytona.api_key = Some("daytona-key".to_string());
        daytona.region = Some("us".to_string());
        assert_eq!(provider_for(&daytona).expect("daytona").kind(), "daytona");

        let mut boxed = agent(RemoteAgentProvider::Box);
        boxed.api_key = Some("box-key".to_string());
        boxed.environment = Some("dev".to_string());
        assert_eq!(provider_for(&boxed).expect("box").kind(), "box");
    }

    #[test]
    fn ssh_requires_a_target() {
        let error = provider_for(&agent(RemoteAgentProvider::Ssh))
            .err()
            .expect("missing target");
        assert!(
            error.to_string().contains("api_url"),
            "the error names the field: {error}"
        );

        let mut ssh = agent(RemoteAgentProvider::Ssh);
        ssh.api_url = Some("deploy@example.com".to_string());
        ssh.image = Some("/opt/goble/worker".to_string());
        assert_eq!(provider_for(&ssh).expect("ssh").kind(), "ssh");
    }
}
