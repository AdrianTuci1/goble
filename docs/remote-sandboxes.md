# Remote sandboxes

Goble can run a worker in a cloud sandbox and drive a terminal in it from a
browser. The pieces are deliberately separate:

| Layer | Crate / binary | Job |
|---|---|---|
| Client | `apps/web` (Expo web) | Shows terminals, sends keystrokes, renders PTY output |
| Gateway | `goble-gateway` | Always-on WebSocket relay with PIN login, sessions, and a worker list |
| Sandbox | `goblin-worker` | Runs in the provider sandbox, opens a PTY per tab, connects out to the gateway |
| Provisioner | `goble-remote` + `goble-desktop-service` | Creates and destroys provider sandboxes, passes worker env |

A worker connects **outbound** to the gateway, so the sandbox needs no inbound
port, public IP, VNC, or desktop session.

## One sandbox per project

`goble_core::workspace_identity::WorkspaceIdentity` derives a stable identity
from the project id and directory:

```
project_id    8f3c...
slug          goble
host          adrians-macbook-pro
sandbox_name  goble-goble-2fbfef8fa780
sandbox_path  /workspace/goble
prompt_name   goble@adrians-macbook-pro
```

`goble-persistence` stores the binding in `project_sandboxes`, keyed by
`project_id`. Every remote tab or window for that project reuses the same
sandbox and the same `/workspace/<slug>` filesystem; the host is metadata, so
opening the project from a different machine does not create a second sandbox.

## Provider adapters

`goble_remote::provider_for` selects an adapter from a
`[remote_agents.<name>]` table.

| Provider | Credentials | Transport | Notes |
|---|---|---|---|
| `e2b` | `api_key` (`E2B_API_KEY`) | REST | `POST /sandboxes`, template + env, auto-pause/resume |
| `modal` | `token_id`, `token_secret` | Python SDK bridge | `scripts/modal_sandbox.py`; set `GOBLE_MODAL_PYTHON` to choose the interpreter |
| `daytona` | `api_key` (`DAYTONA_API_KEY`) | REST | Snapshot or Dockerfile image; `region` becomes `target` |
| `box` | `api_key` (`BOX_API_KEY`) | REST | Box is now Boat; uses `https://boat.dev/api/v1/sandboxes` |
| `cloud_run` | `environment` = GCP project, `region` = location | REST (Admin API v2) | One job per workspace name, one execution per create; non-interactive |
| `docker` | none | Docker CLI | Local development; `--name`, labels, env, `rm -f` |
| `ssh` | none; `api_url` is the destination | `ssh` / `scp` | Installs or starts `goblin` on an existing host |

All adapters implement `SandboxProvider::{create,destroy,status}` and return a
`SandboxHandle` plus a `ProjectSandbox` binding. Tests use local axum mocks or
an injected `CommandRunner`, so no test touches the network.

`api_url` overrides the provider endpoint. `image` names the worker image
(see [Worker image](#worker-image); for SSH: an install directory, a local
binary, or an `http(s)` URL). `region` and `environment` are provider-specific:
on `box`, `environment` is the named Boat environment; on `cloud_run`,
`environment` is the GCP project id.

## Durable sessions: snapshot and resume

A remote session is a binding, not a process. The `project_sandboxes` row keeps
the durable state, and every deploy either reuses the live sandbox, restores a
dead one, or creates a fresh one — it never silently starts clean when the
binding held state.

| Column | Meaning |
|---|---|
| `provider` | Adapter kind that owns the sandbox and its snapshots |
| `sandbox_name`, `workspace_path` | Stable identity from `WorkspaceIdentity` |
| `sandbox_id` | Provider handle of the sandbox that exists now (`NULL` once destroyed) |
| `snapshot_id` | Last provider-side filesystem snapshot; restored by the next deploy |
| `native_session_id` | Harness conversation to resume; passed to the worker as `GOBLE_NATIVE_SESSION_ID` |
| `updated_at` | Last activity; the idle reaper measures from it |

`goble_remote::SandboxBinding` and `goble_remote::SnapshotRef` are the
provider-neutral form of that row. `goble_remote::SandboxProvider` gained
optional methods with defaults, so an adapter without snapshots keeps its old
behaviour exactly: `snapshot_support()`, `snapshot()`,
`create_from_snapshot()`, `resume()` and `release()` (snapshot first, then
destroy, both attempted; a provider that keeps the sandbox itself returns
`ReleaseOutcome::kept()` instead).

### Per-provider support matrix

| Provider | Level | Snapshot | Restore | Notes |
|---|---|---|---|---|
| `modal` | `filesystem` | real: `Sandbox.snapshot_filesystem()` | real: `modal.Image.from_id()` | Release runs *snapshot then terminate* in one bridge process (`snapshot_and_terminate`); `create` carries `snapshot_id` and ignores the request image |
| `e2b` | `pause_resume` | no snapshot object | real: `POST /sandboxes/{id}/resume` | Release pauses instead of deleting; a paused sandbox keeps filesystem and memory, and the binding keeps `sandbox_id` |
| `docker` | `workspace` | unsupported | best effort: named volume `goble-ws-<sandbox>` mounted at the workspace path | `docker rm -f` keeps the named volume; the next container mounts the same files. Process state is not kept. |
| `ssh` | `workspace` | unsupported | best effort: install directory | Release stops the worker and keeps the directory; `destroy` is still the full teardown that removes it |
| `cloud_run` | `none` | unsupported | conversation only | Cloud Run has no durable disk. The documented state-tarball equivalent needs a GCS bucket named in the agent table and the config has no field for one, so the adapter reports `none` rather than faking it. Code is re-cloned from `GOBLE_REPO_URL`; the harness conversation resumes from the native session id. |
| `daytona`, `box` | `none` | unsupported | conversation only | Their native snapshot/pause APIs are not wired here; a dead sandbox starts from the image and the deploy report says `created` |

`created_without_snapshot` in the deploy report means exactly that promise is
kept: the binding held a snapshot, the provider cannot restore it, and a fresh
sandbox ran instead.

### Lifecycle and configuration

| Variable | Default | Meaning |
|---|---|---|
| `GOBLE_SANDBOX_IDLE_SECS` | `1800` | Idle window before the reaper snapshots and stops a project sandbox; `0` disables the reaper |
| `GOBLE_SANDBOX_SNAPSHOT_ON_CLOSE` | on | `0`, `false` or `no` makes a close a plain destroy, so state is not kept |

`DesktopState::deploy_remote_agent` resumes or creates;
`release_remote_agent` stops one project's sandbox and leaves the binding
resumable; `reap_idle_remote_agents` releases every binding idle past the
window; `record_remote_native_session` stores the conversation a worker
reports. Deploy and release report on `remote:deploy` / `remote:release`; the
deploy report carries `resumed` (`reused` / `restored` / `created` /
`created_without_snapshot`), `snapshot_id`, `native_session_id` and
`snapshot_support`.

The worker receives `GOBLE_NATIVE_SESSION_ID` whenever the binding knows a
conversation, so a DSH harness maps it to `--session-id` and continues the
conversation even though the sandbox that ran the previous turn is gone.

### Migration

`project_sandboxes` is migrated in place when the store opens: the nullable
`snapshot_id` and `native_session_id` columns are added when missing. Existing
rows keep their binding and read back with `NULL` for both new fields; no
re-save step is needed. Re-opening an already-migrated database is a no-op.

### Verification

```bash
cargo test -p goble-remote        # adapters, trait defaults, Modal bridge (fake SDK)
cargo test -p goble-persistence   # binding round-trip and the in-place migration
cargo test -p goble-desktop-service state::remote
```

## Worker image

A remote agent runs `goblin` inside a container the provider pulls, so the
image its table names has to carry the worker. This repository publishes that
image; there is nothing to build before a first deploy.

| Registry reference | Tags | Built from |
|---|---|---|
| `ghcr.io/adriantuci1/goble-worker` | `latest` and `main` (the `main` branch), `0.1.0` and `0.1` (a `v0.1.0` release tag), `sha-<short>` | `deploy/worker/Dockerfile`, by `.github/workflows/worker-image.yml` |

It is `linux/amd64` only. The runtime is `debian:bookworm-slim` with `git`,
`ripgrep`, `curl`, `ca-certificates` and a shell, because the agent inside the
sandbox has to clone, search and run commands — which the distroless
`deploy/goblin/Dockerfile` (the Kubernetes image, `--mode=cluster`) cannot do.
The worker runs as the non-root user `goble` (uid 10001) with
`HOME=/home/goble`, and the project's files live under `/workspace`. Its task
store, vault, pid file and log file are moved under that home, so the binary's
`/var/goblin` and `/var/run` defaults — unwritable for a non-root user — never
apply.

The entrypoint is `goblin --mode=local --bind=0.0.0.0:8787`. `local` is the
mode a sandbox wants: `cluster` moves the databases under a PVC, elects a
leader through a Kubernetes `Lease` and snapshots to a mounted volume
(`crates/goblin-worker/src/main.rs`). Connecting to a gateway is not a mode —
the provisioner passes `GOBLE_GATEWAY_URL` and `GOBLE_WORKER_TOKEN` as sandbox
env and the worker dials out. `GET /health` on port 8787 is the readiness
report the image's `HEALTHCHECK` reads.

### DSH runtime variant

The same Dockerfile also builds an image whose worker drives the DeepSeek
Harness headless profile installed in the sandbox instead of the native
harness: `docker build -f deploy/worker/Dockerfile --build-arg RUNTIME=dsh`
(see [remote-dsh-runtime.md](remote-dsh-runtime.md)). It is the plain worker
image plus the harness and `GOBLE_HARNESS=dsh`; the gateway connection, the
desktop protocol, `/health` and the entrypoint are unchanged, so a
`[remote_agents.<name>]` table points at it the same way. Credentials are copied
into the sandbox per run and deleted afterwards, never baked into the image.

What a table puts in `image`:

```toml
[remote_agents.builder]
provider = "modal"
token_id = "..."
token_secret = "..."
image = "ghcr.io/adriantuci1/goble-worker:latest"
```

`goble-cli setup remote` writes exactly that when `--image` is omitted
(`DEFAULT_WORKER_IMAGE` in `goble_core::config`), so a container provider costs
only its credentials. Pass `--image` for a build of your own: a fork's own
package, a registry inside the provider's cloud, or `:main` to track the
branch. SSH is the one exception — its `image` is a remote install directory or
a binary URL, never a container.

The GitHub Container Registry makes a package private when it is first pushed,
and every provider here pulls anonymously, so `goble-worker` has to be set
public in the package's settings once (Actions → the pushed package → Package
settings → Change visibility). A private package only works where the puller
has registry credentials.

## Google Cloud Run

`CloudRunProvider` (`crates/goble-remote/src/cloud_run.rs`) drives the Cloud
Run Admin API v2 at `https://run.googleapis.com`. Cloud Run is request-scoped:
a job runs one container task and exits, with no attachable PTY and no
pause/resume. That matches a one-shot worker run, which is what the adapter
provisions. An interactive workspace on GCP is a GCE VM or a GKE pod running
`goblin-worker` over the `ssh` adapter or the relay.

```toml
[remote_agents.ci]
provider = "cloud_run"
environment = "my-gcp-project"   # the GCP project id
region = "europe-west1"          # the Cloud Run location
image = "europe-west1-docker.pkg.dev/my-gcp-project/goble/worker:latest"
# api_key = "ya29.…"             # optional bearer token
```

- **One job per workspace.** `job_id` derives a deterministic job id from the
  workspace's sandbox name, truncated to Cloud Run's 49 characters while
  keeping the `goble-` prefix and the name's key. `create` reads the job,
  creates it on a 404 and replaces it on a hit, then runs it.
- **Sandbox id is the execution name.** `create` returns the Cloud Run
  execution name (`projects/…/jobs/<job>/executions/<exec>`), so the handle
  names that one execution and `status` reads it. `destroy` deletes the job
  that owns it (`DELETE {job_url}?force=true` on
  `job_id(&handle.sandbox_name)`); a 404 counts as already gone.
- **Token.** `api_key` is used as the bearer token when it is set. Otherwise
  the adapter mints one with `gcloud auth print-access-token` from the
  machine's existing session and reuses it for fifty minutes. Being logged in
  with `gcloud auth login` is enough; the key never has to be pasted.
- **Limits.** A job task timeout caps at 86 400 seconds (one day), retries are
  pinned to 0, and the worker image has to sit in a registry Cloud Run can
  pull from: Artifact Registry or a public one, which includes the published
  `ghcr.io/adriantuci1/goble-worker`.

## Agent-led setup

A fresh machine has no `[remote]` section and no hub settings, so the app and
`goble-cli` share one path for collecting them: the agent asks the questions,
and every write goes through `goble-cli setup`.

| Command | Writes |
|---|---|
| `goble-cli setup status [--json]` | Read-only report: config path, hub path, Remote mode, the gateway URL and whether a worker token is set, each `[remote_agents.*]` table with its status, the hub's settings, and the credential environment keys that are set |
| `goble-cli setup remote --name … --provider …` | One `[remote_agents.<name>]` table (`--api-key`, `--token-id`, `--token-secret`, `--api-url`, `--image`, `--region`, `--environment`, `--disabled`) |
| `goble-cli setup gateway --url … --token …` | `[remote]` pointed at a gateway (`mode = "gateway"`); `--platform` returns it to `mode = "platform"`; a value the file already holds need not be repeated, and the command fails only when neither the flag nor the file supplies a URL and a token |
| `goble-cli setup hub --database … --relay-url … --token … --workflows …` | `~/.goble/hub.toml` |
| `goble-cli setup brief --target remote\|hub` | Prints the brief a setup conversation starts from |

Every write keeps the file's `CONFIG_HEADER` and the sections it does not
touch, so a table added by the agent sits next to the user's models and theme.
The status and the brief are built together in
`goble_core::config::setup::SetupStatus`, so `goble-cli setup status` and the
app's Settings page agree on what is missing.

In the app, Settings → Agent & Approval has **Set up Remote with the agent**
and **Set up the hub with the agent**. Either one opens a new conversation
titled `Remote setup` or `Hub setup` and sends the brief as its first turn.
The Remote brief tells the agent to ask one question at a time, never to
invent a credential, project id, image reference or URL, to write only through
`goble-cli setup …`, and to read `goble-cli setup status` back to the user
after each write. The hub brief asks one question at a time, tells the agent to
write only what the user gave it, through `goble-cli setup hub …`, and then to
run `goble-cli setup status` and say what is ready.
The agent finds `goble-cli` next to the running binary
(`goble_core::binaries::goble_cli`), so it works from a `cargo run` checkout
as well as from an installed app.

```toml
# ~/.goble/config.toml
[remote_agents.builder]
provider = "modal"
token_id = "..."
token_secret = "..."
image = "ghcr.io/adriantuci1/goble-worker:latest"

[remote_agents.reviewer]
provider = "daytona"
api_key = "..."
image = "ghcr.io/adriantuci1/goble-worker:main"
region = "us"
```

Settings → Agent & Approval lists each table, its status, and the environment
variables whose credentials are missing.

## Deploy composition

`goble_desktop_service::state::remote::deploy` builds the sandbox request from
the project identity and starts the worker with:

| Variable | Value |
|---|---|
| `GOBLE_GATEWAY_URL` | Worker WebSocket URL, e.g. `wss://gateway.example/ws/worker` |
| `GOBLE_WORKER_TOKEN` | Shared bearer token accepted by the gateway |
| `GOBLE_WORKER_ID` | Stable worker id, usually the sandbox name |
| `GOBLE_PROJECT_ID` | Project id |
| `GOBLE_WORKSPACE_PATH` | `/workspace/<slug>` |
| `GOBLE_REPO_URL`, `GOBLE_REPO_BRANCH` | Optional clone instructions |

When `GOBLE_GATEWAY_URL` is set, `goblin-worker` keeps a connection to the
gateway with reconnect backoff and serves PTYs over it. The same worker still
serves its local `/ws`, `/health`, and `/status` endpoints.

## Gateway

```bash
cargo run -p goble-gateway -- \
  --bind 127.0.0.1:8788 \
  --pin-file ~/.goble/gateway-pin \
  --worker-token-file ~/.goble/gateway-worker-token \
  --public-url http://127.0.0.1:8788 \
  --data-dir ~/.goble/gateway
```

At startup the gateway prints a PIN when none is supplied and prints a generated
worker token once. Browser sessions live in memory and expire after 12 hours.
The control plane's session bindings are the one thing kept on disk: they are
written to `sessions.json` under `--data-dir` (default `~/.goble/gateway`), so a
restart keeps the same worker + conversation binding per name.

Routes:

| Route | Purpose |
|---|---|
| `GET /` | PIN login (redirects to `/app/` when logged in) |
| `POST /login`, `POST /logout` | Session cookie lifecycle |
| `GET /app/` | Expo web app (session required) |
| `GET /api/workers`, `GET /api/v1/workers` | Connected workers as JSON |
| `POST /api/v1/session` | PIN → bearer token for native clients |
| `GET /panel` | Transport console for raw frame debugging |
| `GET /ws/worker?id=<id>` | Worker endpoint, bearer token |
| `GET /ws/client?worker=<id>` | Browser endpoint, session cookie |
| `POST /api/v1/sessions`, `GET /api/v1/sessions[/{id}]` | Session bindings (worker + conversation), created/reused by name |
| `POST /api/v1/sessions/{id}/runs` | Dispatch a run, one per session, returns a run id |
| `GET /api/v1/runs/{id}`, `.../events`, `.../stream` | Run status, replayable event log, SSE replay + live |
| `POST /api/v1/sessions/{id}/cancel` | Settle a run and ask the worker to stop it |

The `/api/v1/sessions` and `/api/v1/runs` routes are the remote-execution
control plane: everything a phone, a web client, or an ACP bridge needs to drive
an agent in a sandbox over plain HTTP and SSE instead of raw protocol frames. It
uses the same bearer/cookie session as `/api/workers` and is documented in
[gateway-api.md](gateway-api.md).


The gateway accepts loopback Host headers plus `--public-url` and repeated
`--allow-host` values. It speaks plain HTTP/WebSocket; terminate TLS with
Tailscale (`tailscale serve`) or a reverse proxy. `--no-auth` exists for local
development and is refused on a non-loopback bind.

## Browser client

The browser client is the Expo web app in `apps/web`, which shares the phone's
components through `@goble/common`. From the repository root:

```bash
npm ci
npm run web:build
```

The export lands in `apps/web/dist`. Copy that directory into the gateway's
`web/` directory, or point the gateway at it with `--web-dir`.

The gateway serves the bundle from `--web-dir`, then `<exe>/web`, then the
crate's `web/` directory. If the bundle is absent, `/app/` returns a build page.

## Cloud agents in the desktop app

A conversation routed to Remote is a viewer pane: the transcript and the
composer, with the worker connection's state over them (`apps/desktop/src/ui/worker.rs`).
The ways in:

- the "▾" environment menu's **Cloud agent** row (after "New terminal" and "New
  Agent") opens a fresh cloud agent;
- the environment menu's **Remote (worker)** row opens the same surface instead
  of a local shell, since a run there has no shell on this machine;
- a conversation routed to Remote in its composer becomes one as it runs.

A cloud agent with no messages is a landing rather than an empty transcript: the
cloud glyph and "New cloud agent" over the pane, the tab reading
"New cloud agent", and the composer inviting "Kick off a cloud agent"
(`ChatView::with_empty_state_icon` and `with_composer_placeholder`). The first
prompt replaces the landing with the conversation. Until Remote is configured
the surface opens Settings → Agent & Approval instead of a tab.

Sidebar cards say where a conversation runs: a remote one marks its directory
row with the cloud after the path (`ConversationListItem::with_workspace_routing`),
so a starred card in another section reads the same as the folders filtered by
the environment selector.

## Security notes

- Browser access requires the PIN session; the worker endpoint requires a
  separate bearer token. The two are not interchangeable.
- Sessions are opaque, HttpOnly, SameSite=Strict cookies. Login failures add a
  small delay after three attempts.
- Host headers are allowlisted to block DNS rebinding.
- When a browser socket closes, the gateway sends `close_pty` for every pane
  that client opened, so a closed tab does not leave shells running.
- PTY output travels on the worker's event channel, so multiple browser clients
  attached to one worker can see each other's terminal output. Pane input,
  resize, and close are ownership-checked by the worker. Use one client per
  worker when that visibility matters.
- Provider API keys live in `config.toml`; the worker token is only passed in
  the sandbox environment. Treat both as secrets.

## Verification

```bash
cargo test -p goble-remote        # every provider, Cloud Run included
cargo test -p goble-core config   # config schema, hub.toml, setup status and briefs
cargo test -p goble-cli setup     # the write surface
cargo test -p goble-app --test setup_flow
cargo test -p goble-gateway      # relay + remote-execution control plane (see gateway-api.md)
npm run web:build
cargo test -p goblin-worker
cargo check --workspace --all-targets
```

The Expo web app is built by `npm run web:build` and type-checked by
`npm run typecheck`; a headless browser is not part of the test suite.
