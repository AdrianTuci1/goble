import { useEffect, useRef, useSyncExternalStore } from "react";

import {
  clearGatewaySession,
  clearServerUrl,
  loadGatewaySession,
  loadServerUrl,
  loadWorkerId,
  saveAuthMode,
  saveGatewaySession,
  saveServerUrl,
  saveWorkerId,
} from "./session-store";
import { normalizeServerUrl } from "./server-url";
import {
  GatewayError,
  listWorkers,
  pairGateway,
  relayUrl,
  type WorkerSummary,
} from "./gateway";
import type { DesktopMessage, WorkerMessage } from "./protocol";
import { RelaySocket, type RelayStatus } from "./socket";

export type ConnectionPhase = "demo" | "pairing" | "connected" | "error";

export interface ConnectionSnapshot {
  /** Whether this phone has a gateway: no server, pairing, paired, or failed. */
  phase: ConnectionPhase;
  /** The relay socket under a paired gateway. */
  status: RelayStatus;
  server: string | null;
  workers: WorkerSummary[];
  workerId: string | null;
  error: string | null;
}

const INITIAL: ConnectionSnapshot = {
  phase: "demo",
  status: "idle",
  server: null,
  workers: [],
  workerId: null,
  error: null,
};

function describe(error: unknown): string {
  if (error instanceof GatewayError) return error.message;
  if (error instanceof Error) return error.message;
  return "Could not reach the gateway";
}

/**
 * The gateway this phone is paired with, and the relay socket to one worker.
 *
 * One module-level instance owns the socket so every screen reads the same
 * connection: the sign-in screen pairs, settings shows the state, and a session
 * sends turns. Screens subscribe with `useConnection`; worker events fan out
 * through `useWorkerMessages`.
 */
class Connection {
  private snapshot: ConnectionSnapshot = INITIAL;
  private token: string | null = null;
  private socket: RelaySocket | null = null;
  private hydrated = false;
  private readonly listeners = new Set<() => void>();
  private readonly messageListeners = new Set<(message: WorkerMessage) => void>();

  getSnapshot = (): ConnectionSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  subscribeMessages = (
    listener: (message: WorkerMessage) => void,
  ): (() => void) => {
    this.messageListeners.add(listener);
    return () => {
      this.messageListeners.delete(listener);
    };
  };

  /** Restore the paired gateway once, at app start. */
  async hydrate(): Promise<void> {
    if (this.hydrated) return;
    this.hydrated = true;
    const [server, session, workerId] = await Promise.all([
      loadServerUrl(),
      loadGatewaySession(),
      loadWorkerId(),
    ]);
    if (!server || !session) {
      this.patch({ phase: "demo", server });
      return;
    }
    this.token = session.token;
    this.patch({ phase: "connected", server });
    if (workerId) this.patch({ workerId });
    await this.connect();
  }

  /** Pair with a gateway: the PIN buys a session, then the workers load. */
  async pair(url: string, pin: string): Promise<{ ok: true } | { ok: false; message: string }> {
    const normalized = normalizeServerUrl(url);
    if (!normalized.ok) return { ok: false, message: normalized.error };
    this.patch({ phase: "pairing", error: null });
    try {
      const session = await pairGateway(normalized.url, pin);
      await saveServerUrl(normalized.url);
      await saveAuthMode("server");
      await saveGatewaySession(session);
      this.token = session.token;
      this.patch({
        phase: "connected",
        server: normalized.url,
        workers: [],
        workerId: null,
        error: null,
      });
      await this.connect();
      return { ok: true };
    } catch (error) {
      const message = describe(error);
      this.patch({ phase: "error", error: message });
      return { ok: false, message };
    }
  }

  /** Load the worker list and open the relay socket to the chosen one. */
  async connect(): Promise<void> {
    const server = this.snapshot.server;
    if (!server) return;
    try {
      const workers = await listWorkers(server, this.token);
      const saved = await loadWorkerId();
      const workerId =
        workers.find((worker) => worker.id === saved)?.id ??
        workers[0]?.id ??
        null;
      this.patch({
        phase: "connected",
        workers,
        workerId,
        error:
          workers.length === 0
            ? "No worker is connected to this gateway"
            : null,
      });
      if (workerId) await saveWorkerId(workerId);
      this.openSocket();
    } catch (error) {
      const message = describe(error);
      if (error instanceof GatewayError && error.status === 401) {
        await this.forget();
        this.patch({ phase: "error", error: `${message}. Pair again.` });
        return;
      }
      this.patch({ error: message, status: "idle" });
    }
  }

  /**
   * Take the gateway this page is served from as the paired one.
   *
   * The web export is mounted at `/app/` behind the gateway's own PIN login, so
   * the browser already holds the `HttpOnly` session cookie: there is no PIN to
   * ask for and no bearer token to keep. Both `/api/workers` and the client
   * socket accept that cookie, which is why the token stays `null`. This is the
   * browser counterpart of [`pair`], not a shortcut around it — a page served
   * from anywhere else still has to pair.
   */
  async adoptBrowserSession(url: string): Promise<void> {
    const normalized = normalizeServerUrl(url);
    if (!normalized.ok) {
      this.patch({ phase: "error", error: normalized.error });
      return;
    }
    // A static host — or `expo start --web`, which also serves this page under
    // `/app` — answers no `/api/workers`. Probe before claiming a gateway, so a
    // page that is not behind one keeps the demo shell instead of an error.
    try {
      await listWorkers(normalized.url, null);
    } catch {
      this.patch({ phase: "demo", server: null, status: "idle" });
      return;
    }
    this.token = null;
    this.patch({
      phase: "connected",
      server: normalized.url,
      workers: [],
      workerId: null,
      status: "idle",
      error: null,
    });
    await this.connect();
  }

  /** Point the socket at another worker behind the same gateway. */
  async selectWorker(workerId: string): Promise<void> {
    this.patch({ workerId, error: null });
    await saveWorkerId(workerId);
    this.openSocket();
  }

  /** Forget the session; the app falls back to the demo workspace. */
  async disconnect(): Promise<void> {
    this.closeSocket();
    this.token = null;
    await this.forget();
    await clearServerUrl();
    await saveAuthMode("demo");
    this.patch({ ...INITIAL });
  }

  send(message: DesktopMessage): boolean {
    const sent = this.socket?.send(message) ?? false;
    if (!sent) {
      this.patch({ error: "The worker link is not live" });
    }
    return sent;
  }

  private openSocket(): void {
    this.closeSocket();
    const { server, workerId } = this.snapshot;
    if (!server || !workerId) {
      this.patch({ status: "idle" });
      return;
    }
    const socket = new RelaySocket({
      url: relayUrl(server, workerId),
      token: this.token,
      onMessage: (message) => {
        for (const listener of this.messageListeners) listener(message);
      },
      onStatus: (status) => {
        this.patch({ status });
      },
    });
    this.socket = socket;
    socket.connect();
  }

  private closeSocket(): void {
    this.socket?.close();
    this.socket = null;
  }

  private async forget(): Promise<void> {
    this.token = null;
    await clearGatewaySession();
  }

  private patch(next: Partial<ConnectionSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...next };
    for (const listener of this.listeners) listener();
  }
}

export const connection = new Connection();

/** The live connection state, shared by every screen. */
export function useConnection(): ConnectionSnapshot {
  return useSyncExternalStore(connection.subscribe, connection.getSnapshot);
}

/** Run `handler` for every worker event while the screen is mounted. */
export function useWorkerMessages(
  handler: (message: WorkerMessage) => void,
): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(
    () =>
      connection.subscribeMessages((message) => {
        latest.current(message);
      }),
    [],
  );
}
