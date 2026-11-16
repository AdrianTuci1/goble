/**
 * The conversations the worker holds, as the sidebar draws them.
 *
 * A thread started on the phone is stored on the worker under the session id
 * the screen uses, so the drawer lists the same conversation the screen opens —
 * and it is still there after the app is closed. Without a gateway this stays
 * empty and the drawer shows the demo workspace.
 */

import { useEffect, useSyncExternalStore } from "react";

import { useConnection } from "./remote/connection";
import { requestChats, type RemoteChat } from "./remote/entities";
import type { Conversation } from "./types";

let threads: RemoteChat[] = [];
const listeners = new Set<() => void>();

function publish(next: RemoteChat[]): void {
  threads = next;
  for (const listener of listeners) listener();
}

/** Read the worker's threads; a failure leaves the last list in place. */
export function refreshThreads(): void {
  requestChats()
    .then((chats) => publish(chats))
    .catch(() => {
      // A worker that cannot list conversations keeps the current list.
    });
}

export function useWorkerThreads(): RemoteChat[] {
  const remote = useConnection();
  useEffect(() => {
    if (remote.status === "live") refreshThreads();
  }, [remote.status]);
  return useSyncExternalStore(subscribe, getThreads);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getThreads(): RemoteChat[] {
  return threads;
}

/** The drawer's view of one stored conversation. */
export function threadConversation(chat: RemoteChat): Conversation {
  const last = [...chat.messages].reverse().find((message) => message.content);
  return {
    harness: "goble",
    id: chat.id,
    mode: "agent",
    preview: last ? last.content.replace(/\s+/g, " ").slice(0, 80) : "No messages",
    projectName: "Gateway",
    starred: false,
    status: "done",
    title: chat.title,
    updatedAt: chat.updated_at,
  };
}
