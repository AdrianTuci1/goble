// Pure manifest logic for the downloads Worker. No KV, no Request/Response: every
// function here is a value in, value out, so it can be unit tested directly.

/** Manifest format this Worker accepts. */
export const SCHEMA = 2;

/**
 * A channel name: lowercase, starts with a letter, at most 32 characters.
 * Channel names become KV key suffixes, so anything wider is refused.
 */
const CHANNEL_PATTERN = /^[a-z][a-z0-9-]{0,31}$/;

/**
 * @param {unknown} value
 * @returns {value is Record<string, any>}
 */
function isObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Whether a client may ask for this channel.
 * @param {unknown} channel
 * @returns {boolean}
 */
export function isValidChannel(channel) {
  return typeof channel === "string" && CHANNEL_PATTERN.test(channel);
}

/**
 * The release object for a channel, or undefined when the document has none.
 * @param {Record<string, any>} document
 * @param {string} channel
 * @returns {Record<string, any> | undefined}
 */
function channelRelease(document, channel) {
  if (!isObject(document.channels)) return undefined;
  if (!Object.hasOwn(document.channels, channel)) return undefined;
  const release = document.channels[channel];
  return isObject(release) ? release : undefined;
}

/**
 * Check the manifest document a publisher wants to store for one channel.
 * @param {unknown} document the parsed manifest document
 * @param {string} channel the channel that must exist inside `document.channels`
 * @returns {{ ok: true, epoch: number } | { ok: false, error: string }}
 */
export function validateManifest(document, channel) {
  if (!isObject(document)) {
    return { ok: false, error: "the manifest must be a JSON object" };
  }
  if (document.schema !== SCHEMA) {
    return { ok: false, error: `the manifest must declare schema ${SCHEMA}` };
  }
  if (!isObject(document.channels)) {
    return { ok: false, error: "the manifest must have a `channels` object" };
  }
  if (!isValidChannel(channel)) {
    return { ok: false, error: "the channel name is not valid" };
  }
  const release = channelRelease(document, channel);
  if (release === undefined) {
    return { ok: false, error: `the manifest has no \`${channel}\` channel` };
  }
  if (typeof release.version !== "string" || release.version.length === 0) {
    return { ok: false, error: `the \`${channel}\` channel needs a non-empty \`version\` string` };
  }
  if (typeof release.signature !== "string" || release.signature.length === 0) {
    return { ok: false, error: `the \`${channel}\` channel needs a non-empty \`signature\` string` };
  }
  if (!Number.isSafeInteger(release.epoch) || release.epoch < 0) {
    return { ok: false, error: `the \`${channel}\` channel needs an \`epoch\` that is a non-negative integer` };
  }
  if (!Array.isArray(release.artifacts)) {
    return { ok: false, error: `the \`${channel}\` channel needs an \`artifacts\` array` };
  }
  return { ok: true, epoch: release.epoch };
}

/**
 * Parse the body of `POST /admin/publish`: `{ channel, manifest }`, where
 * `manifest` is the JSON document as a string.
 * @param {string} text raw request body
 * @returns {{ ok: true, channel: string, document: Record<string, any>, epoch: number } | { ok: false, error: string }}
 */
export function parsePublishBody(text) {
  if (typeof text !== "string" || text.trim().length === 0) {
    return { ok: false, error: "the body must be a JSON object with `channel` and `manifest`" };
  }
  /** @type {unknown} */
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return { ok: false, error: "the body is not valid JSON" };
  }
  if (!isObject(body)) {
    return { ok: false, error: "the body must be a JSON object" };
  }
  const { channel, manifest } = body;
  if (!isValidChannel(channel)) {
    return { ok: false, error: "`channel` must match ^[a-z][a-z0-9-]{0,31}$" };
  }
  if (typeof manifest !== "string" || manifest.trim().length === 0) {
    return { ok: false, error: "`manifest` must be the manifest document as a JSON string" };
  }
  /** @type {unknown} */
  let document;
  try {
    document = JSON.parse(manifest);
  } catch {
    return { ok: false, error: "`manifest` is not valid JSON" };
  }
  const verdict = validateManifest(document, channel);
  if (!verdict.ok) {
    return { ok: false, error: verdict.error };
  }
  return { ok: true, channel, document: /** @type {Record<string, any>} */ (document), epoch: verdict.epoch };
}

/**
 * An epoch as a comparable number: anything that is not a non-negative safe
 * integer counts as 0.
 * @param {unknown} value
 * @returns {number}
 */
function epochValue(value) {
  return Number.isSafeInteger(value) && /** @type {number} */ (value) >= 0 ? /** @type {number} */ (value) : 0;
}

/**
 * Whether an incoming epoch may replace the stored one. Equal epochs are
 * accepted so that republishing the same document is idempotent; a missing or
 * unusable stored epoch counts as 0.
 * @param {unknown} storedEpoch
 * @param {unknown} incomingEpoch
 * @returns {boolean}
 */
export function epochNotLower(storedEpoch, incomingEpoch) {
  return epochValue(incomingEpoch) >= epochValue(storedEpoch);
}

/**
 * The KV key holding one channel's manifest.
 * @param {string} channel
 * @returns {string}
 */
export function manifestKey(channel) {
  return `manifest:${channel}`;
}

/**
 * The epoch inside stored manifest text, or 0 when the text is missing or
 * unusable. A stored document that cannot be read places no floor on the next
 * publish, because there is nothing to protect.
 * @param {string | null} text
 * @param {string} channel
 * @returns {number}
 */
export function storedEpoch(text, channel) {
  if (typeof text !== "string" || text.length === 0) return 0;
  /** @type {unknown} */
  let document;
  try {
    document = JSON.parse(text);
  } catch {
    return 0;
  }
  if (!isObject(document)) return 0;
  const release = channelRelease(document, channel);
  return release === undefined ? 0 : epochValue(release.epoch);
}

/**
 * The document as it is safe to hand to a client: the published payload and
 * nothing else. Worker secrets live in `env` and are never part of this object,
 * so the published manifest is already free of them.
 * @param {Record<string, any>} document
 * @returns {Record<string, any>}
 */
export function sanitizeManifest(document) {
  return document;
}
