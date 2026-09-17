# 02 — Remote bootstrap

**Status:** `[ ]` not started
**Owns:** standing up a remote workspace and switching routing to it
**Depends on:** [`router-local-vs-remote.md`](router-local-vs-remote.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md), [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md)

## Problem

The user chooses **remote**. The app must turn a remote host into a workspace: connect, transfer the package, configure it from the workspace TOML, verify it, then point the conversation at it.

## Sequence (target)

```mermaid
sequenceDiagram
  participant U as User
  participant A as Goble app
  participant R as Remote host (worker)

  U->>A: choose "remote" workspace
  A->>A: show custom composer; user enters secrets
  U->>A: submit secrets
  A->>R: connect + authenticate (mTLS / SSH) 
  A->>R: transfer/install the workspace package
  A->>R: ship workspace TOML + vault secrets
  R->>R: self-configure (read TOML, resolve providers/models)
  R-->>A: ack + publish workspace endpoint
  A->>A: mark conversation "routed remote"
  A-->>U: conversation continues via remote workspace
```

## Rule: what the install may put on the host

The install ships **the worker binary and nothing else that is ours**, because the binary already carries the harness, the workflow engine and the embedded daemon (`goblin` links `goble-harness-*`, `goble-daemon`, `goble-workflow`, `goble-sandbox`, `goble-replay`).

- **No language runtime.** No Python, no Node, no Ruby, no `venv`. Which language a workspace needs is a property of *that project*, and an agent can install it in the session, through a `run_command` the approval gate shows. A Rails project and a Python project must both work on the same host without the installer having picked one. (R74 removed the Python/CrewAI/Docker steps that presumed otherwise.)
- **No agent runtime of anyone else's.** No CrewAI, no container runtime, no orchestration framework.
- **No GUI of ours.** The host never runs our renderer or a `goble` window; a graphical session on the host exists only as the desktop computer use streams from, and that is `xrdp`, configured by the install, not a build of ours.
- **The heavy, optional piece installs in the background, after the service is up.** The desktop is the only such piece today: the install writes it, launches it with `nohup … &` once `systemctl restart goblin.service` has run, and logs it to `/var/log/goblin-remote-desktop.log`. A provisioning run returns as soon as the worker is serving, and a workspace that never uses computer use pays nothing.

Secrets are **not** part of the install at all: they are pushed over the paired connection ([`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).

## Rule: who reads the TOML

The **remote harness** reads the workspace TOML on the remote host and configures itself there. The TOML is the single source of truth for providers, models, tool/plugin selection, and API-key *references* (never the keys themselves). This lets the TOML be agent-editable without leaking secrets.

## Outputs

- A **remote workspace record** (address, worker id, workspace id, TLS bundle) so the router can reference it.
- The conversation flips from `local` to `Remote { worker_id, address }`.

## Reuse

This rides on the existing worker pairing/install path in `goble-desktop-service` (`pair_worker`, `WorkerClient::connect`, `cluster_helm_install`, `install_worker_ssh`) plus the mTLS bundle signing in `goble-core`. The remote *package* is new: what gets shipped and how it self-configures.

## Tasks

- [ ] Define the "workspace package" to ship to a remote host (harness + TOML + secrets).
- [ ] Add the remote self-configuration step (read TOML → resolve providers/models).
- [ ] Reuse the existing pairing/SSH/helm install path to bring up the remote workspace.
- [ ] Publish a workspace endpoint the router can point a conversation at.
- [ ] Decide what happens to `$INSTALL_PATH/tls/ca-key.pem`: the script writes `WorkerBundle.ca_cert_pem` — a **public** certificate — under a name and a comment that claim a CA private key kept there for disaster recovery, and there is no CA private key in the bundle at all. Either drop the file or ship a real recovery path.
- [x] **Make the install's data root explicit and guard the recursive `chown`.** The generated script derived its data root as `DATA_ROOT="$(dirname "$WORKSPACE_ROOT")"` and then ran `chown -R goblin:goblin "$INSTALL_PATH" "$DATA_ROOT"`. `workspace_root` is a public config field, so a caller passing a shallow one (e.g. `/srv`) made that `chown -R` target a whole system directory, and the derivation was also not what the worker actually uses: `goblin`'s store and vault default to the fixed absolutes `/var/goblin/tasks.db` and `/var/goblin/vault.json` (`crates/goblin-worker/src/main.rs`), and the unit passed neither `--task-store` nor `--vault-path`, so with any non-default `workspace_root` the service flapped against a data root it was not writing to (reproduced by hand).
  - **Resolved:** `ProvisionConfig` now carries `data_root` as an installed value — `DEFAULT_DATA_ROOT = "/var/goblin"`, which is what the in-tree callers (`ProvisionConfig::from_cluster_identity` and `goble-cli`'s `worker-provision`) got from the derivation before, so their install is unchanged. The generated unit starts the worker with `--task-store $DATA_ROOT/tasks.db --vault-path $DATA_ROOT/vault.json` (the env file names both too), so the directory the worker writes to is the one the install owns. Every `chown -R` now goes through the script's own `chown_guarded`, which refuses `/`, an empty path, a bare top level such as `/srv`, and a relative path — loudly (`exit 1`), never by skipping, because a skipped chown is exactly the unwritable store that flaps the service. `provision_worker` also refuses such a `data_root` up front through `validate_data_root`, before it copies anything to the host.
  - **Verified:** `cargo test -p goble-core --test install_systemd_e2e -- --ignored --nocapture` → **exit 0, 1 passed**, which installs twice in the wave-0 systemd container — the default `workspace_root` and `/srv/goblin/workspaces` — and each time sees `systemctl is-active goblin.service` = `active` and `{"status":"Online",…}` from `/health` on the published 8787. `cargo test -p goble-core --lib provision` → **10 passed / 0 failed** (`test_the_install_gives_the_worker_the_data_root_it_is_told_to_use`, `test_the_install_refuses_a_shallow_data_root` — the generated script's own guard, executed — and `test_provisioning_refuses_a_shallow_data_root_before_copying`).
- [x] **Close the `chown_guarded` bypass.** The guard above refuses `/srv` but not a path that merely *looks* different: `/srv//`, `//srv` and any path carrying a `..` component slip past it and still produce `chown -R goblin:goblin /srv`. The comparison is over unnormalised strings, so the shell guard is weaker than it reads, and `workspace_root` is the one field with no Rust-side validation. Normalise before comparing and reject a path that resolves to `/`, to an empty string or to fewer than two components, keeping the shell guard as the second line rather than the only one. Prove it with a unit test over the generated script for `/srv`, `/srv//`, `//srv`, `/srv/../srv`, `/` and an empty string, and re-run the wave-0 systemd container command to show the normal path still installs and starts.
  - **Resolved:** both lines now compare *normalised* paths. Rust-side, `provision.rs` has one textual `normalise_path` (repeated separators collapse, `.` drops out, `..` cancels the component before it and is clamped at the root — nothing touches the filesystem, the path is on the host and need not exist here) and one `validate_install_path(field, path)` that refuses anything not absolute or resolving to fewer than two components; `validate_data_root` is now that function under its old name, and `provision_worker` runs it for `install_path`, `workspace_root` and `data_root` before it copies a single byte to the host — `workspace_root` was the one `ProvisionConfig` field with no Rust-side validation, and the three in-tree callers pass `/opt/goblin`, `/var/goblin/workspaces` and `/var/goblin`, so none of them changes. In the script, `chown_guarded` normalises `$1` with pure string operations, decides on the normalised path and hands *that* path to `chown -R`, so the decision and the chown cannot disagree; it still fails loudly (`exit 1`, message on stderr) rather than skipping, because a skipped chown is the unwritable store that flaps the service. The guard's function name and text are unchanged, so `tests/install_systemd_e2e.rs` still lifts it out of the shipped script with the same `sed` range.
  - **Verified:** `cargo test -p goble-core --lib provision -- --nocapture` → **13 passed / 0 failed**, including `test_the_chown_guard_refuses_a_path_that_only_looks_different`, which runs the guard out of the *generated* script for each of `/srv`, `/srv//`, `//srv`, `/srv/../srv`, `/` and `""` — once with the value as `workspace_root` and once as `data_root` — and asserts `exit 1`, a `refusing to chown` message naming the value, and no `chown -R` call; plus `test_the_guard_chowns_a_path_the_install_owns` (normalising must not cost the install a directory of its own, and the chown targets the normalised path) and `test_provisioning_refuses_a_shallow_workspace_root_before_copying`. `cargo test -p goble-core --test install_systemd_e2e -- --ignored --nocapture` → **exit 0, 1 passed** in 21.13 s (the three runs of this command after the change were all green): the wave-0 systemd container still installs twice (default `workspace_root` and `/srv/goblin/workspaces`), `systemctl is-active goblin.service` = `active` both times, `/health` = `{"worker_id":"t3-worker","status":"Online","paired":true,…}` both times, and on the host the shipped guard refuses `/srv` with `refusing to chown "/srv": it needs a directory level of its own` while `/srv` stays owned by `root`.
