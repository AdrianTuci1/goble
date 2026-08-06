/**
 * E2E test bridge: replaces the real Tauri `invoke` with a window-level hook
 * so the Playwright mock server can intercept commands without changing the
 * production API surface.
 *
 * This module is injected via the test HTML page only when `GOBLE_E2E=1` is set.
 */
interface InvokeHandler {
  (cmd: string, args?: Record<string, unknown>): Promise<unknown>;
}

declare global {
  interface Window {
    __goble_e2e_invoke__?: InvokeHandler;
    __goble_e2e_emit__?: (event: string, payload: unknown) => void;
    __goble_e2e_listeners__?: Record<string, Array<(payload: unknown) => void>>;
  }
}

export function isE2E(): boolean {
  return typeof window !== 'undefined' && !!window.__goble_e2e_invoke__;
}

export function e2eInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const handler = window.__goble_e2e_invoke__;
  if (!handler) return Promise.reject(new Error('E2E invoke handler not registered'));
  return handler(cmd, args) as Promise<T>;
}

export function e2eListen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!window.__goble_e2e_listeners__) {
    window.__goble_e2e_listeners__ = {};
  }
  const listeners = window.__goble_e2e_listeners__;
  if (!listeners[event]) listeners[event] = [];
  const cb = (payload: unknown) => handler(payload as T);
  listeners[event].push(cb);
  return Promise.resolve(() => {
    const idx = listeners[event].indexOf(cb);
    if (idx !== -1) listeners[event].splice(idx, 1);
  });
}

export function e2eEmit(event: string, payload: unknown): void {
  const listeners = window.__goble_e2e_listeners__?.[event];
  if (listeners) {
    listeners.forEach((cb) => cb(payload));
  }
}
