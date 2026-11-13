//! The Cloud Run adapter against a local stand-in for the Admin API v2.
//!
//! The mock answers one prefix — `/v2/projects/my-project/locations/europe-west1/jobs` —
//! and every call is recorded, so a test can assert the job the adapter
//! creates, the execution it starts, the token it sends and the deletion it
//! asks for. The `gcloud` call is an injected [`CommandRunner`], so no test
//! needs a Google account.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use goble_core::config::{RemoteAgentConfig, RemoteAgentProvider};
use goble_remote::{
    provider_for, CloudRunProvider, CommandOutput, CommandRunner, SandboxError, SandboxHandle,
    SandboxProvider, SandboxRequest, SandboxStatus,
};
use serde_json::{json, Value};

const JOBS: &str = "/v2/projects/my-project/locations/europe-west1/jobs";
const EXECUTION: &str =
    "projects/my-project/locations/europe-west1/jobs/goble-goble-2fbfef8fa780/executions/run-1";

// --- fixtures --------------------------------------------------------------

fn request() -> SandboxRequest {
    SandboxRequest {
        project_id: "project-1".to_string(),
        sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
        workspace_path: "/workspace/goble".to_string(),
        image: "europe-west1-docker.pkg.dev/my-project/goble/worker:latest".to_string(),
        command: Vec::new(),
        env: BTreeMap::from([
            (
                "GOBLE_GATEWAY_URL".to_string(),
                "wss://gateway.example/ws/worker".to_string(),
            ),
            ("GOBLE_WORKER_ID".to_string(), "worker-1".to_string()),
        ]),
        timeout_secs: 600,
        metadata: BTreeMap::from([
            ("goble.project".to_string(), "projects/goble".to_string()),
            ("goble.workspace".to_string(), "goble@adrians-mac".to_string()),
        ]),
    }
}

fn agent() -> RemoteAgentConfig {
    RemoteAgentConfig {
        provider: RemoteAgentProvider::CloudRun,
        enabled: true,
        api_key: None,
        token_id: None,
        token_secret: None,
        api_url: None,
        image: Some("europe-west1-docker.pkg.dev/my-project/goble/worker:latest".to_string()),
        region: Some("europe-west1".to_string()),
        environment: Some("my-project".to_string()),
    }
}

fn agent_at(api_url: &str) -> RemoteAgentConfig {
    let mut config = agent();
    config.api_url = Some(api_url.to_string());
    config
}

struct MockRunner {
    output: CommandOutput,
    calls: Mutex<Vec<String>>,
}

impl MockRunner {
    fn ok(stdout: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            output: CommandOutput {
                status: Some(0),
                stdout: stdout.into(),
                stderr: String::new(),
            },
            calls: Mutex::new(Vec::new()),
        })
    }

    fn failing(stderr: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            output: CommandOutput {
                status: Some(1),
                stdout: String::new(),
                stderr: stderr.into(),
            },
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("mock runner lock").clone()
    }
}

#[async_trait]
impl CommandRunner for MockRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        _stdin: Option<&str>,
        _env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError> {
        self.calls
            .lock()
            .expect("mock runner lock")
            .push(format!("{program} {}", args.join(" ")));
        Ok(self.output.clone())
    }
}

// --- the Admin API stand-in ------------------------------------------------

#[derive(Debug, Clone)]
struct Recorded {
    method: Method,
    path: String,
    query: Option<String>,
    authorization: Option<String>,
    body: Value,
}

#[derive(Clone, Default)]
struct Recorder {
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Recorder {
    fn push(&self, recorded: Recorded) {
        self.requests.lock().expect("recorder lock").push(recorded);
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("recorder lock").clone()
    }

    fn calls(&self, method: Method) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == method)
            .collect()
    }
}

#[derive(Clone)]
struct Mock {
    recorder: Recorder,
    /// Whether the job this mock is asked about already exists.
    job_exists: bool,
    /// What `jobs/{job}:run` and `executions/{id}` answer.
    execution: Value,
    /// What `DELETE jobs/{job}` answers.
    delete_status: StatusCode,
}

impl Mock {
    fn new(job_exists: bool, execution: Value) -> Self {
        Self {
            recorder: Recorder::default(),
            job_exists,
            execution,
            delete_status: StatusCode::OK,
        }
    }

    fn running() -> Self {
        Self::new(false, json!({ "name": EXECUTION }))
    }
}

fn ok(body: Value) -> Response {
    (StatusCode::OK, axum::Json(body)).into_response()
}

async fn handle(
    State(mock): State<Mock>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_string();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    mock.recorder.push(Recorded {
        method: method.clone(),
        path: path.clone(),
        query: uri.query().map(str::to_string),
        authorization: headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        body,
    });

    // An execution is read at its own resource path, which lives under the job.
    if method == Method::GET && path.contains("/executions/") {
        return if mock.execution.is_null() {
            StatusCode::NOT_FOUND.into_response()
        } else {
            ok(mock.execution.clone())
        };
    }
    if let Some(rest) = path.strip_prefix(JOBS) {
        if method == Method::GET {
            // The job lookup that decides between create and replace.
            return if mock.job_exists {
                ok(json!({ "name": format!("{JOBS}{rest}") }))
            } else {
                StatusCode::NOT_FOUND.into_response()
            };
        }
        if method == Method::POST && rest.is_empty() {
            return ok(json!({ "name": "created" }));
        }
        if method == Method::PUT {
            return ok(json!({ "name": "replaced" }));
        }
        if method == Method::POST && rest.ends_with(":run") {
            return ok(mock.execution.clone());
        }
        if method == Method::DELETE {
            return mock.delete_status.into_response();
        }
    }
    (StatusCode::NOT_FOUND, "unexpected path").into_response()
}

async fn start_mock(mock: Mock) -> (String, Recorder) {
    let recorder = mock.recorder.clone();
    let app = Router::new().fallback(handle).with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the Cloud Run mock");
    let address = listener.local_addr().expect("read the mock address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve the Cloud Run mock");
    });
    (format!("http://{address}"), recorder)
}

async fn provider(api_url: &str, runner: Arc<MockRunner>) -> CloudRunProvider {
    CloudRunProvider::with_runner(&agent_at(api_url), runner).expect("build the adapter")
}

// --- create ----------------------------------------------------------------

#[tokio::test]
async fn create_makes_the_job_and_starts_one_execution() {
    let (base_url, recorder) = start_mock(Mock::running()).await;
    let runner = MockRunner::ok("ya29.test-token\n");
    let provider = provider(&base_url, Arc::clone(&runner)).await;

    let handle = provider.create(&request()).await.expect("create");
    assert_eq!(handle.provider, "cloud_run");
    assert_eq!(handle.sandbox_id, EXECUTION);
    assert_eq!(handle.sandbox_name, "goble-goble-2fbfef8fa780");

    let requests = recorder.requests();
    let methods: Vec<(Method, String)> = requests
        .iter()
        .map(|request| (request.method.clone(), request.path.clone()))
        .collect();
    assert_eq!(
        methods,
        vec![
            (
                Method::GET,
                format!("{JOBS}/goble-goble-2fbfef8fa780")
            ),
            (Method::POST, JOBS.to_string()),
            (
                Method::POST,
                format!("{JOBS}/goble-goble-2fbfef8fa780:run")
            ),
        ],
        "look the job up, create it, run it"
    );
    assert_eq!(
        requests[1].query.as_deref(),
        Some("jobId=goble-goble-2fbfef8fa780")
    );
    for request in &requests {
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer ya29.test-token"),
            "{:?} {} carries the minted token",
            request.method,
            request.path
        );
    }

    let body = &requests[1].body;
    assert_eq!(
        body["template"]["template"]["containers"][0]["image"],
        json!("europe-west1-docker.pkg.dev/my-project/goble/worker:latest")
    );
    assert_eq!(body["template"]["taskCount"], json!(1));
    assert_eq!(body["template"]["parallelism"], json!(1));
    assert_eq!(body["template"]["template"]["timeout"], json!("600s"));
    assert_eq!(body["template"]["template"]["maxRetries"], json!(0));
    let env = body["template"]["template"]["containers"][0]["env"]
        .as_array()
        .expect("the container's env is a list");
    assert!(
        env.iter()
            .any(|entry| entry["name"] == "GOBLE_GATEWAY_URL"
                && entry["value"] == "wss://gateway.example/ws/worker"),
        "{env:?}"
    );
    assert_eq!(body["labels"]["goble-project"], json!("projects-goble"));
    assert_eq!(body["labels"]["goble-workspace"], json!("goble-adrians-mac"));

    // One gcloud call serves the whole create.
    assert_eq!(runner.calls(), vec!["gcloud auth print-access-token"]);
}

#[tokio::test]
async fn an_existing_job_is_replaced_instead_of_created_again() {
    let (base_url, recorder) =
        start_mock(Mock::new(true, json!({ "name": EXECUTION }))).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    provider.create(&request()).await.expect("create");

    let methods: Vec<Method> = recorder
        .calls(Method::POST)
        .into_iter()
        .map(|request| request.method)
        .collect();
    assert_eq!(methods.len(), 1, "only the run is a POST");
    assert_eq!(
        recorder.calls(Method::PUT).len(),
        1,
        "the existing job is replaced"
    );
    assert!(
        recorder.calls(Method::GET).iter().all(|request| request
            .path
            .ends_with("/goble-goble-2fbfef8fa780")),
        "the lookup names the job"
    );
}

#[tokio::test]
async fn a_command_is_passed_as_the_container_arguments() {
    let (base_url, recorder) = start_mock(Mock::running()).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;
    let mut request = request();
    request.command = vec!["goblin-worker".to_string(), "--once".to_string()];

    provider.create(&request).await.expect("create");

    let created = &recorder.calls(Method::POST)[0];
    assert_eq!(
        created.body["template"]["template"]["containers"][0]["args"],
        json!(["goblin-worker", "--once"])
    );
}

#[tokio::test]
async fn the_token_is_taken_from_the_table_before_gcloud_is_asked() {
    let (base_url, recorder) = start_mock(Mock::running()).await;
    let runner = MockRunner::ok("ya29.unused");
    let mut config = agent_at(&base_url);
    config.api_key = Some("sa-bearer-token".to_string());
    let provider =
        CloudRunProvider::with_runner(&config, Arc::clone(&runner)).expect("build the adapter");

    provider.create(&request()).await.expect("create");
    assert_eq!(
        recorder.requests()[0].authorization.as_deref(),
        Some("Bearer sa-bearer-token")
    );
    assert!(runner.calls().is_empty(), "no gcloud call was needed");
}

#[tokio::test]
async fn a_missing_gcloud_session_says_how_to_get_one() {
    let (base_url, _recorder) = start_mock(Mock::running()).await;
    let provider = provider(&base_url, MockRunner::failing("ERROR: not logged in")).await;

    let error = provider.create(&request()).await.expect_err("no token");
    let message = error.to_string();
    assert!(message.contains("gcloud auth login"), "{message}");
    assert!(message.contains("not logged in"), "{message}");
}

// --- destroy ---------------------------------------------------------------

#[tokio::test]
async fn destroy_deletes_the_job_and_forces_its_executions_to_stop() {
    let (base_url, recorder) = start_mock(Mock::new(true, json!({ "name": EXECUTION }))).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    let handle = SandboxHandle::from_request("cloud_run", EXECUTION, &request());
    provider.destroy(&handle).await.expect("destroy");

    let deletes = recorder.calls(Method::DELETE);
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].path, format!("{JOBS}/goble-goble-2fbfef8fa780"));
    assert_eq!(deletes[0].query.as_deref(), Some("force=true"));
}

#[tokio::test]
async fn deleting_a_job_that_is_already_gone_is_success() {
    let mut mock = Mock::new(true, json!({ "name": EXECUTION }));
    mock.delete_status = StatusCode::NOT_FOUND;
    let (base_url, _recorder) = start_mock(mock).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    let handle = SandboxHandle::from_request("cloud_run", EXECUTION, &request());
    provider.destroy(&handle).await.expect("already gone");
}

// --- status ----------------------------------------------------------------

#[tokio::test]
async fn status_reads_the_execution_the_handle_names() {
    let (base_url, recorder) = start_mock(Mock::running()).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    let handle = SandboxHandle::from_request("cloud_run", EXECUTION, &request());
    assert_eq!(
        provider.status(&handle).await.expect("status"),
        SandboxStatus::Running
    );
    assert_eq!(
        recorder.calls(Method::GET)[0].path,
        format!("/v2/{EXECUTION}")
    );
}

#[tokio::test]
async fn a_finished_execution_reports_missing() {
    let mut mock = Mock::new(true, json!({ "name": EXECUTION }));
    mock.execution = json!({
        "name": EXECUTION,
        "completionTime": "2026-01-01T00:00:00Z",
    });
    let (base_url, _recorder) = start_mock(mock).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    let handle = SandboxHandle::from_request("cloud_run", EXECUTION, &request());
    assert_eq!(
        provider.status(&handle).await.expect("status"),
        SandboxStatus::Missing
    );
}

#[tokio::test]
async fn an_absent_execution_reports_missing() {
    let (base_url, _recorder) = start_mock(Mock::new(true, Value::Null)).await;
    let provider = provider(&base_url, MockRunner::ok("ya29.test-token")).await;

    let handle = SandboxHandle::from_request("cloud_run", EXECUTION, &request());
    assert_eq!(
        provider.status(&handle).await.expect("status"),
        SandboxStatus::Missing
    );

    // A handle with no execution name never reaches the API.
    let empty = SandboxHandle {
        sandbox_id: String::new(),
        ..SandboxHandle::from_request("cloud_run", EXECUTION, &request())
    };
    assert_eq!(
        provider.status(&empty).await.expect("status"),
        SandboxStatus::Missing
    );
}

// --- construction ----------------------------------------------------------

#[test]
fn a_table_without_a_project_or_location_is_refused() {
    let mut config = agent();
    config.environment = None;
    let error = provider_for(&config).err().expect("no project");
    assert!(
        error.to_string().contains("GOOGLE_CLOUD_PROJECT"),
        "{error}"
    );

    let mut config = agent();
    config.region = Some("  ".to_string());
    let error = provider_for(&config).err().expect("no location");
    assert!(error.to_string().contains("GOOGLE_CLOUD_REGION"), "{error}");
}

#[test]
fn the_factory_builds_a_cloud_run_adapter() {
    let provider = provider_for(&agent()).expect("the table is complete");
    assert_eq!(provider.kind(), "cloud_run");
}

#[test]
fn a_long_workspace_name_still_fits_a_job_id() {
    let long = format!("goble-{}-2fbfef8fa780", "slug-".repeat(20));
    let id = goble_remote::cloud_run::job_id(&long);
    assert!(id.len() <= 49, "{id} is {} chars", id.len());
    assert!(id.starts_with("goble-"), "{id}");
    assert!(id.ends_with("-2fbfef8fa780"), "{id}");
}
