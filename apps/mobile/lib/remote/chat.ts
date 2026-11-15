import { notifyAgentNeedsHelp, notifyRunFinished } from "../notifications";
import { connection } from "./connection";
import type { AgentSpec, CommandDecision } from "./protocol";

/** One turn's stream, as the session screen wants it. */
export interface TurnHandlers {
  onDelta: (delta: string) => void;
  onToolStarted: (call: { id: string; name: string }) => void;
  onToolFinished: (call: { id: string; result?: string; error?: string }) => void;
  onAskUser: (question: string, quickReplies: string[]) => void;
  /**
   * The run suspended before running a command and waits on the user's
   * decision. `candidates` are the proposed lines, most likely first.
   */
  onCommandProposed: (proposal: {
    id: string;
    candidates: string[];
    cwd: string;
  }) => void;
  onDone: () => void;
  onError: (message: string) => void;
}

export interface Turn {
  traceId: string;
  /** Stop listening to the stream; the worker keeps running. */
  detach: () => void;
  /**
   * Answer the question the run paused on. The same run continues, so the
   * stream stays attached and more deltas follow.
   */
  answer: (text: string) => boolean;
  /**
   * Decide the command the run suspended on. The same run continues either
   * way: an approval runs the line, a rejection fails the tool call.
   */
  decideCommand: (decision: CommandDecision) => boolean;
}

let counter = 0;

/**
 * Turns whose screen went away before they ended.
 *
 * A session unmounts its stream when it is closed; the worker keeps running.
 * This watcher keeps listening for that trace and raises the same alerts the
 * screen would have, so a turn started on the phone still reports back when the
 * user is somewhere else.
 */
const abandoned = new Map<string, string>();
let watching = false;

function watchAbandoned() {
  if (watching) return;
  watching = true;
  connection.subscribeMessages((message) => {
    if (!("trace_id" in message)) return;
    const title = abandoned.get(message.trace_id);
    if (!title) return;
    switch (message.kind) {
      case "agent_finished":
        abandoned.delete(message.trace_id);
        void notifyRunFinished(title);
        break;
      case "ask_user":
      case "command_proposed":
        // The run waits on a decision nobody is looking at.
        abandoned.delete(message.trace_id);
        void notifyAgentNeedsHelp(title);
        break;
      default:
        break;
    }
  });
}

function nextId(prefix: string): string {
  counter += 1;
  return `${prefix}-${Date.now().toString(36)}-${counter}`;
}

/**
 * The identity a phone turn runs under.
 *
 * The user's message travels in `prompt`, so this stays short: it says what the
 * turn is, and the worker adds the agent's memory and the thread's transcript
 * around it. The worker stores the spec under a per-turn id, so the phone's
 * turns do not rewrite each other.
 */
function specFor(): AgentSpec {
  const now = new Date().toISOString();
  return {
    id: nextId("phone"),
    name: "Phone session",
    description: "A turn started from the mobile app",
    prompt:
      "You are a coding agent working on the user's project through a remote worker. Answer the message you are given, use the workspace tools you need, and keep the reply short enough to read on a phone.",
    tools: [],
    triggers: ["Manual"],
    mcp_ids: [],
    created_at: now,
    updated_at: now,
  };
}

/**
 * Ask the worker behind the gateway to answer one message in a thread, and
 * stream its events back to the caller.
 *
 * The worker keys the conversation on `threadId`, so its stored transcript keeps
 * every turn and the next turn arrives with the earlier messages in front of it.
 * Events fan out with the turn's `trace_id`; every other frame on the relay
 * (another turn, another pane) is ignored here. A worker that streams deltas
 * shows the answer as it is written; the final `thread_agent_reply` fills the
 * answer in for a harness that only reports it at the end.
 *
 * Returns `null` when the relay is not live, so the caller can fall back to its
 * local transcript instead of showing a turn that will never answer.
 */
export function startTurn(
  threadId: string,
  prompt: string,
  handlers: TurnHandlers,
  title = "Session",
): Turn | null {
  const traceId = nextId("trace");
  const spec = specFor();
  let finished = false;
  let sawDelta = false;
  /**
   * The run is paused on a question or a command proposal. The worker reports
   * it as finished (it has nothing left to await), so the end of the stream is
   * not the end of the run until the user answers.
   */
  let suspended = false;
  let finishTimer: ReturnType<typeof setTimeout> | null = null;
  /**
   * The worker answers a turn it accepted with `agent_started` at once. A turn
   * that never starts (no model key, a worker that refused the request) would
   * otherwise leave the screen waiting on a reply that is not coming.
   */
  let liveness: ReturnType<typeof setTimeout> | null = null;
  let detachNow = () => {};

  const started = () => {
    if (!liveness) return;
    clearTimeout(liveness);
    liveness = null;
  };

  const complete = (call: () => void) => {
    if (finished) return;
    finished = true;
    if (finishTimer) clearTimeout(finishTimer);
    if (liveness) clearTimeout(liveness);
    finishTimer = null;
    liveness = null;
    detachNow();
    call();
  };

  /** The run is over; give a reply-only harness a beat to send its text. */
  const settle = () => {
    if (suspended) return;
    if (sawDelta) complete(handlers.onDone);
    else finishTimer = setTimeout(() => complete(handlers.onDone), 400);
  };

  detachNow = connection.subscribeMessages((message) => {
    if (!("trace_id" in message) || message.trace_id !== traceId) return;
    switch (message.kind) {
      case "agent_started":
        started();
        break;
      case "assistant_delta":
        started();
        sawDelta = true;
        handlers.onDelta(message.delta);
        break;
      case "thread_agent_reply":
        started();
        // A run parked on a question or a proposal has no answer yet; its
        // reply arrives once the decision resumes it.
        if (suspended) break;
        if (!sawDelta && message.content) {
          sawDelta = true;
          handlers.onDelta(message.content);
        }
        complete(handlers.onDone);
        break;
      case "tool_call_started":
        started();
        handlers.onToolStarted({ id: message.id, name: message.name });
        break;
      case "tool_call_finished":
        handlers.onToolFinished({ id: message.id, result: message.result });
        break;
      case "tool_call_error":
        handlers.onToolFinished({ id: message.id, error: message.message });
        break;
      case "ask_user":
        started();
        suspended = true;
        handlers.onAskUser(message.question, message.quick_replies);
        break;
      case "command_proposed":
        started();
        suspended = true;
        handlers.onCommandProposed({
          id: message.id,
          candidates: message.candidates,
          cwd: message.cwd,
        });
        break;
      case "agent_log":
        if (message.level === "error") handlers.onError(message.message);
        break;
      case "done":
        settle();
        break;
      case "agent_finished":
        if (message.status === "failed" || message.status === "cancelled") {
          complete(() => handlers.onError(`The run ended: ${message.status}`));
        } else {
          settle();
        }
        break;
      default:
        break;
    }
  });

  const sent = connection.send({
    kind: "run_agent_for_thread_reply",
    trace_id: traceId,
    thread_id: threadId,
    agent_id: spec.id,
    prompt,
    spec,
    mcp_servers: [],
  });
  if (!sent) {
    detachNow();
    return null;
  }
  liveness = setTimeout(
    () =>
      complete(() =>
        handlers.onError(
          "The worker did not start this run. Check its model key and that it is online.",
        ),
      ),
    8000,
  );
  return {
    traceId,
    detach: () => {
      if (!finished) {
        // The screen left first; the watcher above tells the user how it ended.
        abandoned.set(traceId, title);
        watchAbandoned();
      }
      detachNow();
    },
    answer: (text: string) => {
      suspended = false;
      sawDelta = false;
      return connection.send({
        answer: text,
        kind: "answer_user",
        trace_id: traceId,
      });
    },
    decideCommand: (decision: CommandDecision) => {
      suspended = false;
      sawDelta = false;
      return connection.send({
        decision,
        kind: "answer_command",
        trace_id: traceId,
      });
    },
  };
}
