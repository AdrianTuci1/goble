# Goble Test Report

Generated: 2026-07-23

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core | 62 passed |
| goble-cli (lib) | 2 passed |
| goble-cli (integration) | 8 passed |
| goblin-worker (lib) | 7 passed |
| goblin-worker (bin) | 7 passed |
| goble-ui | 4 passed |
| goble-desktop | 0 tests |
| **Total** | **90 passed, 0 failed** |

## Recent additions

- Encrypted credential vault:
  - `goble-core::vault::CredentialVault` with AES-GCM + PBKDF2 passphrase encryption.
  - `goblin-worker::file_vault::FileVault` persists vault to disk and reloads it.
  - `goble-cli secret set|get` subcommands send vault operations to worker over WSS.
  - Protocol extended with `SetVaultSecret`, `GetVaultSecret`, `VaultSecret`, `VaultError`.
- mTLS WebSocket handshake (previous commit).

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace
```

All commands succeeded.
