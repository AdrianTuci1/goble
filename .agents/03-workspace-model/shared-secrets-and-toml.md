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

The second reader of the same table is the desktop handoff: `open_screen` names a credential and the host resolves it where the RDP connection is built, the stored value being the account line `username:password` (see [`../04-agent-runtime/computer-use.md`](../04-agent-runtime/computer-use.md)).

This satisfies "the model uses tools without seeing the key." Note it currently stores values **in plaintext** (consistent with `llm_settings`/`settings`), i.e. it is a stepping stone toward rule #1 (vault-backed ids) rather than the final architecture. Wiring it to `goble-core::vault::CredentialVault` (with the passphrase lifecycle) is the remaining step.

## Where a remote worker's keys live

A remote worker is a workspace in its own right (see [`workspace.md`](workspace.md)): it hosts the agents, their crons and their secrets. So **its vault is the remote workspace's vault and it is authoritative there** — the keys stay on the worker, and the client is a view onto them.

That path already exists and works:

- The worker owns a persistent file vault at `/var/goblin/vault.json` (`crates/goblin-worker/src/main.rs:56`), loads it at startup (`crates/goblin-worker/src/main.rs:158` → `state.load_vault(b"")`) and saves on write.
- `DesktopMessage::SetVaultSecret` / `GetVaultSecret` are handled by the worker (`crates/goblin-worker/src/websocket.rs:201`, `:210`) and driven by the CLI: `goble secret set --worker … --url … --name llm_api_key --value …` and `goble secret get` (`crates/goble-cli/src/commands.rs:221`, `:241`). Setting a key on a worker is an explicit act with a command behind it.
- `goblin-worker/src/llm_factory.rs::default_provider_factory(secrets)` resolves `llm_api_key` out of the worker's secrets and builds the provider; without it a turn fails with "no llm_api_key secret available" (it looks the name up as a map key — see the two hops this leaves open, below).

What does **not** exist is any way for the keys the user already has locally to reach a worker without them typing them again: `DesktopMessage::PushSecrets` is handled by the worker (`crates/goblin-worker/src/websocket.rs:138` — each secret into the store and the file vault, then saved) but nothing in the tree produces it, so it is dead protocol.

### The rule: the worker holds the key, the client only fills a gap

The push is a **seed, not the mechanism**:

1. **The worker's vault wins.** A pushed name the worker already holds is not overwritten — the receiving side enforces this (`AppState::holds_secret`, consulted by the `PushSecrets` handler), so the rule holds no matter who pushes. A client connecting with a different local key does not silently change what the remote workspace authenticates with.
2. **Only gaps are filled.** A freshly provisioned worker has an empty vault and would fail its first remote turn; seeding it once, at pairing time, is what makes the first remote turn work without a manual step. Only the one name the worker's factory resolves (`llm_api_key`, read through `DesktopState::effective_llm_setting` — the same resolution a local turn uses) is sent — never the whole vault, and never a name the worker already has. The workspace's MCP servers are *not* part of the seed: they already travel per run on `RunAgent`, and a `[mcp_servers]` entry in `config.toml` has no `McpServer` manifest to convert.
3. **Changing a remote key is an explicit act**, not a side effect of a local edit: `goble secret set` (and, later, the same call from the UI). A local `config.toml` change must not reach into a worker and rewrite what it holds.
4. It happens **on the paired connection**, after the worker answers `Paired` — never as part of the install. The install script must not carry a key value: it is a file on the host's disk and in `/tmp`. R74 removed the last thing that would have encouraged that.
5. The value travels inside the mTLS connection the worker already requires; it is never logged, never rendered, and never written to the worker's transcript.

What landed (K1):

- The producer is `WorkerClient::connect` (`crates/goble-desktop-service/src/worker_manager.rs`): it resolves the seed with `DesktopState::llm_api_key_secret` (`crates/goble-desktop-service/src/state/llm.rs`), and the reader task sends `PushSecrets` **once**, on the worker's `Paired` answer — nothing leaves before it, and the install carries no key.
- The receiver (`crates/goblin-worker/src/websocket.rs`, `PushSecrets`) takes only the names it does not already hold; a name held in the file vault or in `AppState::secrets` is left alone, and a push that fills nothing writes no vault file.
- Proven by `cargo test -p goble-desktop-service -p goblin-worker` (all green): a real `WorkerClient::connect` against a mock WebSocket server (`worker_manager.rs`) sees `PushSecrets { llm_api_key: <local value> }` after `Paired` and nothing before it, and three `goblin-worker` tests drive the handler onto an empty worker, a vault that holds the name, and a worker that already holds it.

### Two hops the seed does not yet reach (found while doing K1, not fixed)

1. **The factory looks the name up as a map key.** `default_provider_factory` does `secrets.get("llm_api_key")`, but `AppState::store_secret` inserts under `secret.id` (a uuid), so a pushed secret is in the map under a key the factory never asks for and a real turn still ends in "no llm_api_key secret available". Every other reader of `state.secrets` uses `.values()` (or, in `crates/goble-cli/tests/e2e_worker.rs`, keys the map by **name**), so the map is meant to be name-keyed. This is its own decision, not K1's.
2. **The worker's vault writes are all empty-passphrase.** `CredentialVault::set` refuses `b""`, and the `PushSecrets` / `SetVaultSecret` handlers pass `b""` and swallow the error with `.ok()`, so nothing is persisted at `/var/goblin/vault.json` through them today; the in-memory map is what a turn reads.

## Tasks

- [x] Populate a credential from the ask-user card by name; keep the value out of the transcript and expose it to the harness by reference (`credentials` tool + `run_command` expansion).
- [ ] Extend `GobleConfig` to cover plugins, workflows, personas, memory, deep-research.
- [ ] Move credential values from the plaintext `credentials` table into `goble-core::vault::CredentialVault`; keep the same name-reference surface.
- [ ] Add a "config is agent-editable" surface (the agent can read/patch the TOML, never the vault).
- [ ] Add outbound secret-scrubbing on logs/traces/events.
- [x] **Seed a paired worker with the keys it is missing**: a producer for `DesktopMessage::PushSecrets`, fired after the worker answers `Paired`, carrying only the name `llm_factory` resolves (`llm_api_key`) and only when the worker does not already hold it. The receiving side ignores a name it already has, because the worker's vault is authoritative for a remote workspace — the receiver at `crates/goblin-worker/src/websocket.rs:138` now filters on `AppState::holds_secret` and writes nothing when the push fills no gap. Verified by `cargo test -p goble-desktop-service -p goblin-worker` → exit 0, 56 + 38 passed and no failures (the seed observed after `Paired` by a mock WebSocket server, plus the three receiver tests).
- [ ] Ensure the remote bootstrap ships the TOML + encrypted vault blob together.
