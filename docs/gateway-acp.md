# goble-acp — drive Goble gateway sessions from an ACP client (T3 Code)

`goble-acp` is a stdio **Agent Client Protocol (ACP)** server. An ACP client —
T3 Code web/desktop/mobile, or any other ACP agent client — launches it as a
"local ACP command"; the bridge turns each ACP session into a **gateway session**
and each prompt into a **gateway run**. The agent executes inside the Goble
sandbox behind the gateway, never on the machine running the ACP client.

```
T3 Code / any ACP client
        │  newline-delimited ACP JSON-RPC 2.0 over stdin/stdout
        ▼
   goble-acp  ──HTTP/SSE──►  goble-gateway  ──worker protocol──►  sandbox
        │                      /api/v1/sessions                     (agent runs here)
        │                      /api/v1/sessions/{id}/runs
        │                      /api/v1/runs/{id}
        └────────────────────  /api/v1/runs/{id}/stream (SSE)
```

The bridge has no model and no tools of its own, and it never touches the
client's filesystem or terminals: it is a protocol adapter (ACP in, gateway
calls out) plus a small `sessions.json` state file.

- Crate: `crates/goble-acp` (library `goble_acp`, binary `goble-acp`)
- Gateway API: [gateway-api.md](gateway-api.md) (Phase 1 control plane,
  `crates/goble-gateway/src/remote.rs`)
- Behaviour reference: the `bridge-acp` implementation in `AdrianTuci1/dsh-remote`

## Build and run manually

```bash
cargo build -p goble-acp
# binary at target/debug/goble-acp (or target/release/goble-acp)

# minimal protocol smoke test (no gateway needed for initialize):
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}' \
  '{"jsonrpc":"2.0","id":2,"method":"fs/read_text_file","params":{"sessionId":"x","path":"/etc/passwd"}}' \
  | ./target/debug/goble-acp
```

`stdout` carries protocol frames only; every diagnostic goes to `stderr`
(`RUST_LOG`, default `info`; use `RUST_LOG=debug` for frame-level detail).

## Register it in T3 Code

1. **Settings → Providers → Add provider → Show more → Local ACP command.**
2. **Executable override**: the absolute path to the binary, e.g.
   `/Users/you/Projects/goble/target/release/goble-acp`
   (T3 Code spawns it directly, without a shell, so an absolute path is
   required).
3. **Arguments**: none — configuration is environment-only.
4. **Display name**: `Goble gateway` (anything you like).
5. **Environment overrides** on the same provider instance:

   | Variable | Value |
   |---|---|
   | `GOBLE_GATEWAY_URL` | e.g. `https://goble.example.com` or `http://127.0.0.1:8788` |
   | `GOBLE_GATEWAY_TOKEN` | bearer token, **or** use the PIN instead |
   | `GOBLE_GATEWAY_PIN` | the gateway PIN; the bridge pairs it into a token via `POST /api/v1/session` |

   Add `GOBLE_ACP_WORKER` if the gateway has more than one connected worker
   (the gateway answers `409 worker_required` when it cannot choose).
6. Save. The model picker shows no models: the bridge advertises no
   `configOptions` because the model is chosen by the gateway's harness, not by
   ACP. Adding a custom model label in T3 Code only changes the label.

The T3 Code docs describe the same form fields in
`~/Projects/t3code/docs/user/providers-acp.md` ("Add a local command");
`Executable override` = `commandPath`, `Arguments` = `commandArgs`.

## Configuration (environment)

| Variable | Meaning | Default |
|---|---|---|
| `GOBLE_GATEWAY_URL` | Gateway base URL, `http`/`https` only | `http://127.0.0.1:8788` |
| `GOBLE_GATEWAY_TOKEN` | Bearer token for `/api/v1` | — |
| `GOBLE_GATEWAY_PIN` | PIN paired into a token (`POST /api/v1/session`); token wins if both are set | — |
| `GOBLE_ACP_SESSION_NAME` | Fixed gateway session name; every ACP session reuses that binding | derived |
| `GOBLE_ACP_WORKER` | Worker id to bind gateway sessions to | gateway picks the only connected worker |
| `GOBLE_ACP_RUN_TIMEOUT_S` | Run budget forwarded as `timeout_s` | `1800` |
| `GOBLE_ACP_STATE_DIR` | Directory for `sessions.json` (ACP↔gateway map) | `$HOME/.goble/acp` |
| `GOBLE_ACP_STRICT_SETUP` | `1` refuses `mcpServers`/`additionalDirectories` instead of warning | unset (warn) |
| `GOBLE_ACP_RECONNECT_ATTEMPTS` | SSE reconnect attempts after a transport drop | `3` |
| `RUST_LOG` | stderr log filter | `info` |

Notes:

- Without a token or PIN the bridge talks to the gateway unauthenticated, which
  only works for a gateway started with `--no-auth` on loopback.
- The PIN is only used for pairing; the token is cached in memory. A `401`
  mid-session triggers one re-pairing.
- Gateway session names default to `acp/<cwd basename>/<acp session uuid>`.
  `GOBLE_ACP_SESSION_NAME` overrides the whole name (same name → same gateway
  binding; useful for a single fixed remote workspace).
- `cwd` is recorded and used in the session name. It is **not** a path on the
  bridge machine: the sandbox workspace belongs to the gateway/worker.

## Supported ACP surface

| Method | Behaviour |
|---|---|
| `initialize` | Local answer. `protocolVersion: 1`, `agentInfo.name: goble-acp`; `agentCapabilities.loadSession: false`; `promptCapabilities` `{text: true, image: false, audio: false, embeddedContext: true}`; `sessionCapabilities` `{close, resume}`; no auth methods. |
| `authenticate` | Answered `{}` (no auth advertised, so clients do not call it). |
| `session/new` | `{cwd, mcpServers?}` → creates/reuses a gateway session (`POST /api/v1/sessions`), returns a bridge-generated `sessionId` plus `_meta.bridge.gatewaySessionId`. |
| `session/prompt` | `{sessionId, prompt}` → renders blocks to text, dispatches `POST /api/v1/sessions/{id}/runs`, streams `session/update` notifications from the run SSE, settles with `{stopReason}`. |
| `session/cancel` | Notification (a request form is also tolerated): settles the turn `cancelled`, then best-effort `POST /api/v1/sessions/{id}/cancel`. |
| `session/resume` | Re-attaches to the durable gateway binding by name (no history replay), returns `{configOptions: []}`. Survives a bridge restart via `sessions.json`. |
| `session/close` | Forgets the ACP session (`{}`); the gateway session binding is kept. |
| `session/load` | **Refused** with `-32601` and `data.method = "session/load"` — no history replay exists, exactly like `dsh --profile acp`. |

Prompt content blocks:

- `text` — passed through;
- `resource_link` — rendered as `[resource_link name="…" uri="…"]`;
- `resource` with inline `text` — inlined as `[resource uri="…"]\n<text>`;
- `image`, `audio`, `resource` with `blob`, unknown blocks — refused `-32602`
  (the gateway prompt API is text-only; nothing is uploaded from this machine).

### Frames mapping (gateway run events → ACP)

Each SSE `data:` event from `GET /api/v1/runs/{id}/stream` becomes zero or one
`session/update` notification; the terminal event settles `session/prompt`.

| Gateway event | ACP output |
|---|---|
| `{"type":"text","text":"delta"}` | `session/update` → `agent_message_chunk` (stable `messageId` per run, `content.type: "text"`) |
| `{"type":"thinking","text":"delta"}` | `agent_thought_chunk` (empty deltas skipped) |
| `{"type":"tool_call","callId":"…","tool":"…","input":{…}}` | `tool_call` (`title` = tool, `kind: "other"`, `status: "in_progress"`, `rawInput` = input) |
| `{"type":"tool_result","callId":"…","status":"completed"\|"failed","result":"…"}` | `tool_call_update` (`status` normalized, `content: [{"type":"content","content":{"type":"text","text":"…"}}]`, matching captured `dsh --profile acp` frames) |
| `{"type":"session","sessionId":"…"}` | `session_info_update` with `_meta.bridge = {gatewaySessionId, runId}` |
| `{"type":"usage","used":N,"size":M}` | `usage_update` only when **both** numbers are present (never invented) |
| `{"type":"status",…}`, `log`, `ask_user`, `command_proposed`, future kinds | ignored (no ACP v1 equivalent) |
| `{"type":"final","reason":{"kind":"completed"\|"cancelled"\|"error","error":{"message":"…"}?},"text":"…"}` | settles the prompt: `completed` → `{"stopReason":"end_turn"}`, `cancelled` → `{"stopReason":"cancelled"}`, `error` → JSON-RPC `-32603` `turn failed: <message>`. `final.text` is emitted as a chunk only if no `text` event preceded it, so output is never duplicated. |
| `{"type":"end","status":…}` | Optional synthetic terminal; if the stream closes without a `final`, the bridge settles from `GET /api/v1/runs/{id}` instead of guessing. |

Aliases are accepted for forward compatibility with worker-frame names
(`assistant_delta`, `tool_call_started`, `tool_call_finished`, …) and for
`{"type":"event","kind":"…"}` wrappers.

### Error codes

| Situation | JSON-RPC code |
|---|---|
| Gateway `409 session_busy`, or a second prompt on a busy ACP session | `-32000` (`session busy: …`) |
| Unknown ACP session id, gateway `404 unknown_session`/`unknown_run` | `-32002` (`unknown session: …`) |
| Gateway unreachable, `401`/auth failure, `5xx`, malformed gateway JSON | `-32603` (`gateway unreachable: …` etc.) |
| Failed run | `-32603` `turn failed: <gateway error>` (not a silent `end_turn`) |
| `fs/*`, `terminal/*`, unknown ACP methods | `-32601` |
| Malformed JSON / invalid request / invalid params | `-32700` / `-32600` / `-32602` |

### SSE reconnect

Every data event carries a globally contiguous `index`. The bridge resumes with
`from_index = last_index + 1` after a dropped connection (up to
`GOBLE_ACP_RECONNECT_ATTEMPTS` attempts, exponential backoff 250 ms…2 s), so
deltas are neither lost nor duplicated. `: keepalive` comments are ignored.

## Honest limitations (and why)

- **No local code execution.** `fs/*` and `terminal/*` are refused with an
  explicit explanation: file and terminal work happens in the sandbox through
  the agent's own tools. The bridge never sends `fs/*`/`terminal/*` requests to
  the client either.
- **No history replay.** `session/load` is refused; `session/resume` re-attaches
  without replaying messages. Gateway runs/events are process-local, so after a
  gateway restart the run ids are gone (sessions survive; their bindings live in
  `sessions.json` on the gateway).
- **Cancel is gateway-side.** `POST /api/v1/sessions/{id}/cancel` settles the
  run `cancelled` in the gateway, drops late worker frames, and sends a
  best-effort `cancel_run` worker frame. A worker recent enough to understand
  that frame kills the harness process group; an older worker logs an unknown
  frame and keeps the turn running in the sandbox. Either way the gateway has
  settled the run, so the ACP turn settles `cancelled` immediately — do not wait
  for the sandbox to stop.
- **`mcpServers` / `additionalDirectories` cannot be mounted per ACP session.**
  MCP servers are configured by the sandbox image/worker. The default is to
  accept the session, log a warning on stderr, and report it in the logs;
  `GOBLE_ACP_STRICT_SETUP=1` turns it into an explicit `-32602` refusal. T3
  Code's injected `t3-code` MCP tools are therefore not available inside the
  sandbox.
- **No model/config selection.** `session/set_config_option`, `session/set_mode`,
  `session/list`, `session/fork`, `session/delete`, plans and modes are refused
  with `-32601`; the model is chosen by the gateway's harness.
- **Images/audio/embedded blobs are refused** (`-32602`); the gateway prompt API
  is text-only.
- **One prompt per ACP session at a time** (ACP's own constraint). A concurrent
  prompt is rejected `-32000`; the gateway's `409` maps to the same code.
- **No `usage_update` unless the gateway reports both `used` and `size`**;
  inventing a context-window size would be worse than omitting the frame.
- **`cwd` is not a local path.** It only shapes the gateway session name.
- **State is per bridge process.** `sessions.json` is written atomically (mode
  `0600`, temp + rename); two bridge processes sharing one state dir are not
  coordinated. Delete the file to forget all mappings.
- **`stdout` is reserved for ACP.** Never point logs or scripts at it.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `gateway unreachable: … error sending request` | Wrong `GOBLE_GATEWAY_URL`, gateway down, or TLS/DNS problem. `curl $GOBLE_GATEWAY_URL/health` must answer `{"status":"ok",…}`. |
| `gateway authentication failed: PIN pairing rejected` | Wrong `GOBLE_GATEWAY_PIN`, or an expired/invalid `GOBLE_GATEWAY_TOKEN`. |
| `no worker connected` / `worker_required` / `worker offline` (`-32603` with the gateway message) | No worker is connected to the gateway, more than one is connected and `GOBLE_ACP_WORKER` is unset, or the named worker is offline. Check `GET /api/v1/workers` and set `GOBLE_ACP_WORKER`. |
| `session busy: …` (`-32000`) | Another prompt is still running for that session (or another client dispatched a run into the same gateway session). Wait for the turn to settle or cancel it. |
| `unknown session: …` (`-32002`) | The ACP session id is not in this bridge's state (new process without `GOBLE_ACP_STATE_DIR`, deleted `sessions.json`, or `session/close` was called). Start a new `session/new`. |
| `turn failed: …` (`-32603`) | The gateway run failed; the message is the gateway's `error`. Check gateway logs and worker credentials/agent configuration. |
| T3 Code shows no models | Expected: the bridge advertises no `configOptions`; the model is chosen by the gateway harness. Add a custom model label in T3 Code if the UI requires one. |
| Timeouts on long silent tool runs | The stream sends SSE keepalives every 15 s; the bridge treats 120 s of silence as a dead connection and reconnects with `from_index`. Raise `GOBLE_ACP_RECONNECT_ATTEMPTS` if the network is flaky. |
| Nothing on stdout / client sees no frames | The binary only speaks on stdout. Run it manually and pipe a frame in (see Build and run); look at stderr with `RUST_LOG=debug`. |

## Tests

```bash
cargo test -p goble-acp     # unit + mocked-gateway E2E + real-gateway E2E
cargo build -p goble-acp
```

- `src/**` unit tests: config parsing, prompt block rendering, gateway
  response/status tolerance, SSE parser (arbitrary chunk boundaries, CRLF,
  keepalives, multi-line data), event→update mapping using the captured
  `dsh --profile acp` frame shapes, and JSON-RPC error mapping.
- `tests/e2e.rs`: 16 tests spawning the **real binary** against an in-process
  axum mock of the gateway contract — initialize, session/new, prompt →
  updates → `end_turn`, failed turn, cancel, `409 busy`, transport error, stream
  drop + `from_index` reconnect, stream-without-final poll fallback, persistence
  across a restart + resume, unsupported methods, malformed frames, strict MCP
  refusal, session close.
- `tests/real_gateway.rs`: 2 tests against the **real `goble-gateway` crate**
  with a fake worker: a full turn through production dispatch/SSE, and
  gateway-side cancel.
