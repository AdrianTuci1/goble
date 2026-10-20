use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use goble_core::config::{RemoteAgentConfig, RemoteAgentProvider};
use goble_remote::{
    provider_for, CommandOutput, CommandRunner, SandboxError, SandboxHandle, SandboxProvider,
    SandboxRequest, SandboxStatus,
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

fn command_request() -> SandboxRequest {
    let mut request = request();
    request.command = vec!["goblin".to_string()];
    request
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

/// Records each command and returns one fixed output.
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

// --- E2B mock server -------------------------------------------------------

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: Method,
    path: String,
    api_key: Option<String>,
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
                api_key: headers
                    .get("x-api-key")
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
struct MockState {
    recorder: Recorder,
    create_status: StatusCode,
    create_body: String,
}

impl MockState {
    fn new(create_status: StatusCode, create_body: impl Into<String>) -> Self {
        Self {
            recorder: Recorder::default(),
            create_status,
            create_body: create_body.into(),
        }
    }
}

async fn create_sandbox(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state
        .recorder
        .record(Method::POST, "/sandboxes".to_string(), &headers, body);
    (state.create_status, state.create_body.clone())
}

async fn get_sandbox(
    State(state): State<MockState>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::GET,
        format!("/sandboxes/{sandbox_id}"),
        &headers,
        Value::Null,
    );
    if sandbox_id == "sb-missing" {
        (StatusCode::NOT_FOUND, String::new())
    } else {
        (StatusCode::OK, r#"{"sandboxID":"sb-123"}"#.to_string())
    }
}

async fn delete_sandbox(
    State(state): State<MockState>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.recorder.record(
        Method::DELETE,
        format!("/sandboxes/{sandbox_id}"),
        &headers,
        Value::Null,
    );
    (StatusCode::NO_CONTENT, String::new())
}

async fn start_mock(state: MockState) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/sandboxes", post(create_sandbox))
        .route(
            "/sandboxes/{sandbox_id}",
            get(get_sandbox).delete(delete_sandbox),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the mock server");
    let address = listener.local_addr().expect("read the mock address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve the mock");
    });
    (format!("http://{address}"), server)
}

// --- E2B -------------------------------------------------------------------

mod e2b {
    use super::*;
    use goble_remote::e2b::E2BProvider;

    #[tokio::test]
    async fn create_posts_the_template_with_environment_and_metadata() {
        let state = MockState::new(
            StatusCode::OK,
            r#"{"sandboxID":"sb-123","domain":"e2b.example"}"#,
        );
        let recorder = state.recorder.clone();
        let (base_url, server) = start_mock(state).await;
        let provider = E2BProvider::new("e2b-key", format!("{base_url}/"));

        let created = provider.create(&request()).await.expect("create");

        server.abort();
        assert_eq!(
            created,
            SandboxHandle {
                provider: "e2b".to_string(),
                sandbox_id: "sb-123".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let requests = recorder.requests();
        assert_eq!(requests.len(), 1);
        let sent = &requests[0];
        assert_eq!(sent.method, Method::POST);
        assert_eq!(sent.path, "/sandboxes");
        assert_eq!(sent.api_key.as_deref(), Some("e2b-key"));
        assert_eq!(
            sent.body,
            json!({
                "templateID": "ghcr.io/goble/worker:latest",
                "envVars": {
                    "GOBLE_GATEWAY_URL": "wss://gateway.example/ws/worker",
                    "GOBLE_WORKER_ID": "worker-1",
                },
                "metadata": {"goble.project": "project-1"},
                "timeout": 600,
                "autoPause": true,
                "autoResume": {"enabled": true},
            })
        );
    }

    #[tokio::test]
    async fn destroy_and_status_address_the_sandbox_id() {
        let state = MockState::new(StatusCode::OK, "{}");
        let recorder = state.recorder.clone();
        let (base_url, server) = start_mock(state).await;
        let provider = E2BProvider::new("e2b-key", &base_url);

        provider
            .destroy(&handle("e2b", "sb-123"))
            .await
            .expect("destroy");
        let running = provider
            .status(&handle("e2b", "sb-123"))
            .await
            .expect("status");
        let missing = provider
            .status(&handle("e2b", "sb-missing"))
            .await
            .expect("missing status");

        server.abort();
        assert_eq!(running, SandboxStatus::Running);
        assert_eq!(missing, SandboxStatus::Missing);

        let requests = recorder.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, Method::DELETE);
        assert_eq!(requests[0].path, "/sandboxes/sb-123");
        assert_eq!(requests[0].api_key.as_deref(), Some("e2b-key"));
        assert_eq!(requests[1].method, Method::GET);
        assert_eq!(requests[1].path, "/sandboxes/sb-123");
        assert_eq!(requests[1].api_key.as_deref(), Some("e2b-key"));
        assert_eq!(requests[2].method, Method::GET);
        assert_eq!(requests[2].path, "/sandboxes/sb-missing");
    }

    #[tokio::test]
    async fn a_rejected_create_carries_the_http_status_and_body() {
        let state = MockState::new(StatusCode::UNAUTHORIZED, "invalid api key");
        let (base_url, server) = start_mock(state).await;
        let provider = E2BProvider::new("wrong-key", &base_url);

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
                assert_eq!(provider, "e2b");
                assert_eq!(status, 401);
                assert_eq!(body, "invalid api key");
            }
            other => panic!("expected an HTTP error, got {other}"),
        }
    }

    #[tokio::test]
    async fn create_rejects_an_explicit_command_before_calling_out() {
        let provider = E2BProvider::new("e2b-key", "http://127.0.0.1:9");

        let error = provider
            .create(&command_request())
            .await
            .expect_err("a command cannot be forwarded");

        assert!(error.to_string().contains("template entrypoint"), "{error}");
    }
}

// --- Modal -----------------------------------------------------------------

mod modal {
    use super::*;
    use goble_remote::ModalProvider;

    fn provider(runner: Arc<MockRunner>) -> ModalProvider {
        ModalProvider::new("token-id", "token-secret", runner)
            .with_python("python3")
            .with_bridge("/bridge/modal_sandbox.py")
    }

    fn token_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("MODAL_TOKEN_ID".to_string(), "token-id".to_string()),
            ("MODAL_TOKEN_SECRET".to_string(), "token-secret".to_string()),
        ])
    }

    fn stdin_json(call: &Call) -> Value {
        serde_json::from_str(call.stdin.as_deref().expect("stdin JSON")).expect("stdin is JSON")
    }

    #[tokio::test]
    async fn create_runs_the_bridge_and_returns_the_sandbox_id() {
        let runner = MockRunner::ok(r#"{"sandbox_id":"sb-modal-1"}"#);
        let provider = provider(runner.clone());

        let created = provider.create(&command_request()).await.expect("create");

        assert_eq!(
            created,
            SandboxHandle {
                provider: "modal".to_string(),
                sandbox_id: "sb-modal-1".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let call = runner.only_call();
        assert_eq!(call.program, "python3");
        assert_eq!(
            call.args,
            vec!["/bridge/modal_sandbox.py".to_string(), "create".to_string(),]
        );
        assert_eq!(call.env, token_env());
        assert_eq!(
            stdin_json(&call),
            json!({
                "app_name": "goble-goble-2fbfef8fa780",
                "image": "ghcr.io/goble/worker:latest",
                "command": ["goblin"],
                "env": {
                    "GOBLE_GATEWAY_URL": "wss://gateway.example/ws/worker",
                    "GOBLE_WORKER_ID": "worker-1",
                },
                "timeout_secs": 600,
            })
        );
    }

    #[tokio::test]
    async fn destroy_terminates_the_sandbox_by_id() {
        let runner = MockRunner::ok("{}");
        let provider = provider(runner.clone());

        provider
            .destroy(&handle("modal", "sb-modal-1"))
            .await
            .expect("destroy");

        let call = runner.only_call();
        assert_eq!(call.args[1], "destroy");
        assert_eq!(call.env, token_env());
        assert_eq!(stdin_json(&call), json!({"sandbox_id": "sb-modal-1"}));
    }

    #[tokio::test]
    async fn status_maps_the_bridge_values() {
        async fn status_for(stdout: &str) -> SandboxStatus {
            let runner = MockRunner::ok(stdout);
            let provider = provider(runner.clone());
            let status = provider
                .status(&handle("modal", "sb-modal-1"))
                .await
                .expect("status");
            let call = runner.only_call();
            assert_eq!(call.args[1], "status");
            assert_eq!(call.env, token_env());
            assert_eq!(stdin_json(&call), json!({"sandbox_id": "sb-modal-1"}));
            status
        }

        assert_eq!(
            status_for(r#"{"status":"running"}"#).await,
            SandboxStatus::Running
        );
        assert_eq!(
            status_for(r#"{"status":"missing"}"#).await,
            SandboxStatus::Missing
        );
        assert_eq!(
            status_for(r#"{"status":"paused"}"#).await,
            SandboxStatus::Paused
        );
        assert_eq!(
            status_for(r#"{"status":"something-else"}"#).await,
            SandboxStatus::Unknown
        );
    }

    #[tokio::test]
    async fn a_failed_bridge_is_a_command_error() {
        let runner = MockRunner::failing(1, "modal: not authenticated");
        let provider = provider(runner);

        let error = provider
            .create(&command_request())
            .await
            .expect_err("the bridge failed");

        match error {
            SandboxError::Command { command, message } => {
                assert_eq!(command, "modal create");
                assert!(message.contains("not authenticated"), "{message}");
            }
            other => panic!("expected a command error, got {other}"),
        }
    }

    #[tokio::test]
    async fn unparseable_bridge_output_is_a_parse_error() {
        let runner = MockRunner::ok("not json");
        let provider = provider(runner);

        let error = provider
            .create(&command_request())
            .await
            .expect_err("the response is not JSON");

        assert!(matches!(error, SandboxError::Parse { .. }), "{error}");
    }

    #[test]
    fn the_bundled_bridge_script_is_present() {
        let bridge = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join("modal_sandbox.py");
        assert!(bridge.is_file(), "missing {}", bridge.display());
    }
}

// --- Docker ----------------------------------------------------------------

mod docker {
    use super::*;
    use goble_remote::DockerProvider;

    async fn status_for(output: CommandOutput) -> SandboxStatus {
        let provider = DockerProvider::with_runner(MockRunner::new(output));
        provider
            .status(&handle("docker", "container-123"))
            .await
            .expect("status")
    }

    #[tokio::test]
    async fn create_runs_a_detached_container_with_labels_and_env() {
        let runner = MockRunner::ok("container-123\n");
        let provider = DockerProvider::with_runner(runner.clone());

        let created = provider.create(&command_request()).await.expect("create");

        assert_eq!(
            created,
            SandboxHandle {
                provider: "docker".to_string(),
                sandbox_id: "container-123".to_string(),
                sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
                workspace_path: "/workspace/goble".to_string(),
            }
        );

        let call = runner.only_call();
        assert_eq!(call.program, "docker");
        assert_eq!(
            call.args,
            vec![
                "run".to_string(),
                "-d".to_string(),
                "--name".to_string(),
                "goble-goble-2fbfef8fa780".to_string(),
                "--label".to_string(),
                "goble.sandbox=1".to_string(),
                "--label".to_string(),
                "goble.project=project-1".to_string(),
                "-e".to_string(),
                "GOBLE_GATEWAY_URL=wss://gateway.example/ws/worker".to_string(),
                "-e".to_string(),
                "GOBLE_WORKER_ID=worker-1".to_string(),
                "ghcr.io/goble/worker:latest".to_string(),
                "goblin".to_string(),
            ]
        );
        assert_eq!(call.stdin, None);
        assert!(call.env.is_empty());
    }

    #[tokio::test]
    async fn destroy_removes_the_named_container() {
        let runner = MockRunner::ok("");
        let provider = DockerProvider::with_runner(runner.clone());

        provider
            .destroy(&handle("docker", "container-123"))
            .await
            .expect("destroy");

        let call = runner.only_call();
        assert_eq!(call.program, "docker");
        assert_eq!(
            call.args,
            vec![
                "rm".to_string(),
                "-f".to_string(),
                "goble-goble-2fbfef8fa780".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn status_reads_the_container_state() {
        let runner = MockRunner::ok("true\n");
        let provider = DockerProvider::with_runner(runner.clone());

        let status = provider
            .status(&handle("docker", "container-123"))
            .await
            .expect("status");

        assert_eq!(status, SandboxStatus::Running);
        let call = runner.only_call();
        assert_eq!(call.program, "docker");
        assert_eq!(
            call.args,
            vec![
                "inspect".to_string(),
                "-f".to_string(),
                "{{.State.Running}}".to_string(),
                "goble-goble-2fbfef8fa780".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn status_maps_stopped_missing_and_unexpected_states() {
        assert_eq!(
            status_for(CommandOutput {
                status: Some(0),
                stdout: "false\n".to_string(),
                stderr: String::new(),
            })
            .await,
            SandboxStatus::Paused
        );
        assert_eq!(
            status_for(CommandOutput {
                status: Some(1),
                stdout: String::new(),
                stderr: "Error: No such object: goble-goble-2fbfef8fa780".to_string(),
            })
            .await,
            SandboxStatus::Missing
        );
        assert_eq!(
            status_for(CommandOutput {
                status: Some(0),
                stdout: "maybe".to_string(),
                stderr: String::new(),
            })
            .await,
            SandboxStatus::Unknown
        );
    }

    #[tokio::test]
    async fn a_failed_run_is_a_command_error() {
        let runner = MockRunner::failing(125, "docker: command not found");
        let provider = DockerProvider::with_runner(runner);

        let error = provider
            .create(&command_request())
            .await
            .expect_err("docker failed");

        match error {
            SandboxError::Command { command, message } => {
                assert_eq!(command, "docker run goble-goble-2fbfef8fa780");
                assert!(message.contains("command not found"), "{message}");
            }
            other => panic!("expected a command error, got {other}"),
        }
    }
}

// --- factory ---------------------------------------------------------------

mod factory {
    use super::*;

    #[test]
    fn e2b_and_docker_construct_from_config() {
        let mut e2b = agent(RemoteAgentProvider::E2b);
        e2b.api_key = Some("e2b-key".to_string());
        assert_eq!(provider_for(&e2b).expect("e2b").kind(), "e2b");

        let docker = agent(RemoteAgentProvider::Docker);
        assert_eq!(provider_for(&docker).expect("docker").kind(), "docker");
    }

    #[test]
    fn modal_requires_a_token_id_and_secret() {
        let mut modal = agent(RemoteAgentProvider::Modal);

        let error = provider_for(&modal).err().expect("missing token id");
        assert!(
            matches!(
                error,
                SandboxError::MissingCredential {
                    key: "MODAL_TOKEN_ID",
                    ..
                }
            ),
            "{error}"
        );

        modal.token_id = Some("token-id".to_string());
        let error = provider_for(&modal).err().expect("missing token secret");
        assert!(
            matches!(
                error,
                SandboxError::MissingCredential {
                    key: "MODAL_TOKEN_SECRET",
                    ..
                }
            ),
            "{error}"
        );

        modal.token_secret = Some("token-secret".to_string());
        assert_eq!(provider_for(&modal).expect("modal").kind(), "modal");
    }

    #[test]
    fn e2b_requires_an_api_key() {
        let error = provider_for(&agent(RemoteAgentProvider::E2b))
            .err()
            .expect("missing key");
        assert!(
            matches!(
                error,
                SandboxError::MissingCredential {
                    key: "E2B_API_KEY",
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn every_declared_provider_has_an_adapter() {
        let mut daytona = agent(RemoteAgentProvider::Daytona);
        daytona.api_key = Some("daytona-key".to_string());
        assert_eq!(provider_for(&daytona).expect("daytona").kind(), "daytona");

        let mut boxed = agent(RemoteAgentProvider::Box);
        boxed.api_key = Some("box-key".to_string());
        assert_eq!(provider_for(&boxed).expect("box").kind(), "box");

        let mut ssh = agent(RemoteAgentProvider::Ssh);
        ssh.api_url = Some("deploy@example.com".to_string());
        assert_eq!(provider_for(&ssh).expect("ssh").kind(), "ssh");
    }
}
