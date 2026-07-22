# Goble Test Report

Generated: 2026-07-23

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core | 56 passed |
| goble-cli | 4 passed (2 unit + 2 integration) |
| goblin-worker (lib) | 7 passed |
| goblin-worker (bin) | 5 passed |
| goble-ui | 4 passed |
| goble-desktop | 0 tests |
| **Total** | **76 passed, 0 failed** |

## Recent additions

- `goble-core::tls`: ephemeral CA + server/client certificate generation for mTLS pairing.
- `goble-cli setup-worker`: user-friendly alias for `worker-provision`.
- `ExecutionTrace` tree extensions: `metrics`, `find_step`, `parent_step`, roundtrip serialization tests.
- MCP registry backend tests: auth schema, register/remove, serialization, unknown resolution.
- CLI library split (`src/lib.rs`) so argument parsing can be tested directly.

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace
```

All commands succeeded.
