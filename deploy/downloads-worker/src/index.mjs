// Worker entry: KV access and Response plumbing over the pure logic in
// manifest.mjs and route.mjs. No validation rules live here.

import {
  epochNotLower,
  isValidChannel,
  manifestKey,
  parsePublishBody,
  sanitizeManifest,
  storedEpoch,
} from "./manifest.mjs";
import {
  DEFAULT_REPO,
  powershellInstaller,
  shellInstaller,
} from "./install.mjs";
import { bearerToken, downloadTarget, isValidRepo, matchRoute, tokenMatches } from "./route.mjs";

/** Channel served when the request names none. */
const DEFAULT_CHANNEL = "stable";

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "cache-control": "no-store",
};

/**
 * The slice of KV this Worker uses.
 * @typedef {object} KeyValueStore
 * @property {(key: string) => Promise<string | null>} get
 * @property {(key: string, value: string) => Promise<void>} put
 */

/**
 * Worker bindings and variables.
 * @typedef {object} Env
 * @property {KeyValueStore} [MANIFESTS]
 * @property {string} [ADMIN_TOKEN]
 * @property {string} [PUBLIC_BASE_URL]
 * @property {string} [GITHUB_REPO]
 */

/**
 * @param {unknown} value
 * @param {number} [status]
 * @returns {Response}
 */
function json(value, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: JSON_HEADERS });
}

/**
 * @param {number} status
 * @param {string} message
 * @returns {Response}
 */
function error(status, message) {
  return json({ ok: false, error: message }, status);
}

/**
 * @param {string} allowedMethod
 * @returns {Response}
 */
function methodNotAllowed(allowedMethod) {
  return new Response(
    JSON.stringify({ ok: false, error: `use ${allowedMethod} for this route` }),
    { status: 405, headers: { ...JSON_HEADERS, allow: allowedMethod } },
  );
}

/**
 * @param {Request} request
 * @param {string} method
 * @returns {boolean}
 */
function hasMethod(request, method) {
  return typeof request.method === "string" && request.method.toUpperCase() === method;
}

/**
 * @param {Env} env
 * @returns {boolean}
 */
function hasStore(env) {
  return (
    env !== null &&
    env !== undefined &&
    env.MANIFESTS !== null &&
    env.MANIFESTS !== undefined &&
    typeof env.MANIFESTS.get === "function" &&
    typeof env.MANIFESTS.put === "function"
  );
}

/**
 * The absolute base clients should use, falling back to the request origin when
 * `PUBLIC_BASE_URL` is unset.
 * @param {Env} env
 * @param {Request} request
 * @returns {string}
 */
function publicBaseUrl(env, request) {
  const configured =
    typeof env.PUBLIC_BASE_URL === "string" ? env.PUBLIC_BASE_URL.trim().replace(/\/+$/, "") : "";
  return configured === "" ? new URL(request.url).origin : configured;
}

/**
 * @param {Request} request
 * @param {Env} env
 * @returns {Response}
 */
function indexResponse(request, env) {
  const base = publicBaseUrl(env, request);
  const body = `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Goble downloads</title></head>
<body>
<h1>Goble downloads</h1>
<ul>
<li><a href="${base}/channel_versions.json">channel_versions.json</a> (add <code>?channel=beta</code> for another channel)</li>
<li><a href="${base}/install.sh">install.sh</a> — <code>curl -fsSL ${base}/install.sh | sh</code></li>
<li><a href="${base}/install.ps1">install.ps1</a></li>
<li><a href="${base}/health">health</a></li>
</ul>
</body>
</html>
`;
  return new Response(body, {
    status: 200,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
}

/**
 * The installer script for a shell, built from the configured repository and
 * public base. Both values are validated before they reach the script text.
 * @param {Request} request
 * @param {Env} env
 * @param {"sh" | "ps1"} shell
 * @returns {Response}
 */
function serveInstall(request, env, shell) {
  const configured =
    typeof env.GITHUB_REPO === "string" && env.GITHUB_REPO.trim() !== ""
      ? env.GITHUB_REPO.trim()
      : DEFAULT_REPO;
  if (!isValidRepo(configured)) {
    return error(500, "GITHUB_REPO is not a valid `owner/repo`");
  }
  const base = publicBaseUrl(env, request);
  if (shell === "ps1") {
    return new Response(powershellInstaller(configured, base), {
      status: 200,
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "public, max-age=300",
      },
    });
  }
  return new Response(shellInstaller(configured, base), {
    status: 200,
    headers: {
      "content-type": "text/x-shellscript; charset=utf-8",
      "cache-control": "public, max-age=300",
    },
  });
}

/**
 * @param {URL} url
 * @param {Env} env
 * @returns {Promise<Response>}
 */
async function serveManifest(url, env) {
  const requested = url.searchParams.get("channel");
  const channel = requested === null ? DEFAULT_CHANNEL : requested;
  if (!isValidChannel(channel)) {
    return error(400, "`channel` must match ^[a-z][a-z0-9-]{0,31}$");
  }
  if (!hasStore(env)) {
    return error(500, "the MANIFESTS KV binding is not configured");
  }
  const text = await env.MANIFESTS.get(manifestKey(channel));
  if (typeof text !== "string") {
    return error(404, `no manifest is published for the \`${channel}\` channel`);
  }
  return new Response(text, {
    status: 200,
    headers: { "content-type": "application/json", "cache-control": "no-cache" },
  });
}

/**
 * @param {string} tag
 * @param {string} asset
 * @param {Env} env
 * @returns {Response}
 */
function serveDownload(tag, asset, env) {
  const repo = typeof env.GITHUB_REPO === "string" ? env.GITHUB_REPO.trim() : "";
  if (repo === "") {
    return error(500, "the GITHUB_REPO variable is not configured");
  }
  const target = downloadTarget(repo, tag, asset);
  if (target === null) {
    return error(400, "the tag or asset name is not valid");
  }
  return new Response(null, {
    status: 302,
    headers: { location: target, "cache-control": "no-store" },
  });
}

/**
 * @param {Request} request
 * @param {Env} env
 * @returns {Promise<Response>}
 */
async function publish(request, env) {
  const expected = typeof env.ADMIN_TOKEN === "string" ? env.ADMIN_TOKEN : "";
  if (expected === "") {
    return error(500, "ADMIN_TOKEN is not configured on this Worker");
  }
  const provided = bearerToken(request);
  if (provided === null || !tokenMatches(provided, expected)) {
    return error(401, "a valid `Authorization: Bearer <ADMIN_TOKEN>` header is required");
  }
  if (!hasStore(env)) {
    return error(500, "the MANIFESTS KV binding is not configured");
  }

  /** @type {string} */
  let text;
  try {
    text = await request.text();
  } catch {
    return error(400, "the request body could not be read");
  }
  const parsed = parsePublishBody(text);
  if (!parsed.ok) {
    return error(400, parsed.error);
  }

  const key = manifestKey(parsed.channel);
  const stored = await env.MANIFESTS.get(key);
  const storedValue = storedEpoch(stored, parsed.channel);
  if (!epochNotLower(storedValue, parsed.epoch)) {
    return error(409, `epoch ${parsed.epoch} is lower than the published epoch ${storedValue}`);
  }
  await env.MANIFESTS.put(key, JSON.stringify(sanitizeManifest(parsed.document)));
  return json({ ok: true, epoch: parsed.epoch });
}

export default {
  /**
   * @param {Request} request
   * @param {Env} env
   * @param {unknown} _ctx
   * @returns {Promise<Response>}
   */
  async fetch(request, env, _ctx) {
    const url = new URL(request.url);
    const route = matchRoute(url.pathname);

    switch (route.kind) {
      case "index":
        return hasMethod(request, "GET") ? indexResponse(request, env) : methodNotAllowed("GET");
      case "health":
        return hasMethod(request, "GET") ? json({ ok: true }) : methodNotAllowed("GET");
      case "manifest":
        return hasMethod(request, "GET") ? serveManifest(url, env) : methodNotAllowed("GET");
      case "install":
        return hasMethod(request, "GET")
          ? serveInstall(request, env, route.shell)
          : methodNotAllowed("GET");
      case "download":
        return hasMethod(request, "GET")
          ? serveDownload(route.tag, route.asset, env)
          : methodNotAllowed("GET");
      case "publish":
        return hasMethod(request, "POST") ? publish(request, env) : methodNotAllowed("POST");
      case "badRequest":
        return error(400, route.error);
      default:
        return error(404, `no route matches ${url.pathname}`);
    }
  },
};
