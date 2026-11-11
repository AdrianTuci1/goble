# Hub — automation plane

**Status:** `[~]` core library landed (`crates/goble-hub`); scheduler/API wiring pending
**Owns:** turning events into runs on workers, and recording what happened
**Depends on:** [`README.md`](README.md), [`relay-auth.md`](relay-auth.md), [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md)

## What it is

Paseo Hub's shape is the right one: connect the services where work arrives, dispatch to daemons on
machines the user already owns, keep configuration in version control, and record one audit trail.
Goble's version adds the thing Paseo does not have: every dispatched run is a harness session with a
per-turn ledger, so the hub can show what happened, fork a run, replay it, or revert it.

The hub is a **dispatcher and a record**, not an executor. Execution stays in `goblin-worker` /
the embedded daemon; the hub holds no model key and runs no agent.

## Entities

| Entity | Meaning |
|---|---|
| `Trigger` | What starts work: a webhook source, a schedule, a manual run, an MCP call |
| `Workflow` | A named, versioned sequence of steps and its target policy |
| `WorkflowRevision` | An immutable revision: config hash + file path + author + deployed_at |
| `Run` | One execution of a revision: inputs, target, lease, state, outputs |
| `Lease` | The worker a run is bound to, with a deadline and heartbeats |
| `Approval` | A gate a step must pass before it may write or spend |
| `AuditEvent` | Append-only, signed record of every trigger, revision, run, approval and result |

## Intake

| Source | Authentication | Notes |
|---|---|---|
| GitHub | HMAC-SHA256 `X-Hub-Signature-256` with the app secret | issues, PRs, comments, checks, `workflow_dispatch` |
| Slack | `X-Slack-Signature` + timestamp window | slash commands, mentions; Socket Mode avoids a public URL |
| Linear | webhook signing secret | issue create/update, comments |
| Discord | interaction signature (Ed25519) | slash commands |
| Schedule | hub's own cron, per workflow and timezone | one run per occurrence, no catch-up storms |
| HTTP | `Authorization: Bearer <scoped token>` | scripted runs and CI |
| MCP | the hub's MCP server, device-authenticated | an agent can start a run with an explicit grant |

Every webhook is verified before it is parsed for routing. Replayed deliveries are deduplicated by
source + delivery id for 24 hours. A trigger may bind to one workflow only; fan-out is a workflow
step, not a trigger property.

## Workflows and revisions

Workflows live in the repository, under `.goble/workflows/*.toml`, and are deployed by path:

```toml
[workflow]
name = "triage-issue"
on = "github.issues.opened"

[target]
# A tag policy, not a hostname: the hub picks the first healthy worker that matches.
tags = ["builder", "linux"]
sandbox = "modal"          # or "e2b", "daytona", "docker", "ssh", "none"
timeout_secs = 1800

[steps.triage]
prompt = "Read the issue and name the riskiest change."
grants = "ReadOnly"

[steps.fix]
prompt = "Implement the fix and open a draft PR."
grants = "ReadWrite"
needs = ["triage"]
approval = "always"        # a human approves before this step runs
```

- A deploy validates the file, resolves every `secret_ref` (never a value), checks the target tags
  exist, and stores the revision hash. The deployed revision is what runs; editing the file does not
  change an in-flight or future run until it is deployed again.
- A run records the revision hash, so "what ran" is reproducible even after the file changes.
- Workflow YAML/TOML accepts provider-native `options` as an opaque map, exactly as Paseo Hub does;
  the selected harness/provider validates it.

## Dispatch

1. A trigger produces a `RunRequest` with an idempotency key (source delivery id + workflow id).
2. The scheduler selects a target: workspace kind → runtime target
   ([`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md)),
   then a worker by tag, health, and current load.
3. The hub asks the worker for a lease: `lease {run_id, deadline, grants}`. The worker accepts only
   if it can start the run within its concurrency budget, and then heartbeats every 10 s.
4. A lease that misses three heartbeats is released; the run is requeued with the attempt count
   incremented, up to the workflow's `max_attempts`. A run that exhausts attempts lands in
   `failed` with its last trace, never silently disappears.
5. Cancellation is a hub → worker `cancel {run_id}`; the worker interrupts the harness and keeps the
   partial ledger, so a cancelled run is still inspectable and forkable.

Idempotency is enforced in the hub's store: the same key returns the existing run id, so a redelivered
webhook does not start a second run.

## Sandbox lifecycle

The hub uses the existing `goble-remote` providers. The rule is: **a run owns its sandbox**, and a
sandbox never outlives its run's deadline plus a grace period.

| Provider | Lifecycle |
|---|---|
| `modal` | one Modal sandbox per run; `timeout_secs` is its lifetime; labels carry `run_id` |
| `e2b` | `auto_pause: true` and the request timeout; a paused sandbox is resumed for a follow-up turn, then killed by the reaper |
| `daytona` | `auto_stop_interval` from the timeout (minutes) |
| `box` | `ttl_seconds` from the timeout |
| `docker` | `--rm` plus a reaper label; local development only |
| `ssh` | no auto-stop; the hub stops the worker's run, not the host |

The hub side adds a **reaper**: every minute it lists sandboxes carrying `goble:run_id` for runs that
are finished, failed, or past their deadline + 15 minutes, and destroys them. A provider that cannot
be listed (SSH, raw Cloud Run) is reaped by the worker's own `max_lifetime` timer instead.

**Cloud Run.** Cloud Run is request-scoped and has no attachable PTY, so it fits one-shot,
non-interactive runs, not a workspace. Two modes:

- *Cloud Run Jobs*: `gcloud run jobs execute <job> --wait`, logs streamed to the hub, exit code is
  the result. No pause/resume, no filesystem; state must go to GCS or the repo. The job's task
  timeout is the auto-stop.
- *Cloud Run services*: only for a stateless HTTP step; never for a terminal.

An interactive workspace on GCP is a GCE VM (or GKE pod) running `goblin-worker`, connected outbound
to the relay. That is the same shape as any VPS, and it is what the `ssh`/Relay path already
supports.

Warm pools are an optimisation, not a requirement: a pool keeps N pre-created sandboxes per provider
and tag, and the hub hands one out instead of creating one. A pooled sandbox that is not handed out
within its idle timeout is destroyed, so a warm pool cannot become a leak.

## Reversibility

Every run maps to one harness session:

```
Run 1 ── 1 Session (project, worktree, ledger)
          └── turns: fork / replay / revert available from the hub UI and API
```

- The hub stores `run.session_id`, `run.ledger_checkpoint_ids`, and the execution trace id.
- A run's transcript, tool calls and diffs are read from the worker's session, not copied into the
  hub. The hub is a directory of where the evidence lives.
- "Fork this run" creates a new run whose first turn starts from the chosen checkpoint on the same
  worker (or a clone of the worktree on another). "Revert" applies the ledger's inverse to the
  project; both are the same verbs `goble-cli ctl` already exposes.

## Secrets and grants

- Workflows carry `secret_ref` names only. The worker resolves them from its own vault, seeded over
  the paired E2EE channel ([`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).
- Each step has a grant: `ReadOnly` or `ReadWrite`. A step that needs to write requires a fresh
  approval when `approval = "always"`; an approval is a signed event naming the step, the run and the
  device that approved it.
- The hub never receives a model key at rest. If the hub itself is multi-user, keys stay per-user in
  the user's workspace vault, not in the hub database.

## Storage and deployment

- Single user: one binary, SQLite at `~/.goble/hub.db`, embedded scheduler. It dials the relay; no
  inbound port.
- Team: the same binary with Postgres (`DATABASE_URL`) and N replicas behind the relay's registry;
  the scheduler takes a database advisory lock so two replicas cannot lease the same run.
- The hub's own UI is the same `goble-ui` element set as the desktop, served by `goble-web`; the API
  is the documented surface underneath it.

## API and MCP

```
POST /api/v1/runs                 {workflow, revision?, inputs, idempotency_key}
GET  /api/v1/runs/:id             state, target, lease, trace id, session id
POST /api/v1/runs/:id/approve     step, decision
POST /api/v1/runs/:id/cancel
POST /api/v1/runs/:id/fork        checkpoint_id
GET  /api/v1/events               append-only audit stream (SSE/WebSocket)
GET  /api/v1/workflows            revisions and their deployed state
POST /api/v1/workflows/deploy     {path, revision_hash}
```

The MCP server mirrors these as tools (`hub_run_start`, `hub_run_status`, `hub_run_approve`,
`hub_run_cancel`, `hub_run_fork`), with the same scopes. `goble-cli hub …` wraps the same calls, so
an agent can start work without a bespoke integration.

## Audit

Every event is appended to `audit_events` with `(id, at, actor_kind, actor_id, run_id, kind, payload)`,
and the row is signed with the hub's Ed25519 key. A daily Merkle root is signed and stored; the UI
shows the chain so a deleted or edited row is detectable. This is the "one audit trail" requirement,
with a property Paseo's hub does not claim: the trail is verifiable offline.

## Tasks

- [x] Define the run/lease state machine and its store schema.
- [x] Implement webhook adapters with signature verification and deduplication.
- [x] Implement workflow parsing, validation and immutable revisions.
- [~] Implement scheduling, worker selection and lease heartbeats — selection, lease, heartbeat,
      requeue and expiry are implemented; the always-on scheduler loop is pending.
- [~] Add the sandbox reaper and the warm pool — `dispatch::should_reap` is the rule; provider
      listing and the warm pool are pending.
- [ ] Add the Cloud Run Jobs adapter for one-shot runs (documented as non-interactive).
- [~] Link runs to sessions/checkpoints and expose fork/replay/revert — `link_session` is in;
      checkpoint linking and the verbs are pending.
- [~] Add approvals with device-signed events and grant enforcement — the approval state gate is in;
      signed events and grant checks are pending.
- [ ] Add the API, MCP mirror and `goble-cli hub`.
- [x] Add the signed audit log and its Merkle check.
