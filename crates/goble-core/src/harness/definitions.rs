use crate::llm::ToolDefinition;

pub(crate) const HARNESS_SYSTEM_PROMPT: &str = r#"You are an assistant that controls a local Goble agent environment.
You can create/update agents, workflows and teams, run shell commands, read/write files, search the store, deploy agents/workflows to workers, schedule workflows, and check execution status.
Use memory_write to record goals, decisions, constraints and progress into an agent's persistent memory; use memory_read to review what an agent already knows. Memory survives conversation summarization.
Only call a tool when the user explicitly asks for an action you can perform with a tool.
If no tool is needed, reply conversationally.
There is a user guide for this product at ~/.goble/docs/user-guide. For questions about how to set up or use Goble (credentials, workspaces, principals, remote/self-as-worker access, reaching this machine from the mobile app via Tailscale), call the user_guide tool to look up the relevant topic before answering.
When reading or writing files, paths must be inside the workspace directory unless the user explicitly provides an absolute path."#;

/// The tool that spawns a sub-agent. Named here because the child's tool set
/// withholds it once the depth limit is reached (S1's `can_spawn_subagent`).
pub(crate) const SPAWN_SUBAGENT_TOOL: &str = "spawn_subagent";

pub(crate) fn harness_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_agent".to_string(),
            description: "Create a new agent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "prompt": { "type": "string" },
                    "tools": { "type": "array", "items": { "type": "string" } },
                    "mcp_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id", "name", "prompt"]
            }),
        },
        ToolDefinition {
            name: "update_agent".to_string(),
            description: "Update an existing agent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "prompt": { "type": "string" },
                    "tools": { "type": "array", "items": { "type": "string" } },
                    "mcp_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "create_workflow".to_string(),
            description: "Create a new workflow.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "update_workflow".to_string(),
            description: "Update an existing workflow by replacing its definition.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "create_team".to_string(),
            description: "Create a new team of agents.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "metadata": { "type": "object" },
                    "agent_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id", "name"]
            }),
        },
        ToolDefinition {
            name: "update_team".to_string(),
            description: "Update a team by replacing its metadata and members.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "metadata": { "type": "object" },
                    "agent_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Run an allowed shell command with a timeout. To use a stored credential without exposing it, write {{{{credential:<name>}}}} anywhere in the command or args; the harness substitutes the stored value at execution time and it never appears in this tool's arguments or result.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "credentials".to_string(),
            description: "List the names of stored credentials (API keys, tokens). Values are never returned; to use a credential inside run_command, reference it as {{{{credential:<name>}}}}.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "principals".to_string(),
            description: "List every principal with access to this workspace, their kind/name and the grants (grant:scope) each holds.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "user_guide".to_string(),
            description: "Look up a topic in the Goble user guide at ~/.goble/docs/user-guide. Call with no topic to list the available topics, or with a topic name to read that guide entry in full. Use this to answer questions about how to set up or use Goble: workspaces, credentials, principals, agents, tools, sandbox, remote access (exposing a machine via Tailscale / self-as-worker) and mobile access.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "topic": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "list_entities".to_string(),
            description: "List existing agents, workflows, teams or workers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_type": { "type": "string", "enum": ["agents", "workflows", "teams", "workers"] }
                },
                "required": ["entity_type"]
            }),
        },
        ToolDefinition {
            name: "search_store".to_string(),
            description: "Search agents, workflows and teams by name.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "entity_types": { "type": "array", "items": { "type": "string", "enum": ["agents", "workflows", "teams"] } }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "deploy_agent".to_string(),
            description: "Deploy an agent to a worker.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "worker_id": { "type": "string" }
                },
                "required": ["agent_id", "worker_id"]
            }),
        },
        ToolDefinition {
            name: "deploy_workflow".to_string(),
            description: "Deploy a workflow to a worker.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string" },
                    "worker_id": { "type": "string" }
                },
                "required": ["workflow_id", "worker_id"]
            }),
        },
        ToolDefinition {
            name: "schedule_workflow".to_string(),
            description: "Schedule a workflow to run on a trigger.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string" },
                    "trigger_type": { "type": "string", "enum": ["cron", "http", "heartbeat"] },
                    "trigger_value": { "type": "string" }
                },
                "required": ["workflow_id", "trigger_type", "trigger_value"]
            }),
        },
        ToolDefinition {
            name: "get_execution_status".to_string(),
            description: "Get the status of an execution by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "execution_id": { "type": "string" }
                },
                "required": ["execution_id"]
            }),
        },
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write a file inside the workspace. Creates parent directories.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "delete_agent".to_string(),
            description: "Delete an agent by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "delete_workflow".to_string(),
            description: "Delete a workflow by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "delete_team".to_string(),
            description: "Delete a team by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "rename_file".to_string(),
            description: "Rename or move a file inside the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" }
                },
                "required": ["from", "to"]
            }),
        },
        ToolDefinition {
            name: "delete_file".to_string(),
            description: "Delete a file inside the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "git_status".to_string(),
            description: "Run git status in the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "git_diff".to_string(),
            description: "Run git diff in the workspace. Optionally pass a path.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "git_commit".to_string(),
            description: "Stage files and create a git commit.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "files": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["message"]
            }),
        },
        ToolDefinition {
            name: "codebase_search".to_string(),
            description: "Search for a regex inside workspace files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "install_mcp_server".to_string(),
            description: "Install or register an MCP server in the store. Use npm source for npx packages, local for directories, github for repos, or url for direct archives.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "source": { "type": "string", "enum": ["github", "npm", "local", "url", "stdio"] },
                    "source_value": { "type": "string", "description": "package name, owner/repo, path or url depending on source" }
                },
                "required": ["id", "name", "source"]
            }),
        },
        ToolDefinition {
            name: "delete_mcp_server".to_string(),
            description: "Delete a registered MCP server from the store and stop its running client.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "update_mcp_server".to_string(),
            description: "Update a registered MCP server configuration or credentials.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "source_value": { "type": "string" },
                    "manifest": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "search_mcp_servers".to_string(),
            description: "Search the MCP registry and public repositories for available MCP servers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "discover_mcp_tools".to_string(),
            description: "Discover and register concrete tools from an installed MCP server. Use this after installing an MCP server or when its tools are not yet exposed.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Installed MCP server id" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "list_mcp_servers".to_string(),
            description: "List registered MCP servers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web and return results with title, snippet and URL. Uses the configured hosted search backend when its API key is set, otherwise DuckDuckGo. `advanced` runs a deeper search: when using DuckDuckGo it follows pagination to gather more distinct results (defaulting to up to 30); when a hosted backend is configured it requests the advanced model.".
                to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 30, "description": "Max results (default 10, 30 for advanced)" },
                    "advanced": { "type": "boolean", "description": "Deep/advanced search" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "read_url".to_string(),
            description: "Fetch a URL and extract the main text content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "execute_python_code".to_string(),
            description: "Execute Python code in a temporary file and return stdout/stderr.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string" }
                },
                "required": ["code"]
            }),
        },
        ToolDefinition {
            name: "run_agent".to_string(),
            description: "Run a local agent once and return its prompt output.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "input": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: SPAWN_SUBAGENT_TOOL.to_string(),
            description: "Spawn a sub-agent: a real child run on its own bounded conversation and its own working directory. It executes on its own budget and tool set, and its record (activity, elapsed, output) is readable while it runs. `description` is the one-line summary shown in the transcript; `subagent_type` is the child's role; `prompt` is the full instruction the child works from; `run_in_background: false` awaits the child and returns its output as this tool's result (a child that outlives the wait moves to the background and the result is its id), `run_in_background: true` returns the child's id at once and the child keeps running; `cwd` optionally places the child in a workspace-relative directory instead of its own derived one.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "One line, shown on the child's transcript row" },
                    "prompt": { "type": "string", "description": "The instruction the child runs from" },
                    "subagent_type": { "type": "string", "description": "The child's role, e.g. reviewer or researcher" },
                    "run_in_background": { "type": "boolean", "description": "false awaits the child's output; true returns the child's id at once and keeps it running" },
                    "cwd": { "type": "string", "description": "Optional workspace-relative working directory for the child" }
                },
                "required": ["description", "prompt", "subagent_type", "run_in_background"]
            }),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Replace old text with new text in a workspace file.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string" },
                    "new_text": { "type": "string" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
        ToolDefinition {
            name: "memory_write".to_string(),
            description: "Write to an agent's persistent memory (brief, facts, goals, constraints, decisions, milestones, open questions). Memory survives conversation summarization."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "section": { "type": "string", "enum": ["brief", "fact", "goal", "complete_goal", "constraint", "decision", "milestone", "complete_milestone", "open_question"] },
                    "content": { "type": "string" },
                    "rationale": { "type": "string" }
                },
                "required": ["agent_id", "section", "content"]
            }),
        },
        ToolDefinition {
            name: "memory_read".to_string(),
            description: "Read an agent's full persistent memory block.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "open_screen".to_string(),
            description: "Open a remote desktop over the harness window and hand off to it. The agent calls this when it needs a real GUI; the host opens the stream in a screen pane and routes your keyboard/mouse to it. `host` is required; `port` defaults to 3389 and `width`/`height` to 1280x720.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string" },
                    "port": { "type": "integer" },
                    "username": { "type": "string" },
                    "password": { "type": "string" },
                    "width": { "type": "integer" },
                    "height": { "type": "integer" }
                },
                "required": ["host"]
            }),
        },
    ]
}
