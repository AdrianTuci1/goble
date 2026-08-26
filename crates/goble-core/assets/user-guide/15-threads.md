# Threads

Goble can host a **thread server** on a worker: a messaging channel whose participants are humans and agents. Threads are how a workspace's members talk — a person and an agent, or several agents collaborating.

---

## How It Works

- The **thread server** runs on the worker. Each thread has participants (humans and agents) and a message history.
- Goble reuses the core `thread` types and the existing thread store for persistence.
- The thread server's state lives under `~/.goble/threads/` when the workspace runs locally (the workspace payload).

## Using Threads

- Create a thread and add participants.
- Messages posted by a human become turns for the agents in the thread; agents can post results back.
- The threads **UI** is available in the native shell, alongside the chat view.

## Related

- [Workspaces](02-workspaces.md) — where thread state lives.
- [Agents](08-agents.md) — the agents that participate in a thread.
