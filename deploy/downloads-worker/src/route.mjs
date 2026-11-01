// Pure routing, tag/asset validation, and download URL building. No KV, no
// network: `route()` is a pathname in, route descriptor out.

/** Version tags look like `v0.2.0`, `v1.0.0-beta.1`. */
const TAG_PATTERN = /^v[A-Za-z0-9._-]+$/;

/** Asset names are a single path segment of release-file characters. */
const ASSET_PATTERN = /^[A-Za-z0-9._-]+$/;

/** `owner/repo`, each side a single segment. */
const REPO_PATTERN = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;

/** The literal tag that resolves through GitHub's `latest` redirect. */
export const LATEST = "latest";

const GITHUB_BASE = "https://github.com";

/**
 * @typedef {{ kind: "index" }
 *   | { kind: "health" }
 *   | { kind: "manifest" }
 *   | { kind: "install", shell: "sh" | "ps1" }
 *   | { kind: "download", tag: string, asset: string }
 *   | { kind: "publish" }
 *   | { kind: "notFound" }
 *   | { kind: "badRequest", error: string }} Route
 */

/**
 * Split a pathname into decoded segments. Returns null when a segment is not
 * valid percent-encoding, because such a path cannot be judged safely.
 * @param {string} pathname
 * @returns {string[] | null}
 */
function splitPath(pathname) {
  if (typeof pathname !== "string") return null;
  const trimmed = pathname.replace(/^\/+/, "").replace(/\/+$/, "");
  if (trimmed === "") return [];
  /** @type {string[]} */
  const segments = [];
  for (const raw of trimmed.split("/")) {
    try {
      segments.push(decodeURIComponent(raw));
    } catch {
      return null;
    }
  }
  return segments;
}

/**
 * Match a request pathname against the Worker's routes. The HTTP method is
 * checked by the caller, so a known path with the wrong method is 405 and an
 * unknown path is 404.
 * @param {string} pathname
 * @returns {Route}
 */
export function matchRoute(pathname) {
  const segments = splitPath(pathname);
  if (segments === null) {
    return { kind: "badRequest", error: "the path is not valid percent-encoding" };
  }
  if (segments.length === 0) return { kind: "index" };
  if (segments.length === 1) {
    if (segments[0] === "health") return { kind: "health" };
    if (segments[0] === "channel_versions.json") return { kind: "manifest" };
    if (segments[0] === "install.sh") return { kind: "install", shell: "sh" };
    if (segments[0] === "install.ps1") return { kind: "install", shell: "ps1" };
    return { kind: "notFound" };
  }
  if (segments.length === 2 && segments[0] === "admin" && segments[1] === "publish") {
    return { kind: "publish" };
  }
  if (segments.length === 3 && segments[0] === "dl") {
    return { kind: "download", tag: segments[1], asset: segments[2] };
  }
  return { kind: "notFound" };
}

/**
 * A tag is either the literal `latest` or `v` followed by release-tag
 * characters. `..` is refused even though it matches the character class.
 * @param {unknown} tag
 * @returns {boolean}
 */
export function validateTag(tag) {
  if (typeof tag !== "string" || tag.length === 0) return false;
  if (tag === LATEST) return true;
  if (tag.includes("..")) return false;
  return TAG_PATTERN.test(tag);
}

/**
 * An asset is one path segment of release-file characters, without traversal.
 * `.` and `..` are dot segments, not file names.
 * @param {unknown} asset
 * @returns {boolean}
 */
export function validateAsset(asset) {
  if (typeof asset !== "string" || asset.length === 0) return false;
  if (asset === "." || asset.includes("..")) return false;
  return ASSET_PATTERN.test(asset);
}

/**
 * A repository is `owner/repo`, each side one segment, without traversal.
 * @param {unknown} value
 * @returns {boolean}
 */
export function isValidRepo(value) {
  return (
    typeof value === "string" && REPO_PATTERN.test(value) && !value.includes("..")
  );
}

/**
 * The GitHub URL a download route redirects to, or null when any part of the
 * input would escape the release path.
 * @param {string} githubRepo `owner/repo`
 * @param {string} tag version tag or `latest`
 * @param {string} asset release asset file name
 * @returns {string | null}
 */
export function downloadTarget(githubRepo, tag, asset) {
  if (!isValidRepo(githubRepo)) return null;
  if (!validateTag(tag) || !validateAsset(asset)) return null;
  const base = `${GITHUB_BASE}/${githubRepo}/releases`;
  return tag === LATEST
    ? `${base}/latest/download/${asset}`
    : `${base}/download/${tag}/${asset}`;
}

/**
 * The token from an `Authorization: Bearer <token>` header, or null.
 * @param {Request} request
 * @returns {string | null}
 */
export function bearerToken(request) {
  const headers = request === null || request === undefined ? undefined : request.headers;
  if (headers === undefined || typeof headers.get !== "function") return null;
  const header = headers.get("authorization");
  if (typeof header !== "string") return null;
  const match = /^Bearer +(.+)$/i.exec(header.trim());
  if (match === null) return null;
  const token = match[1].trim();
  return token.length === 0 ? null : token;
}

/**
 * Constant-time-ish comparison of two tokens. The length check is not
 * constant time, and does not need to be: the token length is not the secret.
 * @param {unknown} provided
 * @param {unknown} expected
 * @returns {boolean}
 */
export function tokenMatches(provided, expected) {
  if (typeof provided !== "string" || typeof expected !== "string") return false;
  if (expected.length === 0 || provided.length !== expected.length) return false;
  let difference = 0;
  for (let index = 0; index < provided.length; index += 1) {
    difference |= provided.charCodeAt(index) ^ expected.charCodeAt(index);
  }
  return difference === 0;
}
