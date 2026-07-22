# Goble Test Report

Generated: 2026-07-23

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core | 57 passed |
| goble-cli (lib) | 2 passed |
| goble-cli (integration) | 7 passed |
| goblin-worker (lib) | 7 passed |
| goblin-worker (bin) | 5 passed |
| goble-ui | 4 passed |
| goble-desktop | 0 tests |
| **Total** | **82 passed, 0 failed** |

## Recent additions

- mTLS WebSocket handshake:
  - `goble-core::tls`: `mtls_server_config`, `mtls_client_config`, `PairingBundle::server_config/client_config`.
  - `goblin-worker`: serves `wss://` with `--tls-bundle` and requires client cert signed by pairing CA.
  - `goble-cli`: connects via `wss://` with client cert when bundle is supplied; provisioning generates and deploys certs.
- Provisioning now writes `pairing-bundle.json` to worker host and sets `GOBLIN_TLS_BUNDLE`.
- rustls `ring` crypto provider installed by default in binaries and tests.

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace
```

All commands succeeded.
