---
**STATUS: SUPERSEDED / PARTIALLY IMPLEMENTED**
This document described early planning. Many items have since been implemented or superseded by later plans. Keep for historical context, but do not use as a current source of truth.
---

# Plan: Full Worker Runtime

## Goal
Make the Goblin worker run the full Goble agent runtime (same as desktop harness) so that once a worker/cluster is connected, the local model is no longer used; all agent execution, MCP management, state persistence, and scheduling happen on the worker.

## Where we are now
- `goblin-worker` compiles and exposes a WebSocket server.
- A `harness_runner.rs` module already exists and wraps `goble_core::harness::Harness` for agent execution.
- `Runner` already delegates `run_agent` and `run_agent_for_thread_reply` to the harness.
- `websocket.rs` already handles `RunAgent`, `RunAgentForThreadReply`, `ScheduleAgent`, `PushSecrets`, `SetVaultSecret`, `GetVaultSecret`, `ListScheduledTasks`, `CancelScheduledTask`, and `PushMcpServers`.
- `state.rs` has a SQLite `Store`, a `TaskStore`-backed scheduler, an in-memory MCP registry, and an encrypted vault.
- `file_vault.rs` provides disk-persisted credential vault (currently unused).
- `protocol.rs` already has streaming harness events (`AssistantDelta`, `ToolCallStarted`, `ToolCallFinished`, `ToolCallError`, `AskUser`, `MissionUpdated`, `Done`) on `WorkerMessage`.
- The worker build passes; `npm run tauri build` passes for the desktop.

## What is missing / broken
1. `Harness::run_turn` is sync-blocking; `harness_runner.rs` calls it inside an async function with `StreamExt::next()`.
2. The harness stores agents/workflows/teams in the worker SQLite, but there is no way for the desktop to query/update those entities through the worker.
3. `RunTeam` is a stub that only logs a success trace.
4. MCP servers pushed to the worker are only stored in memory; the harness `McpManager` does not install/discover them because it has no installer configured.
5. The worker does not expose an HTTP health/status endpoint for active traces or mission state.
6. There is no E2E test proving that desktop → worker → harness → mock LLM produces `AgentFinished`.
7. `goble-cli/tests/e2e_worker.rs` currently fails waiting for `AgentFinished`.

## Implementation plan

### 1. Finish harness integration in the worker
- Make `harness_runner.rs` spawn the harness turn on a blocking thread (`tokio::task::spawn_blocking`) and bridge events to `WorkerMessage`.
- Ensure `HarnessEvent::Done` is mapped to `WorkerMessage::Done` and then to `AgentFinished`.
- Add a cancellation token so long-running harness turns can be aborted.

### 2. Add worker-side entity storage & sync
- Use the existing worker `Store` to persist agents/workflows/teams/executions.
- Add a new `DesktopMessage` request `QueryEntities { entity_type, query }` and a `WorkerMessage::EntityList` response.
- Implement handler in `websocket.rs` so desktop can list/query worker state without local execution.

### 3. Implement `RunTeam`
- Resolve the team spec from the worker store.
- Run each member agent sequentially or in parallel using the harness runner.
- Aggregate results and emit `AgentFinished`.

### 4. MCP lifecycle on worker
- Configure `McpManager` with an installer cache directory under the workspace root.
- On `PushMcpServers` / `RunAgent`, call `install_mcp_server` and `discover_and_enable_all`.
- Persist discovered tools in the worker store and expose them via `ListEntities`.

### 5. Vault & secrets
- Replace the in-memory vault usage with `FileVault` in `AppState`.
- On startup, load the vault if a passphrase env var is provided; otherwise require `UnlockVault` message.
- `SetVaultSecret` / `GetVaultSecret` handlers should use `FileVault`.

### 6. Execution persistence & status endpoint
- Persist `ExecutionTrace` in the worker store (add schema if missing).
- Add HTTP `GET /traces` and `GET /traces/:id` endpoints.
- Add `DesktopMessage::GetTrace { trace_id }` and `WorkerMessage::Trace { trace }`.

### 7. Tests
- Unit test: `harness_runner` with mock provider emits `AgentStarted`, `AssistantDelta`/`Done`, `AgentFinished`.
- Integration test: WebSocket round-trip `RunAgent` → `AgentFinished` using mock LLM.
- E2E test: start a real worker process, connect via WebSocket, send `RunAgent`, assert `AgentFinished` and persisted trace.
- Fix `goble-cli/tests/e2e_worker.rs` to use the new flow and pass.
- Add test for `RunTeam` with two mock agents.
- Add test for `ScheduleAgent` + scheduler tick.

### 8. Desktop alignment
- Keep desktop commands; they already send the right messages.
- Update `ThreadsPage.tsx` / `ChatArea.tsx` to listen for new `WorkerMessage` variants (`Done`, streaming tool events).
- Update Tauri capabilities and bundle identifier in a follow-up if needed.

## Acceptance criteria
- `cargo test --workspace` passes (including the fixed `e2e_worker.rs`).
- `npm run tauri build` still passes.
- A real worker process can be started, paired via WebSocket, sent `RunAgent`, and returns `AgentFinished` with a persisted trace.
- `ScheduleAgent` persists and triggers the agent on the scheduler loop.
- `RunTeam` executes multiple agents and reports a single trace.
- Vault secrets survive worker restart.

## Files to touch
- `crates/goblin-worker/src/harness_runner.rs`
- `crates/goblin-worker/src/runner.rs`
- `crates/goblin-worker/src/websocket.rs`
- `crates/goblin-worker/src/state.rs`
- `crates/goblin-worker/src/scheduler.rs`
- `crates/goblin-worker/src/mcp.rs`
- `crates/goblin-worker/src/main.rs`
- `crates/goble-core/src/protocol.rs`
- `crates/goble-core/src/store.rs` (add execution/trace tables if missing)
- `crates/goble-cli/tests/e2e_worker.rs`
- `crates/goblin-worker/tests/*`

## Verification command
```bash
cd /root/goble
cargo test --workspace --no-fail-fast
cd crates/goble-desktop && npm run tauri build
```
