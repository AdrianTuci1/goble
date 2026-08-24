# Goble Test Report

Generated: 2026-07-26

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core (lib) | 107 passed |
| goble-core (integration) | 11 passed |
| goble-cli (lib) | 4 passed |
| goble-cli (integration) | 3 passed |
| goblin-worker (lib) | 11 passed |
| goblin-worker (bin) | 15 passed |
| goble-desktop (frontend) | 3 passed |
| **Total** | **154+ passed, 0 failed** |

## Recent additions

- MCP 4-level backend: search, install/list/update/delete, discover, execute + fallback.
- MCP desktop UI with side drawer for vault secret selection and enabled tool toggles.
- Tauri commands and API bindings for all MCP operations.
- `enabled_tools` filtering in tool definitions sent to the LLM.
- Encrypted credential vault.
- mTLS WebSocket handshake.
- Persistent scheduled task store with cron, heartbeat, manual, and HTTP triggers.

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace

cd /root/goble/crates/goble-desktop
npm test
npm run build
npm run tauri build
```

All commands succeeded.

## Native UI (wgpu) baseline — 2026-08-24

Generated during Faza 0 of the Tauri → native migration on `feature/agent-guide-ui`.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 100 passed |
| goble-desktop-service (lib) | 23 passed |
| goble-desktop-native (check) | compiles clean |
| goble-app (check) | compiles clean |
| workspace `cargo check --all-targets` | passes |

Fix included in baseline: `crates/goble-ui/src/elements/sheet.rs` — `Rect::contains` now receives a `PointF` (was passing a `Vector2F`), matching the pattern used by other input elements.
