# Athena over Octomus telemetry

The tables read S3 in place; nothing is copied or crawled. `create_tables.sql`
defines `octomus.events` and `octomus.crashes`, `views.sql` adds the daily
rollups. `events` is partitioned by day (`dt`) and then by hour, and projection
makes a new day or hour readable as soon as an object lands under it.

## Set up

One time, with an identity that can read the telemetry prefix (the operator
profile, not the write-only telemetry user):

```sh
aws athena create-work-group \
  --name octomus \
  --configuration '{"ResultConfiguration":{"OutputLocation":"s3://staticlabs-data/octomus/athena-results/"}}' \
  --region eu-central-1

aws athena create-database --database-input '{"Name":"octomus"}' --region eu-central-1
```

`create_tables.sql` also creates the database, so the second command is only
needed if you run the DDL somewhere else.

Athena takes one statement per call, so run the statements in
`create_tables.sql` and then `views.sql` one at a time, either in the console
or by pasting each into:

```sh
aws athena start-query-execution \
  --work-group octomus --region eu-central-1 \
  --query-string "$STATEMENT"
```

Check the result location once with `aws athena get-work-group --work-group
octomus`. Without it, Athena refuses to start.

The result location is a prefix of the same shared bucket. Athena writes small
CSV files there; it is not covered by the 400-day telemetry lifecycle rule, so
clear it out occasionally if it grows.

## Queries

Installs seen each day:

```sql
SELECT * FROM octomus.dau WHERE dt >= '2026-11-01' ORDER BY dt;
```

Feature usage over a week:

```sql
SELECT feature, outcome, sum(uses) AS uses
FROM octomus.feature_usage
WHERE dt BETWEEN '2026-11-01' AND '2026-11-07'
GROUP BY feature, outcome
ORDER BY uses DESC;
```

Crashes on the newest build:

```sql
SELECT c.fingerprint, arbitrary(c.message) AS sample, count(*) AS reports
FROM octomus.crashes c
GROUP BY c.fingerprint
ORDER BY reports DESC
LIMIT 20;
```

Weekly retention needs a returning-install definition, which is a product
decision: count an install as retained if it sent `app_started` in the second
week after its first. It is not a view yet, because the definition is still
open.

## Cost

Athena bills scanned bytes. Two things keep it cheap:

- **Partition projection.** A filter on `dt` (and `hour` when it matters) reads
  only those prefixes. Without a `dt` filter, a query reads the full 400 days.
- **Column pruning.** Select the columns you need. A query that reads `event`
  and `context.install_id` scans those columns, not the crash backtraces.

At roughly 30 million events a month, the raw JSON is a few gigabytes, so a
full-month scan is cents. Do not add a crawler: it would also publish
partitions that projection already derives.

The layout is not compacted. One object per batch means a scan opens many small
files; if queries become slow, a periodic compaction job can rewrite a day into
larger objects under the same partition.

## Notes

- `events_dedup` exists because an event can be written twice when a timeout
  lands after the Worker's write. Prefer it over `octomus.events` for counts.
- `context.role` separates the desktop from workers and sandboxes. Filter on
  `desktop` for product numbers.
- Installs are anonymous random ids. `dau` counts installations that opted in,
  which is not the same as people.
- Retention is the bucket's lifecycle rule, described in
  [`docs/telemetry-storage.md`](../../../docs/telemetry-storage.md).
