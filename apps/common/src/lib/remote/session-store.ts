import { getPlatformStorage } from "../../platform";
import { normalizeServerUrl } from "./server-url";

const SERVER_KEY = "goble.server_url";
const AUTH_KEY = "goble.auth_mode";
const SESSION_KEY = "goble.gateway_session";
const WORKER_KEY = "goble.worker_id";

let memoryServer: string | null = null;

async function read(key: string): Promise<string | null> {
  try {
    return await getPlatformStorage().get(key);
  } catch {
    return null;
  }
}

async function write(key: string, value: string): Promise<void> {
  try {
    await getPlatformStorage().set(key, value);
  } catch {
    // A host without storage keeps going with the value that is already here.
  }
}

async function remove(key: string): Promise<void> {
  try {
    await getPlatformStorage().remove(key);
  } catch {
    // Ignore missing storage.
  }
}

export async function loadServerUrl(): Promise<string | null> {
  if (memoryServer !== null) return memoryServer;
  const stored = await read(SERVER_KEY);
  memoryServer = stored ? stored : null;
  return memoryServer;
}

export async function saveServerUrl(url: string): Promise<void> {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) throw new Error(normalized.error);
  memoryServer = normalized.url;
  await write(SERVER_KEY, normalized.url);
}

export async function clearServerUrl(): Promise<void> {
  memoryServer = null;
  await remove(SERVER_KEY);
}

/** The relay session this phone paired with: a bearer token, or the cookie. */
export interface StoredGatewaySession {
  token: string | null;
  mode: "token" | "cookie";
  expiresAt: number;
}

export async function loadGatewaySession(): Promise<StoredGatewaySession | null> {
  const raw = await read(SESSION_KEY);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<StoredGatewaySession>;
    if (parsed.mode !== "token" && parsed.mode !== "cookie") return null;
    return {
      token: typeof parsed.token === "string" ? parsed.token : null,
      mode: parsed.mode,
      expiresAt: typeof parsed.expiresAt === "number" ? parsed.expiresAt : 0,
    };
  } catch {
    return null;
  }
}

export async function saveGatewaySession(
  session: StoredGatewaySession,
): Promise<void> {
  await write(SESSION_KEY, JSON.stringify(session));
}

export async function clearGatewaySession(): Promise<void> {
  await remove(SESSION_KEY);
  await remove(WORKER_KEY);
}

export async function loadWorkerId(): Promise<string | null> {
  return read(WORKER_KEY);
}

export async function saveWorkerId(id: string): Promise<void> {
  await write(WORKER_KEY, id);
}

export type AuthMode = "demo" | "server";

export async function loadAuthMode(): Promise<AuthMode | null> {
  const stored = await read(AUTH_KEY);
  if (stored === "demo" || stored === "server") return stored;
  const url = await loadServerUrl();
  return url ? "server" : null;
}

export async function saveAuthMode(mode: AuthMode): Promise<void> {
  await write(AUTH_KEY, mode);
}

export async function clearAuthMode(): Promise<void> {
  await remove(AUTH_KEY);
}
