-- Views over octomus.events and octomus.crashes.
-- Run create_tables.sql first, then each statement here.

-- A retry overwrites the batch object, but an event can still arrive twice if a
-- timeout landed after the write. Keep one row per event_id, the last receipt.
CREATE OR REPLACE VIEW octomus.events_dedup AS
SELECT *
FROM (
  SELECT *,
         row_number() OVER (PARTITION BY event_id ORDER BY received_at DESC) AS rn
  FROM octomus.events
)
WHERE rn = 1;

-- Everything per day: how many events, and how many installs produced them.
CREATE OR REPLACE VIEW octomus.event_counts AS
SELECT dt,
       event,
       count(*)                            AS events,
       count(DISTINCT context.install_id)  AS installs
FROM octomus.events_dedup
GROUP BY dt, event;

-- Active installs. This counts consenting installations, not people: one
-- person with two machines is two installs.
CREATE OR REPLACE VIEW octomus.dau AS
SELECT dt,
       count(DISTINCT context.install_id) AS installs
FROM octomus.events_dedup
WHERE event = 'app_started'
  AND context.role = 'desktop'
GROUP BY dt;

CREATE OR REPLACE VIEW octomus.version_adoption AS
SELECT dt,
       context.app_version                AS version,
       context.channel                    AS channel,
       count(DISTINCT context.install_id) AS installs
FROM octomus.events_dedup
WHERE event = 'app_started'
  AND context.role = 'desktop'
GROUP BY dt, context.app_version, context.channel;

-- Feature usage and how long it took, when the feature reported a duration.
CREATE OR REPLACE VIEW octomus.feature_usage AS
SELECT dt,
       feature,
       outcome,
       count(*)                              AS uses,
       count(DISTINCT context.install_id)    AS installs,
       approx_percentile(duration_ms, 0.50)  AS median_ms,
       approx_percentile(duration_ms, 0.95)  AS p95_ms
FROM octomus.events_dedup
WHERE event = 'feature_used'
GROUP BY dt, feature, outcome;

-- One row per crash fingerprint per day. `sample_message` is redacted by the
-- client before it is sent.
CREATE OR REPLACE VIEW octomus.top_crashes AS
SELECT date(captured_at)                  AS dt,
       fingerprint,
       arbitrary(kind)                    AS kind,
       arbitrary(message)                 AS sample_message,
       count(*)                           AS reports,
       count(DISTINCT context.install_id) AS installs,
       min(captured_at)                   AS first_seen,
       max(captured_at)                   AS last_seen
FROM octomus.crashes
GROUP BY date(captured_at), fingerprint;
