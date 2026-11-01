# Telemetry storage

Octomus writes telemetry to a shared S3 bucket. This page is the record of that
setup, so another project can reuse the bucket without re-deriving it, and so
the next person knows which identity may write and which may read.

## Bucket

| Field | Value |
|---|---|
| Bucket | `staticlabs-data` |
| Region | `eu-central-1` |
| AWS account | `864981730944` |
| Project prefix | `octomus/telemetry` |
| Public access | blocked, all four settings |
| Encryption | SSE-S3 (`AES256`) |
| Retention | 400 days for `octomus/telemetry/` |
| Versioning | off |

The bucket is shared: each project owns one prefix. Octomus owns
`octomus/telemetry` and writes nothing else in it. Bucket-level settings
(public-access block, default encryption) apply to every project, so changing
them is a cross-project decision.

## Layout

```
s3://staticlabs-data/octomus/telemetry/events/dt=YYYY-MM-DD/hour=HH/batch-<first-id>-<last-id>.jsonl
s3://staticlabs-data/octomus/telemetry/crashes/<fingerprint>/<report-id>.json
```

The layout is partitioned twice: `dt=YYYY-MM-DD` is the UTC day, and `hour=HH`
splits that day into 24 prefixes. Both come from the event's `occurred_at`. One
NDJSON object per batch sits in the hour of its events. The key comes from the
batch's own event ids, so a retry overwrites the object instead of adding a
duplicate. The ingest Worker and its
protocol are in [`deploy/telemetry-worker`](../deploy/telemetry-worker/README.md).

## Identities

Two IAM users, deliberately separated by direction.

**Writer — `octomus-telemetry-writer`.** Inline policy
`OctomusTelemetryWrite`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "WriteOctomusTelemetry",
      "Effect": "Allow",
      "Action": "s3:PutObject",
      "Resource": "arn:aws:s3:::staticlabs-data/octomus/telemetry/*"
    }
  ]
}
```

The Worker holds this key as the secrets `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY`. A local profile with the same name and permissions
exists for smoke tests:

```sh
aws s3api put-object \
  --profile octomus-telemetry-writer \
  --bucket staticlabs-data \
  --key octomus/telemetry/probe.txt \
  --body /tmp/probe.txt
```

That identity cannot read, list or delete, and cannot write outside the
telemetry prefix. Both refusals are the policy working as intended; do not
widen it so a laptop can query. Query with the operator identity below.

**Operator — `AdrianTuci`.** The account's admin user, the default AWS CLI
profile. It reads, runs Athena, manages the lifecycle rule and cleans up test
objects. Credentials stay in the operator's own AWS profile and never enter the
repository.

The access key id and secret are not recorded here. If the writer key leaks,
rotate it (see [Rotating the writer key](#rotating-the-writer-key)).

## Reading and querying

Athena reads the layout in place. The tables and views are in
[`infra/telemetry/athena/`](../infra/telemetry/athena/README.md).

DuckDB works without any table setup:

```sql
INSTALL httpfs;
LOAD httpfs;
SET s3_region = 'eu-central-1';

SELECT event, count(*)
FROM read_json_auto('s3://staticlabs-data/octomus/telemetry/events/dt=2026-11-*/*/*.jsonl')
GROUP BY event
ORDER BY 2 DESC;
```

Reading needs credentials with `s3:GetObject` on the prefix: the operator
profile, or a dedicated reader if read access is ever handed to someone else.

## Sharing the bucket with another project

1. Pick a prefix, for example `otherproject/events`.
2. Create a writer user or role whose policy is scoped to that prefix, the same
   way `OctomusTelemetryWrite` is scoped.
3. Give the bucket-level lifecycle rule a prefix filter, so retention applies to
   that project's data and leaves the rest alone.

The existing rule only matches `octomus/telemetry/`. Keep it that way, and add a
second rule for a new prefix instead of broadening this one.

## Retention

The lifecycle rule `octomus-telemetry-retention` expires objects under
`octomus/telemetry/` after 400 days and aborts incomplete multipart uploads
after one day. Retention is bounded by the bucket's rule, so deleting the rule
extends retention for everything under the prefix.

Check it with:

```sh
aws s3api get-bucket-lifecycle-configuration \
  --bucket staticlabs-data --region eu-central-1
```

## Rotating the writer key

Create the new key, put it on the Worker, delete the old key:

```sh
aws iam create-access-key --user-name octomus-telemetry-writer --output json
cd deploy/telemetry-worker
npx wrangler secret put AWS_ACCESS_KEY_ID
npx wrangler secret put AWS_SECRET_ACCESS_KEY
aws iam delete-access-key --user-name octomus-telemetry-writer \
  --access-key-id <old-access-key-id>
```

A freshly created key can answer `InvalidAccessKeyId` for a few seconds while
IAM propagates it. Wait and retry instead of recreating the key.

After rotating, update the local `octomus-telemetry-writer` profile as well:

```sh
aws configure set aws_access_key_id <new-id> --profile octomus-telemetry-writer
aws configure set aws_secret_access_key <new-secret> --profile octomus-telemetry-writer
aws sts get-caller-identity --profile octomus-telemetry-writer
```

## Verifying the setup

```sh
aws s3api get-bucket-location --bucket staticlabs-data --output json
aws s3api get-public-access-block --bucket staticlabs-data --region eu-central-1
aws s3api get-bucket-encryption --bucket staticlabs-data --region eu-central-1
aws s3api get-bucket-lifecycle-configuration --bucket staticlabs-data --region eu-central-1
curl -s https://telemetry.octomus.dev/health
```

The health answer reports `"storage":"s3"` and `"configured":true` when the
Worker sees its secrets.
