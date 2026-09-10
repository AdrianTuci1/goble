# 07 — Observability

**Status:** `[~]` partial: executions/traces/logs exist in backend; not surfaced in native shell
**Owns:** executions, traces, logs from the harness
**Depends on:** [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md), [`../06-renderer/README.md`](../06-renderer/README.md)

## What this is

The harness emits events as it runs (agent started/finished, tool calls, assistant deltas, log lines). Observability turns those into a timeline the user can inspect: which agent ran, on which worker, what it did, and what the trace says.

**Every agent we create is itself observable** — an agent is not a black box. Each agent has a live status (idle / running / waiting on a sub-agent / paused), its own executions, its own sub-agent tree, and its own trace. That is the runtime mirror of the planning tracker: the `.agents/` tree tracks the *work to build the product*, while observability tracks the *agents the product runs*.

## Model

- **Agent** — a created, long-lived entity with status + its own executions and sub-agents (see [`../04-agent-runtime/agent-state-and-compaction.md`](../04-agent-runtime/agent-state-and-compaction.md)).
- **Execution** — one run of an agent (id, agent, worker, status, start/end).
- **Sub-agent** — a disposable child run (see [`../04-agent-runtime/subagents.md`](../04-agent-runtime/subagents.md)); observable as its own execution under its parent.
- **Trace** — the ordered event list inside an execution (tool calls, assistant deltas, logs).
- **Logs** — filtered textual stream, global or per-agent/execution/worker.

## Docs

- [`executions-and-trace.md`](executions-and-trace.md) — execution + trace model and the event flow.
- [`logs.md`](logs.md) — the log stream, filtering, and secret scrubbing.

## Reuse & existing code

- Backend: `goble-desktop-service::DesktopState` already has `list_executions`, `get_execution_trace`, `get_logs`, and `handle_worker_message` records agent events (`AgentStarted`/`AgentFinished`/`AgentLog`/`AssistantDelta`/`ToolCallStarted`).
- `goble-core::execution::{ExecutionTrace, TraceEvent, LogLevel}`.

## Key gap

The **native shell does not surface any of it yet** — there is no executions/traces/logs page in `app/src/ui`. The legacy React app has them; the native shell does not.

## Tasks

- [ ] Add an executions/traces view to the native shell.
- [ ] Add a logs view (filter by execution/worker, scrubbed).
- [ ] Add an **agent-level** observability view: live status per agent, its sub-agent tree, and its trace.
- [ ] Ensure remote-worker events stream into the same views.
