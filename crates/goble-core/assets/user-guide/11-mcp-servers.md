# MCP Servers

Model Context Protocol (MCP) servers let the agent reach external tools and data — GitHub, databases, filesystems, your own APIs — through a standard schema.

---

## Adding a Server

MCP servers are configured per workspace; each account is linked to a **principal** and to the credentials the server needs (by `secret_ids`). From the app's MCP panel you can:

- Add a server by name, command and its config.
- Attach MCP credentials (referenced by credential name, never by value — see [Credentials](04-credentials.md)).
- See the server's status and the tool list it exposes.

## Agent Use

The agent can be given an MCP connection by id (via an agent's `mcp_ids`). It can also **request an MCP install mid-conversation**; the harness surfaces the request and the install proceeds from the harness, not only from the UI panel.

## Status and Tools

When a server is connected, Goble surfaces its status and the tool list it contributes in the renderer. Tool calls go out to the server and their results stream back into the turn.

---

## Related

- [Tools](09-tools.md) — the tool set MCP servers extend.
- [Principals and Access](05-principals-and-access.md) — each MCP account is linked to a principal.
