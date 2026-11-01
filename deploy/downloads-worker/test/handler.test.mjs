import assert from "node:assert/strict";
import test from "node:test";

import worker from "../src/index.mjs";
import { SCHEMA } from "../src/manifest.mjs";

const ORIGIN = "https://downloads.goble.dev";
const REPO = "goble-dev/goble";
const ADMIN_TOKEN = "s3cret-admin-token-value";
const ASSET = "Goble-0.2.0-arm64.dmg";

function fakeKv() {
  return {
    store: new Map(),
    async get(key) {
      return this.store.has(key) ? this.store.get(key) : null;
    },
    async put(key, value) {
      this.store.set(key, value);
    },
  };
}

function makeEnv(overrides = {}) {
  const kv = fakeKv();
  const env = {
    MANIFESTS: kv,
    ADMIN_TOKEN,
    PUBLIC_BASE_URL: ORIGIN,
    GITHUB_REPO: REPO,
    ...overrides,
  };
  return { env, kv };
}

function release(overrides = {}) {
  return {
    version: "0.2.0",
    signature: "a".repeat(128),
    epoch: 7,
    artifacts: [
      {
        os: "macos",
        arch: "aarch64",
        url: `${ORIGIN}/dl/v0.2.0/${ASSET}`,
        sha256: "b".repeat(64),
        size: 12_345_678,
      },
    ],
    ...overrides,
  };
}

function document(channel, channelRelease = release()) {
  return { schema: SCHEMA, channels: { [channel]: channelRelease } };
}

function publishRequest(body, token = ADMIN_TOKEN) {
  const headers = { "content-type": "application/json" };
  if (token !== null) headers.authorization = `Bearer ${token}`;
  return new Request(`${ORIGIN}/admin/publish`, {
    method: "POST",
    headers,
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
}

function bodyFor(channel, channelRelease) {
  return JSON.stringify({ channel, manifest: JSON.stringify(document(channel, channelRelease)) });
}

async function get(path, env) {
  return worker.fetch(new Request(`${ORIGIN}${path}`), env, {});
}

async function publish(env, body, token = ADMIN_TOKEN) {
  return worker.fetch(publishRequest(body, token), env, {});
}

test("GET /health answers with ok", async () => {
  const { env } = makeEnv();
  const response = await get("/health", env);
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /application\/json/);
  assert.deepEqual(await response.json(), { ok: true });
});

test("GET / links the manifest and the health probe from PUBLIC_BASE_URL", async () => {
  const { env } = makeEnv();
  const response = await get("/", env);
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /text\/html/);
  const body = await response.text();
  assert.ok(body.includes(`${ORIGIN}/channel_versions.json`));
  assert.ok(body.includes(`${ORIGIN}/health`));
});

test("GET / falls back to the request origin without PUBLIC_BASE_URL", async () => {
  const { env } = makeEnv({ PUBLIC_BASE_URL: undefined });
  const body = await (await get("/", env)).text();
  assert.ok(body.includes(`${ORIGIN}/channel_versions.json`));
});

test("GET /channel_versions.json is a 404 before anything is published", async () => {
  const { env } = makeEnv();
  const response = await get("/channel_versions.json", env);
  assert.equal(response.status, 404);
  const body = await response.json();
  assert.equal(body.ok, false);
  assert.match(body.error, /stable/);
});

test("POST /admin/publish stores the manifest and GET serves it back", async () => {
  const { env, kv } = makeEnv();
  const published = await publish(env, bodyFor("stable"));
  assert.equal(published.status, 200);
  assert.deepEqual(await published.json(), { ok: true, epoch: 7 });
  assert.equal(kv.store.has("manifest:stable"), true);

  const response = await get("/channel_versions.json", env);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-type"), "application/json");
  assert.equal(response.headers.get("cache-control"), "no-cache");
  assert.deepEqual(JSON.parse(await response.text()), document("stable"));
});

test("?channel=beta serves the beta manifest, and a bad channel name is a 400", async () => {
  const { env } = makeEnv();
  await publish(env, bodyFor("beta", release({ version: "0.3.0-beta.1", epoch: 2 })));

  const beta = await get("/channel_versions.json?channel=beta", env);
  assert.equal(beta.status, 200);
  assert.equal(JSON.parse(await beta.text()).channels.beta.version, "0.3.0-beta.1");

  const stable = await get("/channel_versions.json", env);
  assert.equal(stable.status, 404);

  for (const channel of ["Beta", "1beta", "beta_1", ""]) {
    const response = await get(`/channel_versions.json?channel=${encodeURIComponent(channel)}`, env);
    assert.equal(response.status, 400, channel);
  }
});

test("downloads redirect to the tagged GitHub release", async () => {
  const { env } = makeEnv();
  const response = await get(`/dl/v0.2.0/${ASSET}`, env);
  assert.equal(response.status, 302);
  assert.equal(
    response.headers.get("location"),
    `https://github.com/${REPO}/releases/download/v0.2.0/${ASSET}`,
  );
});

test("downloads from latest use GitHub's latest release", async () => {
  const { env } = makeEnv();
  const response = await get(`/dl/latest/${ASSET}`, env);
  assert.equal(response.status, 302);
  assert.equal(
    response.headers.get("location"),
    `https://github.com/${REPO}/releases/latest/download/${ASSET}`,
  );
});

test("traversal and malformed segments on /dl are 400s", async () => {
  const { env } = makeEnv();
  const attempts = [
    "/dl/v0.2.0/..%2F..%2Fetc%2Fpasswd",
    "/dl/..%2Fv0.2.0/Goble.dmg",
    `/dl/v0.2.0/${ASSET}%2F..%2Fx`,
    "/dl/v0.2.0/..%2e%2e",
    "/dl/0.2.0/Goble.dmg",
    "/dl/v0.2.0/Goble%20macos.dmg",
  ];
  for (const path of attempts) {
    const response = await get(path, env);
    assert.equal(response.status, 400, path);
    const body = await response.json();
    assert.equal(body.ok, false);
  }
});

test("literal dot segments are normalized by the URL before routing, so they never redirect", async () => {
  const { env } = makeEnv();
  for (const path of ["/dl/latest/..", "/dl/v0.2.0/.", "/dl/../admin/publish"]) {
    const request = new Request(`${ORIGIN}${path}`);
    assert.equal(new URL(request.url).pathname.includes(".."), false, path);
    const response = await worker.fetch(request, env, {});
    assert.notEqual(response.status, 302, path);
    assert.equal(response.headers.get("location"), null, path);
  }
});

test("a Worker without GITHUB_REPO cannot build a download target", async () => {
  const { env } = makeEnv({ GITHUB_REPO: undefined });
  assert.equal((await get(`/dl/v0.2.0/${ASSET}`, env)).status, 500);
});

test("publishing without a token is a 401", async () => {
  const { env, kv } = makeEnv();
  const response = await publish(env, bodyFor("stable"), null);
  assert.equal(response.status, 401);
  assert.equal((await response.json()).ok, false);
  assert.equal(kv.store.size, 0);
});

test("publishing with the wrong token is a 401", async () => {
  const { env, kv } = makeEnv();
  for (const token of ["wrong", "s3cret-admin-token-valuE", `${ADMIN_TOKEN}x`]) {
    const response = await publish(env, bodyFor("stable"), token);
    assert.equal(response.status, 401, token);
  }
  assert.equal(kv.store.size, 0);
});

test("a Worker without ADMIN_TOKEN refuses to publish", async () => {
  const { env, kv } = makeEnv({ ADMIN_TOKEN: undefined });
  const response = await publish(env, bodyFor("stable"));
  assert.equal(response.status, 500);
  assert.equal(kv.store.size, 0);
});

test("an invalid publish body is a 400", async () => {
  const { env, kv } = makeEnv();
  const bodies = [
    "not json",
    "{}",
    JSON.stringify({ channel: "stable" }),
    JSON.stringify({ channel: "Stable", manifest: JSON.stringify(document("stable")) }),
    JSON.stringify({ channel: "stable", manifest: JSON.stringify({ schema: 1, channels: { stable: release() } }) }),
    JSON.stringify({ channel: "stable", manifest: JSON.stringify({ schema: SCHEMA, channels: { stable: release({ signature: "" }) } }) }),
    JSON.stringify({ channel: "stable", manifest: JSON.stringify({ schema: SCHEMA, channels: { stable: release({ epoch: -1 }) } }) }),
    JSON.stringify({ channel: "stable", manifest: JSON.stringify({ schema: SCHEMA, channels: { beta: release() } }) }),
  ];
  for (const body of bodies) {
    const response = await publish(env, body);
    assert.equal(response.status, 400, body);
  }
  assert.equal(kv.store.size, 0);
});

test("an equal epoch is accepted", async () => {
  const { env } = makeEnv();
  assert.equal((await publish(env, bodyFor("stable", release({ epoch: 7 })))).status, 200);
  const again = await publish(env, bodyFor("stable", release({ epoch: 7 })));
  assert.equal(again.status, 200);
  assert.deepEqual(await again.json(), { ok: true, epoch: 7 });
});

test("a higher epoch replaces the stored manifest", async () => {
  const { env, kv } = makeEnv();
  await publish(env, bodyFor("stable", release({ epoch: 7 })));
  const response = await publish(env, bodyFor("stable", release({ version: "0.2.1", epoch: 8 })));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { ok: true, epoch: 8 });
  assert.equal(JSON.parse(kv.store.get("manifest:stable")).channels.stable.version, "0.2.1");
});

test("a lower epoch is a 409 and leaves the stored manifest alone", async () => {
  const { env, kv } = makeEnv();
  await publish(env, bodyFor("stable", release({ epoch: 8 })));
  const response = await publish(env, bodyFor("stable", release({ version: "0.1.9", epoch: 7 })));
  assert.equal(response.status, 409);
  const body = await response.json();
  assert.equal(body.ok, false);
  assert.match(body.error, /epoch/);
  assert.equal(JSON.parse(kv.store.get("manifest:stable")).channels.stable.epoch, 8);
});

test("channels keep separate epochs", async () => {
  const { env } = makeEnv();
  await publish(env, bodyFor("stable", release({ epoch: 9 })));
  const beta = await publish(env, bodyFor("beta", release({ epoch: 1 })));
  assert.equal(beta.status, 200);
});

test("an unreadable stored manifest places no floor on the next publish", async () => {
  const { env, kv } = makeEnv();
  kv.store.set("manifest:stable", "not json");
  const response = await publish(env, bodyFor("stable", release({ epoch: 1 })));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { ok: true, epoch: 1 });
});

test("a Worker without the MANIFESTS binding cannot read or write manifests", async () => {
  const { env } = makeEnv({ MANIFESTS: undefined });
  assert.equal((await get("/channel_versions.json", env)).status, 500);
  assert.equal((await publish(env, bodyFor("stable"))).status, 500);
});

test("known paths with the wrong method are 405 and unknown paths are 404", async () => {
  const { env } = makeEnv();
  const wrongMethods = [
    new Request(`${ORIGIN}/health`, { method: "POST", body: "{}" }),
    new Request(`${ORIGIN}/channel_versions.json`, { method: "PUT", body: "{}" }),
    new Request(`${ORIGIN}/admin/publish`, { method: "GET" }),
    new Request(`${ORIGIN}/dl/v0.2.0/${ASSET}`, { method: "DELETE" }),
  ];
  for (const request of wrongMethods) {
    const response = await worker.fetch(request, env, {});
    assert.equal(response.status, 405, `${request.method} ${request.url}`);
    assert.equal(response.headers.get("allow"), request.url.endsWith("/admin/publish") ? "POST" : "GET");
  }
  for (const path of ["/nope", "/dl/v0.2.0", "/admin"]) {
    const response = await get(path, env);
    assert.equal(response.status, 404, path);
    assert.equal((await response.json()).ok, false);
  }
});

test("the admin token never appears in a GET response", async () => {
  const { env } = makeEnv();
  await publish(env, bodyFor("stable"));
  const paths = ["/", "/health", "/channel_versions.json", "/channel_versions.json?channel=beta", "/nope", `/dl/v0.2.0/${ASSET}`];
  for (const path of paths) {
    const response = await get(path, env);
    const text = `${await response.text()} ${JSON.stringify([...response.headers])}`;
    assert.equal(text.includes(ADMIN_TOKEN), false, path);
  }
});
