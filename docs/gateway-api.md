# The gateway's remote-execution API

The gateway (`crates/goble-gateway`) has two jobs. It is still the WebSocket
relay documented in [remote-sandboxes.md](remote-sandboxes.md): workers dial out
to `/ws/worker`, browsers and phones attach to `/ws/client`, and frames are
copied between them. On top of that relay it serves a small **remote-execution
control plane** — the piece a phone, a web client, a CLI, or an ACP bridge
(`crates/goble-acp`) uses to drive an agent in a sandbox without opening a
WebSocket and speaking the raw `goble-core::protocol` frames itself.

The control plane reuses the worker connections the relay already owns. It does
not open a second socket: `POST /api/v1/sessions/{id}/runs` sends the same
`run_agent_for_thread_reply` frame a browser client would relay, and the
gateway's frame tap correlates the worker's reply frames by `trace_id`.

```
client (phone / web / ACP bridge)
  │  POST /api/v1/sessions, POST .../runs, GET /api/v1/runs/{id}/stream
  ▼
goble-gateway  ── control plane: bindings, runs, events, SSE ──┐
  │  /ws/worker (outbound from the sandbox)                    │
  ▼                                                           │
goblin-worker  ── run_agent_for_thread_reply, assistant_delta, │
                  tool_call_*, agent_finished, thread_agent_reply
```

## Authentication

Every `/api/v1/**` route except `/health` uses the session the
[phone pairing](mobile-remote.md#pairing) already establishes:

1. `POST /api/v1/session` with `{"pin": "<PIN>"}` answers
   `{"token": "...", "expires_in": 43200}`.
2. Every request sends `Authorization: Bearer <token>`.
3. A browser served from the gateway's own origin can use the `HttpOnly` session
   cookie from `POST /login` instead — which is what makes `EventSource` work,
   because it cannot set headers.

`/health` stays open. Errors are JSON:

```json
{"error": "session_busy", "message": "session ses_… is already running run run_…"}
```

| HTTP | `error` | Meaning |
|---|---|---|
| 400 | `invalid_request` | Missing/blank/oversized field. |
| 401 | `login required` | No or expired session (same body as `/api/workers`). |
| 404 | `unknown_session`, `unknown_run` | The id is not in the store. |
| 409 | `session_busy` | The session already has a run in flight. |
| 409 | `worker_required` | Creating a session without `worker` while several are connected. |
| 503 | `no_worker_connected` | Creating a session without `worker` while none is connected. |
| 503 | `worker_offline` | The bound worker is not connected, or dropped before the frame was sent. |

`GET /api/v1/workers` is an alias of the existing `GET /api/workers` and lists
connected workers (`workers[].id` is the `worker` to pass when creating a
session).

## Endpoints

| Method | Path | Body | Success | Notes |
|---|---|---|---|---|
| `POST` | `/api/v1/session` | `{"pin": "…"}` | `200` `{"token","expires_in"}` | Existing pairing route. |
| `GET` | `/api/v1/workers` | — | `200` `{"workers":[…]}` | Alias of `/api/workers`. |
| `POST` | `/api/v1/sessions` | `{"name"?, "worker"?}` | `201` new / `200` reused: session object + `"reused": bool` | Create-or-reuse, stable by `name`. |
| `GET` | `/api/v1/sessions` | — | `200` `{"sessions":[…]}` | Oldest first. |
| `GET` | `/api/v1/sessions/{id}` | — | `200` session object | |
| `POST` | `/api/v1/sessions/{id}/cancel` | — (body ignored) | `200` `{"ok":true,"run_id":"run_…"\|null}` | Settles the running run; see [Cancel](#cancel). |
| `POST` | `/api/v1/sessions/{id}/runs` | `{"task", "timeout_s"?, "wait"?}` | `202` run object (`wait:true`: `200` after settle) | One run per session. |
| `GET` | `/api/v1/runs/{id}` | — | `200` run object | |
| `GET` | `/api/v1/runs/{id}/events?from_index=N` | — | `200` `{"run_id","status","from_index","next_index","events"}` | `from_index` is inclusive and clamped. |
| `GET` | `/api/v1/runs/{id}/stream?from_index=N` | — | `200 text/event-stream` | SSE replay + live; see [SSE](#sse). |

### Session object

```json
{
  "id": "ses_…",
  "name": "phone",
  "worker_id": "goble-goble-2fbfef8fa780",
  "worker_connected": true,
  "thread_id": "thr_…",
  "agent_id": "agent_…",
  "status": "idle",
  "active_run_id": null,
  "created_at": "2026-11-18T00:20:00.000Z",
  "updated_at": "2026-11-18T00:20:00.000Z",
  "run_count": 3,
  "last_run_id": "run_…"
}
```

A session is a **binding**, not a process: `worker_id` says which connected
worker runs its turns and `thread_id` names the conversation the worker stores
them under. The worker keeps that conversation in its own store, so a later run
on the same session sees the earlier turns — including after a gateway restart,
because the binding is persisted.

`status` is `idle` or `running`; `active_run_id` names the running run. A
session's single run slot is what makes the second `POST .../runs` a `409`.

### Creating and reusing sessions

`POST /api/v1/sessions` with `{"name": "phone"}`:

- the first call creates a binding and answers `201` with `"reused": false`;
- a later call with the same `name` answers `200` with the same `id`,
  `thread_id` and `agent_id`, and `"reused": true`.

`name` is at most 200 bytes. Without a `name`, every call creates a fresh
anonymous binding — there is nothing to reuse by.

`worker` selects the worker. When it is omitted:

- exactly one worker connected → that worker;
- none connected → `503 no_worker_connected`;
- several connected → `409 worker_required`; list them with
  `GET /api/v1/workers` and pass `worker` explicitly.

An explicit `worker` may be offline: the binding is saved and `POST .../runs`
answers `503 worker_offline` until it connects. Passing a different `worker` for
an existing name rebinds it — while it is idle. While a run is in flight that
is `409 session_busy`.

### Starting a run

`POST /api/v1/sessions/{session_id}/runs`

```json
{"task": "run the test suite and summarize failures", "timeout_s": 1800, "wait": false}
```

- `task` is required, at most 256 KiB, and becomes the user message of the turn.
- `timeout_s` defaults to 600 and is clamped to `1..=3600`. When the worker has
  not settled the turn by then the run becomes `timeout` (see
  [lifecycle](#run-lifecycle)).
- `wait` is `false` by default: the call answers `202` as soon as the frame is
  on the worker's socket. With `wait: true` it answers `200` after the run
  settles. A `wait: true` call made while the session is busy waits up to a
  30-second queue window for the slot before it gets `409 session_busy`.

The response is the run object. A run that could not be dispatched at all
(worker offline, session missing, session busy) never appears as `running`.

The gateway builds the frame itself from the session's binding. A client never
sends `agent_id`, `spec` or `mcp_servers`; the gateway supplies a generated
agent identity (`agent_id`, name = session name) and the session's `thread_id`:

```json
{
  "kind": "run_agent_for_thread_reply",
  "trace_id": "thr_…-run_…",
  "thread_id": "thr_…",
  "agent_id": "agent_…",
  "prompt": "<task>",
  "spec": {"id": "agent_…", "name": "<session name>", "description": "…",
           "prompt": "", "tools": [], "triggers": [], "mcp_ids": [],
           "created_at": "…", "updated_at": "…"},
  "mcp_servers": []
}
```

The `trace_id` is the correlation key: every worker frame for this turn carries
it, and the gateway stops caring about the trace when the run settles.

### Run object

```json
{
  "id": "run_…",
  "session_id": "ses_…",
  "worker_id": "goble-goble-2fbfef8fa780",
  "thread_id": "thr_…",
  "trace_id": "thr_…-run_…",
  "task": "run the test suite and summarize failures",
  "status": "succeeded",
  "created_at": "2026-11-18T00:21:00.000Z",
  "started_at": "2026-11-18T00:21:00.000Z",
  "finished_at": "2026-11-18T00:21:42.000Z",
  "final_text": "3 failures in goble-ui…",
  "error": null,
  "usage": null,
  "events_count": 42
}
```

Statuses: `running`, then exactly one of `succeeded`, `failed`, `cancelled`,
`timeout`. `final_text` is the answer (`thread_agent_reply`, or the streamed
assistant text when the worker does not send a reply); `error` is set for
`failed` and `timeout`; `usage` is filled only when the worker reports one: a
remote-DSH turn reports a text usage summary (the protocol has no numeric token
frame yet), so `usage.text` carries that summary, and a newer worker can add
`used`/`size` without changing the shape.

## Events

`GET /api/v1/runs/{id}/events?from_index=N` returns the run's stored event log
from index `N` (inclusive, clamped to the end) plus `next_index` to resume from.
SSE carries exactly the same objects. Every event has a globally contiguous
`index` starting at 0 and an `at` timestamp.

| `type` | Fields | Worker frame it comes from |
|---|---|---|
| `status` | `status` (`running`/`succeeded`/`failed`/`cancelled`/`timeout`), optionally `phase` (`turn_end`, `agent_state`), `state` | the run's start, `done` (remote-DSH turn end), `agent_state_update`, the settle |
| `text` | `text` (delta to append) | `assistant_delta` |
| `thinking` | `text` | a future reasoning delta frame (`kind` containing `thinking`/`reasoning`) |
| `tool_call` | `callId`, `tool`, `input` | `tool_call_started` |
| `tool_result` | `callId`, `tool`, `status` (`completed`/`failed`), `result` | `tool_call_finished`, `tool_call_error`, `agent_tool_result` |
| `log` | `level`, `message`, `stepId` | `agent_log` |
| `ask_user` | `question`, `quickReplies` | `ask_user` |
| `command_proposed` | `callId`, `candidates`, `cwd` | `command_proposed` |
| `usage` | `text` (remote-DSH summary), optionally `used`/`size`, `frame` | an `agent_log` with `stepId: "usage"`, or a future numeric usage frame |
| `final` | `reason` (`{"kind":"completed"}` / `{"kind":"cancelled"}` / `{"kind":"error","error":{"message":"…"}}`), `text` | written when the run settles |
| `event` | `kind`, `frame` | any other frame kind: stored and forwarded unchanged |

A run's first event is `status running`; its last two are a `status` event and
the terminal `final` event. `text` events are deltas: concatenate them for the
streaming answer, and prefer `final.text` (or the run's `final_text`) once the
turn ends.

### SSE

```
GET /api/v1/runs/{run_id}/stream?from_index=42
Accept: text/event-stream
```

- Every `data:` line is one event object above; the indexes are the same as the
  JSON event log, so `from_index`/`next_index` resume a dropped connection
  exactly (`GET .../stream?from_index=<next_index>`).
- A `: keepalive` comment is sent right after the replay and again after every
  15 seconds without a new event.
- The stream ends immediately after the `final` event. Clients that prefer
  polling can use `GET /api/v1/runs/{id}` instead.

```console
$ curl -N -H "Authorization: Bearer $TOKEN" \
    "http://127.0.0.1:8788/api/v1/runs/$RUN_ID/stream?from_index=0"
data: {"index":0,"at":"…","type":"status","status":"running"}

: keepalive

data: {"index":1,"at":"…","type":"text","text":"Running "}

data: {"index":2,"at":"…","type":"tool_call","callId":"call-1","tool":"bash","input":{"cmd":"cargo test"}}

data: {"index":3,"at":"…","type":"tool_result","callId":"call-1","tool":"bash","status":"completed","result":"…"}

data: {"index":4,"at":"…","type":"status","status":"succeeded"}

data: {"index":5,"at":"…","type":"final","reason":{"kind":"completed"},"text":"…"}

```

A browser on the gateway's origin can use the cookie session directly:

```js
const events = new EventSource(`/api/v1/runs/${runId}/stream`);
events.onmessage = (message) => console.log(JSON.parse(message.data));
```

## Run lifecycle

```
POST /runs ──► running ──► succeeded   (thread_agent_reply, or agent_finished Success)
                  │   ──► failed      (agent_finished Failure)
                  │   ──► cancelled   (POST .../cancel)
                  │   ──► timeout     (timeout_s elapsed)
                  └   ──► failed      (worker socket closed before settling)
```

- **Settling on the worker's turn.** A successful `agent_finished` starts a
  short grace window (400 ms) because the worker emits `thread_agent_reply`
  right after it; the reply becomes `final_text`. If no reply arrives the
  streamed assistant text is used.
- **Timeouts.** The gateway settles the run as `timeout` with
  `run timed out after Ns` and frees the session. The worker's turn may keep
  running in the sandbox; a timeout does not send a cancel frame (only
  [Cancel](#cancel) does).
- **Disconnect.** If the worker's socket closes for good (not a reconnect), every
  running run on it becomes `failed` with
  `the worker disconnected before the run finished`. A session is therefore
  never left `running` without a live worker behind it.
- **Process restart.** Runs and their event logs are process-local, so after a
  gateway restart an old run id is `404 unknown_run`; sessions are idle and
  reusable, with their threads intact.

### Cancel

`POST /api/v1/sessions/{id}/cancel` settles the running run as `cancelled`
(idempotent: with no run in flight it answers `{"ok":true,"run_id":null}`). It
also sends `{"kind":"cancel_run","trace_id":"…"}` to the worker. A worker that
runs the remote-DSH runtime kills that run's harness process group and reports
`agent_finished: Cancelled` on the trace (already-settled, so the gateway just
ignores it). The in-process native harness has no kill switch exposed over the
protocol; the worker logs "no cancellable run is live for this trace" there.
Clients should treat the run as finished when the gateway says `cancelled` and
not wait for the sandbox to stop.

### Pausing on a question

`ask_user` and `command_proposed` are surfaced as events, but Phase 1 exposes no
answer endpoint yet: a turn that pauses on a decision keeps its run `running`
until `timeout_s` or a cancel settles it. The conversation and the trace stay on
the worker, so a later version can add `POST /api/v1/runs/{id}/answers` without
moving the binding.

## Persistence

`--data-dir` (env `GOBLE_GATEWAY_DATA`, default `~/.goble/gateway`) holds
`sessions.json`, the session bindings only. The gateway writes it atomically
(temporary file + rename) after every binding or run-count change, and loads it
at startup. Runs and event logs live in memory, capped at 256 runs; a restart
does not keep them.

Without `--data-dir` (tests, embedders calling `Gateway::new`) the control plane
keeps everything in memory.

## Client usage

```console
$ TOKEN=$(curl -s -X POST http://127.0.0.1:8788/api/v1/session \
    -H 'content-type: application/json' -d '{"pin":"ABCD2345"}' | jq -r .token)

$ SESSION=$(curl -s -X POST http://127.0.0.1:8788/api/v1/sessions \
    -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
    -d '{"name":"phone","worker":"my-sandbox"}')
$ SESSION_ID=$(echo "$SESSION" | jq -r .id)

$ RUN=$(curl -s -X POST "http://127.0.0.1:8788/api/v1/sessions/$SESSION_ID/runs" \
    -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
    -d '{"task":"say hello","timeout_s":120}')
$ RUN_ID=$(echo "$RUN" | jq -r .id)

$ curl -N -H "authorization: Bearer $TOKEN" \
    "http://127.0.0.1:8788/api/v1/runs/$RUN_ID/stream"
```

`scripts/gateway-smoke.sh` does exactly this end to end against a throwaway
gateway, the `fake_worker` example, and `curl`.

## Verification

```sh
cargo test -p goble-gateway            # unit tests + tests/gateway.rs + tests/remote_api.rs
cargo build -p goble-gateway
scripts/gateway-smoke.sh               # optional live smoke test (needs curl + jq)
```

`tests/remote_api.rs` covers the contract with a fake worker: auth 401 on every
route, session create/reuse, the dispatched frame shape, the event log, SSE
replay from an index with keepalive, one-run-per-session 409, cancel, timeout,
worker-disconnect cleanup, `wait: true`, and bindings across a gateway restart.

## Not in Phase 1

- worker-side kill exists only for remote-DSH runs (`cancel_run`); a native
  harness turn cannot be stopped from the gateway (see [Cancel](#cancel));
- no answer endpoint for `ask_user` / `command_proposed`;
- no queueing: one run per session, `409` otherwise;
- no run persistence across a gateway restart;
- no push notifications: the phone must keep its socket or poll.
