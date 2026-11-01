-- Athena tables over the Octomus telemetry layout.
--
--   s3://staticlabs-data/octomus/telemetry/events/dt=YYYY-MM-DD/hour=HH/*.jsonl
--   s3://staticlabs-data/octomus/telemetry/crashes/<fingerprint>/<id>.json
--
-- Run each statement on its own. Partition projection replaces the crawler and
-- `MSCK REPAIR`: a new day or hour is readable as soon as the first object
-- lands, because the partition comes from the key, not from the catalog.
--
-- Two partition levels, both from the event's `occurred_at` in UTC: `dt` is the
-- day (`events/dt=2026-11-01/...`) and `hour` is the hour inside that day
-- (`hour=20/...`). A query filtered on `dt` reads one day; adding `hour` reads
-- one hour.
--
-- The events table lists the union of the schema-2 payloads. A row carries only
-- the columns of its own event type; the rest read as NULL.

CREATE DATABASE IF NOT EXISTS octomus;

CREATE EXTERNAL TABLE IF NOT EXISTS octomus.events (
  schema       int,
  event_id     string,
  occurred_at  timestamp,
  received_at  timestamp,
  context      struct<
    app_version: string,
    channel:     string,
    os:          string,
    os_version:  string,
    arch:        string,
    install_id:  string,
    link_id:     string,
    role:        string,
    session_id:  string
  >,
  event        string,
  locale       string,
  uptime_secs  bigint,
  exit_code    int,
  feature      string,
  outcome      string,
  duration_ms  bigint,
  `from`       string,
  `into`       string
)
PARTITIONED BY (dt string, hour string)
ROW FORMAT SERDE 'org.openx.data.jsonserde.JsonSerDe'
LOCATION 's3://staticlabs-data/octomus/telemetry/events/'
TBLPROPERTIES (
  'projection.enabled'        = 'true',
  'projection.dt.type'        = 'date',
  'projection.dt.range'       = '2026-11-01,NOW',
  'projection.dt.format'      = 'yyyy-MM-dd',
  'projection.dt.interval'    = '1',
  'projection.dt.interval.unit' = 'DAYS',
  'projection.hour.type'      = 'integer',
  'projection.hour.range'     = '0,23',
  'projection.hour.digits'    = '2'
);

-- One object per crash, no partitions: the footprint is small and browsing by
-- fingerprint is the point.
CREATE EXTERNAL TABLE IF NOT EXISTS octomus.crashes (
  schema       int,
  id           string,
  fingerprint  string,
  captured_at  timestamp,
  received_at  timestamp,
  kind         string,
  message      string,
  location     string,
  thread       string,
  backtrace    string,
  note         string,
  context      struct<
    app_version: string,
    channel:     string,
    os:          string,
    os_version:  string,
    arch:        string,
    install_id:  string,
    link_id:     string,
    role:        string,
    session_id:  string
  >,
  breadcrumbs  array<struct<
    at:      timestamp,
    level:   string,
    target:  string,
    message: string,
    repeats: int
  >>
)
ROW FORMAT SERDE 'org.openx.data.jsonserde.JsonSerDe'
LOCATION 's3://staticlabs-data/octomus/telemetry/crashes/';
