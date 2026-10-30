use super::*;
use crate::harness::*;

use crate::harness::runner::{shell_line, split_command_line};
use futures::StreamExt;
use goble_sandbox::Sandbox;

struct FakePaneSession {
    output: String,
    lines: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl PaneSession for FakePaneSession {
    async fn run_in_pane(&self, line: &str) -> Result<String> {
        self.lines.lock().unwrap().push(line.to_string());
        Ok(self.output.clone())
    }
}

/// One `run_command` tool call, driven through a real turn.
async fn run_command_result(
    runner: Arc<dyn CommandRunner>,
    command: &str,
    args: serde_json::Value,
) -> std::result::Result<String, String> {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc-pane".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({ "command": command, "args": args }),
            }],
            usage: None,
        },
    ));
    // These tests exercise the runner's routing, not the approval gate.
    let harness = Harness::new(store)
        .with_llm(llm)
        .with_runner(runner)
        .with_auto_approve(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "run it", "mock", "mock")
        .collect()
        .await;
    for event in &events {
        match event {
            HarnessEvent::ToolCallFinished { result, .. } => return Ok(result.clone()),
            HarnessEvent::ToolCallError { message, .. } => return Err(message.clone()),
            _ => {}
        }
    }
    panic!("the turn produced no tool outcome: {events:?}");
}

/// The agent's shell tool runs in the pane's session and returns the pane's
/// output — the whole point of agent + terminal mode.

#[tokio::test]
async fn a_pane_session_runs_the_agents_command_and_returns_its_output() {
    let pane = Arc::new(FakePaneSession {
        output: "total 0\nfile.txt".to_string(),
        lines: std::sync::Mutex::new(Vec::new()),
    });
    let runner = Arc::new(
        RoutedCommandRunner::new(Arc::new(SandboxedCommandRunner::default_tools()))
            .with_pane_session(Some(pane.clone())),
    );
    assert!(runner.runs_in_pane());

    let result = run_command_result(runner, "ls", serde_json::json!(["-la"]))
        .await
        .expect("the pane's command succeeds");
    assert_eq!(result, "total 0\nfile.txt");
    assert_eq!(
        pane.lines.lock().unwrap().as_slice(),
        ["ls -la"],
        "the pane ran the composed command line"
    );
}

#[tokio::test]
async fn a_pane_session_is_not_gated_by_the_sandbox_allow_list() {
    let pane = Arc::new(FakePaneSession {
        output: "ran in the pane".to_string(),
        lines: std::sync::Mutex::new(Vec::new()),
    });
    let runner = Arc::new(
        RoutedCommandRunner::new(Arc::new(SandboxedCommandRunner::default_tools()))
            .with_pane_session(Some(pane)),
    );
    let result = run_command_result(runner, "rm", serde_json::json!(["-rf", "/tmp/nothing"]))
        .await
        .expect("the pane's shell is not allow-listed");
    assert_eq!(result, "ran in the pane");
}

#[tokio::test]
async fn the_sandbox_runs_the_tool_when_the_pane_has_no_session() {
    let runner = Arc::new(RoutedCommandRunner::new(Arc::new(
        SandboxedCommandRunner::default_tools(),
    )));
    assert!(!runner.runs_in_pane());

    let err = run_command_result(runner, "rm", serde_json::json!(["-rf", "/"]))
        .await
        .expect_err("the sandbox still refuses an unknown command");
    assert!(
        err.contains("not in the allowed list"),
        "the sandbox's allow-list is still enforced: {err}"
    );
}

#[test]
fn a_pane_line_composes_the_argv_and_quotes_what_the_shell_would_change() {
    assert_eq!(shell_line("ls", &[]).as_deref(), Some("ls"));
    assert_eq!(
        shell_line("git", &["status".to_string()]).as_deref(),
        Some("git status")
    );
    assert_eq!(
        shell_line("echo", &["a b".to_string()]).as_deref(),
        Some("echo 'a b'")
    );
    assert_eq!(
        shell_line("echo", &["it's".to_string()]).as_deref(),
        Some("echo 'it'\\''s'")
    );
    assert_eq!(shell_line("   ", &[]), None);
}

#[test]
fn a_composed_line_splits_back_into_the_same_argv() {
    assert_eq!(
        split_command_line("git status"),
        Some(("git".to_string(), vec!["status".to_string()]))
    );
    assert_eq!(
        split_command_line("echo 'a b'"),
        Some(("echo".to_string(), vec!["a b".to_string()]))
    );
    assert_eq!(
        split_command_line("echo 'it'\\''s'"),
        Some(("echo".to_string(), vec!["it's".to_string()]))
    );
    assert_eq!(split_command_line("   "), None);
}

/// The approval card carries a full line. Approving it in a sandboxed pane must
/// run the command with its arguments, not fail because `"ls -la"` is not a
/// literal allow-list entry.
#[tokio::test]
async fn an_approved_line_runs_with_its_arguments_in_the_sandbox() {
    let runner = SandboxedCommandRunner::default_tools();
    let out = runner
        .run_line("echo 'hello world'")
        .await
        .expect("an allowed command with arguments runs");
    assert_eq!(out, "hello world");
}

/// The user saw this exact line and ran it, so the static allow-list must not
/// overrule that decision. The unattended path keeps the gate.
#[tokio::test]
async fn an_approved_line_is_authorized_even_outside_the_allow_list() {
    let runner = SandboxedCommandRunner::default_tools();
    let out = runner
        .run_line("printf approved")
        .await
        .expect("the approved command runs");
    assert_eq!(out, "approved");

    let err = runner
        .run("printf", &["approved".to_string()])
        .await
        .expect_err("an unapproved command keeps the allow-list gate");
    assert!(err.to_string().contains("not in the allowed list"), "{err}");
}

#[tokio::test]
async fn a_pane_receives_the_approved_line_verbatim() {
    let pane = Arc::new(FakePaneSession {
        output: "ran in the pane".to_string(),
        lines: std::sync::Mutex::new(Vec::new()),
    });
    let runner = RoutedCommandRunner::new(Arc::new(SandboxedCommandRunner::default_tools()))
        .with_pane_session(Some(pane.clone()));
    let out = runner
        .run_line("ls -la | head -5")
        .await
        .expect("the pane's shell runs the line");
    assert_eq!(out, "ran in the pane");
    assert_eq!(pane.lines.lock().unwrap().as_slice(), ["ls -la | head -5"]);
}

#[tokio::test]
async fn test_harness_sandboxed_command_blocks_disallowed() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc13".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "rm", "args": ["-rf", "/"]}),
            }],
            usage: None,
        },
    ));
    let runner = Arc::new(SandboxedCommandRunner::default_tools());
    // Auto-approved so the allow-list refusal is what the test observes,
    // rather than the approval suspension that now precedes every command.
    let harness = Harness::new(store)
        .with_llm(llm)
        .with_runner(runner)
        .with_auto_approve(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "dangerous", "mock", "mock")
        .collect()
        .await;
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallError { message, .. } if message.contains("not in the allowed list"))));
}

#[tokio::test]
async fn sandboxed_runner_without_sandbox_runs_directly() {
    let runner = SandboxedCommandRunner::default_tools();
    let out = runner
        .run("echo", &["fallthrough".to_string()])
        .await
        .unwrap();
    assert!(out.contains("fallthrough"));
}

#[tokio::test]
async fn sandboxed_runner_with_sandbox_none_is_noop() {
    let runner = SandboxedCommandRunner::default_tools().with_sandbox(None);
    let out = runner.run("echo", &["noop".to_string()]).await.unwrap();
    assert!(out.contains("noop"));
}

#[tokio::test]
async fn sandboxed_runner_degrades_when_backend_unavailable() {
    let profile = goble_sandbox::SandboxProfile::hardened("prod");
    let sb: Box<dyn Sandbox> = Box::new(goble_sandbox::NoopSandbox::new(profile));
    let runner = SandboxedCommandRunner::default_tools().with_sandbox(Some(sb));
    let out = runner.run("echo", &["degraded".to_string()]).await.unwrap();
    assert!(out.contains("degraded"));
}
