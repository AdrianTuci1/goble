# Agents

An **agent** is a named unit of work in the harness: a prompt, a tool set, optional MCP connections, and (on the worker) a persona. Agents are persisted in the store, so they survive restarts and can be targeted by a conversation.

---

## Creating an Agent

Ask the agent in the chat, e.g. "create an agent called `code_review` that reviews PRs". It calls `create_agent` with:

| Field | Meaning |
|-------|---------|
| `id` | Stable id (slug) |
| `name` | Display name |
| `description` | What the agent does |
| `prompt` | The system/instruction prompt |
| `tools` | Tool names the agent may use |
| `mcp_ids` | MCP connections it can reach |

Agents can also be created or edited from natural language and bound to a workspace config.

---

## Sub-agents

A sub-agent is an agent with a **bounded budget and lifecycle**. The harness spawns sub-agents so the main chat loop stays responsive while a long-running task works in parallel. Its progress and results are surfaced back into the main thread.

## Teams

A **team** groups agents. Create a team, name it, and list its `agent_ids`. Teams make sense when a task needs several roles collaborating; each team can also be deployed to a worker.

---

## Lifecycle

- **Create / update / delete** — via the corresponding tools (`create_agent`, `update_agent`, `delete_agent`).
- **Deploy** — attach an agent to a worker with `deploy_agent` (see [Remote Access](06-remote-access.md)).
- **Persist** — agents (and per-agent conversation state) are stored so an agent survives restarts and local↔remote routing.

## Related

- [Tools](09-tools.md) — the tool set an agent uses.
- [Personas](10-skills.md) — the persona that informs an agent's system prompt.
