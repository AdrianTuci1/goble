# Goble Trace & Observability Plan

**Goal:** Make agent execution traces discoverable, filterable, and detailed without cluttering threads. Threads keep only the final agent reply; full observability lives in the trace page.

**Branch:** `feature/agent-guide-ui`

---

## 1 — Store trace_id on agent thread messages

### Objective
When an agent posts a final reply to a thread, the message should carry the originating `trace_id` so the UI can link from the thread to the trace.

### Files to touch
- `crates/goble-core/src/thread.rs` (ThreadMessage model)
- `crates/goble-desktop/src-tauri/src/thread_store.rs` (post_message, tests)
- `crates/goble-desktop/src-tauri/src/lib.rs` (ThreadMessageSummary)
- `crates/goble-desktop/src-tauri/src/state.rs` (ThreadAgentReply handler)
- `crates/goble-desktop/src/tauri/api.ts` (ThreadMessageSummary interface)

### Implementation
1. Add `trace_id: Option<String>` to `ThreadMessage`.
2. Add `trace_id` parameter to `ThreadStore::post_message` and persist it.
3. Add `trace_id` to `ThreadMessageSummary` returned by backend commands.
4. When handling `ThreadAgentReply`, pass the trace_id into `post_message`.

### Verification
- `cargo test -p goble-desktop-tauri --lib` passes.
- `cargo test -p goble-core` passes.

### Commit
`feat(trace): store trace_id on agent thread messages`

---

## 2 — Thread message link to trace

### Objective
Agent messages in threads show a small "View trace" link that opens the trace page filtered to that execution.

### Files to touch
- `crates/goble-desktop/src/pages/ThreadsPage.tsx`
- `crates/goble-desktop/src/pages/ThreadsPage.css`
- `crates/goble-desktop/src/stores/appStore.ts`

### Implementation
1. If `msg.author.kind === 'agent'` and `msg.trace_id`, render a "Trace" link/button on the message.
2. Clicking it sets `selectedTraceId` in store, navigates to `/traces`, and opens the right sidebar history tab (optional).
3. Add subtle CSS for the trace link.

### Verification
- Build passes.
- A thread message from an agent has a clickable trace link.

### Commit
`feat(threads): link agent messages to their execution trace`

---

## 3 — Backend: store full execution traces

### Objective
Executions currently only store summary info. We need to persist the full trace (logs, tool calls, deltas, errors) so the trace page can show details.

### Files to touch
- `crates/goble-core/src/execution.rs` (ExecutionTrace model)
- `crates/goble-core/src/store.rs` (executions table + trace storage)
- `crates/goble-desktop/src-tauri/src/state.rs` (handle worker messages and update trace)
- `crates/goble-desktop/src-tauri/src/lib.rs` (get_execution_trace command)

### Implementation
1. Define a serializable `ExecutionTrace` structure:
   - steps: Vec<ExecutionStep>
   - each step: kind (log, delta, tool_call_started, tool_call_finished, tool_call_error, ask_user, done), timestamp, payload.
2. Extend executions table with a `trace` JSON column (or separate table).
3. In `state.rs`, handle `WorkerMessage` variants `AgentLog`, `AssistantDelta`, `ToolCallStarted`, `ToolCallFinished`, `ToolCallError`, `AskUser`, `Done` and append them to the in-memory trace for the active `trace_id`.
4. On `AgentFinished`, persist the trace to the store.
5. Add Tauri command `get_execution_trace(trace_id: String)` returning the trace.

### Verification
- Backend compiles and tests pass.
- Existing execution tests still pass.

### Commit
`feat(trace): persist full execution traces from worker messages`

---

## 4 — Trace page: stacked list with expand and filters

### Objective
Replace the simple `AgentTracePage` with a rich stacked list of executions. Each row shows summary and expands to show the full trace.

### Files to touch
- `crates/goble-desktop/src/pages/AgentTracePage.tsx`
- `crates/goble-desktop/src/pages/AgentTracePage.css` (or add to Pages.css)
- `crates/goble-desktop/src/tauri/api.ts` (getExecutionTrace)
- `crates/goble-desktop/src/stores/appStore.ts` (selectedTraceId, expandedTraceId)
- `crates/goble-desktop/src/App.tsx` (listen to executions:updated)

### Implementation
1. Load executions from store and refresh on `executions:updated` event.
2. Filters:
   - Status: all / running / success / error
   - Agent select
   - Worker select
   - Text search in trace content
3. Each execution row shows:
   - Date/time (started_at)
   - Status badge
   - Agent name
   - Worker name
   - First line of final content or last log
4. Clicking a row expands it and fetches full trace via `getExecutionTrace`.
5. Expanded view shows trace steps in a vertical timeline:
   - Log entries: timestamp + level + message
   - Assistant deltas: rendered text
   - Tool calls: name, arguments, result or error
   - Ask user: question + quick replies
   - Done: final status
6. If `selectedTraceId` is set on page load, auto-expand that execution and scroll to it.

### Verification
- `npm run build` passes.
- `npm run test` passes.
- Trace page renders executions and expanding a row shows trace details.

### Commit
`feat(trace): stacked execution list with filters and expandable details`

---

## 5 — RightSidebar history improvements

### Objective
The right sidebar history panel should surface active executions and allow quick navigation to trace details.

### Files to touch
- `crates/goble-desktop/src/components/RightSidebar.tsx`
- `crates/goble-desktop/src/components/RightSidebar.css`

### Implementation
1. In HistoryPanel, split running executions from completed ones.
2. Show running executions with a spinner/pulse indicator at the top.
3. Clicking an execution sets `selectedTraceId` and navigates to `/traces`.
4. Add CSS for running indicator.

### Verification
- Right sidebar shows running executions distinctly.
- Click navigates to trace page.

### Commit
`feat(right-sidebar): highlight running executions and link to trace page`

---

## 6 — End-to-end verification and final push

### Verification commands
```bash
# Frontend
cd crates/goble-desktop
npm run build
npm run test

# Backend
cd crates/goble-desktop/src-tauri
cargo check -p goble-desktop-tauri --all-targets
cargo test -p goble-desktop-tauri --lib

# Core
cargo test -p goble-core
```

### Final commit
`chore: verify trace observability plan`

### Push
Push all commits to `feature/agent-guide-ui`.

---

## Notes
- Keep threads clean: only the final `ThreadAgentReply` message appears there.
- Full tool call logs, deltas, and errors live in the trace page.
- Do not add new placeholder UI; replace existing stubs with working behavior.
- No graph/tree visualization; only stacked list + expand.


---

## Verification Results

Run on branch `feature/agent-guide-ui`:

- Frontend build: `npm run build` ✅
- Frontend tests: `npm run test` — 11 passed ✅
- Backend check: `cargo check -p goble-desktop-tauri --all-targets` ✅ no warnings
- Backend tests: `cargo test -p goble-desktop-tauri --lib` — 23 passed ✅
- Core tests: `cargo test -p goble-core` — 149 passed ✅

All commits pushed to `feature/agent-guide-ui`.
