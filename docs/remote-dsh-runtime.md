# Remote DSH runtime

Goble's remote sandbox normally runs the worker's own embedded harness: the
worker receives a `run_agent_for_thread_reply` frame, answers it from its
in-process daemon, and streams the usual trace frames. This document describes
the **alternative runtime**: a worker image that also carries the
[DeepSeek Harness](https://github.com/AdrianTuci1/dsh-remote) (DSH) and, when
`GOBLE_HARNESS=dsh`, drives the harness installed inside the sandbox instead.

It is Phase 4 of the dsh-remote parity work. The reference implementation is
dsh-remote's `gateway/sandbox.py` + `gateway/harness/cordis.patch.yml`, which
runs:

```
dsh --profile headless --patch <harness.yml> --json \
    --permission-mode danger-full-access --approval-policy never \
    [--session-id <native>] -
```

with the task on stdin and newline-delimited JSON events on stdout. Goble keeps
that contract and maps its events onto the **existing** worker protocol frames,
so the gateway, the web app and the mobile app need no change to drive a remote
DSH agent.

## Pieces

| Layer | Where | Job |
|---|---|---|
| Harness image | `deploy/worker/Dockerfile` (`--build-arg RUNTIME=dsh`), or `deploy/worker-dsh/Dockerfile` | Installs a pinned `@deepseek-ai/dsh` and the Goble harness composition; still starts the normal worker |
| Composition | `deploy/worker/harness.dsh.yml` | Headless profile + DeepSeek account credentials + JSONL sessions + default model |
| Runtime selection | `GOBLE_HARNESS=dsh` (baked into the DSH image) | The worker routes thread replies to the remote runner instead of the native harness |
| Runner | `crates/goblin-worker/src/dsh_runner.rs` | Spawns the harness, feeds the task on stdin, parses NDJSON, projects frames, enforces cancel/timeout/resume, installs and deletes credentials |
| Runtime catalog | `crates/goble-core/src/harness/remote.rs` (`RemoteHarnessSpec`, `DSH_SPEC`) | Names the runtime, its image, harness home, default model and credential source for a provisioner or UI |
| Docs | this file | Image, environment, protocol, credentials, limits |

## Building the image

The worker image variant is built from this checkout. It is the same image as
the plain worker — same entrypoint, same `/health`, same outbound gateway
connection — with the harness added.

```sh
# The one-command path (uses deploy/worker/Dockerfile, RUNTIME=dsh):
scripts/build-worker-dsh-image.sh ghcr.io/example/goble-worker-dsh:dev

# Equivalent, by hand:
docker build -f deploy/worker/Dockerfile \
  --build-arg RUNTIME=dsh \
  --build-arg DSH_VERSION=0.2.2-rc.3 \
  -t goble-worker-dsh:dev .
```

`DSH_VERSION` pins the npm package (`0.2.2-rc.3`, the version dsh-remote pins).
Bumping it is an image change, not a worker change: the worker only consumes the
headless `--json` event contract.

`deploy/worker-dsh/Dockerfile` is the thin variant for a fork that already has a
base worker image:

```sh
docker build -f deploy/worker-dsh/Dockerfile \
  --build-arg WORKER_IMAGE=ghcr.io/adriantuci1/goble-worker:latest \
  -t goble-worker-dsh:dev .
```

Both variants install the same pinned harness and the same
`deploy/worker/harness.dsh.yml`; keep `DSH_VERSION` in step between them.

The DSH runtime installs Node 24 by copying `/usr/local` from the official
`node:24-bookworm-slim` image (bookworm's own Node is 18, which the harness does
not support), runs
`npm i -g --allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,protobufjs @deepseek-ai/dsh@<version>`,
and adds `procps` for process supervision. The worker itself is still the
Rust binary built from this workspace and is still the entrypoint.

The image has not been published by the repository's CI: `.github/workflows/worker-image.yml`
builds the plain worker today. Build it locally (or add a matrix entry that
passes `RUNTIME=dsh`) before putting it in a `[remote_agents.<name>]` table.

## Using it for a remote agent

Point a remote-agent table at the DSH image, exactly like the plain worker image
(see [remote-sandboxes.md](remote-sandboxes.md)):

```toml
[remote_agents.dsh]
provider = "docker"          # or modal / e2b / daytona / box / cloud_run
image = "goble-worker-dsh:dev"
```

The provisioner passes the usual `GOBLE_GATEWAY_URL`, `GOBLE_WORKER_TOKEN`,
`GOBLE_WORKER_ID`, `GOBLE_PROJECT_ID` and `GOBLE_WORKSPACE_PATH`; the image
itself sets `GOBLE_HARNESS=dsh`, so nothing in the provisioner has to change for
the runtime to be selected. What a provisioner (or an operator running
`docker run --env`) can additionally set is in [Environment](#environment), and
how credentials reach the sandbox is in [Credentials](#credentials).

Runs are started with the ordinary desktop protocol frame, so a client uses the
same code path for a native worker and a DSH worker:

```json
{"kind": "run_agent_for_thread_reply", "trace_id": "trace-1",
 "thread_id": "thread-1", "agent_id": "agent-1", "prompt": "fix the tests",
 "spec": {...}, "mcp_servers": []}
```

`mcp_servers` and the agent spec are stored by the worker for bookkeeping, but
the harness composes its own tools from the image: the spec's tool list does not
configure DSH. That is honest rather than wired-through: the sandbox is the
boundary and the harness is the agent.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `GOBLE_HARNESS` | unset (plain image), `dsh` (DSH image) | Selects the remote runtime. Only `dsh` is implemented; any other value keeps the native harness |
| `GOBLE_DSH_BIN` | `dsh` | The harness program. A test or a custom image can point it at a wrapper |
| `GOBLE_DSH_HARNESS` | `/opt/dsh/harness.yml` | Composition passed as `--patch` |
| `DSH_HOME` / `GOBLE_DSH_HOME` | `$HOME/.dsh`, else `/home/goble/.dsh` | Harness home: JSONL sessions, credentials file, generated overlay |
| `GOBLE_DSH_WORKSPACE` | `$GOBLE_WORKSPACE_PATH`, else `/workspace` | Working directory of a run (`cwd` of the harness) |
| `GOBLE_DSH_MODEL` | `deepseek-flash` (image) | Model the worker writes into its generated overlay; the harness's composition default when unset |
| `GOBLE_DSH_TIMEOUT_SECONDS` | `1800` | Per-run budget; the process group is killed when it expires |
| `GOBLE_DSH_CREDENTIALS_SECRET` | `dsh_credentials` | Worker vault secret that holds the DeepSeek account credentials |
| `GOBLE_NATIVE_SESSION_ID` | unset | The conversation a restored sandbox binding resumes (written by the provisioner in Phase 3). Used for a thread that has no newer session of its own |
| `DSH_CREDENTIALS_YAML` | unset | Inline credentials for one worker lifetime (what the dsh-remote gateway passes to its sandbox) |
| `DSH_CREDENTIALS_FILE` | unset | A credentials file the caller owns: read per run, never rewritten or deleted |
| `DSH_TELEMETRY_DISABLED`, `npm_config_update_notifier` | `1`, `false` (image) | Keep the harness quiet in a sandbox |

The worker's own environment is inherited by the harness, except
`DSH_CREDENTIALS_YAML`, which is removed: the credentials are a file for the
run, so the model's shell tools cannot read them from their environment either.

## Protocol frames

No new request frame is needed: a DSH turn is an ordinary
`run_agent_for_thread_reply`. A DSH-capable worker handles that frame in its own
task (so the connection stays free to read the next frame) and projects the
harness's `--json` stream onto the frames the clients already render:

| DSH `--json` event | Worker frame |
|---|---|
| `session` (`sessionId`) | none (remembered for `--session-id` resume) |
| `text` | `assistant_delta` (`delta`) |
| `thinking` | `agent_log` at `debug` (Goble has no reasoning frame) |
| `tool_call` (`callId`, `tool`, `input`) | `tool_call_started` (`id`, `name`, `arguments`) |
| `tool_result` `status: completed` | `tool_call_finished` (`id`, `result`) |
| `tool_result` `status: error` | `tool_call_error` (`id`, `message`) |
| `status` `step_end` with `usage` | `agent_log` at `info` (`usage: … tokens`) |
| `status` `turn_end` | `done` |
| `final` (`text`, `reason`) | the turn's reply text |
| `error` (`message`) | `agent_log` at `error` |

The run also emits the lifecycle frames every harness emits: `agent_started`
when the trace opens, and `agent_finished` with `success`, `failure` or
`cancelled` when it settles; `thread_agent_reply` carries the answer text (and is
what a chat client completes its turn on).

Two additions on the worker protocol:

- **`cancel_run {trace_id}`** (`DesktopMessage`): stop the run behind that
  trace. The worker kills the harness's process group (`SIGKILL` to `-pid`, so
  tools the agent started die with it) and settles the trace as `cancelled`. An
  unknown or already-finished trace is a no-op; cancelling a run that settled in
  the meantime is not an error.
- **Unknown frames**: a `DesktopMessage` whose `kind` this worker does not know
  (a newer client) is logged and ignored instead of failing the connection. A
  known frame with a broken payload is still reported as an error. This is what
  lets `cancel_run` and future frames be additive.

A listener that only knows the old frames ignores the unknown kinds (the relay
copies frames opaquely), which is why the runtime needs no gateway change.

### Resume

Each thread's harness session id is stored in the worker's own store under
`dsh_session:<thread_id>`, taken from the `session`/`final` events. The next
turn on that thread passes `--session-id <id>`, so the harness continues the
same conversation (subject to DSH's own adoption rules: same working directory,
same composition). A turn that fails still records the id it saw: the next
attempt may resume it.

A sandbox restored from a snapshot has an empty store but the provisioner
passes the binding's conversation as `GOBLE_NATIVE_SESSION_ID`. The worker uses
it for a thread that has no remembered session yet (a binding-level fallback),
and the thread's own session takes over from the first turn on. That is how a
snapshot/resume deploy (Phase 3) continues the DSH conversation it left.

### Timeout and cancellation

A run is killed when `GOBLE_DSH_TIMEOUT_SECONDS` elapses (default 30 minutes);
the timeout is reported as an `agent_log` error plus `agent_finished`
`failure`. `cancel_run` is immediate. The harness runs in its own process group,
so neither path leaves a tool subprocess behind.

## Credentials

The DeepSeek account credentials are **never baked into the image** and the
harness home is not part of a snapshot with them in it. The worker resolves them
in this order for each run:

1. `DSH_CREDENTIALS_YAML` in the worker's environment (what a provisioner that
   knows the value would pass; the dsh-remote gateway's pattern);
2. `DSH_CREDENTIALS_FILE`, read once per run (a file the caller owns — the
   worker never rewrites or deletes it);
3. the worker's vault secret `dsh_credentials` (rename with
   `GOBLE_DSH_CREDENTIALS_SECRET`), or the in-memory secret of that name pushed
   with `PushSecrets`.

When the worker owns the value it writes `$DSH_HOME/.credentials.yaml` with mode
`0600` just before the spawn, and deletes it when the **last** concurrent DSH run
finishes — concurrent threads of one project share the file while they run.
`TriggerSnapshot` also deletes it before uploading a snapshot, so a filesystem
snapshot of the sandbox cannot contain it. The next run re-installs it from the
vault or the environment; nothing depends on it surviving.

How to seed the secret. The worker already has a vault, so a reviewed credential
file can be pushed to it with the CLI (the same frame the desktop uses):

```sh
# On a machine where `dsh login` has produced ~/.dsh/.credentials.yaml:
goble-cli secret set \
  --worker "$GOBLE_WORKER_ID" --url "$GOBLE_GATEWAY_URL" \
  --name dsh_credentials --value "$(cat ~/.dsh/.credentials.yaml)"
```

Or start the container with `DSH_CREDENTIALS_YAML` in its environment if the
provider supports arbitrary sandbox env (the `docker` adapter does through
`docker run --env`; the higher-level provisioner currently passes only its fixed
worker variables — see [Limits](#limits-and-unsupported)).

Credentials are not visible to the model: DSH's subprocess layer scrubs
credential-shaped environment names, and the file lives under the harness home,
not the workspace. Treat the whole sandbox as trusted for the duration of a run:
the harness runs with `danger-full-access` inside it, and an agent can read any
file the user can.

## Limits and unsupported

Honest list — these are deliberately not implemented in this phase:

- **No interactive approval or questions.** The composition runs with
  `--permission-mode danger-full-access --approval-policy never` (the container
  is the boundary, as in dsh-remote). `ask_user`/`command_proposed` never fire
  for a DSH run.
- **No token-level streaming.** DSH's headless `--json` projection emits `text`
  when an assistant message commits, not per token, so `assistant_delta` frames
  are per message. `thinking` text exists but Goble has no reasoning frame; it
  lands in a debug log line.
- **No reasoning UI.** See above; the transcript shows the answer and tools, not
  the model's reasoning.
- **One answer per invocation.** The headless profile is one task per process;
  a follow-up message is a new process resuming the session (which is what the
  worker does), not an interactive loop.
- **Cancel only works for DSH runs.** `cancel_run` stops a remote harness; the
  native runtime has no equivalent and answers with an `agent_log` warning.
- **No automatic GitHub token bootstrap.** dsh-remote writes an ephemeral
  `GIT_CONFIG_GLOBAL`/`GH_CONFIG_DIR` per sandbox. Goble does not inject a
  GitHub token here; configure `git`/`gh` credentials inside the image or the
  sandbox however the deployment does that, and remember they are part of that
  sandbox's trust boundary.
- **No provider-level snapshot integration in this phase.** Credential cleanup
  is at the run and `TriggerSnapshot` boundary; the snapshot/resume binding work
  lives in `goble-remote` (Phase 3) and composes with this runtime but is not
  re-implemented in the worker.
- **Resume is not portable across incompatible deployments.** DSH refuses to
  adopt a session recorded under a different working directory or composition;
  the worker reports that error instead of silently starting a fresh session.
- **`mcp_servers` in the request do not configure the harness.** They are stored
  for the worker's own bookkeeping; DSH's tools come from the image.
- **Image publishing.** CI publishes only the plain worker image today; build
  and push the DSH variant before pointing a remote-agent table at it.
- **The docker smoke script is optional.** `scripts/smoke-worker-dsh.sh` checks
  the image contents with the local Docker daemon; it is not part of `cargo
  test` and no CI job runs it.

## Tests

The runtime is covered without a model or a Docker daemon:

- `crates/goblin-worker/src/dsh_runner.rs` unit tests: argv shape, generated
  overlay, envelope parsing for every documented event, usage summaries, cancel
  registry, credential file permissions/cleanup.
- `crates/goblin-worker/tests/dsh_runtime.rs`: a fake `dsh` shell script emits
  the documented NDJSON. Asserts text/tool/usage/turn-end frame mapping, stdin
  delivery, `--session-id` on resume, process-group kill on cancel, timeout
  kill, error reporting, a missing binary, credential install/delete, and the
  model overlay.
- `crates/goblin-worker/tests/dsh_runtime_e2e.rs`: the worker binary with
  `GOBLE_HARNESS=dsh`, a fake harness and a mock relay speaks the desktop
  protocol end to end — a normal turn streams deltas/tool frames/reply, and a
  `cancel_run` frame settles a second turn as `cancelled`.

Optional, with a Docker daemon and network:

```sh
scripts/build-worker-dsh-image.sh goble-worker-dsh:dev
scripts/smoke-worker-dsh.sh goble-worker-dsh:dev
```
