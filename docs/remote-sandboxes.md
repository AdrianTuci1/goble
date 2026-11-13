# Remote sandboxes

Goble can run a worker in a cloud sandbox and drive a terminal in it from a
browser. The pieces are deliberately separate:

| Layer | Crate / binary | Job |
|---|---|---|
| Client | native app, `goble-web` (WASM) | Shows terminals, sends keystrokes, renders PTY output |
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
(for SSH: an install directory, a local binary, or an `http(s)` URL). `region`
and `environment` are provider-specific: on `box`, `environment` is the named
Boat environment; on `cloud_run`, `environment` is the GCP project id.

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
  pull from (Artifact Registry or GCR).

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
image = "ubuntu:24.04"

[remote_agents.reviewer]
provider = "daytona"
api_key = "..."
image = "ghcr.io/example/goble-worker:latest"
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
  --public-url http://127.0.0.1:8788
```

At startup the gateway prints a PIN when none is supplied and prints a generated
worker token once. Sessions live in memory and expire after 12 hours.

Routes:

| Route | Purpose |
|---|---|
| `GET /` | PIN login (redirects to `/app/` when logged in) |
| `POST /login`, `POST /logout` | Session cookie lifecycle |
| `GET /app/` | WASM terminal client (session required) |
| `GET /api/workers` | Connected workers as JSON |
| `GET /panel` | Transport console for raw frame debugging |
| `GET /ws/worker?id=<id>` | Worker endpoint, bearer token |
| `GET /ws/client?worker=<id>` | Browser endpoint, session cookie |

The gateway accepts loopback Host headers plus `--public-url` and repeated
`--allow-host` values. It speaks plain HTTP/WebSocket; terminate TLS with
Tailscale (`tailscale serve`) or a reverse proxy. `--no-auth` exists for local
development and is refused on a non-loopback bind.

## Browser client

`crates/goble-web` is a Rust WASM client (wasm-bindgen + web-sys) with its own
canvas terminal renderer and a `vt100` screen model. It provides:

- PIN-protected boot through the gateway session cookie.
- Worker list refreshed every 5 seconds, with an empty state and reconnect.
- Multiple tabs, one WebSocket and one PTY per tab.
- Live input, resize (`ResizeObserver`), scrollback, selection and copy/paste.
- Reconnect with backoff; a new PTY is opened and previous scrollback is kept.
- Keyboard shortcuts for tabs, reconnect, scrollback, and clipboard.

Build the bundle into the gateway's asset directory:

```bash
scripts/build-web.sh
```

The script builds `goble-web` for `wasm32-unknown-unknown` and runs
`wasm-bindgen --target web`. It requires a `wasm-bindgen` CLI whose version
matches the crate (0.2.126):

```bash
cargo install wasm-bindgen-cli --version 0.2.126 --locked
```

The gateway serves the bundle from `--web-dir`, then `<exe>/web`, then the
crate's `web/` directory. If the bundle is absent, `/app/` returns a build page.

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
cargo test -p goble-gateway
cargo test -p goble-web
cargo test -p goblin-worker
cargo check --workspace --all-targets
```

The web client's browser flow is exercised through unit tests for protocol,
input encoding, the VT screen, layout, and the session state machine. A
headless browser is not part of the test suite.
