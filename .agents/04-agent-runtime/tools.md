# 04 — Tools & skills

**Status:** `[~]` partial: tool-call plumbing exists; registry/skills to build
**Owns:** the tool registry that the harness exposes to agents, plus skills (instruction docs)
**Depends on:** [`README.md`](README.md)

## Problem

An agent's capability = the set of tools it can call. Tools come from three sources: built-ins (shell, file, git, web), **MCP servers** (see [`mcp.md`](mcp.md)), and **skills** (documented procedures the agent follows). The harness needs one uniform registry.

## Model

```rust
struct Tool {
    name: String,
    description: String,
    input_schema: Schema,       // JSON schema
    kind: ToolKind,             // Builtin | Mcp { server } | Skill { doc }
    exec: ...,                  // local fn | rpc to worker/MCP
}
```

- **Registry** = named lookup, discoverable by the agent, backed by a prompt/tool list.
- **Skills** = markdown instruction docs (a "how to do X" procedure) loaded and injected into context, often paired with tools. grok-build's `xai-grok-tools` + `xai-grok-tools-api` are the reference.
- **Tool calls stream back** to the renderer (see [`../06-renderer/renderer-architecture.md`](../06-renderer/renderer-architecture.md)) and are recorded in traces (see [`../07-observability/executions-and-trace.md`](../07-observability/executions-and-trace.md)).

## Rules

- Tool selection is the agent's job via the prompt; the harness only exposes what the workspace config allowed (see [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).
- Every tool call must be **observable** and **auditable**.

## Tasks

- [ ] Build the tool registry abstraction with a uniform schema.
- [ ] Load skills as tool-adjacent instruction docs and inject into context.
- [ ] Emit tool-call events to renderer + trace.
