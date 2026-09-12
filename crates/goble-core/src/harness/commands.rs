use crate::llm::LlmToolCall;
use crate::store::Store;
use anyhow::Result;

use super::credentials::expand_credential_refs;
use super::runner::shell_line;
use super::CommandRunner;

/// The tool whose free-form command line is proposed for approval before it runs
/// (A6). The `git_*` tools are not gated: they take structured arguments, not a
/// command the user can edit.
pub(crate) const COMMAND_TOOL: &str = "run_command";

/// The command lines a `run_command` call proposes to the user.
///
/// The line is rendered as the pane's shell would run it, but any
/// `{{credential:<name>}}` placeholder is left unexpanded so a secret is never
/// shown in a proposal.
pub(crate) fn command_candidates(call: &LlmToolCall) -> Vec<String> {
    let command = call.arguments["command"].as_str().unwrap_or_default();
    let args: Vec<String> = call.arguments["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    shell_line(command, &args).into_iter().collect()
}

/// Run one approved command line verbatim through the harness's runner.
///
/// The approved text is the shell line the user saw; credential placeholders are
/// expanded here, at execution time, so they still never reach the transcript.
pub(crate) async fn run_approved_command(
    store: &Store,
    runner: &dyn CommandRunner,
    line: &str,
) -> Result<String> {
    let line = expand_credential_refs(store, line)?;
    runner.run(&line, &[]).await
}

pub(super) async fn run_command(
    store: &Store,
    runner: &dyn CommandRunner,
    args: &serde_json::Value,
) -> Result<String> {
    let command = expand_credential_refs(store, args["command"].as_str().unwrap_or_default())?;
    let cmd_args: Vec<String> = args["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let cmd_args = cmd_args
        .iter()
        .map(|a| expand_credential_refs(store, a))
        .collect::<Result<Vec<_>>>()?;
    runner.run(&command, &cmd_args).await
}
