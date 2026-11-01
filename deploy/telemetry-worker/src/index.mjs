// Telemetry ingest for Octomus.
//
// The desktop client keeps events on disk and drains them in batches: one
// request carries up to `MAX_BATCH` events, and the Worker writes one NDJSON
// object per batch into S3. That keeps both halves of the bill small — one
// request per drain instead of one per event, and one object per batch instead
// of one object per event.
//
// The wire type is the client's `QueuedItem`:
//
//   {"kind":"event","value":{...EventEnvelope...}}
//   {"kind":"crash","value":{...CrashReport...}}
//
// A batch is a JSON array of event items. A single item is accepted too, so a
// client from before batching keeps working. The collector in
// `crates/goble-telemetry-server` speaks the same shapes.
//
// Layout in the bucket (the bucket is shared, the project prefix is `octomus`):
//
//   <prefix>/events/dt=YYYY-MM-DD/hour=HH/batch-<first-id>-<last-id>.jsonl
//   <prefix>/crashes/<fingerprint>/<report-id>.json
//
// The key is derived from the batch's own event ids, so a retry after a
// timeout overwrites the same object instead of adding a second copy. The day
// and hour come from the event's `occurred_at`, never from the Worker's clock:
// an event queued offline for two days lands in the partition it happened in.

import { AwsClient } from "aws4fetch";

const MAX_BODY_BYTES = 1024 * 1024;
const MAX_BATCH = 200;
const ID = /^[A-Za-z0-9_-]{1,64}$/;
const DEFAULT_REGION = "eu-central-1";
const DEFAULT_BUCKET = "staticlabs-data";
const DEFAULT_PREFIX = "octomus/telemetry";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === "GET" && (url.pathname === "/" || url.pathname === "/health" || url.pathname === "/v1/health")) {
      return health(env);
    }

    if (request.method !== "POST") {
      return json({ error: "method not allowed" }, 405, { allow: "GET, POST" });
    }

    if (!authorised(request, env)) {
      return json({ error: "a collector token is required" }, 401);
    }

    const text = await request.text();
    if (text.length > MAX_BODY_BYTES) {
      return json({ error: "body too large" }, 413);
    }

    let payload;
    try {
      payload = JSON.parse(text);
    } catch {
      return json({ error: "body is not JSON" }, 400);
    }

    try {
      if (url.pathname === "/v1/events") {
        return await ingestEvents(payload, env);
      }
      if (url.pathname === "/v1/crashes") {
        return await ingestCrash(payload, env);
      }
      return json({ error: "not found" }, 404);
    } catch (error) {
      // A storage failure is retryable: the client keeps the batch and sends it
      // again, and the deterministic key makes the retry idempotent.
      console.error("telemetry: the S3 write failed", error);
      return json({ error: "could not store the batch" }, 503, { "retry-after": "5" });
    }
  },
};

async function ingestEvents(payload, env) {
  const items = Array.isArray(payload) ? payload : [payload];
  if (items.length > MAX_BATCH) {
    return json({ error: `a batch carries at most ${MAX_BATCH} events` }, 413);
  }

  const receivedAt = new Date().toISOString();
  const groups = new Map();
  let dropped = 0;

  for (const item of items) {
    if (!item || item.kind !== "event" || !item.value || typeof item.value !== "object") {
      dropped += 1;
      continue;
    }

    const value = item.value;
    if (!ID.test(value.event_id ?? "") || typeof value.event !== "string" || !value.event) {
      dropped += 1;
      continue;
    }

    const when = new Date(value.occurred_at);
    if (Number.isNaN(when.getTime())) {
      dropped += 1;
      continue;
    }

    // `toISOString` normalises offsets; the partition is always UTC.
    const iso = when.toISOString();
    const partition = `${iso.slice(0, 10)}/${iso.slice(11, 13)}`;
    if (!groups.has(partition)) {
      groups.set(partition, []);
    }
    groups.get(partition).push({ ...value, received_at: receivedAt });
  }

  let written = 0;
  for (const [partition, events] of groups) {
    const [day, hour] = partition.split("/");
    const ids = events.map((event) => event.event_id);
    const key = eventKey(prefix(env), day, hour, ids);
    const body = events.map((event) => JSON.stringify(event)).join("\n") + "\n";
    await putObject(env, key, body, "application/x-ndjson");
    written += events.length;
  }

  if (dropped > 0) {
    console.warn(`telemetry: accepted ${written} events, dropped ${dropped} malformed items`);
  }

  return json({ accepted: written, dropped }, 202);
}

async function ingestCrash(payload, env) {
  const item = payload;
  if (!item || item.kind !== "crash" || !item.value || typeof item.value !== "object") {
    return json({ error: "this endpoint takes crash reports" }, 400);
  }

  const report = item.value;
  if (!ID.test(report.id ?? "") || !ID.test(report.fingerprint ?? "")) {
    return json({ error: "a crash report needs an id and a fingerprint" }, 400);
  }

  const key = crashKey(prefix(env), report.fingerprint, report.id);
  const body = JSON.stringify({ ...report, received_at: new Date().toISOString() });
  await putObject(env, key, body, "application/json");

  return json({ accepted: 1 }, 202);
}

// Put one object with SigV4. `aws4fetch` signs the request in the Worker; the
// credentials are secrets and never appear in a key or a response.
async function putObject(env, key, body, contentType) {
  const region = env.AWS_REGION || DEFAULT_REGION;
  const bucket = env.S3_BUCKET || DEFAULT_BUCKET;

  if (!env.AWS_ACCESS_KEY_ID || !env.AWS_SECRET_ACCESS_KEY) {
    throw new Error("the S3 credentials are not configured");
  }

  const client = new AwsClient({
    accessKeyId: env.AWS_ACCESS_KEY_ID,
    secretAccessKey: env.AWS_SECRET_ACCESS_KEY,
    service: "s3",
    region,
  });

  // Encode each segment, keep the slashes that make the partitions.
  const path = key.split("/").map((segment) => encodeURIComponent(segment)).join("/");
  const url = `https://${bucket}.s3.${region}.amazonaws.com/${path}`;
  const response = await client.fetch(url, {
    method: "PUT",
    headers: {
      "content-type": contentType,
      "cache-control": "no-store",
    },
    body,
  });

  if (!response.ok) {
    // The body can carry a request id, never a credential.
    const detail = await response.text();
    console.error(`telemetry: s3 answered ${response.status}`, detail.slice(0, 200));
    throw new Error(`s3 answered ${response.status}`);
  }
}

function health(env) {
  const configured = Boolean(
    env.AWS_ACCESS_KEY_ID && env.AWS_SECRET_ACCESS_KEY && (env.S3_BUCKET || DEFAULT_BUCKET),
  );
  return json({
    status: "ok",
    schema: 2,
    service: "octomus-telemetry",
    batch_max: MAX_BATCH,
    storage: "s3",
    configured,
  }, configured ? 200 : 500);
}

export function prefix(env) {
  return String(env.S3_PREFIX || DEFAULT_PREFIX).trim().replace(/^\/+|\/+$/g, "");
}

export function eventKey(root, day, hour, ids) {
  const sorted = [...ids].sort();
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  return join(root, "events", `dt=${day}`, `hour=${hour}`, `batch-${first}-${last}.jsonl`);
}

export function crashKey(root, fingerprint, id) {
  return join(root, "crashes", fingerprint, `${id}.json`);
}

function join(...parts) {
  return parts.filter((part) => part !== "").join("/");
}

function json(body, status, headers = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      ...headers,
    },
  });
}

function authorised(request, env) {
  const expected = env.TELEMETRY_TOKEN;
  if (!expected) {
    return true;
  }

  const bearer = (request.headers.get("authorization") ?? "").replace(/^Bearer /, "");
  const presented = request.headers.get("x-goble-token") ?? bearer;
  if (!presented || presented.length !== expected.length) {
    return false;
  }

  // A shared token is a speed bump, not a password. Comparing every byte keeps
  // its length from being measurable through timing.
  let difference = 0;
  for (let index = 0; index < expected.length; index += 1) {
    difference |= presented.charCodeAt(index) ^ expected.charCodeAt(index);
  }
  return difference === 0;
}
