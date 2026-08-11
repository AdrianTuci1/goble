# Goble Agent Runtime Enhancement Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Give runtime agents real tools: console, read/edit file, thinking/mulling, self-improvement, and a persistent editable state/checklist that survives context limits via summarization.

**Architecture:**
- Add a looped **Agent Runtime** in `goblin-worker` that calls the LLM with a system prompt and a set of tools.
- The runtime exposes a **state object** (`AgentState`) that is separate from chat history: it is a JSON checklist/notes blob that the agent can read, add, update, and mark as done. This state is persisted to disk and sent to the LLM on every turn.
- Tools are synchronous functions: `console`, `read_file`, `edit_file`, `list_files`, `thinking`, `mull`, `finish`, `update_state`.
- The agent runs until it calls `finish` or reaches a max step limit.
- When the accumulated messages exceed a token threshold, the runtime asks the LLM to summarize the non-essential parts into a few lines. The context then becomes: `[system prompt, summary, current state, last user prompt, tool results]`.
- All actions are logged into the existing `ExecutionTrace`.

**Tech Stack:** Rust (`goblin-worker`, `goble-core`), existing `LlmProvider`/`CompletionRequest`/`CompletionResponse`/`ToolDefinition`/`LlmToolCall` types in `goble-core`.

---

## Task 1: Define agent runtime types and state

**Objective:** Add an `AgentRuntime` struct and a serializable `RuntimeState` that the agent can edit.

**Files:**
- Create: `crates/goblin-worker/src/agent_runtime.rs`
- Create: `crates/goblin-worker/src/agent_runtime/state.rs` (module)
- Modify: `crates/goblin-worker/src/lib.rs` (add module)

**Step 1: Write failing test**

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    pub checklist: Vec<ChecklistItem>,
    pub notes: Vec<String>,
    pub self_feedback: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

impl RuntimeState {
    pub fn add_checklist(&mut self, text: String) -> String;
    pub fn mark_done(&mut self, id: &str) -> bool;
    pub fn add_note(&mut self, text: String);
}

#[test]
fn test_runtime_state_checklist() {
    let mut state = RuntimeState::default();
    let id = state.add_checklist("read file".to_string());
    assert!(state.mark_done(&id));
    assert!(state.checklist[0].done);
}
```

**Step 2: Run test to verify failure**

```bash
cd /root/goble
cargo test -p goblin-worker agent_runtime::state
```

Expected: FAIL — module not found.

**Step 3: Implement state module**

Add `crates/goblin-worker/src/agent_runtime/state.rs` with the structs and methods above. Persist to disk using `Workspace` path (e.g. `workspace/.runtime_state.json`).

**Step 4: Run test to verify pass**

```bash
cargo test -p goblin-worker agent_runtime::state
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/goblin-worker/src/agent_runtime/state.rs crates/goblin-worker/src/agent_runtime.rs crates/goblin-worker/src/lib.rs
git commit -m "feat(agent-runtime): persistent runtime state with checklist"
```

---

## Task 2: Define agent tools

**Objective:** Add tool definitions and synchronous tool handlers that the agent can invoke.

**Files:**
- Create: `crates/goblin-worker/src/agent_runtime/tools.rs`
- Modify: `crates/goblin-worker/src/agent_runtime.rs`

**Step 1: Write failing test**

```rust
#[test]
fn test_read_file_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
    let result = ToolRegistry::read_file(tmp.path().join("a.txt")).unwrap();
    assert_eq!(result, "hello");
}

#[test]
fn test_edit_file_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "hello world").unwrap();
    ToolRegistry::edit_file(tmp.path().join("a.txt"), "hello", "hi").unwrap();
    let content = std::fs::read_to_string(tmp.path().join("a.txt")).unwrap();
    assert_eq!(content, "hi world");
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goblin-worker agent_runtime::tools
```

Expected: FAIL.

**Step 3: Implement tools module**

Tools to implement:

| Tool | Arguments | Behavior |
|------|-----------|----------|
| `console` | `message` | Append to a runtime log/stdout; return OK. |
| `read_file` | `path` | Read file contents from workspace or allowed paths. |
| `edit_file` | `path`, `old_string`, `new_string` | Replace `old_string` with `new_string` in file. |
| `list_files` | `path` (optional) | List files in directory. |
| `thinking` | `thought` | Append to trace, no LLM call. |
| `mull` | `topic` | Append to `self_feedback` list in runtime state. |
| `update_state` | `json_patch` | Apply a JSON patch to runtime state (add checklist, mark done, add note). |
| `finish` | `summary` | End the agent loop with success status. |

Return JSON string results. Validate paths are within workspace root to prevent escaping.

**Step 4: Run test to verify pass**

```bash
cargo test -p goblin-worker agent_runtime::tools
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/goblin-worker/src/agent_runtime/tools.rs crates/goblin-worker/src/agent_runtime.rs
git commit -m "feat(agent-runtime): tool registry (console, read/edit file, thinking, mull, state)"
```

---

## Task 3: Implement agent loop with tool calls

**Objective:** Build the main loop: send system prompt + state + history to LLM, parse tool calls, execute tools, repeat until finish.

**Files:**
- Modify: `crates/goblin-worker/src/agent_runtime.rs`
- Modify: `crates/goblin-worker/src/runner.rs` (replace ad-hoc LLM call with runtime)

**Step 1: Write failing test**

```rust
#[tokio::test]
async fn test_agent_loop_runs_tool_and_finishes() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(WorkerId::generate());
    {
        let mut cfg = state.config.lock();
        cfg.workspace_root = tmp.path().to_path_buf();
    }
    let runtime = AgentRuntime::new(state.clone());
    let mut spec = AgentSpec::new("test", "write 'done' to result.txt and finish");
    spec.tools = vec!["read_file".into(), "edit_file".into(), "finish".into()];
    let trace_id = uuid::Uuid::new_v4().to_string();
    runtime.run(trace_id, spec.id.clone(), spec, Box::new(MockProvider::new(...))).await.unwrap();
    let result = std::fs::read_to_string(tmp.path().join("agent-workspace/test/result.txt")).unwrap();
    assert_eq!(result.trim(), "done");
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goblin-worker agent_runtime::tests::test_agent_loop_runs_tool_and_finishes
```

Expected: FAIL.

**Step 3: Implement agent loop**

In `agent_runtime.rs`:

```rust
pub struct AgentRuntime {
    state: Arc<AppState>,
    tools: ToolRegistry,
    max_steps: usize,
}

impl AgentRuntime {
    pub async fn run(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        provider: Box<dyn LlmProvider>,
    ) -> anyhow::Result<ExecutionTrace> {
        let mut trace = ExecutionTrace::new(agent_id);
        trace.id = trace_id.clone();
        // ... setup workspace, load RuntimeState
        // Build messages: system + state + user prompt
        // Loop up to max_steps:
        //   - call provider.complete(req)
        //   - log content to trace
        //   - for each tool call, execute tool, append result as assistant + tool messages
        //   - if finish tool, mark success and break
        // Persist RuntimeState after each step
    }
}
```

System prompt should include:
- Agent description and goal.
- Available tools with JSON schemas.
- Instructions to use `update_state` to track progress.
- Instructions to use `finish` when done.
- Notes on workspace path.

**Step 4: Run test to verify pass**

```bash
cargo test -p goblin-worker agent_runtime
```

Expected: PASS (may need iteration).

**Step 5: Commit**

```bash
git add crates/goblin-worker/src/agent_runtime.rs crates/goblin-worker/src/runner.rs
git commit -m "feat(agent-runtime): LLM loop with tool execution"
```

---

## Task 4: Implement context summarization

**Objective:** When messages exceed a token threshold, ask the LLM to summarize older messages into a short summary; keep system, state, and last few turns.

**Files:**
- Modify: `crates/goblin-worker/src/agent_runtime.rs`

**Step 1: Write failing test**

```rust
#[test]
fn test_summarize_trims_context() {
    let mut ctx = ContextWindow::new(4);
    ctx.add_user("a".into());
    ctx.add_assistant("b".into());
    ctx.add_user("c".into());
    ctx.add_assistant("d".into());
    ctx.add_user("e".into());
    let (summary, recent) = ctx.materialize("existing summary");
    assert_eq!(recent.len(), 4); // system + state + last user + assistant
    assert!(summary.contains("..."));
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goblin-worker agent_runtime::context
```

Expected: FAIL.

**Step 3: Implement context window manager**

```rust
pub struct ContextWindow {
    messages: Vec<Message>,
    max_messages: usize,
}

impl ContextWindow {
    pub fn materialize(&mut self, system: Message, state: Message, provider: &dyn LlmProvider) -> Vec<Message> {
        if self.messages.len() <= self.max_messages {
            let mut out = vec![system, state];
            out.extend(self.messages.clone());
            return out;
        }
        // Summarize everything except the last N turns
        let to_summarize = self.messages[..self.messages.len() - self.max_messages].to_vec();
        let summary = self.summarize(provider, &to_summarize).await;
        let mut out = vec![system, Message::system(format!("Summary of earlier work: {summary}"))];
        out.extend(state);
        out.extend(self.messages[self.messages.len() - self.max_messages..].to_vec());
        out
    }
}
```

Use a separate cheap/small model for summarization if configured, otherwise the same provider. Threshold should be configurable (default 12 messages, ~50 turns hard cap).

**Step 4: Run test to verify pass**

```bash
cargo test -p goblin-worker agent_runtime::context
```

Expected: PASS.

**Step 5: Integrate into agent loop**

Replace direct message vector with `ContextWindow`. Call `materialize` before each LLM turn.

**Step 6: Commit**

```bash
git add crates/goblin-worker/src/agent_runtime.rs
git commit -m "feat(agent-runtime): summarize context when it exceeds threshold"
```

---

## Task 5: Add self-improvement tool

**Objective:** Agent can record feedback about itself in runtime state and later refine its approach based on that feedback.

**Files:**
- Modify: `crates/goblin-worker/src/agent_runtime/state.rs`
- Modify: `crates/goblin-worker/src/agent_runtime/tools.rs`
- Modify: `crates/goblin-worker/src/agent_runtime.rs` (system prompt)

**Step 1: Write failing test**

```rust
#[test]
fn test_self_improvement_feedback() {
    let mut state = RuntimeState::default();
    state.add_self_feedback("avoid infinite loops".to_string());
    assert_eq!(state.self_feedback.len(), 1);
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goblin-worker agent_runtime::state
```

Expected: FAIL.

**Step 3: Implement `self_improve` tool and state field**

Add `self_feedback: Vec<String>` to `RuntimeState`. Add tool `self_improve` with `feedback` argument that appends to `self_feedback` and returns "noted".

In the system prompt, include: `Read previous self_feedback items and apply them. Add new feedback when you notice a mistake.`

**Step 4: Run test to verify pass**

```bash
cargo test -p goblin-worker agent_runtime
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/goblin-worker/src/agent_runtime/state.rs crates/goblin-worker/src/agent_runtime/tools.rs crates/goblin-worker/src/agent_runtime.rs
git commit -m "feat(agent-runtime): self-improvement feedback tool"
```

---

## Task 6: Wire runtime into runner and replace ad-hoc LLM calls

**Objective:** Replace `runner.run_agent_for_thread_reply` and `runner.run_agent` with the new AgentRuntime loop.

**Files:**
- Modify: `crates/goblin-worker/src/runner.rs`
- Modify: `crates/goblin-worker/src/scheduler.rs` (trigger uses runner)
- Modify: `crates/goblin-worker/src/websocket.rs` (maybe pass provider config)

**Step 1: Write failing test**

Update existing tests in `runner.rs` to expect trace with tool steps.

```bash
cargo test -p goblin-worker runner::tests
```

Expected: FAIL.

**Step 2: Refactor runner to use AgentRuntime**

```rust
impl Runner {
    pub async fn run_agent(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
    ) -> anyhow::Result<()> {
        let provider = self.build_provider_from_secrets().await?;
        let runtime = AgentRuntime::new(self.state.clone());
        runtime.run(trace_id, agent_id, spec, provider).await?;
        Ok(())
    }

    async fn build_provider_from_secrets(&self) -> anyhow::Result<Box<dyn LlmProvider>> {
        let secrets = self.state.secrets.lock().clone();
        let key = secrets.get("llm_api_key")
            .and_then(|s| String::from_utf8(s.encrypted_value.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("no llm_api_key"))?;
        let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "openai".into());
        Ok(Box::new(goble_core::llm::create_provider(&provider, &key, None)))
    }
}
```

For thread replies, use the same runtime; the prompt is the user message. The agent can use `finish` to emit the final reply content.

**Step 3: Run test to verify pass**

```bash
cargo test -p goblin-worker
```

Expected: PASS (all 16 tests + new ones).

**Step 4: Commit**

```bash
git add crates/goblin-worker/src/runner.rs crates/goblin-worker/src/scheduler.rs crates/goblin-worker/src/websocket.rs
git commit -m "refactor(runner): use AgentRuntime for all agent executions"
```

---

## Task 7: Surface runtime state and traces in desktop UI

**Objective:** Desktop can view the agent's state (checklist, notes, self-feedback) and execution trace.

**Files:**
- Modify: `crates/goble-core/src/protocol.rs` (add WorkerMessage variants)
- Modify: `crates/goblin-worker/src/agent_runtime.rs` (emit state updates)
- Modify: `crates/goble-desktop/src/stores/appStore.ts` (store trace states)
- Modify: `crates/goble-desktop/src/pages/SettingsPage.tsx` or new `AgentTracePage.tsx`

**Step 1: Write failing test**

Add protocol roundtrip test for `AgentStateUpdate`:

```rust
#[test]
fn test_roundtrip_agent_state_update() {
    let msg = WorkerMessage::AgentStateUpdate { trace_id: "t1".into(), state: RuntimeState::default() };
    let bytes = serde_json::to_vec(&msg).unwrap();
    let decoded: WorkerMessage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, msg);
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-core protocol::tests::test_roundtrip_agent_state_update
```

Expected: FAIL.

**Step 3: Add message variant and emit from runtime**

Add `WorkerMessage::AgentStateUpdate { trace_id, state }` and `WorkerMessage::AgentToolResult { trace_id, step_id, name, result }`.

Emit after each tool execution and state persistence.

**Step 4: Run test to verify pass**

```bash
cargo test -p goble-core protocol
```

Expected: PASS.

**Step 5: Desktop UI**

- Add `agentStates: Record<string, RuntimeState>` to appStore.
- Add minimal page: list traces, show checklist with checkboxes, notes, self-feedback.

**Step 6: Commit**

```bash
git add crates/goble-core/src/protocol.rs crates/goblin-worker/src/agent_runtime.rs crates/goble-desktop/src/stores/appStore.ts crates/goble-desktop/src/pages/AgentTracePage.tsx
git commit -m "feat(ui): display agent runtime state and checklist"
```

---

## Task 8: Add end-to-end integration test with mock provider

**Objective:** Prove a full agent can read, edit, think, mull, and finish using a scripted mock LLM.

**Files:**
- Create: `tests/agent_runtime_e2e.rs`

**Step 1: Write test**

```rust
#[tokio::test]
async fn test_agent_writes_file_and_finishes() {
    let tmp = tempfile::tempdir().unwrap();
    // Build a mock provider that returns tool calls in sequence:
    // 1. console("hello")
    // 2. thinking("plan")
    // 3. update_state(add checklist)
    // 4. edit_file(result.txt, "", "done")
    // 5. finish("completed")
    let state = AppState::new(WorkerId::generate());
    state.secrets.lock().insert("llm_api_key".into(), Secret::new_text("mock"));
    let runtime = AgentRuntime::new(state.clone());
    let spec = AgentSpec::new("writer", "create result.txt with 'done'").with_tools(vec!["edit_file", "finish", "update_state"])
        .with_mcp_ids(vec![]);
    let trace_id = "trace-1".into();
    runtime.run(trace_id, spec.id.clone(), spec, Box::new(scripted_mock_provider)).await.unwrap();
    let content = std::fs::read_to_string(tmp.path().join("agent-workspace/writer/result.txt")).unwrap();
    assert_eq!(content, "done");
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble agent_runtime_e2e
```

Expected: FAIL.

**Step 3: Implement scripted mock provider helper**

In `goble-core::llm`, add a `ScriptedProvider` that returns predefined responses.

**Step 4: Run test to verify pass**

```bash
cargo test -p goble agent_runtime_e2e
```

Expected: PASS.

**Step 5: Commit**

```bash
git add tests/agent_runtime_e2e.rs crates/goble-core/src/llm.rs
git commit -m "test(e2e): agent runtime file editing and finish"
```

---

## Task 9: Document agent runtime behavior

**Objective:** Write docs explaining how agents work, what tools are available, how state is persisted, and how context is summarized.

**Files:**
- Create: `docs/agent-runtime.md`

**Step 1: Write docs**

Sections:
- Overview
- System prompt
- Available tools
- Runtime state schema
- Context summarization algorithm
- Self-improvement loop
- Security: workspace sandboxing, path validation

**Step 2: Commit**

```bash
git add docs/agent-runtime.md
git commit -m "docs: agent runtime behavior and tools"
```

---

## Risks & Open Questions

1. **Tool result size:** If a file is very large, sending it back to LLM can blow context. Add a `max_chars` limit and truncate with a note.
2. **Edit_file collisions:** If file changes between read and edit, the old_string may not match. Consider adding a `read_then_edit` atomic tool or checksum.
3. **Infinite loops:** Max steps guard exists, but should we also detect repeated tool calls? Add a simple dedup or cooldown.
4. **LLM provider cost:** Summarization uses extra tokens. Make threshold configurable per agent.
5. **State schema migrations:** Runtime state is JSON. If we change schema, old persisted state may fail. Add a version field.
