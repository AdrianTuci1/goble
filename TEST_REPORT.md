# Goble Test Report

Generated: 2026-07-23

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core | 62 passed |
| goble-cli (lib) | 2 passed |
| goble-cli (integration) | 9 passed |
| goblin-worker (lib) | 11 passed |
| goblin-worker (bin) | 13 passed |
| goble-ui | 4 passed |
| goble-desktop | 0 tests |
| **Total** | **101 passed, 0 failed** |

## Recent additions

- Persistent scheduled task store:
  - `goblin-worker::task_store::TaskStore` backed by SQLite.
  - `goblin-worker::scheduler::Scheduler` supports cron, heartbeat, manual, and HTTP triggers.
  - Scheduler loop runs every 5s and dispatches triggers asynchronously.
  - Worker main opens task store and starts the scheduler loop.
  - Protocol extended with `ListScheduledTasks`, `CancelScheduledTask`, `ScheduledTasks`, `TaskCancelled`.
  - `goble-cli schedule-manage list|cancel` subcommands.

- Encrypted credential vault (previous commit).
- mTLS WebSocket handshake (previous commits).

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace
```

All commands succeeded.
