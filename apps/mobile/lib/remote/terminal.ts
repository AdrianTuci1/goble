/**
 * A real shell on the worker, seen from the phone.
 *
 * A session opens one pane with `open_pty`, streams the shell's bytes back as
 * base64 `pty_output` frames and sends keystrokes as `pty_input`. The worker
 * owns the child: the pane dies with the relay socket that opened it, so a
 * screen that comes back after a drop opens a fresh pane instead of waiting for
 * an id that no longer exists.
 *
 * The bytes are a terminal stream, not text: they carry cursor moves, colours
 * and carriage returns. [`TerminalBuffer`] folds them into plain lines, which is
 * what the phone's monospace surface draws; the full escape grammar (alternate
 * screen, mouse reporting) is left to the desktop, which owns the real terminal.
 */

import { useMemo, useSyncExternalStore } from "react";

import { connection } from "./connection";
import { decodePtyOutput, encodePtyInput } from "./protocol";

/** Characters kept per pane; older lines fall off the top. */
const MAX_OUTPUT = 160_000;

export type TerminalState = "idle" | "opening" | "live" | "exited" | "error";

export interface TerminalSnapshot {
  state: TerminalState;
  output: string;
  exitCode: number | null;
  error: string | null;
}

/** How much of the stream the screen shows; the view scrolls the whole thing. */
const INITIAL: TerminalSnapshot = {
  error: null,
  exitCode: null,
  output: "",
  state: "idle",
};

type EscapeState = "text" | "esc" | "csi" | "osc" | "osc-esc" | "charset";

/**
 * Folds a pty byte stream into lines.
 *
 * Printable characters overwrite at a cursor, so a progress bar that redraws
 * itself with `\r` ends up on one line instead of dozens. Escape sequences are
 * consumed and dropped: the surface has no colours, and a half-received
 * sequence at a chunk boundary is kept until the rest of it arrives.
 */
export class TerminalBuffer {
  private lines: string[] = [""];
  private cursor = 0;
  private escape: EscapeState = "text";

  write(chunk: string): void {
    for (const character of chunk) this.feed(character);
    this.trim();
  }

  clear(): void {
    this.lines = [""];
    this.cursor = 0;
    this.escape = "text";
  }

  toString(): string {
    return this.lines.join("\n");
  }

  private feed(character: string): void {
    switch (this.escape) {
      case "esc":
        if (character === "[") this.escape = "csi";
        else if (character === "]") this.escape = "osc";
        else if (character === "(" || character === ")") this.escape = "charset";
        else this.escape = "text";
        return;
      case "csi":
        // A CSI sequence ends on its final byte; parameters are numbers and
        // separators in between.
        if (character >= "@" && character <= "~") this.escape = "text";
        return;
      case "osc":
        if (character === "\u0007") this.escape = "text";
        else if (character === "\u001b") this.escape = "osc-esc";
        return;
      case "osc-esc":
        this.escape = character === "\\" ? "text" : "osc";
        return;
      case "charset":
        this.escape = "text";
        return;
      default:
        break;
    }

    if (character === "\u001b") {
      this.escape = "esc";
    } else if (character === "\n") {
      this.lines.push("");
      this.cursor = 0;
    } else if (character === "\r") {
      this.cursor = 0;
    } else if (character === "\b") {
      this.cursor = Math.max(0, this.cursor - 1);
    } else if (character === "\t") {
      this.put("    ");
    } else if (character < " " || character === "\u007f") {
      // Other control bytes (bell, NUL, delete) carry no text.
      return;
    } else {
      this.put(character);
    }
  }

  private put(text: string): void {
    const index = this.lines.length - 1;
    const line = this.lines[index] ?? "";
    const before = line.slice(0, this.cursor);
    const after = line.slice(this.cursor + text.length);
    this.lines[index] = before + text + after;
    this.cursor += text.length;
  }

  private trim(): void {
    let length = 0;
    for (const line of this.lines) length += line.length + 1;
    while (length > MAX_OUTPUT && this.lines.length > 1) {
      const removed = this.lines.shift() ?? "";
      length -= removed.length + 1;
    }
  }
}

/**
 * One phone pane. The session listens to the relay for its own pane id only;
 * the worker fans every frame out to every client, so the id is the filter.
 */
export class TerminalSession {
  private snapshot: TerminalSnapshot = INITIAL;
  private readonly buffer = new TerminalBuffer();
  private paneId: string | null = null;
  /**
   * The id this session is waiting for: the pane it just asked for, before the
   * worker confirmed it. Output already sent for it is kept.
   */
  private pendingPaneId: string | null = null;
  private readonly listeners = new Set<() => void>();
  private detach: (() => void) | null = null;
  private size = { cols: 80, rows: 24 };
  private opening = false;

  getSnapshot = (): TerminalSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Open a shell, or report why one cannot be opened. */
  open(): void {
    if (this.opening || this.paneId !== null) return;
    const paneId = this.nextPaneId();
    this.pendingPaneId = paneId;
    if (!connection.send({
      cols: this.size.cols,
      cwd: "",
      kind: "open_pty",
      pane_id: paneId,
      rows: this.size.rows,
    })) {
      this.pendingPaneId = null;
      this.patch({ error: "The worker link is not live", state: "error" });
      return;
    }
    this.opening = true;
    this.patch({ error: null, state: "opening" });
    this.listen();
    // A worker that never confirms the pane would otherwise leave the screen
    // "opening" until the app restarts.
    setTimeout(() => {
      if (!this.opening) return;
      this.opening = false;
      this.pendingPaneId = null;
      this.patch({
        error: "The worker did not open a shell.",
        state: "error",
      });
    }, 8_000);
  }

  /** Send what the user typed, exactly as typed (no newline added). */
  write(text: string): void {
    if (!this.paneId || this.snapshot.state !== "live") return;
    connection.send({
      data: encodePtyInput(text),
      kind: "pty_input",
      pane_id: this.paneId,
    });
  }

  /** Send a line and press Return. Terminal Enter is CR, not LF. */
  submit(text: string): void {
    this.write(`${text}\r`);
  }

  /** Ctrl-C: the shell's own interrupt, so a running command stops. */
  interrupt(): void {
    this.write("\u0003");
  }

  resize(cols: number, rows: number): void {
    const next = { cols: Math.max(20, cols), rows: Math.max(5, rows) };
    if (next.cols === this.size.cols && next.rows === this.size.rows) return;
    this.size = next;
    if (!this.paneId) return;
    connection.send({
      cols: next.cols,
      kind: "pty_resize",
      pane_id: this.paneId,
      rows: next.rows,
    });
  }

  /** Close the pane and keep the output that was already drawn. */
  close(): void {
    if (this.paneId) {
      connection.send({ kind: "close_pty", pane_id: this.paneId });
    }
    this.paneId = null;
    this.pendingPaneId = null;
    this.opening = false;
    this.patch({ state: "exited" });
  }

  /** Forget the transcript and open a new pane. */
  restart(): void {
    this.close();
    this.buffer.clear();
    this.patch({ ...INITIAL });
    this.open();
  }

  private nextPaneId(): string {
    return `phone-${Math.random().toString(36).slice(2, 10)}`;
  }

  private listen(): void {
    if (this.detach) return;
    this.detach = connection.subscribeMessages((message) => {
      if (message.kind === "pty_opened") {
        if (message.pane_id !== this.pendingPaneId) return;
        this.paneId = message.pane_id;
        this.opening = false;
        this.patch({ state: "live" });
      } else if (message.kind === "pty_output") {
        if (!this.matches(message.pane_id)) return;
        this.buffer.write(decodePtyOutput(message.data));
        this.patch({ output: this.buffer.toString() });
      } else if (message.kind === "pty_exit") {
        if (!this.matches(message.pane_id)) return;
        this.paneId = null;
        this.opening = false;
        this.patch({ exitCode: message.code, state: "exited" });
      } else if (message.kind === "pty_error") {
        if (!this.matches(message.pane_id)) return;
        this.paneId = null;
        this.opening = false;
        this.patch({ error: message.message, state: "error" });
      }
    });
  }

  private matches(paneId: string): boolean {
    return paneId === this.paneId || paneId === this.pendingPaneId;
  }

  private patch(next: Partial<TerminalSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...next };
    for (const listener of this.listeners) listener();
  }
}

const sessions = new Map<string, TerminalSession>();

/** The pane for one screen, kept across navigation within the app's lifetime. */
export function terminalFor(key: string): TerminalSession {
  let session = sessions.get(key);
  if (!session) {
    session = new TerminalSession();
    sessions.set(key, session);
  }
  return session;
}

/** The live terminal state for one screen. */
export function useTerminal(key: string): {
  session: TerminalSession;
  snapshot: TerminalSnapshot;
} {
  const session = useMemo(() => terminalFor(key), [key]);
  const snapshot = useSyncExternalStore(session.subscribe, session.getSnapshot);
  return { session, snapshot };
}
