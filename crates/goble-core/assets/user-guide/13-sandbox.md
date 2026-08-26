# Sandbox and CWD

Goble confines the agent's shell and file operations to a workspace root, and gives each agent (and sub-agent) its own working directory.

---

## Command Sandbox

`run_command` is **allowlisted** and runs with a **timeout**. Not every command is permitted: the harness checks the command against the allowlist before spawning it. When a command isn't allowed, the agent is told and can choose a permitted path.

## Working Directory

Each agent and sub-agent gets a CWD subdirectory under the workspace root — `workspace/<agent>/` — so parallel or nested agents don't collide. File tools resolve and confine to the logical workspace root: a relative path is joined to it, and an absolute path must begin inside it (or be explicitly allowed by the user).

## Isolation

Isolation (filesystem and network) is enforced **per workspace**, reusing the platform sandbox where portable. Remote workspaces carry the same sandbox rules; the agent's CWD and constraints follow the workspace, not the machine.

---

## Related

- [Tools](09-tools.md) — `run_command` and the file tools.
- [Workspaces](02-workspaces.md) — the workspace root that defines the boundary.
