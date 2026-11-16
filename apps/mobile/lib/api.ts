import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

import {
  clearAuthMode,
  clearGatewaySession,
  clearServerUrl,
  demoSnapshot,
  loadServerUrl,
  normalizeServerUrl,
  type HomeSnapshot,
} from "@goble/common";

const DEVICE_KEY = "goble.device_token";
const SERVERS_KEY = "goble.known_servers";
const PROBE_TIMEOUT_MS = 5_000;
const FETCH_TIMEOUT_MS = 8_000;

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

export async function loadDeviceToken(): Promise<string | null> {
  return safeGet(DEVICE_KEY);
}

export async function saveDeviceToken(token: string): Promise<void> {
  await safeSet(DEVICE_KEY, token);
}

export async function clearDeviceToken(): Promise<void> {
  await safeDelete(DEVICE_KEY);
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

export async function signOut(): Promise<void> {
  await clearServerUrl();
  await clearDeviceToken();
  await clearGatewaySession();
  await clearAuthMode();
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
