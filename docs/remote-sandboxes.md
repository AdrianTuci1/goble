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
| `docker` | none | Docker CLI | Local development; `--name`, labels, env, `rm -f` |
| `ssh` | none; `api_url` is the destination | `ssh` / `scp` | Installs or starts `goblin` on an existing host |

All adapters implement `SandboxProvider::{create,destroy,status}` and return a
`SandboxHandle` plus a `ProjectSandbox` binding. Tests use local axum mocks or
an injected `CommandRunner`, so no test touches the network.

`api_url` overrides the provider endpoint. `image` names the worker image
(for SSH: an install directory, a local binary, or an `http(s)` URL). `region`
and `environment` are provider-specific; `environment` is the named Boat
environment.

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
cargo test -p goble-remote
cargo test -p goble-gateway
cargo test -p goble-web
cargo test -p goblin-worker
cargo check --workspace --all-targets
```

The web client's browser flow is exercised through unit tests for protocol,
input encoding, the VT screen, layout, and the session state machine. A
headless browser is not part of the test suite.
