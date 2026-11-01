# Octomus telemetry ingest

The Worker behind `https://telemetry.octomus.dev`. It receives the batches the
desktop client drains from `~/.goble/telemetry-queue/` and writes them to S3.

There is one request per batch and one object per batch. That is the whole cost
story: at 30 million events a month, 100 events per request is 300,000 requests
and 300,000 objects, instead of 30 million of each.

## Protocol

`POST /v1/events` takes a JSON array of the client's `QueuedItem`:

```json
[
  {"kind":"event","value":{"schema":2,"event_id":"…","occurred_at":"2026-11-01T20:10:00Z","event":"app_started","context":{…}}}
]
```

A single item is accepted too, so a client from before batching keeps working.
The collector in `crates/goble-telemetry-server` speaks the same shapes. A batch
carries at most 200 events and a body at most 1 MiB.

`POST /v1/crashes` takes one `{"kind":"crash","value":{…CrashReport…}}`. Crashes
stay one object each: they are rare, and keeping one file per report makes the
archive easy to browse.

`GET /health` answers without touching S3.

## Storage

The bucket is shared by projects; `octomus/telemetry` is this project's
namespace.

```
s3://staticlabs-data/octomus/telemetry/events/dt=YYYY-MM-DD/hour=HH/batch-<first-id>-<last-id>.jsonl
s3://staticlabs-data/octomus/telemetry/crashes/<fingerprint>/<report-id>.json
```

Each line is the event envelope with one extra field, `received_at`. The day and
hour come from the event's `occurred_at` in UTC, never from the Worker's clock,
so an event queued offline for two days lands in the partition it happened in.

The object key is derived from the batch's own event ids. A retry after a
timeout overwrites the same object instead of adding a second copy; an event id
therefore appears at most once per object.

The bucket blocks public access, uses SSE-S3, and expires objects under
`octomus/telemetry/` after 400 days. See
[`docs/telemetry-storage.md`](../../docs/telemetry-storage.md) for the IAM
writer and the local AWS profile that can read the data.

## Deploy

```sh
npx wrangler deploy
```

The S3 credentials are Worker secrets, scoped to
`s3:PutObject` on `arn:aws:s3:::staticlabs-data/octomus/telemetry/*`:

```sh
npx wrangler secret put AWS_ACCESS_KEY_ID
npx wrangler secret put AWS_SECRET_ACCESS_KEY
```

The custom domain is declared in `wrangler.toml`; Wrangler creates the DNS
record in the `octomus.dev` zone. To reject anonymous posts, set a shared token
with `npx wrangler secret put TELEMETRY_TOKEN` and give the client the same
value (`GOBLE_TELEMETRY_TOKEN`, not wired yet).

## Cost

The client checks its queue every minute but sends only when 20 events are
waiting, the oldest item has waited ten minutes, or the app is exiting. At 30
million events a month that is a few hundred thousand requests; at 1,000 daily
users with two-hour sessions it is about 360,000 objects, because a quiet client
writes six an hour instead of sixty.

| Item | Amount at ~1,000 DAU | Cost |
|---|---|---|
| Worker requests | ~360,000 | inside the paid plan's 10 million |
| S3 PUT requests | ~360,000 | $1.80 |
| S3 storage | ~15 GB (30M events) | $0.35 |
| Athena scans | partition-pruned | cents per query |

The Workers paid plan is $5/month. The marginal cost of telemetry is under $3 a
month. One object per event would be 30 million writes, or about $150 in S3
operations alone, which is why batching is not optional.

## Querying

Athena reads the layout directly; the tables live in
[`infra/telemetry/athena/`](../../infra/telemetry/athena/). DuckDB works too:

```sql
INSTALL httpfs;
LOAD httpfs;
SET s3_region = 'eu-central-1';

SELECT event, count(*)
FROM read_json_auto('s3://staticlabs-data/octomus/telemetry/events/dt=2026-11-*/*/*.jsonl')
GROUP BY event
ORDER BY 2 DESC;
```

The AWS profile `octomus-telemetry-writer` can only write to the telemetry
prefix. Reading needs the account's normal credentials; do not widen the
writer's policy to make a laptop query work.
