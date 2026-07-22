# Goble Test Report

Date: 2026-07-22

## Summary

All workspace tests pass, including the new end-to-end WebSocket test that exercises desktop-to-worker agent execution.

## Test Results

- goble-core: 32 passed
- goble-desktop: 0 passed (scaffold, no tests)
- goble-ui: 4 passed
- goblin-worker (lib): 6 passed
- goblin-worker (bin): 4 passed
- goble-cli (lib): 0 passed
- goble-cli e2e_worker: 1 passed

Total: 47 passed, 0 failed.

## End-to-End Test

File: `crates/goble-cli/tests/e2e_worker.rs`

Scenario:
1. Spawn goblin worker on localhost port 0.
2. Connect via WebSocket at `ws://{addr}/ws`.
3. Send `PairRequest` with valid pairing code hash.
4. Send `RunAgent` with an `AgentSpec`.
5. Receive `AgentStarted` and `AgentFinished` for the same trace_id.

Result: PASS.

## CLI Commands Verified

- `goble-cli worker add` (scaffolded)
- `goble-cli worker list`
- `goble-cli worker remove`
- `goble-cli pair`
- `goble-cli run`
- `goble-cli schedule`

## Notes

- cargo check workspace: clean (only dead-code warnings on scaffold fields)
- cargo test workspace: clean
- cargo fmt applied
