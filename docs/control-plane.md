# The control plane

A running Goble window serves one Unix control socket. Two clients speak to it
today: the `ctl` subcommand of `goble-cli`, and `goble-mcp`, a Model Context
Protocol server on stdio. Both use the typed protocol in `goble-control`; there
is no second implementation of the verbs.

The control plane is local and user-scoped. It is not a network listener.

## The socket

The app binds the socket at startup, after the window and its state exist:

1. `$GOBLE_CONTROL_SOCKET`, when set and non-empty.
2. `$HOME/.goble/control.sock` otherwise.

The socket file is created mode `0600`, and a missing parent directory is
created mode `0700`. If another process already accepts connections there, the
app logs a warning and keeps running without a control surface; it never
deletes a live socket. A stale socket (a Unix socket with no listener) is
replaced. On clean shutdown the app removes its socket.

A control call while the app is not running fails with a structured
`unavailable` error naming the socket and asking whether the app is running.

## Wire protocol

One newline-delimited JSON object per message, in both directions. There are no
unsolicited events: one request gets exactly one response.

Request:

```json
{"id": 7, "verb": "panes.send", "params": {"pane_id": 3, "text": "ls", "enter": true}}
```

Success:

```json
{"id": 7, "result": {"kind": "sent", "send": {"pane_id": 3, "bytes": 2, "entered": true}}}
```

Failure:

```json
{"id": 7, "error": {"code": "missing_pane", "message": "pane 3 is not open"}}
```

`id` is a per-connection integer chosen by the client. Unknown verbs are a
structured `unknown_verb` error, not a dropped connection. Messages larger than
4 MiB are refused before parsing.

### Verbs

| Verb | Params | Result `kind` |
|---|---|---|
| `panes.list` | — | `panes` |
| `panes.new` | `space_id?`, `cwd?`, `kind?` (`chat` \| `terminal`) | `pane` |
| `panes.split` | `pane_id`, `axis` (`horizontal` \| `vertical`) | `pane` |
| `panes.close` | `pane_id` | `closed` |
| `panes.focus` | `pane_id` | `pane` |
| `panes.send` | `pane_id`, `text`, `enter?` | `sent` |
| `panes.resize` | `pane_id`, `cols`, `rows` | `ok` |
| `spaces.list` | — | `spaces` |
| `spaces.switch` | `space_id` | `space` |
| `agents.state` | `pane_id?` | `agent_states` |
| `agents.prompt` | `pane_id`, `text` | `prompt` |
| `inbox.list` | — | `inbox` |
| `inbox.ack` | `id?`, `all?` | `acknowledged` |
| `layout.get` | `space_id` | `layout` |
| `layout.set` | `space_id`, `kind` (`single` \| `horizontal` \| `vertical` \| `bsp` \| `master_stack` \| `floating`) | `layout` |
| `pty.list` | — | `pty_sessions` |
| `pty.attach` | `session_id` | `pty_session` |
| `pty.detach` | `session_id` | `ok` |
| `pty.kill` | `session_id` | `ok` |

`panes.send` writes text, not a command: Enter is pressed only when `enter` is
true. `layout.set single` closes every other pane in the space. `inbox.ack`
with neither `id` nor `all` is a valid request that acknowledges nothing.

### Error codes

| Code | Meaning |
|---|---|
| `unknown_verb` | Not one of the verbs above. |
| `invalid_request` | The verb's parameters are missing or out of range. |
| `missing_pane` | The named pane is not open. |
| `missing_space` | The named space is not open. |
| `missing_item` | The named inbox item is gone. |
| `unsupported` | Valid verb, but the current window state refuses it (for example, closing the last pane). |
| `pty_unavailable` | The PTY daemon is not reachable or refused the operation. |
| `unavailable` | The app is not running, is shutting down, or did not answer in time. |
| `io` | The control socket itself failed. |
| `internal` | A handler bug; the app keeps running. |

## `goble ctl`

The command-line client is the `ctl` subcommand of `goble-cli`:

```sh
goble-cli ctl <verb> [--json] [--socket PATH]
```

`--json` prints the typed payload (or a structured error on stderr).
`--socket PATH` overrides the default socket; `--json` and `--socket` are global
to every verb, so they may appear before or after it.

| Verb | Example |
|---|---|
| `list-panes` | `goble-cli ctl list-panes` |
| `new-pane` | `goble-cli ctl new-pane --space-id 0 --cwd /tmp --kind terminal` |
| `split-pane` | `goble-cli ctl split-pane 3 --axis vertical` |
| `close-pane` | `goble-cli ctl close-pane 3` |
| `focus-pane` | `goble-cli ctl focus-pane 3` |
| `send-input` | `goble-cli ctl send-input 3 "cargo test" --enter` |
| `resize-pane` | `goble-cli ctl resize-pane 3 120 40` |
| `list-spaces` | `goble-cli ctl list-spaces` |
| `switch-space` | `goble-cli ctl switch-space 1` |
| `agent-state` | `goble-cli ctl agent-state --pane-id 3` |
| `prompt-agent` | `goble-cli ctl prompt-agent 3 "review the diff"` |
| `inbox` | `goble-cli ctl inbox` |
| `ack-inbox` | `goble-cli ctl ack-inbox --id i1` or `--all` |
| `layout` | `goble-cli ctl layout 0` (read), `--kind horizontal` (set) |
| `pty-list` | `goble-cli ctl pty-list` |
| `pty-attach` | `goble-cli ctl pty-attach session-1` |
| `pty-detach` | `goble-cli ctl pty-detach session-1` |
| `pty-kill` | `goble-cli ctl pty-kill session-1` |

`new-pane` defaults to a terminal in the active space, `split-pane` defaults to
a horizontal split, and `layout` without `--kind` reads the layout.

A missing app or socket is a structured error and a non-zero exit:

```console
$ goble-cli ctl list-panes
error: unavailable: could not connect to the Goble control socket at /home/me/.goble/control.sock: No such file or directory (os error 2); is the Goble app running?

$ goble-cli ctl --json list-panes
{"error":{"code":"unavailable","message":"could not connect to the Goble control socket at ...; is the Goble app running?"}}
```

## `goble-mcp`

`goble-mcp [--socket PATH]` is an MCP server over stdio: newline-delimited
JSON-RPC 2.0 on stdin/stdout, diagnostics on stderr. It implements the three
methods an MCP client needs:

- `initialize` — negotiates a protocol revision and reports
  `serverInfo.name = "goble-mcp"` and the `tools` capability. Supported
  revisions are `2024-11-05`, `2025-03-26`, and `2025-06-18`; an unknown
  request falls back to `2025-06-18`. `notifications/initialized` is accepted.
- `tools/list` — returns the ten tools below with their JSON input schemas.
- `tools/call` — runs one tool against the control socket.
- `ping` is answered with `{}` as well.

Tool names and their control verbs:

| Tool | Args | Control verb |
|---|---|---|
| `goble_list_panes` | — | `panes.list` |
| `goble_new_pane` | `space_id?`, `cwd?`, `kind?` | `panes.new` |
| `goble_split_pane` | `pane_id`, `axis?` (default `horizontal`) | `panes.split` |
| `goble_close_pane` | `pane_id` | `panes.close` |
| `goble_send_input` | `pane_id`, `text`, `enter?` | `panes.send` |
| `goble_agent_state` | `pane_id?` | `agents.state` |
| `goble_inbox` | — | `inbox.list` |
| `goble_ack_inbox` | `id` or `all: true` | `inbox.ack` |
| `goble_switch_space` | `space_id` | `spaces.switch` |
| `goble_layout` | `space_id`, `kind?` | `layout.get` when `kind` is absent, else `layout.set` |

A successful call returns the typed payload as one text block:

```json
{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\"kind\":\"panes\",\"panes\":[...]}"}],"isError":false}}
```

A control failure (missing pane, app not running, refused operation) is a tool
result with `isError: true`; its text is the structured error:

```json
{"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"{\"error\":{\"code\":\"missing_pane\",\"message\":\"pane 99 is not open\"}}"}],"isError":true}}
```

Argument mistakes are JSON-RPC errors instead, because no tool ran:

- unknown method: `-32601`
- unknown tool or malformed arguments: `-32602`
- unparseable line: `-32700`

Example session:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"goble_list_panes","arguments":{}}}
```

The server opens one control-socket connection per tool call, so an app restart
does not leave a stale connection behind.

## Testing

```sh
cargo test -p goble-control   # the protocol, codec, and socket server
cargo test -p goble-mcp       # initialize/tools/list/tools/call, a mock backend,
                              # and the tools end-to-end against an in-process server
cargo test -p goble-cli       # ctl parsing and the ctl -> command mapping
```

The app-side dispatch lives in `app/src/control/mod.rs`; the shared vocabulary
and transport live in `crates/goble-control`.
