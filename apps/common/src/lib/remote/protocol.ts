/**
 * The worker relay's wire format.
 *
 * The gateway copies frames verbatim between a client and the worker behind it,
 * so the phone speaks exactly the protocol the desktop speaks: `goble-core`'s
 * `DesktopMessage` out and `WorkerMessage` back, one JSON frame per message,
 * tagged by `kind` in snake_case. This module owns the subset the phone uses and
 * the byte plumbing a pty needs; it holds no transport and no state.
 */

/** One agent run the phone asks the worker to start. */
export interface AgentSpec {
  id: string;
  name: string;
  description: string;
  prompt: string;
  tools: string[];
  triggers: string[];
  mcp_ids: string[];
  created_at: string;
  updated_at: string;
}

export interface McpServer {
  id: string;
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
}

/**
 * The answer to a command the agent suspended before running. The worker takes
 * the text as the line to run (`approve`), the line the user rewrote (`edit`),
 * or the reason the call is refused (`reject`). `goble-core` tags the variant
 * and carries the text in `payload`.
 */
export type CommandDecision = {
  type: "approve" | "edit" | "reject";
  payload: string;
};

export type DesktopMessage =
  | {
      kind: "run_agent";
      trace_id: string;
      agent_id: string;
      spec: AgentSpec;
      mcp_servers: McpServer[];
    }
  | {
      kind: "run_agent_for_thread_reply";
      trace_id: string;
      thread_id: string;
      agent_id: string;
      prompt: string;
      spec: AgentSpec;
      mcp_servers: McpServer[];
    }
  | { kind: "open_pty"; pane_id: string; cwd: string; cols: number; rows: number }
  | { kind: "pty_input"; pane_id: string; data: string }
  | { kind: "pty_resize"; pane_id: string; cols: number; rows: number }
  | { kind: "close_pty"; pane_id: string }
  | { kind: "list_scheduled_tasks" }
  | { kind: "cancel_scheduled_task"; task_id: string }
  | {
      kind: "upload_attachment";
      upload_id: string;
      name: string;
      data: string;
    }
  | {
      kind: "transcribe_audio";
      request_id: string;
      provider: string;
      model: string;
      mime: string;
      data: string;
    }
  | { kind: "query_entities"; entity_type: string; query?: string | null }
  | { kind: "get_trace"; trace_id: string }
  | { kind: "answer_user"; trace_id: string; answer: string }
  | {
      kind: "answer_command";
      trace_id: string;
      decision: CommandDecision;
    }
  | { kind: "update_agent"; agent_id: string; spec: AgentSpec }
  | { kind: "remove_agent"; agent_id: string }
  | { kind: "ping" };

export type LogLevel = "debug" | "info" | "warn" | "error";

/** The events a worker streams back. Unknown kinds are dropped by the caller. */
export type WorkerMessage =
  | { kind: "paired" }
  | { kind: "agent_started"; trace_id: string; agent_id: string }
  | {
      kind: "agent_log";
      trace_id: string;
      step_id: string;
      level: LogLevel;
      message: string;
    }
  | { kind: "agent_finished"; trace_id: string; status: string }
  | { kind: "assistant_delta"; trace_id: string; delta: string }
  | {
      kind: "tool_call_started";
      trace_id: string;
      id: string;
      name: string;
      arguments: unknown;
    }
  | { kind: "tool_call_finished"; trace_id: string; id: string; result: string }
  | { kind: "tool_call_error"; trace_id: string; id: string; message: string }
  | {
      kind: "ask_user";
      trace_id: string;
      question: string;
      quick_replies: string[];
    }
  | {
      kind: "command_proposed";
      trace_id: string;
      id: string;
      candidates: string[];
      cwd: string;
    }
  | { kind: "mission_updated"; trace_id: string; mission_id: string; status: string }
  | {
      kind: "thread_agent_reply";
      trace_id: string;
      thread_id: string;
      content: string;
    }
  | { kind: "done"; trace_id: string }
  | { kind: "trace"; trace_id: string; trace: unknown }
  | { kind: "entity_list"; entity_type: string; items: unknown[] }
  | {
      kind: "attachment_stored";
      upload_id: string;
      path?: string;
      error?: string;
    }
  | {
      kind: "transcription_result";
      request_id: string;
      text?: string;
      error?: string;
    }
  | { kind: "pty_opened"; pane_id: string }
  | { kind: "pty_output"; pane_id: string; data: string }
  | { kind: "pty_exit"; pane_id: string; code: number | null }
  | { kind: "pty_error"; pane_id: string; message: string }
  | {
      kind: "scheduled_tasks";
      tasks: { id: string; agent_id: string; trigger: unknown; enabled: boolean }[];
    }
  | { kind: "task_cancelled"; task_id: string }
  | { kind: "pong" }
  | { kind: "status_report"; worker_id: string; status: string; load: number };

export function encodeMessage(message: DesktopMessage): string {
  return JSON.stringify(message);
}

/**
 * Parse one frame from the relay. The relay fans every worker frame out to every
 * attached client and also forwards pty frames for panes other clients own, so a
 * frame this build does not know is not an error: it is ignored.
 */
export function decodeMessage(raw: string): WorkerMessage | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const kind = (parsed as { kind?: unknown }).kind;
  return typeof kind === "string" ? (parsed as WorkerMessage) : null;
}

const BASE64 =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** Base64 of the bytes of `text`, the encoding `pty_input.data` expects. */
export function encodePtyInput(text: string): string {
  const bytes = utf8Bytes(text);
  let out = "";
  for (let index = 0; index < bytes.length; index += 3) {
    const first = bytes[index] ?? 0;
    const second = bytes[index + 1];
    const third = bytes[index + 2];
    out += BASE64[first >> 2]!;
    out += BASE64[((first & 0x03) << 4) | ((second ?? 0) >> 4)]!;
    out += second === undefined ? "=" : BASE64[((second & 0x0f) << 2) | ((third ?? 0) >> 6)]!;
    out += third === undefined ? "=" : BASE64[third & 0x3f]!;
  }
  return out;
}

/** The text of a base64 `pty_output.data` payload. */
export function decodePtyOutput(data: string): string {
  return utf8Text(base64Bytes(data));
}

function utf8Bytes(text: string): number[] {
  const bytes: number[] = [];
  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x80) bytes.push(code);
    else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code < 0x10000) {
      bytes.push(
        0xe0 | (code >> 12),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return bytes;
}

function utf8Text(bytes: number[]): string {
  let out = "";
  for (let index = 0; index < bytes.length; ) {
    const first = bytes[index]!;
    if (first < 0x80) {
      out += String.fromCharCode(first);
      index += 1;
    } else if (first < 0xe0) {
      const second = bytes[index + 1] ?? 0;
      out += String.fromCharCode(((first & 0x1f) << 6) | (second & 0x3f));
      index += 2;
    } else if (first < 0xf0) {
      const second = bytes[index + 1] ?? 0;
      const third = bytes[index + 2] ?? 0;
      out += String.fromCharCode(
        ((first & 0x0f) << 12) | ((second & 0x3f) << 6) | (third & 0x3f),
      );
      index += 3;
    } else {
      const second = bytes[index + 1] ?? 0;
      const third = bytes[index + 2] ?? 0;
      const fourth = bytes[index + 3] ?? 0;
      const code =
        ((first & 0x07) << 18) |
        ((second & 0x3f) << 12) |
        ((third & 0x3f) << 6) |
        (fourth & 0x3f);
      out += String.fromCodePoint(code);
      index += 4;
    }
  }
  return out;
}

function base64Bytes(data: string): number[] {
  const clean = data.replace(/[^A-Za-z0-9+/]/g, "");
  const bytes: number[] = [];
  for (let index = 0; index < clean.length; index += 4) {
    const chunk = [0, 1, 2, 3].map((offset) =>
      BASE64.indexOf(clean[index + offset] ?? "A"),
    );
    const [first = 0, second = 0, third = 0, fourth = 0] = chunk;
    bytes.push((first << 2) | (second >> 4));
    if (index + 2 < clean.length) bytes.push(((second & 0x0f) << 4) | (third >> 2));
    if (index + 3 < clean.length) bytes.push(((third & 0x03) << 6) | fourth);
  }
  return bytes;
}
