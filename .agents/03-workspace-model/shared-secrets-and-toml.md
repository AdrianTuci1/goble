# 03 — Shared secrets & the agent-editable TOML

**Status:** `[~]` partial: `GobleConfig` TOML exists; secret-by-reference flows exist; agent-edit path missing
**Owns:** how secrets/API keys and the workspace config are stored and referenced
**Depends on:** [`README.md`](README.md)

## Problem

Multiple agents in a workspace share the same provider keys, but keys must never be stored in plain text or in the TOML the agent can edit. The config must be something the agent itself can read and edit without that becoming a leak.

## Separation: secrets vs config

- **Secrets (the keys themselves)** live in the **vault** (`goble-core::vault::CredentialVault`), encrypted, referenced by an `api_key_secret_id`. Agents and the TOML reference the **id**, never the value. The remote host also needs the value, so the vault blob ships encrypted alongside the workspace package; it is decrypted in-memory by the harness.
- **Config (the agent-visible part)** is the workspace **TOML**. It holds providers, models, tool/plugin selection, rules. It contains *ids* that point into the vault.

## The TOML model

The seed already exists in `goble-core::config`:

```rust
struct GobleConfig {
    version: u32,
    llm: LlmConfig { default_provider, providers: Vec<ProviderConfig> },
    theme: ThemeConfig { dark, accent },
}

struct ProviderConfig {
    name, api_key_secret_id, base_url?, model,
}
```

`GobleConfig::to_toml()` / `from_toml()` give round-trip. This is the starting point; it grows to cover plugins, workflows, personas, memory, etc.

## Rules

1. **TOML never contains a key value** — only `api_key_secret_id`.
2. The **agent can read + edit the TOML** (it owns its config); the vault is what protects the actual keys.
3. Adding a provider key = writing a secret to the vault + adding a provider block that references it.
4. The same TOML + vault ship to a remote host and are read **there** (see [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md)).
5. Outbound rendering/logging scrubs secret values (reuse `xai-grok-secrets` sanitizer — see [`../04-agent-runtime/harness-reuse-map.md`](../04-agent-runtime/harness-reuse-map.md)).

## Workspace home & config location

The workspace home is `~/.goble` (mirrors `~/.grok`). `GobleConfig` is read from /
written to `~/.goble/config.toml` on startup and on provider/theme changes; the
SQLite state (`goble_store.sqlite`), threads, sessions, worktrees and per-principal
context (`principals/<id>/`) all live under it. `DesktopState::open_default()` resolves
paths from the home instead of the CWD and migrates legacy `./goble_store.sqlite` +
`data_dir/com.goble.desktop/threads` once.

The home is split into a **base** (every user: identity/auth/config/sessions/logs
`principals/<id>/`) and a **workspace payload** (only a local workspace: bundled
tooling, worktrees, threads, local store). A remote-only user has just the base; the
workspace payload stays on the remote worker.

## Principals & access grants

Every principal with access to the workspace is recorded in the store `principals`
table and each holds a set of `access_grants` (`grant` over `scope`, e.g. `run` over
`workspace`, `read` over `mcp:search`). The local user is
`PrincipalId::default_user()`. The harness exposes a `principals` tool that lists
principals and their grants (names/grants only; never secret values).

## Ask-user credential populate (current, pragmatic)

When the ask-user card captures a credential, the card now has a **name** field + a masked **value** field. On submit only the *name* is referenced in the answer; the value is stored by name in the store `credentials` table (`set_credential`/`get_credential`/`list_credential_names`) and never enters the transcript. The harness exposes two tools:

- `credentials` — lists stored credential **names** (never values).
- `run_command` — expands `{{credential:<name>}}` server-side at execution time, so the model writes the placeholder and the harness substitutes the value into the process argv. The model sees the name, never the key.

This satisfies "the model uses tools without seeing the key." Note it currently stores values **in plaintext** (consistent with `llm_settings`/`settings`), i.e. it is a stepping stone toward rule #1 (vault-backed ids) rather than the final architecture. Wiring it to `goble-core::vault::CredentialVault` (with the passphrase lifecycle) is the remaining step.

## Tasks

- [x] Populate a credential from the ask-user card by name; keep the value out of the transcript and expose it to the harness by reference (`credentials` tool + `run_command` expansion).
- [ ] Extend `GobleConfig` to cover plugins, workflows, personas, memory, deep-research.
- [ ] Move credential values from the plaintext `credentials` table into `goble-core::vault::CredentialVault`; keep the same name-reference surface.
- [ ] Add a "config is agent-editable" surface (the agent can read/patch the TOML, never the vault).
- [ ] Add outbound secret-scrubbing on logs/traces/events.
- [ ] Ensure the remote bootstrap ships the TOML + encrypted vault blob together.
