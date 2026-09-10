# Getting Started

Goble is a native desktop app that runs an autonomous agent on your machine. It understands your workspace, runs shell commands, reads and writes files, manages credentials, searches the web, and orchestrates agents and workflows. The UI is a hand-written wgpu renderer — no browser, no embedded webview.

Every machine (or VM, or cluster) that runs Goble is **one workspace**. On that machine Goble keeps a single hidden home folder, `~/.goble`, that mirrors the structure you may already know from `~/.grok`.

---

## Installation

Install the latest release from the Goble releases page, then launch the app. macOS:

```bash
brew install --cask goble
open -a Goble
```

Linux (AppImage):

```bash
chmod +x goble.AppImage
./goble.AppImage
```

Windows uses the same installer flow as macOS. Goble writes everything under the current user's home, so once it runs it needs no extra setup beyond adding a model provider.

---

## First Launch

On first launch, Goble asks you to choose a model provider and provide an API key. The key is stored as a **credential** — see [Credentials](04-credentials.md) — so the agent can use your tools without ever seeing the key in the conversation.

Goble seeds `~/.goble` on first open:

```
~/.goble/
  config.toml                 # agent-visible configuration
  version.json
  principal_id                # identity of this machine's principal
  auth.json                   # session credentials
  goble_store.sqlite          # app state (chats, agents, messages, credentials, grants)
  docs/user-guide/*.md        # this guide
  sessions/  logs/  principals/
  bundles/agents  bundles/skills  bundles/personas ...
  worktrees/  threads/  downloads/  bin/  plugins/  skills/  workflows/
```

The **base** directory (identity, auth, config, sessions, logs, docs, principals) exists for every machine. The **workspace payload** (bundled tooling, worktrees, threads, local store) only materializes when the workspace actually runs on this machine — a remote-only client keeps a minimal home.

---

## The Chat

After setup you land in the chat view. At the bottom is a floating composer card where you type a message. Send it and the agent runs a turn:

- **User prompt** — your message, rendered as a header.
- **Agent message** — Goble's answer, markdown-rendered.
- **Reasoning block** — the agent's (collapsible) chain of thought.
- **Tool calls** — rendered as cards: commands that ran, files read or edited, web searches, tool invocations. The assistant message shows a `⚙ tool` chip for each one.

The model selector, profile, attach and stop buttons live in the composer card. Stop cancels the running turn; an autonomy toggle controls whether Goble asks before each tool run.

---

## Key Concepts

### Workspaces
A workspace is the worker a task runs on. It can be **local** (this machine) or **remote** (a provisioned host). See [Workspaces](02-workspaces.md).

### Principals
A principal is an identity that can reach a workspace — the local user, a remote operator, a service account. Goble records every principal that has access and the grants it holds. See [Principals and Access](05-principals-and-access.md).

### Credentials
Secrets (API keys, tokens) are stored by name; Goble exposes only the **name** to the agent and substitutes the value at execution time. See [Credentials](04-credentials.md).

### Agents
An agent is a prompt plus a tool set and optional MCP connections, persisted in the store. You can also create teams of agents, or spawn sub-agents for parallel work. See [Agents](08-agents.md).

### Tools
Goble exposes tools for shell, files, credentials, principals, search, MCP servers and the user guide. See [Tools](09-tools.md).

---

## Where to Go Next

| Document | What You Will Learn |
|----------|-------------------|
| [Workspaces](02-workspaces.md) | Local vs remote workspaces and the `~/.goble` layout |
| [Authentication](03-authentication.md) | Model providers, API keys, identity |
| [Credentials](04-credentials.md) | How secrets stay hidden from the model |
| [Remote Access](06-remote-access.md) | Exposing a machine, self-as-worker, [mobile access](07-mobile-access.md) |
| [Tools](09-tools.md) | The command, file, credential, and guide tools |
