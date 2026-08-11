# Goble Agent Runtime

The agent runtime (`crates/goblin-worker/src/agent_runtime/`) is an autonomous execution loop that lets an LLM read and write files, manage a per-agent checklist, and finish its own run.

## Overview

When the worker receives a `RunAgent` or `RunAgentForThreadReply` message, `Runner` creates an `AgentRuntime` and gives it:

- the `AgentSpec` (goal, optional MCP ids),
- a configured `Box<dyn LlmProvider>`,
- a freshly created workspace directory,
- a persisted `RuntimeState`.

The runtime then enters a loop of up to 50 steps. Each step sends the system prompt, current state, and conversation history to the LLM, receives either tool calls or a final message, executes the requested tools, and repeats until the agent calls `finish` or the step limit is reached.

## System prompt

The system prompt is built in `agent_runtime/runtime.rs` and contains:

- the agent description and goal from `AgentSpec::prompt`,
- instructions to use tools and call `finish` when done,
- the current checklist with item ids,
- any notes stored in `RuntimeState`,
- implicit workspace path (tools only resolve paths inside it).

Because the state is re-sent on every turn, the LLM can see previous checklist progress and notes without relying on the full message history.

## Available tools

All tools are synchronous functions handled by `ToolRegistry`. Each tool returns a `ToolResult` that may also carry an updated `RuntimeState`.

| Tool | Purpose |
|------|---------|
| `console` | Log a free-form message to the in-memory console log. |
| `read_file` | Read a text file within the workspace; truncates to 100,000 chars. |
| `edit_file` | Replace `old_string` with `new_string` in a workspace file. Empty `old_string` creates a new file. |
| `list_files` | List files and directories inside the workspace. |
| `thinking` | Record a thought in the execution trace without changing state. |
| `mull` | Append a note to `RuntimeState.notes`. |
| `update_state` | Add a checklist item, mark an item done, or append a note. |
| `self_improve` | Append a feedback item to `RuntimeState.self_feedback`. |
| `finish` | End the loop and provide a final summary. |

Tools receive JSON arguments and return a plain string result that is sent back to the LLM as a tool message.

## Runtime state schema

`RuntimeState` is persisted as `<workspace>/.runtime_state.json`.

```rust
pub struct RuntimeState {
    pub version: u32,
    pub checklist: Vec<ChecklistItem>,
    pub notes: Vec<String>,
    pub self_feedback: Vec<String>,
}

pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}
```

The `version` field supports future migrations. Currently only version `1` is defined.

## Loop lifecycle

1. `Runner::run_agent` installs any configured MCP servers and connects them.
2. `AgentRuntime::run` loads (or creates) the workspace and `RuntimeState`.
3. Each iteration builds a `CompletionRequest` with the system prompt, state, history, and tool definitions.
4. Tool calls are executed in order; their results become tool messages.
5. State-changing tools (`update_state`, `self_improve`, `mull`) return a new `RuntimeState` which is saved to disk.
6. When `finish` is called, the loop ends with `ExecutionStatus::Success` and the summary is returned.
7. If the max step count is reached without `finish`, the trace is marked successful but a warning is logged.

## Context summarization algorithm

To avoid unbounded message growth the runtime applies two limits:

- `MAX_HISTORY_MESSAGES = 40`: after each turn the oldest non-system messages are dropped.
- `SUMMARIZE_THRESHOLD = 80`: when history grows beyond this limit, the runtime asks the LLM to summarize the older messages into a short paragraph. The summary is inserted as a user message and the oldest messages are removed.

The summarization request is sent to the same provider; no separate model is required. This is a simple stopgap until token counting is implemented.

## Self-improvement loop

The `self_improve` tool appends feedback strings to `RuntimeState.self_feedback`. On future runs the feedback items are included in the system prompt, so the agent can adjust its behavior based on observations from previous runs.

Because `RuntimeState` is persisted per workspace, self-feedback survives process restarts.

## Security

- All file tools resolve their path and verify it stays inside the workspace root using `starts_with` on canonical paths.
- Reading a file with a relative escape such as `../etc/passwd` returns an error.
- `edit_file` with an empty `old_string` can only create files under the workspace.
- Each agent gets its own workspace directory, so agents cannot read or overwrite each other's files.
- MCP server credentials are passed through environment variables mapped from worker secrets, never embedded in prompts or state.

## Configuration

The runtime is currently configured through constants in `agent_runtime/runtime.rs`:

- `DEFAULT_MAX_STEPS` (50)
- `MAX_HISTORY_MESSAGES` (40)
- `SUMMARIZE_THRESHOLD` (80)

These can be promoted to `AgentSpec` or worker config fields when per-agent tuning is needed.

## Integration test

See `crates/goblin-worker/tests/agent_runtime_integration.rs` for an end-to-end test using a scripted mock provider. The test verifies that the agent can write a file via `edit_file` and finish, and that thread replies return the expected summary.
