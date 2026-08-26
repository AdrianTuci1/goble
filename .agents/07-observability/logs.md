# 07 — Logs

**Status:** `[~]` backend built; UI missing
**Owns:** the log stream and its filtering
**Depends on:** [`README.md`](README.md)

## Model

- `LogEntry { id, timestamp, message }` — global or scoped.
- Sources: harness log lines, worker status, MCP server stderr, pairing/install events.
- Severity comes from `goble-core::execution::LogLevel` (Debug/Info/Warn/Error).

## Filtering

- By **execution** (trace id), **worker** id, **level**, or full-text.
- Backend already has `get_logs` and `add_log` on `DesktopState`.

## Secret scrubbing

Logs must **never** contain secret values. Reuse `xai-grok-secrets` (regex sanitizer, see [`../04-agent-runtime/harness-reuse-map.md`](../04-agent-runtime/harness-reuse-map.md)) on the outbound log/event path before it reaches the UI or any telemetry.

## Tasks

- [ ] Add a logs view to the native shell (filter by execution/worker/level).
- [ ] Route the `agent:log` events into the app state (`root_view` currently ignores `agent:*`).
- [ ] Run all log lines through the secret sanitizer before display/emit.
