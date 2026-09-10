# Workspaces

A machine, VM or cluster that runs Goble is **one workspace**, and that workspace is the worker a task runs on. A single user carries one identity but may drive several workspaces; each workspace has its own home folder and its own set of principals with access.

A workspace `kind` is either **local** or **remote**:

- **Local** — this machine runs the agent and its tools directly.
- **Remote** — the workspace lives on a provisioned host (a worker) you connected to; this machine acts as a client.

Promoting a workspace local → remote keeps the same id, so agents, conversations and state survive the move.

---

## The `~/.goble` Home

The home folder is split into two layers, mirroring `~/.grok`:

### Base (every machine)
Always created, regardless of whether the workspace runs locally or remotely:

```
~/.goble/
  config.toml          # agent-visible config
  README.md
  version.json
  principal_id         # identity of this machine's principal
  auth.json            # session credentials
  goble_store.sqlite   # app state
  docs/user-guide/     # this guide, seeded on first launch
  sessions/            # per-session transcripts
  logs/
  principals/<id>/     # per-principal context for every principal with access
  relocations/
```

### Workspace payload (this machine backends)
Only when the workspace runs **on this machine** (local / self-as-worker):

```
~/.goble/
  bundles/agents/  bundles/roles/  bundles/personas/  bundles/skills/
  worktrees/
  threads/
  downloads/  bin/  completions/
  plugins/  skills/  workflows/
  vendor/  marketplace-cache/
```

A remote-only client never materializes the payload — it holds identity and essentials locally and reaches the worker over the wire.

---

## Choosing a Workspace

On first launch Goble presents a workspace-type prompt. Pick **local** to run the agent on this machine, or **remote** to point at a provisioned worker. The choice is persisted and can be changed later; routing stays attached to the conversation so each chat can override which workspace it targets.

---

## Related

- [Remote Access](06-remote-access.md) — exposing a machine, self-as-worker, and connecting from elsewhere.
- [Configuration](16-configuration.md) — the `config.toml` the agent can read and patch.
