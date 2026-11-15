import { Platform } from "react-native";

import {
  decodeMessage,
  encodeMessage,
  type DesktopMessage,
  type WorkerMessage,
} from "./protocol";

export type RelayStatus = "idle" | "connecting" | "live" | "lost";

const BASE_BACKOFF_MS = 500;
const MAX_BACKOFF_MS = 8_000;

type NativeWebSocket = new (
  url: string,
  protocols?: string | string[],
  options?: { headers?: Record<string, string> },
) => WebSocket;

/**
 * One WebSocket to the gateway's worker relay, kept open.
 *
 * The relay fans every worker frame out to every attached client, so frames are
 * filtered by the caller: a pane id for a pty this phone did not open, or an
 * event for another trace, is simply dropped. A dropped socket retries with
 * backoff and a fresh socket: the worker owns the panes, so they die with it.
 */
export class RelaySocket {
  private socket: WebSocket | null = null;
  private status: RelayStatus = "idle";
  private attempt = 0;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private closed = true;

  constructor(
    private readonly options: {
      url: string;
      token: string | null;
      onMessage: (message: WorkerMessage) => void;
      onStatus: (status: RelayStatus) => void;
    },
  ) {}

  get currentStatus(): RelayStatus {
    return this.status;
  }

  connect(): void {
    this.closed = false;
    this.open();
  }

  close(): void {
    this.closed = true;
    this.clearRetry();
    const socket = this.socket;
    this.socket = null;
    this.setStatus("idle");
    if (socket) {
      try {
        socket.close();
      } catch {
        // Already gone; nothing to release.
      }
    }
  }

  send(message: DesktopMessage): boolean {
    const socket = this.socket;
    if (!socket || socket.readyState !== 1) return false;
    socket.send(encodeMessage(message));
    return true;
  }

  private open(): void {
    this.clearRetry();
    this.setStatus("connecting");

    let socket: WebSocket;
    try {
      socket = this.createSocket();
    } catch {
      this.scheduleRetry();
      return;
    }
    this.socket = socket;

    socket.onopen = () => {
      if (this.socket !== socket) return;
      this.attempt = 0;
      this.setStatus("live");
    };

    socket.onmessage = (event: { data?: unknown }) => {
      if (typeof event.data !== "string") return;
      const message = decodeMessage(event.data);
      if (message) this.options.onMessage(message);
    };

    socket.onerror = () => {
      // `onclose` follows and owns the retry; this only stops the platform
      // from surfacing an unhandled error.
    };

    socket.onclose = () => {
      if (this.socket !== socket) return;
      this.socket = null;
      if (this.closed) {
        this.setStatus("idle");
        return;
      }
      this.scheduleRetry();
    };
  }

  private createSocket(): WebSocket {
    const { url, token } = this.options;
    // A browser cannot set handshake headers, so it relies on the login cookie.
    // Native WebSockets take headers, which is how the bearer session travels.
    if (Platform.OS === "web" || !token) return new WebSocket(url);
    const Native = WebSocket as unknown as NativeWebSocket;
    return new Native(url, undefined, {
      headers: { authorization: `Bearer ${token}` },
    });
  }

  private scheduleRetry(): void {
    this.clearRetry();
    this.setStatus("lost");
    const delay = Math.min(
      MAX_BACKOFF_MS,
      BASE_BACKOFF_MS * 2 ** this.attempt,
    );
    this.attempt += 1;
    this.retry = setTimeout(() => {
      this.retry = null;
      if (!this.closed) this.open();
    }, delay);
  }

  private clearRetry(): void {
    if (this.retry) {
      clearTimeout(this.retry);
      this.retry = null;
    }
  }

  private setStatus(status: RelayStatus): void {
    if (this.status === status) return;
    this.status = status;
    this.options.onStatus(status);
  }
}
