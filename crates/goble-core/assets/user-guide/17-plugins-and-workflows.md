# Workflows and Plugins

Beyond single turns, Goble can run **workflows** — multi-step routines — and be extended with **plugins** and user-installed skills.

---

## Workflows

A workflow is a named list of steps. Create it via the `create_workflow` tool with steps, then either run it or **schedule** it on a trigger:

| Trigger | Description |
|---------|-------------|
| `cron` | Run on a schedule |
| `http` | Run when an HTTP request arrives |
| `heartbeat` | Run on a periodic heartbeat |

Workflows can be **deployed** to a worker and their execution status checked with `get_execution_status`.

A **deep-research** routine is a workflow of steps (search, reason, synthesize) with a budget and progress events; the result can be saved to workspace memory.

## Plugins

Plugins (and user-installed skills and workflows) live under `~/.goble/plugins/`, `~/.goble/skills/`, and `~/.goble/workflows/`. They extend the harness with new capabilities loaded into the workspace payload when the workspace runs locally.

## The Marketplace

`~/.goble/marketplace-cache/` stores marketplace metadata for discoverable plugins and skills. Installed content can be referenced by the agent like any bundled capability (see [Skills](10-skills.md)).

---

## Related

- [Tools](09-tools.md) — the workflow management tools.
- [Agents](08-agents.md) — the agents a workflow may orchestrate.
