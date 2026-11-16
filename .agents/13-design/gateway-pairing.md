# 13 — Gateway pairing: dsh-remote ↔ T3 Code, goble today, and the Settings page

**Status:** `[~]` design — this document contains no product code
**Owns:** how a client is admitted to a gateway, what T3 Code actually asks a user for, what goble's gateway does today, and the Settings → Gateway page that closes the gap.
**Depends on:** [`../11-relay-and-hub/README.md`](../11-relay-and-hub/README.md), [`../11-relay-and-hub/relay-auth.md`](../11-relay-and-hub/relay-auth.md), [`../06-renderer/ui-spec/10-settings-tab.md`](../06-renderer/ui-spec/10-settings-tab.md)

## How to read this document

Two source trees are outside this repository, so they get anchors, the same scheme the renderer docs use:

- `D/…` = `/workspace/repos/dsh-remote/…` — the Python gateway, the ACP bridge, and its verification record.
- `T/…` = `/workspace/repos/t3code/…` — the T3 Code checkout (read only, never built or run here).
- No prefix = this repository.

Every claim is followed by `path:line`. Ranges are used when the whole range is the evidence. The document deliberately separates **what the software does** (§1–§2) from **what the owner asked for** (§4).

---

## 0. The short answer

1. **T3 Code never pairs with the dsh gateway.** The user adds a **Local ACP command** provider in T3, and T3 launches a local `bridge.py` as a child process. The gateway URL and key are merely environment variables on that provider instance (`D/bridge-acp/docs/t3code-verify.md:43-65`; `D/bridge-acp/bridge.py:36-37`). There is no URL field, no OAuth, no pairing ceremony, and no gateway API key field in T3.
2. **gobble's gateway is a different kind of server.** It is an opaque WebSocket relay: PIN → human session, shared bearer → worker registration, frames copied verbatim (`crates/goble-gateway/src/lib.rs:1-16`). It has no sessions, runs, event log, file API, or SSE.
3. **The desktop has no gateway client at all.** It only writes `GOBLE_GATEWAY_URL` / `GOBLE_WORKER_TOKEN` into a sandbox's environment when it deploys a worker (`crates/goble-desktop-service/src/state/remote.rs:30-37`, `:122-137`), and connects to workers directly (`crates/goble-desktop-service/src/worker_manager.rs:30`). Nothing in `app/` or `goble-desktop-service` starts or dials `goble-gateway`.
4. **"Exactly like" is achievable as a user contract, not as a protocol.** The same page shape — URL, name, one credential, Test, Pair, worker list, Unpair, persisted state — is a desktop-service + app-UI job; the gateway already exposes the endpoints that make it possible (`POST /api/v1/session`, `GET /api/workers`, `POST /logout`).
5. The one part that can never be "exactly like" without new server work is the **execution surface**: dsh's HTTP runs/SSE/files/cancel versus goble's relayed frames. Treat that as a separate roadmap, not as a Settings page.

---

## 1. dsh-remote ↔ T3 Code: the exact pairing

### 1.1 What the user performs

| Step | What the user does | Evidence |
|---|---|---|
| 1 | Starts the gateway (and sets `GATEWAY_API_KEY` if it is not loopback-only). | `D/gateway/README.md:14-20`; `D/README.md:23-36` |
| 2 | In T3: **Settings → Providers → Add provider → Show more → Local ACP command**. | `T/apps/web/src/components/settings/AcpRegistrySearchStep.tsx:197-206`; `T/docs/user/providers-acp.md:31-38` |
| 3 | Fills **Executable override** (absolute path to Python) and **Arguments**, one literal per row (`-u`, `/…/bridge.py`). No shell, no `~`, no `$VAR`. | `T/packages/provider-acp-registry/src/settings.ts:35-51`; `T/apps/web/src/components/settings/ProviderSettingsForm.tsx:216-251`; `T/packages/provider-acp-registry/src/server/AcpRegistrySupport.ts:1873-1876` |
| 4 | Gives the instance a **display name** and instance id. | `T/apps/web/src/components/settings/AddProviderInstanceDialog.tsx:235-241`; contract in `T/packages/contracts/src/providerInstance.ts:75-84` |
| 5 | Optionally adds **environment overrides**: `GATEWAY_URL`, and `GATEWAY_API_KEY` when the gateway has one. The environment is merged over the T3 server's `process.env` for the child. | `D/bridge-acp/docs/t3code-verify.md:58`, `:97-98`; `T/packages/provider-core/src/server/instanceEnvironment.ts:7-31` |
| 6 | Saves. T3 spawns the command when a thread opens; the bridge dies with the session. | `D/bridge-acp/README.md:96-112`; `D/bridge-acp/docs/t3code-verify.md:101-115`, `:380` |

There is **no gateway URL field** in the T3 provider form. The form's own fields are `source`, `agentId`, `commandPath`, `commandArgs`, `authMethodId`, `distribution`, `customModels` (`T/packages/provider-acp-registry/src/settings.ts:7-70`); the URL lives only in the environment list. T3's `apiKey` fields belong to model providers, not to ACP instances (`T/apps/web/src/components/settings/ProviderSettingsForm.test.ts:24-33`).

> T3's own `/pair#token=…` client pairing (`T/apps/web/src/environments/primary/auth.ts:157-164`, `T/apps/web/src/authBootstrap.test.ts:217-222`) pairs a **browser with the T3 server**. It is unrelated to the gateway; conflating the two is the most common source of confusion in this area.

### 1.2 Transport and authentication

```
T3 server ──stdio, newline-delimited ACP JSON-RPC──► bridge.py ──HTTP/SSE──► gateway ──Modal──► sandbox
```

- **T3 ↔ bridge:** ACP JSON-RPC on stdin/stdout, one JSON object per line. T3's ACP client uses an NDJSON codec and splits frames on `\n` itself so a malformed line ends the session (`T/packages/effect-acp/src/protocol.ts:184-210`; child stdio in `T/packages/effect-acp/src/_internal/stdio.ts:13-22`).
- **bridge ↔ gateway:** ordinary HTTP/1.1 JSON plus one SSE stream (`D/bridge-acp/bridge.py:157-185`).
- **Gateway auth:** one shared key. When `GATEWAY_API_KEY` is set, every route except `GET /health` requires exactly `Authorization: Bearer <GATEWAY_API_KEY>`; the whole header is compared in constant time, so a bare key or a wrong scheme is a 401 (`D/gateway/app.py:46-59`). The 401 carries `WWW-Authenticate: Bearer` and `{"detail":"invalid or missing API key"}` (`D/gateway/README.md:36-52`).
- **ACP auth:** the bridge advertises **no** auth methods and answers `authenticate` with `{}` (`D/bridge-acp/bridge.py:359-368`). T3 therefore never asks for a gateway credential; only the env var carries it.
- **No pairing, no per-client identity, no revocation.** Rotating the key breaks every client at once. This is the baseline the owner is comparing against.

### 1.3 Endpoints (and which ones the T3 path actually uses)

| Method | Path | Purpose | Used by T3 path? | Evidence |
|---|---|---|---|---|
| `GET` | `/health` | Liveness; always open; `{ok, provider, auth}` | No | `D/gateway/app.py:186-188` |
| `POST` | `/sessions` | Create or reuse a session by name | **Yes** (`session/new`) | `D/gateway/app.py:191-197`; `D/bridge-acp/bridge.py:144-145` |
| `GET` | `/sessions` | List sessions | No | `D/gateway/app.py:199-201` |
| `GET` | `/sessions/{id}` | Session plus the last 10 runs | No | `D/gateway/app.py:204-209` |
| `POST` | `/sessions/{id}/runs` | Dispatch `{task, wait, timeout_s, queue_timeout_s}` | **Yes** (`session/prompt`) | `D/gateway/app.py:211-242`; `D/bridge-acp/bridge.py:147-155` |
| `GET` | `/runs/{id}` | Status, exit code, final text | **Yes** (`finish`) | `D/gateway/app.py:258-264`; `D/bridge-acp/bridge.py:187` |
| `POST` | `/runs/{id}/events` | Provider callback (Cloud Run) | No | `D/gateway/app.py:267-274` |
| `GET` | `/runs/{id}/events` | JSON event log, `from_index` | No | `D/gateway/app.py:276-279` |
| `GET` | `/runs/{id}/stream` | SSE replay + live, `from_index`, `: keepalive` every 10 s | **Yes** | `D/gateway/app.py:281-323`; `D/bridge-acp/bridge.py:157-185` |
| `POST` | `/sessions/{id}/cancel` | Close the running run and free the session | **No** | `D/gateway/app.py:245-256` |
| `POST` | `/sessions/{id}/files` | Multipart upload (`file`, optional `dest`) | No | `D/gateway/app.py:327-366` |
| `GET` | `/sessions/{id}/files?path=…` | Directory listing | No | `D/gateway/app.py:368-383` |
| `GET` | `/sessions/{id}/files/{path}` | Raw bytes, confined to `/workspace` | No | `D/gateway/app.py:385-411` |

Two drifts worth naming because they affect any parity claim:

- The gateway **has** `POST /sessions/{id}/cancel` (`D/gateway/app.py:245`) but the README's API table omits it (`D/gateway/README.md:61-74`) and the bridge says "the gateway exposes no cancel endpoint" (`D/bridge-acp/bridge.py:682-689`). So cancel exists but is not on the T3 path.
- File transfer exists on the gateway but the bridge refuses ACP `fs/*` and `terminal/*` and never calls the file routes (`D/bridge-acp/README.md:139-147`; `D/bridge-acp/bridge.py:11-15`). The T3 integration therefore exercises **session create, run create, SSE, run get** only.

### 1.4 What each side must store

| Side | What is stored | Where | Evidence |
|---|---|---|---|
| T3 | Instance id, driver `acpRegistry`, display name, `enabled`, environment (`GATEWAY_URL`, optional `GATEWAY_API_KEY`), `source: "local"`, `commandPath`, `commandArgs` | `<baseDir>/userdata/settings.json`, key `providerInstances` | `T/packages/contracts/src/settings.ts:1563-1565`; `D/bridge-acp/docs/t3code-verify.md:27`, `:43-65` |
| T3 | A sensitive env value is a **secret reference**, not the secret; the secret itself is resolved by the server's secret provider | `settings.json` + T3 secret store | `T/packages/contracts/src/providerInstance.ts:32-46`; `T/packages/contracts/src/secretReference.ts:22-24` |
| bridge | ACP id → gateway session id map, so `session/resume` survives a restart | `bridge-acp/.state/sessions.json` | `D/bridge-acp/bridge.py:216-220` |
| gateway | Sessions, runs, event logs | SQLite in `GATEWAY_DATA_DIR` | `D/gateway/store.py:1-3`, `:22-24`; `D/gateway/README.md:7-10` |

There is **no gateway-issued token or cookie on the T3 path**. The static key is re-read from the environment on every request (`D/bridge-acp/bridge.py:100-108`).

---

## 2. goble's gateway today

### 2.1 What it is for

`goble-gateway` is an always-on **worker relay** for sandboxes behind NAT: a worker dials out to `/ws/worker`, a browser or phone dials `/ws/client`, and the gateway copies text/binary frames between them verbatim. It is not a renderer and not a database (`crates/goble-gateway/src/lib.rs:1-16`; `crates/goblin-worker/src/gateway_client.rs:1-14`). Its own roadmap calls it "the seed" of the relay, with passkey/QR pairing planned to replace the PIN (`.agents/11-relay-and-hub/README.md:11-13`, `:60-64`).

### 2.2 Every route and WebSocket endpoint

| Method | Path | Auth | Purpose | Evidence |
|---|---|---|---|---|
| `GET` | `/health` | **None** | `{status:"ok", workers:N}` | `crates/goble-gateway/src/http.rs:57`, `:107-112` |
| `GET` | `/` | Optional | Login page, or redirect to `/app/` when already authenticated | `crates/goble-gateway/src/http.rs:58`, `:114-119` |
| `POST` | `/login` | PIN form | Verify PIN, set `goble_gateway_session` cookie, redirect to `/app/` | `crates/goble-gateway/src/http.rs:59`, `:121-158` |
| `POST` | `/logout` | Session (cookie or bearer) | Destroy the session and clear the cookie | `crates/goble-gateway/src/http.rs:60`, `:160-175` |
| `GET` | `/panel` | Session | HTML "connected workers" + transport console | `crates/goble-gateway/src/http.rs:61`, `:177-186` |
| `GET` | `/api/workers` | Session or bearer | JSON worker list, sorted by id | `crates/goble-gateway/src/http.rs:62`, `:188-189`, `:229-240` |
| `POST` | `/api/v1/session` | PIN JSON | Exchange PIN for `{token, expires_in}`; 401 `{"error":"wrong_pin"}` | `crates/goble-gateway/src/http.rs:63`, `:190-227` |
| `GET` | `/app`, `/app/` | Session | Client entry point | `crates/goble-gateway/src/http.rs:64-65` |
| `GET` | `/app/{*path}` | Session | Client assets, path-traversal safe | `crates/goble-gateway/src/http.rs:66` |
| `GET` | `/ws/worker?id=<id>` | **Worker bearer** | Worker registration; opaque relay | `crates/goble-gateway/src/http.rs:67`; `crates/goble-gateway/src/ws.rs:28-41` |
| `GET` | `/ws/client?worker=<id>` | Session or bearer | Client attach; 404 if the worker is not connected | `crates/goble-gateway/src/http.rs:68`; `crates/goble-gateway/src/ws.rs:78-100` |

The gateway acts on exactly two client frame kinds, `open_pty` / `close_pty`, only to close orphaned panes when a browser tab dies (`crates/goble-gateway/src/ws.rs:208-229`). Every other frame is opaque.

### 2.3 Auth model: three different credentials

| Credential | Who holds it | Check | Lifetime / storage | Evidence |
|---|---|---|---|---|
| **PIN** | A human, typed once | `verify_pin(hash, candidate)`; failures add an increasing delay after 3 misses | Hash only, in the process; 8 characters from a 32-symbol alphabet | `crates/goble-gateway/src/auth.rs:7-26`; `crates/goble-gateway/src/state.rs:319-334` |
| **Session token** | Browser (cookie) or native client (bearer) | A random 256-bit token, looked up with its expiry checked on every use | 12 h; in-memory `HashMap`; `/logout` revokes | `crates/goble-gateway/src/config.rs:7-8`; `crates/goble-gateway/src/state.rs:288-317` |
| **Worker bearer** | Every worker image / deployment | Constant-time compare of `Authorization: Bearer` **or** `X-Worker-Token` against one shared token | Generated at startup and printed once unless `--worker-token-file` | `crates/goble-gateway/src/state.rs:272-277`; `crates/goble-gateway/src/main.rs:96-105` |

Notes that matter for the Settings copy:

- The PIN is **8 characters**, not six digits. `relay-auth.md` says "six-digit" (`.agents/11-relay-and-hub/relay-auth.md:9`), but the generator uses 8 symbols and omits `I/O/0/1` (`crates/goble-gateway/src/auth.rs:20-26`). Quote the code, not the old prose.
- The session token is accepted as a **cookie or a bearer**; that is what lets a native client keep a pairing (`crates/goble-gateway/src/state.rs:288-292`; `docs/mobile-remote.md:24-30`).
- The worker token is **not** a client credential. It authorizes `/ws/worker` only; it cannot call `/api/workers` (the check at `crates/goble-gateway/src/http.rs:229-232` uses `authenticated`, which only looks at session tokens).
- Both `/login` and `/api/v1/session` share the same failure delay and reset-on-success (`crates/goble-gateway/src/http.rs:131-145`, `:200-215`).
- A non-loopback bind refuses to start without a PIN; `--no-auth` is loopback-only (`crates/goble-gateway/src/config.rs:68-80`; `crates/goble-gateway/src/main.rs:74-94`).

### 2.4 How a desktop or browser pairs today

| Client | Flow | Evidence |
|---|---|---|
| Browser | `GET /` → login form → `POST /login` PIN → `Set-Cookie: goble_gateway_session=…; HttpOnly; SameSite=Strict; Max-Age=43200` → `/app/`; the WebSocket `/ws/client` uses the same cookie | `crates/goble-gateway/src/http.rs:121-158`; `crates/goble-gateway/src/http.rs:303-311`; `crates/goble-gateway/src/state.rs:288-293` |
| Phone / native | `GET /health` → `POST /api/v1/session {"pin"}` → `{token, expires_in}` → every request and the WS handshake carry `Authorization: Bearer <token>` | `docs/mobile-remote.md:22-36`; `apps/mobile/lib/remote/gateway.ts:49-100`, `:127-140` |
| Old gateway fallback | If `/api/v1/session` answers 404, the phone posts the PIN to `/login` and keeps the cookie | `apps/mobile/lib/remote/gateway.ts:44-47`, `:104-124` |
| Desktop app | **Nothing.** There is no `goble_gateway` import, no `Gateway::new`, no HTTP call, no `/ws/client` in `app/src` or `crates/goble-desktop-service/src`. The only occurrences are user-facing error strings | `crates/goble-desktop-service/src/state/remote.rs:107`, `:113`, `:277`, `:282` |
| Desktop ↔ worker (a different pairing) | The desktop opens `ws://host:7878/ws` directly and sends `DesktopMessage::PairRequest { worker_id, pairing_code_hash, link_id }`; the worker accepts via mTLS bundle or a stored pairing-code hash and answers `WorkerMessage::Paired` | `crates/goble-desktop-service/src/worker_manager.rs:25-59`; `crates/goblin-worker/src/websocket.rs:377-396`; `crates/goble-core/src/protocol.rs:10-20`, `:178` |

That last row is the source of the naming collision: `PairRequest` is a **desktop ↔ worker** message, not a gateway message. The gateway would forward it opaquely if a desktop reached a worker through the relay, but it never parses it (`crates/goble-gateway/src/ws.rs:43-100`). `goblin-worker` also has an HTTP `POST /pair` with its own `PairRequest` type (`crates/goblin-worker/src/pairing.rs:9-52`) — a third, unrelated use of the same name.

### 2.5 What persists

| State | Persisted? | Where | Evidence |
|---|---|---|---|
| Browser/native sessions | **No** | In-memory `HashMap`, lost on restart | `crates/goble-gateway/src/state.rs:60-70`; `crates/goble-gateway/src/lib.rs:14-16` |
| Worker registrations | **No** | In-memory; a worker reconnects with backoff | `crates/goble-gateway/src/state.rs:155-184`; `crates/goblin-worker/src/gateway_client.rs:188-216` |
| PIN | Only its hash, only for the process lifetime | Derived at startup from `--pin`, `--pin-file`, or generated | `crates/goble-gateway/src/main.rs:74-94` |
| Worker token | Not on disk unless `--worker-token-file` was used | Process memory / operator's secret file | `crates/goble-gateway/src/main.rs:96-105` |
| Gateway URL + worker token on the desktop | **Yes, plaintext** | `~/.goble/config.toml` `[remote]` | `crates/goble-core/src/config/mod.rs:453-470`; `crates/goble-cli/src/setup.rs:188-209` |
| Desktop worker rows | Yes | SQLite `workers` table (`pairing_status`, config JSON) | `crates/goble-core/src/store/workers.rs:14-30`; `crates/goble-core/src/store/schema.rs:23-32` |
| Worker pairing code | Yes, encrypted | Credential vault, key `worker:<id>:pairing_code` | `crates/goble-desktop-service/src/state/workers.rs:93-110` |

The gateway is deliberately not a persistence layer. Any Settings design that promises "last seen" must store it **client-side**, and any run/event history would require new gateway state or worker-side history.

### 2.6 Where the desktop starts or connects to it

- **It does not start it.** The gateway binary is a standalone server (`crates/goble-gateway/src/main.rs:57-153`); `crates/goble-gateway/src/lib.rs:10-11` says it "can be … spawned by the desktop application", but no code in `app/`, `goble-desktop-native`, `goble-desktop-service`, or `goble-cli` spawns it (`grep -rn "goble-gateway"` over those trees returns only docs and error strings).
- **It does not dial it.** The desktop's remote path only packages `GOBLE_GATEWAY_URL` and `GOBLE_WORKER_TOKEN` into a sandbox request so the **worker** dials the gateway (`crates/goble-desktop-service/src/state/remote.rs:30-37`, `:122-137`; `crates/goblin-worker/src/gateway_client.rs:29-39`).
- **It talks to workers directly.** `WorkerClient::connect` builds `config.websocket_url()` (`ws://host:7878/ws`, or `wss://` with an mTLS bundle) and sends `PairRequest` (`crates/goble-desktop-service/src/worker_manager.rs:25-59`; `crates/goble-core/src/worker.rs:45`, `:84-91`).

So today there are **two independent client topologies**, plus a deploy path:

```
browser / phone ──► goble-gateway ──► worker        (human clients)
desktop        ──► worker:7878/ws                  (direct, mTLS or pairing code)
desktop        ──► sandbox provider ──► worker ──► gateway  (deploy only; desktop never speaks to it)
```

A Gateway page is the first desktop feature that would put the desktop in the first topology.

---

## 3. Method-by-method comparison

Legend for **Gap owner**: `GW` = gateway crate, `DS` = desktop service, `UI` = app client, `W` = worker/protocol, `—` = already works.

| Capability | dsh-remote | goble today | Verdict | Gap owner |
|---|---|---|---|---|
| **Pair / auth** | One static `GATEWAY_API_KEY`; bearer on every route but `/health`; no per-client session, no revoke (`D/gateway/app.py:46-59`). | PIN → HttpOnly cookie (browser) or `{token, expires_in}` (native), 12 h, in-memory, revocable by `/logout`; shared worker bearer for machines (`crates/goble-gateway/src/http.rs:121-227`; `crates/goble-gateway/src/state.rs:288-317`). | **Different shape, goble stricter.** dsh has no pairing UI because it has no pairing. | DS + UI for the desktop client; GW already has the endpoint |
| **List workers** | `GET /sessions` lists agent sessions (`D/gateway/app.py:199-201`). | `GET /api/workers` lists connected workers with `id`, `connected_at`, `last_seen` (`crates/goble-gateway/src/http.rs:229-240`; `crates/goble-gateway/src/state.rs:128-140`). | **Different noun, equivalent discovery.** | DS + UI |
| **Open session** | `POST /sessions {name}` creates or reuses a durable binding; `GET /sessions/{id}` adds runs (`D/gateway/app.py:191-209`). | No HTTP. `GET /ws/client?worker=` attaches to a worker; `open_pty` starts a pane (`crates/goble-gateway/src/ws.rs:78-100`; `crates/goble-web/src/protocol.rs:50-72`). | **Different shape.** Worker owns the thread; gateway owns no session. | UI/DS (attach through WS); GW unchanged |
| **Run prompt** | `POST /sessions/{id}/runs {task, timeout_s}`; queueing via `queue_timeout_s` (`D/gateway/app.py:211-242`). | No HTTP. Relays `run_agent` / `run_agent_for_thread_reply` frames; the worker's runner executes (`crates/goble-core/src/protocol.rs:20-70`; `crates/goble-web/src/protocol.rs:66-77`). | **Different shape.** Same outcome, no run resource. | UI/DS + protocol; GW unchanged |
| **Stream events** | `GET /runs/{id}/stream` replays from `from_index` and stays live with `: keepalive`; `GET /runs/{id}/events` is the log (`D/gateway/app.py:276-323`). | Worker frames `assistant_delta`, `agent_finished`, `tool_call_*` are fanned out to every attached client; no replay, no log (`crates/goble-gateway/src/state.rs:212-238`; `crates/goble-web/src/protocol.rs:82-160`). | **Missing replay.** Live streaming works. | GW for replay/log; UI for live |
| **Cancel** | `POST /sessions/{id}/cancel` closes the running run (`D/gateway/app.py:245-256`), though the bridge does not call it (`D/bridge-acp/bridge.py:682-689`). | No cancel-run message; only `cancel_scheduled_task` (`crates/goble-core/src/protocol.rs:42-44`). The gateway cannot synthesize one because frames are opaque. | **Missing.** | W (protocol + worker); GW unchanged |
| **Files** | Upload / list / download under `/workspace`, 400/403/409/501 rules (`D/gateway/README.md:98-128`; `D/gateway/app.py:327-411`). | None. The worker's HTTP surface is `/`, `/health`, `/platform`, `/mcp`, `/pair`, `/status`, `/ws`; there is no file route (`crates/goblin-worker/src/main.rs:235-243`). | **Missing.** | GW + W (new API) |
| **Health** | `GET /health` → `{ok, provider, auth}`; always open (`D/gateway/app.py:186-188`). | `GET /health` → `{status:"ok", workers:N}`; always open (`crates/goble-gateway/src/http.rs:107-112`). | **Supported, different shape.** | UI adapts; GW unchanged |

Three consequences for any "parity" plan:

1. The only places where goble is genuinely **behind dsh** on the wire are replay/log, cancel, and files. None of them is a Settings-page item; each needs worker or gateway work.
2. The places where goble is **ahead** are human pairing (PIN → revocable session), worker discovery, and multi-client fan-out. A Settings page should expose those, not hide them behind a dsh-style API key.
3. The desktop is the **only missing client**. Browser and phone already implement the full pairing flow and the relay protocol (`docs/mobile-remote.md:22-37`; `apps/mobile/lib/remote/gateway.ts:49-140`).

---

## 4. Verdict on the owner's sentence

> "gobble's gateway should work exactly like the dsh-remote gateway we linked to T3, and Settings should have the pairing option and everything needed."

**What already matches.** Machine authentication with one bearer secret (dsh `GATEWAY_API_KEY`, goble worker token), an always-open `/health`, a machine-readable list endpoint, constant-time comparison, and a client that dials out rather than requiring an inbound port. goble additionally has a human pairing exchange that dsh does not: `POST /api/v1/session` returning a 12-hour bearer for the same session a browser gets as a cookie (`crates/goble-gateway/src/http.rs:190-227`; `crates/goble-gateway/src/state.rs:288-317`). The browser and phone already use it (`docs/mobile-remote.md:22-36`).

**What does not match.** (a) The desktop has no gateway client and no Settings surface for it; (b) the gateway has no sessions, runs, event log, SSE replay, cancel, or file API; (c) the desktop's only gateway-related state is a plaintext worker token in `config.toml`, which is a machine credential, not a user pairing; (d) the gateway is in-memory, so "last seen" and worker history have nowhere to live server-side; (e) T3's actual integration is not a gateway form at all — it is a local ACP command plus env vars, with a separate bridge process.

**What "exactly like" can mean.** The same **user contract**: one page that names a gateway, takes one credential, tests reachability, pairs, lists workers, shows state and last seen, unpairs, and survives restart, with the credential stored encrypted and never echoed. That is implementable entirely in `goble-desktop-service` + `app`, because the gateway already has `/health`, `/api/v1/session`, `/api/workers`, `/logout`, and a bearer-accepting `/ws/client`.

**What it cannot mean.** It cannot mean protocol equality. dsh is an execution control plane (sessions, runs, SSE, files, SQLite), and T3 reaches it through a **separate ACP bridge process**; goble is an opaque relay and its execution lives in the worker. Copying T3's form literally would produce the wrong UI (a command + args + env form), and copying dsh's API literally would require turning the relay into a control plane — a much larger roadmap item, and one that conflicts with the relay's stated design ("carries bytes and routes by id; it is not a renderer and not a database", `.agents/11-relay-and-hub/README.md:11-13`).

**If "linked to T3" is the actual goal.** Then the deliverable is a goble **ACP bridge** — a stdio adapter that translates ACP to the relay/worker protocol — registered in T3 as a Local ACP command, exactly the way `D/bridge-acp/bridge.py` is. T3 would then list goble workers as an agent, and no goble Settings page would be involved. That is a different project from G1–G9; doing both is two projects. The owner should say which sentence they mean.

**Plain recommendation.** Build the Gateway page and the desktop client (G1–G9). Do **not** claim run/file parity until the worker and gateway grow those APIs; say "pairing and worker discovery parity" instead.

---

## 5. Proposed Settings design: Settings → Gateway

### 5.1 Where it sits

Add a new `SettingsCategory::Gateway` after `Connections` and before `Models` in the rail order, label **Gateway**. The page follows the existing flat-row settings language: a heading per group, `setting_row` / `field_row`, a hairline rule between rows, `note` for help text, and a keyboard focus order produced by `pane_controls` (`app/src/ui/types.rs:330-365`; `app/src/ui/settings.rs:124-180`, `:623-720`; doc style in `.agents/06-renderer/ui-spec/10-settings-tab.md:1-10`).

The page has three groups:

```
Gateway
  Paired · https://gateway.example.ts.net · 2 workers · expires in 11h 04m

  Connection
    Server URL   [https://gateway.example.ts.net          ]
    Name         [Home gateway                            ]
    PIN          [••••••••]        (only while unpaired)
    [ Test ]  [ Pair ]  [ Unpair ]  [ Refresh ]
    Last seen    Today 10:04 · 2 workers

  Workers
    worker-a     connected 10:02 · last frame 10:04
    worker-b     connected 09:31 · last frame 10:03
    (or) No workers connected. Start a worker with GOBLE_GATEWAY_URL and
         GOBLE_WORKER_TOKEN, then refresh.

  Deploy
    Worker token [set · change in config.toml]   (machine credential, not the PIN)
    Remote agents …                              (existing rows move here, or stay)
```

### 5.2 Fields

| Field | Control | Required | Secret | Behaviour |
|---|---|---|---|---|
| **Server URL** | text | yes | no | Accepts `https://host[:port][/prefix]`, `http://127.0.0.1:8788`, or a pasted `wss://host[:port][/prefix]/ws/worker`. A path prefix is kept; a trailing `/ws/worker` is stripped. HTTP calls use `<base>`, the socket uses `<base>/ws/client`. Reject anything that is not http(s)/ws(s), and reject query/fragment. |
| **Name** | text | no | no | Display label; defaults to the URL's host. T3's `displayName`/instance-id pair is the analog (`T/packages/contracts/src/providerInstance.ts:75-84`). |
| **PIN** | password | only when unpaired | **yes** | Typed for Pair, never persisted, cleared on success and on leaving the page. The gateway stores only a hash (`crates/goble-gateway/src/auth.rs:5-16`). |
| **Worker token** | masked, read-only here | no | **yes** | The `[remote]` deploy credential. It is shown as "set/not set" and edited in `config.toml`; it is **not** a client credential and cannot list workers (`crates/goble-gateway/src/http.rs:229-232`). |

Do not add an "API key" field. goble has no gateway-wide API key; that is dsh's model. The two credentials are the **PIN → session** and the **worker token**, and they belong to different people.

### 5.3 Actions

| Action | What it does | Enabled when | Evidence / API |
|---|---|---|---|
| **Test** | `GET /health` with a 10 s timeout; show status, worker count, and the URL used. Does not mutate pairing. | URL parses | `crates/goble-gateway/src/http.rs:107-112` |
| **Pair** | `POST /api/v1/session {"pin"}` → store token + expiry; then `GET /api/workers` to populate the list. | URL parses, PIN non-empty, not already paired | `crates/goble-gateway/src/http.rs:190-227`; `:229-240` |
| **Unpair** | `POST /logout` with the bearer (best effort), then clear the token, expiry, and worker list locally. | paired | `crates/goble-gateway/src/http.rs:160-175` |
| **Refresh** | `GET /api/workers`; update `last_seen` and the list; flip to "expired" on 401. | paired | `crates/goble-gateway/src/http.rs:229-240` |
| **Use worker** (optional, G8) | Attach this desktop to the selected worker through `ws(s)://host/ws/client?worker=<id>` with `Authorization: Bearer <token>`. | paired, worker selected | `crates/goble-gateway/src/state.rs:288-292`; `crates/goble-gateway/src/ws.rs:78-100` |

### 5.4 States

| State | Drawn as | Entered by |
|---|---|---|
| `Idle` | Empty fields, Pair disabled, note "Point this at a goble-gateway to see its workers." | first open, or after Unpair |
| `Editing` | Fields hold drafts; Test/Pair enabled as validity allows | any field change |
| `Testing` | Test button reads "Testing…", other buttons disabled | Test |
| `Pairing` | Pair button reads "Pairing…", PIN field frozen | Pair |
| `Paired` | Status line with host, worker count, expiry; PIN hidden; Unpair enabled | successful Pair or restore |
| `PairedEmpty` | `Paired` plus "No workers connected" and the env-var hint | workers list is empty |
| `Expired` | "Session expired · pair again"; PIN field returns; Unpair still available (local clear) | 401 on refresh, or expiry reached |
| `Error` | Status line with the message; fields keep their values; Test/Pair stay enabled | any failed action |
| `VaultLocked` | "Unlock the vault to keep this pairing"; Pair can run in memory-only mode | Pair while vault is locked |

The page must never draw a paired state from a token alone: a token whose expiry has passed is `Expired`, even before the first request.

### 5.5 What is persisted, and where

| Value | Where | Why there | Evidence / precedent |
|---|---|---|---|
| `gateway.url` | store setting (SQLite) | not secret; survives restart | `crates/goble-core/src/store/settings.rs:48-60`; existing `set_setting` use in `crates/goble-desktop-service/src/state/settings.rs:26-36` |
| `gateway.name` | store setting | not secret | same |
| `gateway.session_expires_at` | store setting (RFC 3339) | lets the UI say "expired" without a request | same |
| `gateway.last_seen` | store setting (RFC 3339) | client-side fact; the gateway keeps no history | same |
| `gateway.session` | **vault** (`DesktopState::set_vault_secret`) | bearer secret at rest is encrypted with the user's passphrase | `crates/goble-desktop-service/src/state/vault.rs:11-28`; vault blob in `crates/goble-desktop-service/src/state/load.rs:140` |
| PIN | **nowhere** | only needed to mint the session | `crates/goble-gateway/src/auth.rs:5-16` |

Do **not** put the session token in `[remote]` / `config.toml`. `[remote].worker_token` is the deploy credential and is already plaintext (`crates/goble-core/src/config/mod.rs:453-470`); mixing a short-lived client token into it would make two different lifetimes share one key and one file. If the vault is locked, Pair should still work but mark the pairing "until quit" rather than write plaintext.

### 5.6 Exact user flow

1. User opens **Settings → Gateway**. The page reads `gateway.url` / `gateway.name` from the store. With none, it shows `Idle`.
2. User types `https://gateway.example.ts.net` (or pastes `wss://gateway.example.ts.net/ws/worker`; the page shows the normalised form). `Name` defaults to `gateway.example.ts.net`.
3. User presses **Test**. The page shows `Testing…`, calls `GET /health`, then shows `ok · 2 workers · https://…` or an error. Nothing is saved yet.
4. User types the PIN and presses **Pair**. The page shows `Pairing…`, calls `POST /api/v1/session`, receives `{token, expires_in}`, stores the token in the vault (or memory when locked), writes URL/name/expiry/last-seen to the store, then calls `GET /api/workers`.
5. The page draws `Paired · 2 workers · expires in 11h 04m` and the worker rows, each with `id`, `connected_at`, and `last_seen`. If the list is empty, it draws `PairedEmpty` with the env-var hint.
6. **Refresh** repeats `GET /api/workers` and updates `last_seen`. A 401 flips to `Expired` and keeps the URL/name so re-pairing is one PIN entry.
7. **Unpair** calls `POST /logout` with the bearer; clears the vault key, the store keys, the worker list, and the PIN draft; returns to `Idle`. If the gateway is unreachable, it clears locally and says the server session will expire within 12 h.
8. On restart, the desktop reads URL/name/expiry from the store and the token from the vault. A non-expired token triggers one silent `GET /api/workers`; success draws `Paired`, 401 draws `Expired`. A locked vault draws `VaultLocked` and does not call the network.

### 5.7 Failure and empty states

| Trigger | State | Copy (draft) | Recovery |
|---|---|---|---|
| URL empty / unparseable | `Error` | "Enter the gateway's origin, such as https://gateway.example.ts.net:8788." | edit URL |
| `GET /health` connection refused / timeout | `Error` | "No answer from gateway.example.ts.net. Check the URL and that goble-gateway is running." | Test again |
| `GET /health` non-200 | `Error` | "The gateway answered 502 while checking health." | Retry |
| `POST /api/v1/session` 401 `wrong_pin` | `Error` | "That PIN was refused." | re-enter PIN |
| `POST /api/v1/session` 404 | `Error` | "This gateway build has no pairing endpoint (404). Upgrade goble-gateway or use the browser panel." | cannot pair; Test still works |
| `POST /api/v1/session` 5xx | `Error` | "The gateway answered 503 while pairing." | Retry |
| `GET /api/workers` 401 after pairing | `Expired` | "Session expired. Pair again with the current PIN." | Pair |
| `GET /api/workers` 200 `[]` | `PairedEmpty` | "Paired. No workers are connected yet." + `GOBLE_GATEWAY_URL`/`GOBLE_WORKER_TOKEN` hint | start a worker, Refresh |
| Vault locked at Pair | `VaultLocked` | "Unlock the vault to keep this pairing; otherwise it lasts until you quit." | unlock, or accept memory-only |
| Vault locked at Unpair | `Error` | "Unlock the vault to forget the stored session." | unlock |
| Token expired locally (clock) | `Expired` | "This pairing expired. Pair again." | Pair |
| Gateway unreachable at Unpair | `Idle` + note | "Forgot this gateway locally. Its session expires within 12 hours." | none |

### 5.8 Non-goals (v1)

- No run/prompt composer on this page. Running work stays in the worker's normal surface; the page only pairs and lists.
- No gateway-initiated push, no background polling while the page is closed.
- No multiple gateways in v1 (T3's `providerInstances` is a map, so multi is a legitimate later ask — see §8).
- No file browser, no run history, no cancel button. Those require the server work in §3.

---

## 6. Discrete implementation items

Summary:

| ID | Title | Side | Depends on |
|---|---|---|---|
| G1 | Gateway v1 pairing contract and tests | gateway crate | — |
| G2 | Desktop-side gateway HTTP client | desktop service | G1 |
| G3 | `DesktopState` pairing state and persistence | desktop service | G2 |
| G4 | Settings category and idle Gateway page | app UI | — |
| G5 | App state, edits, and action wiring | app UI | G3, G4 |
| G6 | Worker list, refresh, and empty state | app UI + desktop service | G5 |
| G7 | Restore, expiry, and unpair states | desktop service + app UI | G6 |
| G8 | Use the gateway as a worker transport | desktop service (+ UI toggle) | G3 |
| G9 | Deploy credential row and documentation | app UI + docs | G7 |

Everything below is verified by tests that run **in this container**; the column "Proof" names the command. Anything that cannot be proved here is called out per item and repeated in §7.

### G1 — Gateway v1 pairing contract and tests

- **Side:** gateway crate.
- **Files:** `crates/goble-gateway/tests/gateway.rs`; `crates/goble-gateway/src/http.rs`.
- **Change:** correct the stale doc comment that promises `/api/v1/workers` while the router registers `/api/workers` (`crates/goble-gateway/src/http.rs:190-195` vs `:62`). Add tests for `POST /api/v1/session`: success returns a non-empty `token` and `expires_in`, `Cache-Control: no-store`; wrong PIN is 401 `{"error":"wrong_pin"}`; a no-auth gateway still returns a token; the token works as a bearer on `/api/workers` and on the `/ws/client` handshake; `POST /logout` with the bearer invalidates it.
- **Acceptance:** `cargo test -p goble-gateway --test gateway` passes with ≥ 27 tests (the current suite is 22, all green — see §7); no route is added or renamed; the only source change outside tests is the comment.
- **Proof:** `cargo test -p goble-gateway --test gateway`.
- **Container note:** the suite already starts an in-process axum server on a random loopback port (`crates/goble-gateway/tests/gateway.rs:35-66`), so no external network is needed. TLS, Tailscale, and a real phone are not tested.

### G2 — Desktop-side gateway HTTP client

- **Side:** desktop service.
- **Files:** new `crates/goble-desktop-service/src/gateway_client.rs`; `crates/goble-desktop-service/src/lib.rs` (`pub mod gateway_client;`); `crates/goble-desktop-service/Cargo.toml` (`reqwest = { workspace = true }`).
- **Change:** a small typed client with `normalize_base` (accept `http(s)://host[:port][/prefix]`, `ws(s)://host[:port][/prefix]/ws/worker`; strip a trailing `/ws/worker`, query, and fragment; reject everything else), a 10 s timeout, and four calls: `health()`, `pair(pin) -> Session { token, expires_in }`, `workers()`, `logout(token)`. Errors: `InvalidUrl`, `Unreachable`, `WrongPin`, `Unauthorized`, `NoPairingEndpoint`, `Http(u16)`, `Decode`. Never log the token or PIN; redact the `Authorization` header in errors.
- **Acceptance:** tests against an in-process axum fixture cover: normal pair + list; wrong PIN → `WrongPin`; 404 → `NoPairingEndpoint`; 401 on list → `Unauthorized`; 500 → `Http(500)`; timeout → `Unreachable`; URL normalisation table (including `wss://…/ws/worker` → `https://…` and a path-prefixed base).
- **Proof:** `cargo test -p goble-desktop-service gateway_client`.
- **Container note:** fixture is axum on loopback; no real gateway.

### G3 — `DesktopState` pairing state and persistence

- **Side:** desktop service.
- **Files:** new `crates/goble-desktop-service/src/state/gateway.rs`; `crates/goble-desktop-service/src/state/mod.rs`; `crates/goble-desktop-service/src/state/types.rs`; `crates/goble-desktop-service/src/state/load.rs`.
- **Change:** `GatewayConnection { name, url, paired, expires_at, last_seen, workers }`; `gateway_test`, `gateway_pair(name, url, pin)`, `gateway_unpair`, `gateway_refresh`; store keys `gateway.url`, `gateway.name`, `gateway.session_expires_at`, `gateway.last_seen`; token in the vault under `gateway.session`; emit `gateway:updated`; restore on startup; treat a token past `expires_at` as unpaired without a request.
- **Acceptance:** pair writes URL/name/expiry to the store and an encrypted token to the vault; a fresh `DesktopState` restores the pair; expiry hides the token without a network call; unpair clears both stores; a locked vault yields memory-only pairing and does not write the token.
- **Proof:** `cargo test -p goble-desktop-service gateway`.
- **Container note:** in-memory SQLite + loopback fixture; the vault is exercised with a test passphrase.

### G4 — Settings category and idle Gateway page

- **Side:** app UI.
- **Files:** `app/src/ui/types.rs` (`SettingsCategory`, `SettingsControl`, `is_text_field`), `app/src/ui/settings.rs` (`pane_controls`, `build_gateway`, dispatch), `app/src/state/ui_state/mod.rs` (fields), `app/src/state/ui_state/settings.rs` (snapshot helpers), `app/src/ui/snapshot.rs`, `app/src/root_view/rebuild.rs`, `app/src/state/ui_state/{refresh,mock}.rs`.
- **Change:** `SettingsCategory::Gateway` in `ALL` after `Connections`; controls `GatewayUrl`, `GatewayName`, `GatewayPin`, `GatewayTest`, `GatewayPair`, `GatewayUnpair`, `GatewayRefresh`, `GatewayWorker(String)`; `pane_controls` returns them in draw order; `build_gateway` draws the groups in §5.1 with the idle copy; the three text controls are `is_text_field`.
- **Acceptance:** the rail contains "Gateway"; a drawn frame contains "Gateway", "Server URL", "PIN", "Test", "Pair", "Unpair"; the focus order is exactly the list above; `Esc` leaves the page without writing anything.
- **Proof:** `cargo test -p goble-app settings_gateway` (new tests in `app/src/ui/settings_category_tests.rs`, using the existing `frame` + `text_index` pattern (`app/src/ui/settings_category_tests.rs:79-135`; `app/src/integration_testing/ui_render.rs:773`, `:1878-1905`)).
- **Container note:** drawn-command tests only; no GUI/display.

### G5 — App state, edits, and action wiring

- **Side:** app UI.
- **Files:** `app/src/ui/actions.rs` (new callbacks), `app/src/actions/make_actions.rs`, `app/src/state/ui_state/settings.rs`, `app/src/state/ui_state/mod.rs`, `app/src/ui/snapshot.rs`, `app/src/root_view/rebuild.rs`.
- **Change:** fields `gateway_url`, `gateway_name`, `gateway_pin`, `gateway_state` (`Idle/Editing/Testing/Pairing/Paired/Expired/Error`), `gateway_workers`, `gateway_error`; text-edit and focus handlers; Test calls `desktop.gateway_test`, Pair calls `gateway_pair`, Unpair calls `gateway_unpair`; buttons disabled while a call is in flight; PIN cleared on success.
- **Acceptance:** typed URL/name/PIN survive a per-frame rebuild; Test transitions Testing → Idle/Error; Pair is disabled with an empty URL or PIN; a second click while in flight is a no-op; the PIN never appears in the snapshot after a successful pair.
- **Proof:** `cargo test -p goble-app settings_gateway`.
- **Container note:** the HTTP side is faked behind a `DesktopState` test double; G2/G3 cover the real client.

### G6 — Worker list, refresh, and empty state

- **Side:** app UI + desktop service.
- **Files:** `app/src/ui/settings.rs`, `app/src/state/ui_state/settings.rs`, `crates/goble-desktop-service/src/state/gateway.rs`.
- **Change:** render `GatewayWorker` rows with `id`, `connected_at`, `last_seen`; Refresh reloads; empty list draws the `PairedEmpty` copy; a 401 flips to `Expired`; the selected worker is stored as `gateway.worker` (non-secret).
- **Acceptance:** a fake two-worker list draws both ids and the relative times; `[]` draws "No workers are connected yet."; 401 draws "Session expired."; Refresh does not duplicate rows.
- **Proof:** `cargo test -p goble-app settings_gateway` + `cargo test -p goble-desktop-service gateway`.
- **Container note:** `last_seen` is updated only when a worker frame is relayed (`crates/goble-gateway/src/state.rs:212-224`), so the UI must label it "last frame", not "healthy". A connected but idle worker can have an old `last_seen`.

### G7 — Restore, expiry, and unpair states

- **Side:** desktop service + app UI.
- **Files:** `crates/goble-desktop-service/src/state/gateway.rs`, `crates/goble-desktop-service/src/state/load.rs`, `app/src/ui/settings.rs`, `app/src/state/ui_state/settings.rs`.
- **Change:** restore on launch; silent `GET /api/workers` only when the token is not expired; `VaultLocked` state; Unpair calls `/logout` best effort, clears local state even when offline, and explains the 12 h server-side tail; expiry text updates without a request.
- **Acceptance:** restart with a valid token shows `Paired` after one refresh; restart with an expired token shows `Expired` with no request; Unpair online calls `/logout`; Unpair offline clears locally and says so; Unpair with a locked vault shows the unlock copy and changes nothing.
- **Proof:** `cargo test -p goble-desktop-service gateway` + `cargo test -p goble-app settings_gateway`.
- **Container note:** the "restart" is a second `DesktopState` over the same temp store; no process restart is required.

### G8 — Use the gateway as a worker transport

- **Side:** desktop service (+ optional UI toggle).
- **Files:** `crates/goble-desktop-service/src/worker_manager.rs`, `crates/goble-desktop-service/src/state/workers.rs`, `crates/goble-desktop-service/src/state/types.rs`, `app/src/ui/settings.rs` (the "Use worker" row).
- **Change:** add `WorkerTransport::{Direct, Gateway}` to the worker config; for `Gateway`, build `ws(s)://gateway/ws/client?worker=<id>` and send `Authorization: Bearer <session token>` on the handshake; keep `PairRequest` unchanged (the worker still sees the same protocol); reconnect reuses the mode. Direct remains the default so no existing behavior changes.
- **Acceptance:** a fake gateway WS asserts the bearer header, accepts the socket, relays one text frame to the worker, and rejects the handshake without the header; a direct-mode test is unchanged; a gateway 404 ("worker not connected") surfaces as a typed error.
- **Proof:** `cargo test -p goble-desktop-service gateway_transport`.
- **Container note:** loopback tungstenite/axum only; Tailscale/TLS and a real NAT'd sandbox are not testable here.
- **Why it is separate:** without G8 the page can pair and list but cannot *use* the pairing for desktop work; with it, the desktop joins the browser/phone topology.

### G9 — Deploy credential row and documentation

- **Side:** app UI + docs.
- **Files:** `app/src/ui/settings.rs`, `crates/goble-core/assets/user-guide/06-remote-access.md`, `crates/goble-core/assets/user-guide/16-configuration.md`, `docs/mobile-remote.md`, `.agents/11-relay-and-hub/README.md`.
- **Change:** show the `[remote]` worker token on the Gateway page as a masked "Deploy" row with a pointer to `config.toml`; do not move it to the vault in this item. Fix the mobile client comment that says `/api/v1/workers` where the code serves `/api/workers` (`apps/mobile/lib/remote/gateway.ts:3`, call site at `:136`), and the "six-digit PIN" claim (`.agents/11-relay-and-hub/relay-auth.md:9` vs `crates/goble-gateway/src/auth.rs:20-26`). Document the two credentials explicitly: PIN → session for humans; worker token → `/ws/worker` for machines.
- **Acceptance:** drawn frame shows a masked "Worker token · set" row and never the value; the user guide and mobile doc name `/api/workers`; no doc says six-digit without noting the 8-character code.
- **Proof:** `cargo test -p goble-app settings_gateway`; `git grep -n "api/v1/workers"` returns no stale references.
- **Container note:** docs are checked by grep, not rendered.

---

## 7. What was verified, and what cannot be

**Ran here:** `cargo test -p goble-gateway --test gateway` → **22 passed, 0 failed** (recorded in this session). That suite starts a real in-process axum server and real tungstenite sockets on loopback (`crates/goble-gateway/tests/gateway.rs:35-66`), so it proves the relay/auth/worker-list behavior the Settings page will consume. It does **not** cover `POST /api/v1/session`: none of the 22 test names touches it, and a grep of the test file finds no `api/v1/session` (only `SESSION_COOKIE` parsing at `crates/goble-gateway/tests/gateway.rs:104-108`). That is why G1 exists.

**Read, not run:**

- **T3 Code checkout.** No `node_modules`; the dsh-remote verification records the Node-version and install blockers it hit (`D/bridge-acp/docs/t3code-verify.md:297-306`). All T3 claims here are source claims, not runtime claims.
- **dsh-remote.** Python and Modal are not invoked. The endpoint and bridge claims are read from `D/gateway/app.py` and `D/bridge-acp/bridge.py`.
- **gobble app UI.** No display/GUI in this container. G4–G7 must be proved with drawn-command tests (`frame`, `text_index`), not screenshots.
- **A real gateway on the network.** No Tailscale/remote host here. TLS termination, `--public-url`, the `Secure` cookie path (`crates/goble-gateway/src/http.rs:288-312`), and a real phone are unverified.
- **Modal / a real sandbox worker.** No run, no file transfer, no mTLS worker bundle.

**How to prove the unverifiable later:**

1. G1/G2/G3: in-process axum + temp SQLite + test vault passphrase (already the repo's pattern).
2. G4–G7: `cargo test -p goble-app settings_gateway` with drawn commands; add one test per state in §5.4.
3. G8: a fake gateway WebSocket plus a fake worker, both loopback; assert headers and one relayed frame.
4. Real end-to-end: run `goble-gateway --bind 127.0.0.1:8788 --pin …`, start `goblin-worker` with `GOBLE_GATEWAY_URL=ws://127.0.0.1:8788/ws/worker` and the printed token, then pair the desktop and open a pane.
5. T3 end-to-end: if the owner later wants a goble ACP bridge, the dsh-remote §7 recipe is the template (`D/bridge-acp/docs/t3code-verify.md:333-376`).

---

## 8. Open questions for the owner

1. **Which sentence is the goal: the desktop pairs to a gateway, or T3 lists goble workers?** The first is G1–G9 (a Settings page and a desktop client). The second is an ACP bridge registered in T3, like dsh's (`D/bridge-acp/README.md:1-12`), and needs no goble Settings. The design above assumes the first; G8 is the dividing line between "list" and "use".
2. **One gateway or many?** T3 keeps a map of provider instances (`T/packages/contracts/src/settings.ts:1563-1565`); goble's `[remote]` is a single section. v1 here is one active gateway. If "exactly like T3" includes several, the store keys become a map and the page grows a picker.
3. **Should the worker token ever move to the vault?** Today it is plaintext in `config.toml` (`crates/goble-core/src/config/mod.rs:453-470`). Moving it changes `goble-cli setup gateway` and the sandbox deploy path, so it is deliberately out of G1–G9.
4. **Is unpair expected to revoke server-side?** The gateway supports `POST /logout` by bearer and the session is in memory, but a gateway restart already invalidates every session. Confirm that "forget locally + wait out 12 h" is acceptable when offline.
5. **Does the owner want the dsh-style execution surface too** (runs, SSE replay, cancel, files)? That is a gateway + worker roadmap, not a Settings page, and it cuts against the relay's "carries bytes, not a database" boundary (`.agents/11-relay-and-hub/README.md:11-13`).
6. **Should the desktop's direct `ws://worker:7878/ws` path stay the default after G8?** Keeping it avoids a regression for LAN/mTLS users; making gateway the default is a product decision with a migration cost.
7. **PIN lifetime and rotation.** The gateway prints one PIN at startup and keeps only its hash (`crates/goble-gateway/src/main.rs:74-94`). Should Settings offer "rotate PIN" by restarting the gateway, or is that an operator/CLI concern?
