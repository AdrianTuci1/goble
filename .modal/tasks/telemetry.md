# Task: telemetry v2 — urgent

Telemetry is the urgent workstream. Goble is about to ship binaries, and right
now the client is a crash reporter with three events while the collector writes
local JSONL. Make it a product-grade, privacy-preserving pipeline that answers:
how many consenting installs are active per day/week/month, which features are
used, which versions are healthy, and which crashes hurt most.

## Read first

- `crates/goble-telemetry/src/{lib,config,report,queue,upload,redact,panic,breadcrumbs}.rs`
- `crates/goble-telemetry-server/src/{lib,main}.rs`
- `.agents/07-observability/crash-reporting-and-updates.md`
- `app/src/main.rs` (the only current call sites)

## 1. Event envelope, schema 2

`SCHEMA` becomes `2`. Every event carries a stable envelope:

```rust
pub struct EventEnvelope {
    pub schema: u32,            // 2
    pub event_id: String,       // UUIDv4, generated once when the event is created
    pub occurred_at: DateTime<Utc>,
    pub context: ClientContext,
    #[serde(flatten)]
    pub event: TelemetryEvent,
}
```

- `event_id` is generated when the event is enqueued and survives retries. The
  queue stores the envelope; an upload retry must not mint a new id.
- `occurred_at` is the client clock at creation, RFC 3339 UTC. It survives
  retries and queue persistence.
- `context` moves out of `AppStarted` into the envelope, so `AppExited` and
  `FeatureUsed` carry `install_id`, `session_id`, version, channel, OS, and arch
  too.
- `ClientContext` gains `link_id: Option<String>` and `role: Role`, where
  `Role::{Desktop, Worker, Sandbox}` defaults to `Desktop` and can be set with
  `GOBLE_TELEMETRY_ROLE`. `link_id` is random, pseudonymous, and resettable; it
  is never derived from email, IP, hostname, or hardware.
- Keep a compatibility path: the collector accepts schema 1 (the old bare
  `TelemetryEvent`) and stores it with a synthesized `event_id` and
  `occurred_at` at ingest. Document that v1 rows are identifiable by the
  `legacy-` id prefix.

## 2. Link id for linked installations

- `TelemetryState` gains `link_id: Option<String>`; `ensure_link_id` writes a
  UUIDv4 next to `install_id`. Add `Telemetry::link_with(link_id)` so a
  connected installation can adopt the link before its first event.
- Propagate the link over the authenticated pairing/provisioning handshake, not
  over any unauthenticated channel. The worker pairing lives in
  `crates/goblin-worker/src/pairing.rs`; the desktop side is
  `crates/goble-desktop-service/src/state/workers.rs`. Add the field to the
  pairing payload and persist it on both sides. If a worker/sandbox does not run
  telemetry yet, leave the field in the payload and document the adoption seam.
- Never merge links automatically. A future `link_merged` event is the only way
  to alias two links; add the event variant now (with `from` and `into`) but do
  not emit it without an explicit user action.

## 3. Feature catalog and instrumentation

Add `crates/goble-telemetry/src/features.rs` with a closed enum and stable
snake_case names, so a typo cannot create a new series. Include at least:

`onboarding_completed`, `first_turn_completed`, `first_terminal_command`,
`turn_completed`, `tool_call_finished`, `approval_decision`,
`terminal_command_run`, `session_summary`, `update_check`, `crash`.

Each event carries a fixed outcome enum (`success`, `error`, `cancelled`,
`denied`) and optional duration in milliseconds, never free text. Add
`Telemetry::feature(Feature, Outcome, Option<Duration>)` and deprecate the
string-based `feature_used` or keep it as an internal shim.

Instrument the real outcome sites, not paint or per-frame code:

- a turn finishing, in the agent runtime;
- a tool call finishing, with the outcome and duration;
- an approval decision, allowed or denied;
- a terminal command block finishing;
- onboarding completion and first turn/first terminal command, each emitted at
  most once per installation (persist the flag in `TelemetryState`);
- update check, once per check.

All events run at 100% until scale; add a per-feature `sample_rate` in config
with a default of 1.0 so sampling is a decided policy later, not an accident.

## 4. Collector: idempotency, time, S3

- Store `received_at` at ingest and use `occurred_at` for the day partition; an
  event that was queued for two days must land in the day it happened.
- Accept the same `event_id` twice without producing two logical rows. The local
  store keeps an idempotency marker (for example `events/seen/<event_id>`), and
  the S3 sink writes one object per event, keyed by id, so a retry overwrites.
- Add an S3 sink behind env:
  `GOBLE_TELEMETRY_S3_BUCKET`, `GOBLE_TELEMETRY_S3_PREFIX` (default `raw`), and
  the standard AWS credential chain. Prefer `aws-sdk-s3` with rustls. Layout:
  `raw/events/dt=YYYY-MM-DD/<event_id>.json` and
  `raw/crashes/<fingerprint>/<report-id>.json`. Local storage stays the default
  and remains the source of truth; if S3 is configured, a failed write returns a
  retryable 5xx so the client retries instead of silently losing the event.
- Add `GET /v1/health` that reports whether the S3 sink is configured and
  reachable without leaking credentials.

## 5. Analytics: Athena

Add `infra/telemetry/athena/` with:

- `create_tables.sql`: `events` and `crashes` external tables over the S3
  layout, JSON SerDe, partition projection on `dt`, `received_at` and
  `occurred_at` as timestamps.
- `views.sql`: `events_v2`, `events_dedup` (one row per `event_id`),
  `link_canonical` (resolve `link_merged` aliases), `dau`, `wau`, `mau`,
  `weekly_retention`, `version_adoption`, `crash_free_sessions`,
  `top_crashes`, `feature_usage`.
- `README.md`: how to create the database, run the DDL, query the views, the
  retention policy (raw 400 days, aggregates longer), and the cost notes
  (partition projection, column pruning).

Count active installs by distinct `install_id` with `app_started`; count linked
people only through `link_canonical` and only for `role = desktop`. Say plainly
in the README that this counts consenting installs, not people.

## 6. Docs and tests

Add `docs/telemetry.md`: consent, fields, redaction, identity (`install_id`,
`session_id`, `link_id`, `role`), dedup, sampling, retention, how to add an
event, and the exact Athena queries for the launch dashboard.

Tests (targeted, no workspace-wide run):

- schema 2 roundtrip for every event variant;
- a v1 event accepted and stored with a `legacy-` id;
- an enqueued event keeps its `event_id` and `occurred_at` across a save/load
  and a retry;
- `link_id` persists and is absent until set;
- duplicate `event_id` ingest produces one logical row;
- S3 key uses `occurred_at`, not `received_at`;
- feature catalog names are unique and stable;
- redaction still applies to crash text and breadcrumbs.

Run `cargo check -p goble-telemetry -p goble-telemetry-server -p goble-app
--all-targets`, `cargo test -p goble-telemetry -p goble-telemetry-server`, and
`cargo clippy -p goble-telemetry -p goble-telemetry-server --all-targets`.

Commit in reviewable units (envelope, link id, features, collector/S3, Athena,
docs) and write `{{job_dir}}/out/summary.md`.
