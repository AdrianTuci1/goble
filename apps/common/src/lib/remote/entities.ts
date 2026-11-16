/**
 * Request/response over the worker relay.
 *
 * The relay carries the desktop's fire-and-forget protocol, so a read is a
 * `DesktopMessage` out and a matching `WorkerMessage` back with no request id of
 * its own. This module pairs the two by kind and hands the caller a promise, so
 * a screen can await the worker's agents, schedules or conversations the way it
 * would await an HTTP call. The wire shapes stay in `protocol.ts`.
 */

import { connection } from "./connection";
import type { AgentSpec } from "./protocol";

/** One agent the worker has stored, as `query_entities` reports it. */
export interface RemoteAgent {
  id: string;
  name: string;
  spec: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

/** One routine the worker's scheduler holds. */
export interface RemoteSchedule {
  id: string;
  agent_id: string;
  trigger: unknown;
  enabled: boolean;
}

/** One conversation the worker has stored, with its transcript. */
export interface RemoteChat {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  messages: RemoteChatMessage[];
}

export interface RemoteChatMessage {
  id: string;
  role: string;
  content: string;
  created_at: string;
}

const REQUEST_TIMEOUT_MS = 8_000;

interface Waiter {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

/** Waiters per response kind, so two screens can ask at the same time. */
const waiters = new Map<string, Waiter[]>();

function settle(key: string, value: unknown): void {
  const group = waiters.get(key);
  if (!group) return;
  waiters.delete(key);
  for (const waiter of group) {
    clearTimeout(waiter.timer);
    waiter.resolve(value);
  }
}

connection.subscribeMessages((message) => {
  if (message.kind === "entity_list") {
    settle(`entities:${message.entity_type}`, message.items);
  } else if (message.kind === "scheduled_tasks") {
    settle("scheduled_tasks", message.tasks);
  }
});

/**
 * Send `message` and wait for `key`. A link that is not live fails at once,
 * and a worker that never answers fails after the timeout so a screen can show
 * the reason instead of a spinner that never stops.
 */
function request<T>(
  key: string,
  send: () => boolean,
  timeoutMs = REQUEST_TIMEOUT_MS,
): Promise<T> {
  if (!send()) {
    return Promise.reject(new Error("The worker link is not live"));
  }
  return new Promise<T>((resolve, reject) => {
    const group = waiters.get(key) ?? [];
    const waiter: Waiter = {
      reject,
      resolve: (value) => resolve(value as T),
      timer: setTimeout(() => {
        const current = waiters.get(key);
        if (!current) return;
        const index = current.indexOf(waiter);
        if (index >= 0) current.splice(index, 1);
        if (current.length === 0) waiters.delete(key);
        reject(new Error("The worker did not answer"));
      }, timeoutMs),
    };
    group.push(waiter);
    waiters.set(key, group);
  });
}

/** Agents the worker has stored. */
export function requestAgents(): Promise<RemoteAgent[]> {
  return request<RemoteAgent[]>("entities:agents", () =>
    connection.send({ entity_type: "agents", kind: "query_entities" }),
  );
}

/** The conversations the worker has stored, newest first. */
export function requestChats(): Promise<RemoteChat[]> {
  return request<RemoteChat[]>("entities:chats", () =>
    connection.send({ entity_type: "chats", kind: "query_entities" }),
  );
}

/** The routines the worker's scheduler holds. */
export function requestScheduledTasks(): Promise<RemoteSchedule[]> {
  return request<RemoteSchedule[]>("scheduled_tasks", () =>
    connection.send({ kind: "list_scheduled_tasks" }),
  );
}

/** Create or rewrite an agent; the store upserts on the spec's id. */
export function saveAgent(spec: AgentSpec): boolean {
  return connection.send({
    agent_id: spec.id,
    kind: "update_agent",
    spec,
  });
}

/** Remove an agent from the worker's store. */
export function removeAgent(agentId: string): boolean {
  return connection.send({ agent_id: agentId, kind: "remove_agent" });
}

/** Drop a routine; the worker confirms with `task_cancelled`. */
export function cancelScheduledTask(taskId: string): boolean {
  return connection.send({ kind: "cancel_scheduled_task", task_id: taskId });
}

