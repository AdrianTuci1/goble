# Tools

Tools are how the agent acts on the world. Goble registers a consistent set; each tool has a name, a description and a JSON schema for its arguments. Tool calls are emitted to the renderer and to the trace.

---

## Built-in Tools

| Tool | Description |
|------|-------------|
| `create_agent` / `update_agent` / `delete_agent` | Manage agents |
| `create_workflow` / `update_workflow` / `delete_workflow` | Manage workflows |
| `create_team` / `update_team` / `delete_team` | Manage teams |
| `deploy_agent` / `deploy_workflow` | Deploy to a worker |
| `schedule_workflow` | Run a workflow on a trigger |
| `get_execution_status` | Status of an execution |
| `list_entities` | List agents, workflows, teams or workers |
| `search_store` | Search by name |
| `run_command` | Run an allowed shell command (with timeout) |
| `read_file` / `write_file` / `edit_file` / `rename_file` / `delete_file` | Workspace file operations |
| `git_status` / `git_diff` | Workspace git |
| `codebase_search` | Search the codebase |
| `credentials` | List credential **names** only |
| `principals` | List principals and their grants |
| `user_guide` | Look up a topic in this guide |

---

## `run_command` and Credentials

`run_command` runs an allowed command with a timeout. To use a stored secret without exposing it, write `{{credential:<name>}}` anywhere in the command or its arguments; the harness substitutes the value at execution time and it never appears in the tool's argument or result. See [Credentials](04-credentials.md).

## File Tools

File tools operate **only inside the workspace directory**, unless you provide an absolute path explicitly. Paths are resolved against the workspace root and confined to it.

## The User Guide Tool

`user_guide` lists the topics in this guide, or returns one topic's full text. The agent uses it to answer questions about how Goble works — setup, credentials, workspaces, remote and mobile access — before responding. See [Remote Access](06-remote-access.md) and [Mobile Access](07-mobile-access.md).

---

## Extending Tools

Tools can be extended with [MCP servers](11-mcp-servers.md) for external integrations, and by loading [skills](10-skills.md) into context.
