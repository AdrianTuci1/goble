import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

import { demoSnapshot } from "./mock";
import type { HomeSnapshot } from "./types";

const SERVER_KEY = "goble.server_url";
const DEVICE_KEY = "goble.device_token";
const SERVERS_KEY = "goble.known_servers";
const AUTH_KEY = "goble.auth_mode";
const PROBE_TIMEOUT_MS = 5_000;
const FETCH_TIMEOUT_MS = 8_000;

let memoryServer: string | null = null;

/** SecureStore has no web implementation; fall back to localStorage there. */
function webStorage(): Storage | null {
  if (Platform.OS !== "web") return null;
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

async function safeGet(key: string): Promise<string | null> {
  const web = webStorage();
  if (web) return web.getItem(key);
  try {
    return await SecureStore.getItemAsync(key);
  } catch {
    return null;
  }
}

async function safeSet(key: string, value: string): Promise<void> {
  const web = webStorage();
  if (web) {
    web.setItem(key, value);
    return;
  }
  try {
    await SecureStore.setItemAsync(key, value);
  } catch {
    // SecureStore is unavailable on some test hosts. Keep going with the
    // in-memory value so the screen still works.
  }
}

async function safeDelete(key: string): Promise<void> {
  const web = webStorage();
  if (web) {
    web.removeItem(key);
    return;
  }
  try {
    await SecureStore.deleteItemAsync(key);
  } catch {
    // Ignore missing native storage.
  }
}

export async function loadServerUrl(): Promise<string | null> {
  if (memoryServer !== null) return memoryServer;
  const stored = await safeGet(SERVER_KEY);
  memoryServer = stored ? stored : null;
  return memoryServer;
}

export async function saveServerUrl(url: string): Promise<void> {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) throw new Error(normalized.error);
  memoryServer = normalized.url;
  await safeSet(SERVER_KEY, normalized.url);
}

export async function clearServerUrl(): Promise<void> {
  memoryServer = null;
  await safeDelete(SERVER_KEY);
}

export async function loadDeviceToken(): Promise<string | null> {
  return safeGet(DEVICE_KEY);
}

export async function saveDeviceToken(token: string): Promise<void> {
  await safeSet(DEVICE_KEY, token);
}

export async function clearDeviceToken(): Promise<void> {
  await safeDelete(DEVICE_KEY);
}

export type NormalizedServer =
  | { ok: true; url: string }
  | { ok: false; error: string };

export function normalizeServerUrl(input: string): NormalizedServer {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, error: "Enter a gateway URL" };
  const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)
    ? trimmed
    : `https://${trimmed}`;

  let parsed: URL;
  try {
    parsed = new URL(withScheme);
  } catch {
    return { ok: false, error: "That does not look like a URL" };
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    return { ok: false, error: "Use an http or https URL" };
  }
  if (parsed.protocol === "http:" && !isLocalHost(parsed.hostname)) {
    return { ok: false, error: "Public gateways need https://" };
  }
  if (!parsed.hostname) return { ok: false, error: "That URL is missing a host" };
  return { ok: true, url: `${parsed.protocol}//${parsed.host}` };
}

export function isLocalHost(hostname: string): boolean {
  const host = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  return (
    host === "localhost" ||
    host === "127.0.0.1" ||
    host === "::1" ||
    host.endsWith(".local") ||
    /^10(?:\.\d{1,3}){3}$/.test(host) ||
    /^192\.168(?:\.\d{1,3}){2}$/.test(host) ||
    /^172\.(1[6-9]|2\d|3[0-1])(?:\.\d{1,3}){2}$/.test(host)
  );
}

export async function probeServer(url: string): Promise<{ ok: boolean; message: string }> {
  const normalized = normalizeServerUrl(url);
  if (!normalized.ok) return { ok: false, message: normalized.error };

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const response = await fetch(`${normalized.url}/health`, {
      headers: { accept: "application/json" },
      signal: controller.signal,
    });
    if (!response.ok) {
      return { ok: false, message: `The gateway answered ${response.status}` };
    }
    const body = (await response.json().catch(() => null)) as
      | { status?: string }
      | null;
    if (body?.status !== "ok") {
      return { ok: false, message: "That URL did not look like a Goble gateway" };
    }
    return { ok: true, message: "Gateway reachable" };
  } catch {
    return { ok: false, message: "Could not reach that gateway" };
  } finally {
    clearTimeout(timer);
  }
}

export type AuthMode = "demo" | "server";

export interface KnownServer {
  url: string;
  label: string;
  usedAt: string;
}

/** Suggested gateway on this machine; the gateway binds here by default. */
export const LOCAL_GATEWAY = "http://127.0.0.1:8788";

export async function loadKnownServers(): Promise<KnownServer[]> {
  const raw = await safeGet(SERVERS_KEY);
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (item): item is KnownServer =>
        typeof item === "object" &&
        item !== null &&
        typeof (item as KnownServer).url === "string" &&
        typeof (item as KnownServer).label === "string",
    );
  } catch {
    return [];
  }
}

export async function rememberServer(server: KnownServer): Promise<void> {
  const existing = await loadKnownServers();
  const next = [server, ...existing.filter((item) => item.url !== server.url)];
  await safeSet(SERVERS_KEY, JSON.stringify(next.slice(0, 6)));
}

/** The friendly name the user gave this gateway, if they named it. */
export async function loadServerName(url: string | null): Promise<string | null> {
  if (!url) return null;
  const known = await loadKnownServers();
  return known.find((item) => item.url === url)?.label ?? null;
}

export async function loadAuthMode(): Promise<AuthMode | null> {
  const stored = await safeGet(AUTH_KEY);
  if (stored === "demo" || stored === "server") return stored;
  const url = await loadServerUrl();
  return url ? "server" : null;
}

export async function saveAuthMode(mode: AuthMode): Promise<void> {
  await safeSet(AUTH_KEY, mode);
}

export async function signOut(): Promise<void> {
  await clearServerUrl();
  await clearDeviceToken();
  await safeDelete(AUTH_KEY);
}

export async function fetchHome(): Promise<HomeSnapshot> {
  const base = await loadServerUrl();
  if (!base) return demoSnapshot();

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const token = await loadDeviceToken();
    const response = await fetch(`${base}/api/v1/home`, {
      headers: {
        accept: "application/json",
        ...(token ? { authorization: `Bearer ${token}` } : {}),
      },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`home ${response.status}`);
    const live = (await response.json()) as HomeSnapshot;
    const name = await loadServerName(base);
    return {
      ...live,
      server: {
        ...live.server,
        name: name ?? live.server.name,
        url: base,
        online: true,
      },
      source: "live",
    };
  } catch {
    const demo = demoSnapshot();
    const name = await loadServerName(base);
    return {
      ...demo,
      server: {
        url: base,
        name: name ?? demo.server.name,
        online: false,
      },
      source: "offline",
    };
  } finally {
    clearTimeout(timer);
  }
}
