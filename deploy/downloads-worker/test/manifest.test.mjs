import assert from "node:assert/strict";
import test from "node:test";

import {
  SCHEMA,
  epochNotLower,
  isValidChannel,
  manifestKey,
  parsePublishBody,
  sanitizeManifest,
  storedEpoch,
  validateManifest,
} from "../src/manifest.mjs";

const CHANNEL = "stable";
const SIGNATURE = "a".repeat(128);

function release(overrides = {}) {
  return {
    version: "0.2.0",
    signature: SIGNATURE,
    epoch: 7,
    artifacts: [
      {
        os: "macos",
        arch: "aarch64",
        url: "https://downloads.goble.dev/dl/v0.2.0/Goble-0.2.0-arm64.dmg",
        sha256: "b".repeat(64),
        size: 12_345_678,
      },
    ],
    ...overrides,
  };
}

function document(overrides = {}, channelRelease = release()) {
  return { schema: SCHEMA, channels: { [CHANNEL]: channelRelease }, ...overrides };
}

function publishBody() {
  return JSON.stringify({ channel: CHANNEL, manifest: JSON.stringify(document()) });
}

test("channel names are lowercase and start with a letter", () => {
  for (const channel of ["stable", "beta", "dev", "a", "nightly-2"]) {
    assert.equal(isValidChannel(channel), true, channel);
  }
  for (const channel of ["", "Stable", "1beta", "-beta", "beta_1", "beta.1", "a".repeat(33), null, 2]) {
    assert.equal(isValidChannel(channel), false, String(channel));
  }
});

test("a published body yields the channel, the document and its epoch", () => {
  const parsed = parsePublishBody(publishBody());
  assert.equal(parsed.ok, true);
  assert.equal(parsed.channel, CHANNEL);
  assert.equal(parsed.epoch, 7);
  assert.deepEqual(parsed.document, document());
});

test("a body must be a JSON object with channel and manifest", () => {
  for (const body of ["", "   ", "not json", "[]", "null", '"text"', "{}", JSON.stringify({ channel: CHANNEL })]) {
    assert.equal(parsePublishBody(body).ok, false, body);
  }
});

test("manifest must be a JSON string, not an object", () => {
  const body = JSON.stringify({ channel: CHANNEL, manifest: document() });
  const parsed = parsePublishBody(body);
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /manifest/);
});

test("a body with a bad channel is refused", () => {
  for (const channel of ["Stable", "1beta", "", "beta/../stable"]) {
    const body = JSON.stringify({ channel, manifest: JSON.stringify(document()) });
    assert.equal(parsePublishBody(body).ok, false, channel);
  }
});

test("the document schema must be 2", () => {
  for (const schema of [1, "2", null, undefined, 3]) {
    const body = JSON.stringify({
      channel: CHANNEL,
      manifest: JSON.stringify({ schema, channels: { [CHANNEL]: release() } }),
    });
    assert.equal(parsePublishBody(body).ok, false, String(schema));
  }
  assert.deepEqual(validateManifest({ schema: 1, channels: { [CHANNEL]: release() } }, CHANNEL), {
    ok: false,
    error: "the manifest must declare schema 2",
  });
});

test("channels must be an object containing the requested channel", () => {
  for (const channels of [undefined, null, [], "stable", 2]) {
    const verdict = validateManifest({ schema: SCHEMA, channels }, CHANNEL);
    assert.equal(verdict.ok, false, String(channels));
    assert.match(verdict.error, /channels/);
  }
  const missing = validateManifest({ schema: SCHEMA, channels: { beta: release() } }, CHANNEL);
  assert.equal(missing.ok, false);
  assert.match(missing.error, /stable/);
  assert.equal(validateManifest({ schema: SCHEMA, channels: { [CHANNEL]: "text" } }, CHANNEL).ok, false);
});

test("an inherited property is not a channel", () => {
  const verdict = validateManifest({ schema: SCHEMA, channels: {} }, "constructor");
  assert.equal(verdict.ok, false);
});

test("a channel needs a non-empty version and signature", () => {
  for (const overrides of [{ version: "" }, { version: 2 }, { signature: "" }, { signature: undefined }, { signature: 7 }]) {
    const verdict = validateManifest(document({}, release(overrides)), CHANNEL);
    assert.equal(verdict.ok, false, JSON.stringify(overrides));
  }
});

test("the epoch must be a non-negative safe integer", () => {
  for (const epoch of [-1, 1.5, "7", null, undefined, Number.MAX_SAFE_INTEGER + 1, NaN]) {
    const verdict = validateManifest(document({}, release({ epoch })), CHANNEL);
    assert.equal(verdict.ok, false, String(epoch));
    assert.match(verdict.error, /epoch/);
  }
  assert.deepEqual(validateManifest(document({}, release({ epoch: 0 })), CHANNEL), { ok: true, epoch: 0 });
  assert.deepEqual(validateManifest(document({}, release({ epoch: 12 })), CHANNEL), { ok: true, epoch: 12 });
});

test("artifacts must be an array", () => {
  for (const artifacts of [undefined, null, {}, "x"]) {
    const verdict = validateManifest(document({}, release({ artifacts })), CHANNEL);
    assert.equal(verdict.ok, false, String(artifacts));
    assert.match(verdict.error, /artifacts/);
  }
  assert.equal(validateManifest(document({}, release({ artifacts: [] })), CHANNEL).ok, true);
});

test("the document itself must be an object", () => {
  for (const bad of [null, [], "text", 7, undefined]) {
    assert.equal(validateManifest(bad, CHANNEL).ok, false, String(bad));
  }
});

test("an epoch may replace the stored one when it is equal or higher", () => {
  assert.equal(epochNotLower(7, 7), true);
  assert.equal(epochNotLower(7, 8), true);
  assert.equal(epochNotLower(7, 4), false);
  assert.equal(epochNotLower(0, 0), true);
  assert.equal(epochNotLower(undefined, 0), true);
  assert.equal(epochNotLower(null, 3), true);
  assert.equal(epochNotLower(3, undefined), false);
});

test("keys are one per channel", () => {
  assert.equal(manifestKey("stable"), "manifest:stable");
  assert.equal(manifestKey("beta"), "manifest:beta");
});

test("the stored epoch is read back, and 0 when nothing usable is stored", () => {
  assert.equal(storedEpoch(JSON.stringify(document()), CHANNEL), 7);
  assert.equal(storedEpoch(null, CHANNEL), 0);
  assert.equal(storedEpoch("", CHANNEL), 0);
  assert.equal(storedEpoch("not json", CHANNEL), 0);
  assert.equal(storedEpoch("null", CHANNEL), 0);
  assert.equal(storedEpoch(JSON.stringify({ schema: SCHEMA, channels: {} }), CHANNEL), 0);
  assert.equal(storedEpoch(JSON.stringify(document()), "beta"), 0);
});

test("the served document is the published payload and carries no wrapper", () => {
  const parsed = parsePublishBody(publishBody());
  assert.equal(parsed.ok, true);
  const served = sanitizeManifest(parsed.document);
  assert.deepEqual(served, document());
  assert.equal(Object.hasOwn(served, "channel"), false);
  assert.equal(Object.hasOwn(served, "manifest"), false);
});
