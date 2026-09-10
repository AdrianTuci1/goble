# 08 — Threads & messaging server (deferred)

**Status:** `[ ]` deferred — direction recorded
**Owns:** the messaging server that gives a workspace its "threads" and human↔agent channels
**Depends on:** [`../03-workspace-model/workspace.md`](../03-workspace-model/workspace.md), [`../04-agent-runtime/inter-agent-communication.md`](../04-agent-runtime/inter-agent-communication.md)

## Direction

A **worker hosts the thread-messaging server**. Combined with agents + crons + secrets, that makes the worker a **workspace**. Threads are the shared channel where human participants and agents all talk.

- The thread-messaging server is where the **inter-agent bus** and the **human threads** converge — one channel, many participant kinds (humans + agents).
- The `goble-core::thread` module already has `Thread`, `ThreadMessage`, participants, reactions, tags — a solid base.
- DesktopState exposes `thread_store()`, `ThreadSummary`/`ThreadMessageSummary`, `migrate_legacy_chats_to_threads`, `run_agent_for_thread_reply`.

## Why deferred

The threads UI needs the workspace model + the worker-hosted server to be real first. Deferring threads UI does not defer the backend/architecture — we keep the same channel concept for inter-agent communication and the future messaging server.

## Tasks (when this is picked up)

- [ ] Define the thread-messaging server hosted on the worker (participants = humans + agents).
- [ ] Reuse `goble-core::thread` types and the existing thread store.
- [ ] Add the threads UI to the native shell (the legacy React app has a Threads page).
