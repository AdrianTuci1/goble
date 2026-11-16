import { normalizeServerUrl } from "./server-url";

/** One worker behind the gateway, as `/api/v1/workers` reports it. */
export interface WorkerSummary {
  id: string;
  connected_at: string;
  last_seen: string;
}

/**
 * How a paired phone authenticates.
 *
 * `token` is what a current gateway hands out at `/api/v1/session`: the phone
 * keeps it and sends `Authorization: Bearer …`, which a native WebSocket can do.
 * `cookie` is the fallback for a gateway build without that endpoint; it only
 * works where the platform keeps the login cookie for the WebSocket.
 */
export interface GatewaySession {
  token: string | null;
  mode: "token" | "cookie";
  expiresAt: number;
}

const REQUEST_TIMEOUT_MS = 10_000;

export class GatewayError extends Error {
  readonly status: number | null;

  constructor(message: string, status: number | null = null) {
    super(message);
    this.name = "GatewayError";
    this.status = status;
  }
}

function withTimeout(): { signal: AbortSignal; done: () => void } {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  return { signal: controller.signal, done: () => clearTimeout(timer) };
}

/**
 * Exchange the gateway PIN for a session.
 *
 * A current gateway answers `/api/v1/session` with a bearer token. An older
 * build answers 404 there; for those, fall back to the login form, which sets
 * the browser cookie — the same session, kept by the platform instead.
 */
export async function pairGateway(
  url: string,
  pin: string,
): Promise<GatewaySession> {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) throw new GatewayError(normalized.error);
  const base = normalized.url;
  const request = withTimeout();

  try {
    const response = await fetch(`${base}/api/v1/session`, {
      body: JSON.stringify({ pin }),
      headers: { accept: "application/json", "content-type": "application/json" },
      method: "POST",
      signal: request.signal,
    });
    if (response.ok) {
      const body = (await response.json()) as {
        token?: unknown;
        expires_in?: unknown;
      };
      if (typeof body.token !== "string" || body.token.length === 0) {
        throw new GatewayError("The gateway did not return a session");
      }
      const seconds =
        typeof body.expires_in === "number" ? body.expires_in : 12 * 60 * 60;
      return {
        token: body.token,
        mode: "token",
        expiresAt: Date.now() + seconds * 1000,
      };
    }
    if (response.status === 401) {
      throw new GatewayError("That PIN was refused", 401);
    }
    if (response.status !== 404) {
      throw new GatewayError(
        `The gateway answered ${response.status} while pairing`,
        response.status,
      );
    }
  } finally {
    request.done();
  }

  return pairWithCookie(base, pin);
}

/** The pre-token gateway: post the PIN to the login form and keep the cookie. */
async function pairWithCookie(
  base: string,
  pin: string,
): Promise<GatewaySession> {
  const request = withTimeout();
  try {
    const response = await fetch(`${base}/login`, {
      body: `pin=${encodeURIComponent(pin)}`,
      // The platform follows the redirect to /app/ and keeps the cookie.
      credentials: "include",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      method: "POST",
      signal: request.signal,
    });
    if (response.status === 401) {
      throw new GatewayError("That PIN was refused", 401);
    }
    if (!response.ok) {
      throw new GatewayError(
        `The gateway answered ${response.status} while pairing`,
        response.status,
      );
    }
    return { token: null, mode: "cookie", expiresAt: 0 };
  } finally {
    request.done();
  }
}

/** The workers currently connected to the gateway. */
export async function listWorkers(
  url: string,
  token: string | null,
): Promise<WorkerSummary[]> {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) throw new GatewayError(normalized.error);
  const request = withTimeout();
  try {
    const response = await fetch(`${normalized.url}/api/workers`, {
      credentials: "include",
      headers: {
        accept: "application/json",
        ...(token ? { authorization: `Bearer ${token}` } : {}),
      },
      signal: request.signal,
    });
    if (response.status === 401) {
      throw new GatewayError("The session expired", 401);
    }
    if (!response.ok) {
      throw new GatewayError(
        `The gateway answered ${response.status}`,
        response.status,
      );
    }
    const body = (await response.json()) as { workers?: unknown };
    if (!Array.isArray(body.workers)) return [];
    return body.workers.filter((worker): worker is WorkerSummary => {
      const id = (worker as { id?: unknown })?.id;
      return typeof id === "string";
    });
  } finally {
    request.done();
  }
}

/** The relay socket for one worker: `ws(s)://…/ws/client?worker=<id>`. */
export function relayUrl(url: string, workerId: string): string {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) throw new GatewayError(normalized.error);
  const base = normalized.url
    .replace(/^http:\/\//, "ws://")
    .replace(/^https:\/\//, "wss://");
  return `${base}/ws/client?worker=${encodeURIComponent(workerId)}`;
}
