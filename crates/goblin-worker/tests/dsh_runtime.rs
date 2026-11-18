//! The remote DSH runtime, end to end at the process boundary.
//!
//! Every test points `GOBLE_DSH_BIN`/`DshConfig::bin` at a fake `dsh` shell
//! script that emits the same newline-delimited JSON the real headless profile
//! emits (see `@deepseek-ai/dsh-headless/src/json-stream.ts`), so the runner's
//! spawn, stdin, parsing, frame projection, timeout, cancel and credential
//! handling are all exercised for real — no fixture trait, no mocked process.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goble_core::protocol::WorkerMessage;
use goblin_worker::dsh_runner::{
    run, DshConfig, DshEventSink, DshRequest, DshRuns, DEFAULT_CREDENTIALS_SECRET,
};

/// A sink that records every frame the runner projects.
#[derive(Default)]
struct RecordingSink {
    messages: Mutex<Vec<WorkerMessage>>,
}

impl RecordingSink {
    fn messages(&self) -> Vec<WorkerMessage> {
        self.messages.lock().unwrap().clone()
    }

    fn kinds(&self) -> Vec<String> {
        self.messages()
            .iter()
            .map(|message| {
                serde_json::to_value(message).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    fn count(&self, kind: &str) -> usize {
        self.kinds()
            .iter()
            .filter(|candidate| *candidate == kind)
            .count()
    }
}

impl DshEventSink for RecordingSink {
    fn emit(&self, message: WorkerMessage) {
        self.messages.lock().unwrap().push(message);
    }
}

/// Write an executable fake `dsh` with `body` as its shell body.
fn fake_dsh(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-dsh.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake dsh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make fake dsh executable");
    }
    path
}

/// A config rooted in `dir`: fake binary, temporary home and workspace.
fn config_for(dir: &Path, bin: PathBuf) -> DshConfig {
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    DshConfig {
        bin: bin.display().to_string(),
        harness_path: dir.join("harness.yml"),
        home: dir.join("dsh-home"),
        model: None,
        workspace,
        timeout: Duration::from_secs(15),
        credentials_yaml: None,
        keep_credentials: false,
        credentials_secret: DEFAULT_CREDENTIALS_SECRET.to_string(),
        extra_args: Vec::new(),
        extra_env: Vec::new(),
        binding_session_id: None,
    }
}

fn request(trace: &str, task: &str, session: Option<&str>) -> DshRequest {
    DshRequest {
        trace_id: trace.to_string(),
        task: task.to_string(),
        native_session_id: session.map(str::to_string),
    }
}

/// Wait for a file to appear and return its content (the tests use it for a
/// value the fake script writes at startup).
async fn wait_for_file(path: &Path, timeout: Duration) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.trim().is_empty() {
                return content;
            }
        }
        if tokio::time::Instant::now() > deadline {
            panic!("{} never appeared", path.display());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn text_tool_calls_usage_and_turn_end_become_worker_frames() {
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    let task_file = dir.path().join("task.txt");
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' "$@" > "$DSH_TEST_ARGS"
cat > "$DSH_TEST_TASK"
printf '%s\n' '{"type":"session","sessionId":"session-abc","cwd":"/workspace"}'
printf '%s\n' '{"type":"status","phase":"turn_start","turn":1}'
printf '%s\n' '{"type":"text","text":"hello "}'
printf '%s\n' '{"type":"text","text":"world"}'
printf '%s\n' '{"type":"tool_call","callId":"c1","tool":"bash","input":{"command":"ls"}}'
printf '%s\n' '{"type":"tool_result","callId":"c1","status":"completed","result":"file.txt"}'
printf '%s\n' '{"type":"status","phase":"step_end","turn":1,"step":1,"usage":{"inputTokens":10,"outputTokens":5,"totalTokens":15}}'
printf '%s\n' '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}'
printf '%s\n' '{"type":"final","sessionId":"session-abc","reason":{"kind":"completed"},"text":"hello world"}'
exit 0
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.extra_env = vec![
        ("DSH_TEST_ARGS".to_string(), args_file.display().to_string()),
        ("DSH_TEST_TASK".to_string(), task_file.display().to_string()),
    ];

    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(
        &config,
        &request("trace-1", "do the thing", None),
        &sink,
        &runs,
    )
    .await;

    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.final_text, "hello world");
    assert_eq!(outcome.streamed_text, "hello world");
    assert_eq!(outcome.native_session_id.as_deref(), Some("session-abc"));
    assert!(outcome.turn_ended);
    assert_eq!(
        outcome.usage.map(|usage| usage.total_tokens),
        Some(Some(15))
    );

    let kinds = sink.kinds();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "assistant_delta")
            .count(),
        2,
        "{kinds:?}"
    );
    assert_eq!(sink.count("tool_call_started"), 1, "{kinds:?}");
    assert_eq!(sink.count("tool_call_finished"), 1, "{kinds:?}");
    assert_eq!(sink.count("agent_log"), 1, "the usage line: {kinds:?}");
    assert_eq!(sink.count("done"), 1, "exactly one turn end: {kinds:?}");

    let tool = sink
        .messages()
        .into_iter()
        .find_map(|message| match message {
            WorkerMessage::ToolCallStarted {
                id,
                name,
                arguments,
                ..
            } => Some((id, name, arguments)),
            _ => None,
        })
        .expect("a tool_call frame");
    assert_eq!(tool.0, "c1");
    assert_eq!(tool.1, "bash");
    assert_eq!(tool.2, serde_json::json!({"command": "ls"}));

    let usage_log = sink
        .messages()
        .into_iter()
        .find_map(|message| match message {
            WorkerMessage::AgentLog { message, .. } => Some(message),
            _ => None,
        })
        .expect("a usage log line");
    assert!(usage_log.contains("10 in / 5 out"), "{usage_log}");
    assert!(usage_log.contains("total 15"), "{usage_log}");

    // The task reached the harness on stdin, with a trailing newline.
    assert_eq!(
        std::fs::read_to_string(&task_file).unwrap(),
        "do the thing\n"
    );

    // The argv is dsh-remote's shape; a fresh run names no session.
    let args = read_lines(&args_file);
    assert_eq!(args[0], "--profile");
    assert_eq!(args[1], "headless");
    assert_eq!(args[2], "--patch");
    assert_eq!(args[3], config.harness_path.display().to_string());
    assert!(args.contains(&"--json".to_string()), "{args:?}");
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--permission-mode", "danger-full-access"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--approval-policy", "never"]));
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert!(
        !args.iter().any(|arg| arg == "--session-id"),
        "a fresh run resumes nothing: {args:?}"
    );
}

#[tokio::test]
async fn a_resumed_run_passes_the_native_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' "$@" > "$DSH_TEST_ARGS"
cat >/dev/null
printf '%s\n' '{"type":"session","sessionId":"session-existing"}'
printf '%s\n' '{"type":"text","text":"continued"}'
printf '%s\n' '{"type":"status","phase":"turn_end","turn":2,"reason":{"kind":"completed"}}'
printf '%s\n' '{"type":"final","sessionId":"session-existing","reason":{"kind":"completed"},"text":"continued"}'
exit 0
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.extra_env = vec![("DSH_TEST_ARGS".to_string(), args_file.display().to_string())];

    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(
        &config,
        &request("trace-2", "keep going", Some("session-existing")),
        &sink,
        &runs,
    )
    .await;

    assert!(outcome.ok, "{outcome:?}");
    let args = read_lines(&args_file);
    let session_at = args
        .iter()
        .position(|arg| arg == "--session-id")
        .expect("a resumed run names its session");
    assert_eq!(args[session_at + 1], "session-existing");
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert_eq!(
        outcome.native_session_id.as_deref(),
        Some("session-existing")
    );
}

#[tokio::test]
async fn a_cancel_kills_the_whole_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let child_file = dir.path().join("child.pid");
    let parent_file = dir.path().join("parent.pid");
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' '{"type":"session","sessionId":"session-long"}'
printf '%s\n' '{"type":"status","phase":"turn_start","turn":1}'
sleep 300 &
echo $! > "$DSH_TEST_CHILD"
echo $$ > "$DSH_TEST_PARENT"
wait
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.extra_env = vec![
        (
            "DSH_TEST_CHILD".to_string(),
            child_file.display().to_string(),
        ),
        (
            "DSH_TEST_PARENT".to_string(),
            parent_file.display().to_string(),
        ),
    ];

    let sink = Arc::new(RecordingSink::default());
    let runs = DshRuns::new();
    let runner = {
        let sink = sink.clone();
        let runs = runs.clone();
        let config = config.clone();
        tokio::spawn(async move {
            run(
                &config,
                &request("trace-cancel", "wait", None),
                sink.as_ref(),
                &runs,
            )
            .await
        })
    };

    let child_pid = wait_for_file(&child_file, Duration::from_secs(10)).await;
    let parent_pid = wait_for_file(&parent_file, Duration::from_secs(10)).await;
    assert!(runs.is_running("trace-cancel"));
    assert!(runs.cancel("trace-cancel"), "the live run is cancelled");
    assert!(
        !runs.cancel("trace-cancel"),
        "an already-stopped run is not cancellable twice"
    );

    let started = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(10), runner)
        .await
        .expect("the cancelled run settles")
        .expect("the runner task joins");
    assert!(outcome.cancelled, "{outcome:?}");
    assert!(!outcome.ok);
    assert!(!outcome.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel does not wait for the timeout"
    );
    assert!(!runs.is_running("trace-cancel"));

    // Both the harness and the `sleep` it started are gone: the runner killed
    // the process group, not just the direct child.
    for pid in [parent_pid.trim(), child_pid.trim()] {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let alive = std::process::Command::new("kill")
                .args(["-0", pid])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if !alive {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "process {pid} outlived the cancelled run"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[tokio::test]
async fn a_run_over_its_budget_is_killed_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' '{"type":"session","sessionId":"session-slow"}'
printf '%s\n' '{"type":"status","phase":"turn_start","turn":1}'
sleep 120
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.timeout = Duration::from_millis(300);

    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let started = Instant::now();
    let outcome = run(
        &config,
        &request("trace-timeout", "slow", None),
        &sink,
        &runs,
    )
    .await;

    assert!(outcome.timed_out, "{outcome:?}");
    assert!(!outcome.ok);
    assert!(!outcome.cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "it returns promptly"
    );
    let error = outcome.error.unwrap_or_default();
    assert!(error.contains("did not finish"), "{error}");
    assert!(
        sink.kinds().iter().any(|kind| kind == "agent_log"),
        "the timeout is reported on the trace: {:?}",
        sink.kinds()
    );
    assert!(!runs.is_running("trace-timeout"));
}

#[tokio::test]
async fn a_failed_turn_reports_the_harness_error() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' '{"type":"session","sessionId":"session-bad"}'
printf '%s\n' '{"type":"error","message":"model route not signed in"}'
exit 1
"#,
    );
    let config = config_for(dir.path(), script);
    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(&config, &request("trace-bad", "fail", None), &sink, &runs).await;

    assert!(!outcome.ok);
    assert_eq!(outcome.error.as_deref(), Some("model route not signed in"));
    assert_eq!(outcome.exit_code, Some(1));
    let error_logs: Vec<String> = sink
        .messages()
        .into_iter()
        .filter_map(|message| match message {
            WorkerMessage::AgentLog { level, message, .. }
                if level == goble_core::execution::LogLevel::Error =>
            {
                Some(message)
            }
            _ => None,
        })
        .collect();
    assert!(
        error_logs
            .iter()
            .any(|message| message.contains("model route not signed in")),
        "{error_logs:?}"
    );
}

#[tokio::test]
async fn a_missing_dsh_binary_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_for(dir.path(), dir.path().join("no-such-dsh"));
    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(&config, &request("trace-missing", "hi", None), &sink, &runs).await;

    assert!(!outcome.ok);
    let error = outcome.error.unwrap_or_default();
    assert!(error.contains("could not start"), "{error}");
    assert!(sink.count("agent_log") >= 1);
}

#[tokio::test]
async fn credentials_are_installed_for_the_run_and_deleted_afterwards() {
    let dir = tempfile::tempdir().unwrap();
    let seen_file = dir.path().join("credentials-seen.txt");
    let script = fake_dsh(
        dir.path(),
        r#"
cat >/dev/null
if [ -f "$DSH_HOME/.credentials.yaml" ]; then
  echo "present" > "$DSH_TEST_SEEN"
  cat "$DSH_HOME/.credentials.yaml" >> "$DSH_TEST_SEEN"
else
  echo "absent" > "$DSH_TEST_SEEN"
fi
printf '%s\n' '{"type":"session","sessionId":"session-creds"}'
printf '%s\n' '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}'
printf '%s\n' '{"type":"final","sessionId":"session-creds","reason":{"kind":"completed"},"text":"ok"}'
exit 0
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.credentials_yaml = Some("token: deepseek-account-secret".to_string());
    config.extra_env = vec![("DSH_TEST_SEEN".to_string(), seen_file.display().to_string())];

    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(&config, &request("trace-creds", "hi", None), &sink, &runs).await;
    assert!(outcome.ok, "{outcome:?}");

    let seen = std::fs::read_to_string(&seen_file).unwrap();
    assert!(
        seen.starts_with("present"),
        "the harness saw credentials: {seen}"
    );
    assert!(
        seen.contains("token: deepseek-account-secret"),
        "the credentials content arrived: {seen}"
    );
    assert!(
        !config.credentials_path().exists(),
        "the credentials file is deleted after the last run"
    );
    // The generated runtime overlay is not a secret and stays for the next run.
    assert!(config.runtime_patch_path().is_file());
    let patch = std::fs::read_to_string(config.runtime_patch_path()).unwrap();
    assert!(patch.contains("session-persistence-jsonl"));
    assert!(patch.contains(&config.sessions_path().display().to_string()));
}

#[tokio::test]
async fn a_model_override_becomes_a_second_patch_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' "$@" > "$DSH_TEST_ARGS"
cat >/dev/null
printf '%s\n' '{"type":"session","sessionId":"session-model"}'
printf '%s\n' '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}'
printf '%s\n' '{"type":"final","sessionId":"session-model","reason":{"kind":"completed"},"text":"ok"}'
exit 0
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.model = Some("deepseek-pro".to_string());
    config.extra_env = vec![("DSH_TEST_ARGS".to_string(), args_file.display().to_string())];

    let sink = RecordingSink::default();
    let runs = DshRuns::new();
    let outcome = run(&config, &request("trace-model", "hi", None), &sink, &runs).await;
    assert!(outcome.ok, "{outcome:?}");

    let args = read_lines(&args_file);
    let patches: Vec<&String> = args
        .windows(2)
        .filter(|pair| pair[0] == "--patch")
        .map(|pair| &pair[1])
        .collect();
    assert_eq!(
        patches.len(),
        2,
        "the composition then the generated overlay: {args:?}"
    );
    assert_eq!(patches[0], &config.harness_path.display().to_string());
    assert_eq!(
        patches[1],
        &config.runtime_patch_path().display().to_string()
    );
    let overlay = std::fs::read_to_string(config.runtime_patch_path()).unwrap();
    assert!(overlay.contains("agent-default-model"));
    assert!(overlay.contains("\"deepseek-pro\""));
}

/// A worker restored from a snapshot resumes the conversation its binding
/// named, and once a turn has run the thread's own remembered session wins.
#[tokio::test]
async fn a_thread_reply_resumes_the_binding_session_then_its_own() {
    use goble_core::agent::AgentId;
    use goble_core::worker::WorkerId;
    use goblin_worker::dsh_runner::run_thread_reply;
    use goblin_worker::AppState;

    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    let script = fake_dsh(
        dir.path(),
        r#"
printf '%s\n' "$@" > "$DSH_TEST_ARGS"
cat >/dev/null
printf '%s\n' '{"type":"session","sessionId":"session-native-1"}'
printf '%s\n' '{"type":"text","text":"ok"}'
printf '%s\n' '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}'
printf '%s\n' '{"type":"final","sessionId":"session-native-1","reason":{"kind":"completed"},"text":"ok"}'
exit 0
"#,
    );
    let mut config = config_for(dir.path(), script);
    config.binding_session_id = Some("session-binding".to_string());
    config.extra_env = vec![("DSH_TEST_ARGS".to_string(), args_file.display().to_string())];

    let state = AppState::new(WorkerId("worker-dsh".to_string()));
    state.set_store_path(dir.path().join("tasks.db")).unwrap();
    let agent = AgentId("agent-dsh".to_string());

    let reply = run_thread_reply(
        &state,
        &config,
        "trace-bind-1".to_string(),
        "thread-bind".to_string(),
        agent.clone(),
        "first".to_string(),
    )
    .await
    .expect("the first turn runs");
    assert_eq!(reply.as_deref(), Some("ok"));
    let args = read_lines(&args_file);
    let session_at = args
        .iter()
        .position(|arg| arg == "--session-id")
        .expect("the binding session is used");
    assert_eq!(args[session_at + 1], "session-binding");

    // The turn reported its native session; the next turn on the thread must
    // resume that one instead of the binding's.
    let _ = std::fs::remove_file(&args_file);
    let reply = run_thread_reply(
        &state,
        &config,
        "trace-bind-2".to_string(),
        "thread-bind".to_string(),
        agent,
        "second".to_string(),
    )
    .await
    .expect("the second turn runs");
    assert_eq!(reply.as_deref(), Some("ok"));
    let args = read_lines(&args_file);
    let session_at = args
        .iter()
        .position(|arg| arg == "--session-id")
        .expect("the remembered session is used");
    assert_eq!(args[session_at + 1], "session-native-1");
}
