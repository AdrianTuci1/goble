# Goble Test Report

Date: 2026-07-23

## Summary

Workspace stabilized, dead-code warnings cleaned, sequential execution trace implemented,
MCP registry added, worker pool for multi-VPS scheduling added, and test suite expanded.

## Test Results

- goble-core: 45 passed
- goble-desktop: 0 passed (scaffold, no tests)
- goble-ui: 4 passed
- goblin-worker (lib): 7 passed
- goblin-worker (bin): 5 passed
- goble-cli (lib): 0 passed
- goble-cli e2e_worker: 1 passed
- goble-cli extended_models: 3 passed
- goble-cli multi_worker_dispatch: 1 passed

Total: 66 passed, 0 failed.

## New Features

### Sequential Execution Trace

- `ExecutionTrace` now supports `add_root_step`, `add_child_step`, `find_step_mut`, `sequential_view`.
- Steps form an arborescence, allowing nested steps like "prepare workspace" → "attach mcp servers" → "execute agent logic".
- `draw_executions` in the desktop TUI renders the tree with indentation and status colors.

### MCP Registry

- `McpRegistry` in `goble-core` seeds built-in servers (PostgreSQL, Filesystem).
- `search` supports natural language matching on name/id/capabilities.
- `resolve` returns the best matching server.
- `instantiate` clones a template and assigns a `credentials_key`.

### Worker Pool

- `WorkerPool` supports `RoundRobin`, `LowestLoad`, and `TaggedFirst` strategies.
- `WorkerSnapshot` captures status, load, and tags for scheduling decisions.
- Offline workers are skipped.

### Expanded Tests

- `crates/goble-cli/tests/e2e_worker.rs`: pair + run agent + assert start/finish.
- `crates/goble-cli/tests/extended_models.rs`: MCP registry, worker pool, execution trace.
- `crates/goble-cli/tests/multi_worker_dispatch.rs`: two workers, both run agents concurrently.
- `protocol.rs`: roundtrip for `AgentLog` and `StatusReport`.
- `runner.rs`: verify root step and team execution.
- `worker_pool.rs`: round-robin, lowest load, tagged first, offline skip, empty pool.

## Notes

- `cargo check --workspace`: clean.
- `cargo test --workspace`: clean.
- `cargo fmt` applied.
