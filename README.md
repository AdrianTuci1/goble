# Goble

Goble is a desktop app that turns your chat into an autonomous agent control center. It is built with **Tauri** (Rust + React) and pairs with **Goblin** workers running on your own VPS.

Everything is configurable from chat: pick a model, describe what you need, and Goble handles the rest — agents, workers, cluster identity, and credentials are resolved in the background.

## Quick setup

1. **Start the desktop app**
   ```bash
   cd crates/goble-desktop
   npm install
   npm run tauri dev
   ```

2. **Configure a model from the chat or Settings → LLM**
   Paste your API key (OpenAI, Anthropic, OpenRouter, etc.) and pick a model. The key is stored in the local encrypted vault.

3. **Connect a worker**
   Say something like *“connect a worker on my VPS at 1.2.3.4”* or go to **Settings → Workers** and click **Install / upgrade worker**. Provide SSH credentials, and Goble will:
   - Download the latest Goblin release
   - Generate a cluster identity and mTLS CA
   - Install PEM keys on the VPS
   - Pair the worker automatically in the background

4. **Start working**
   Create agents by describing them in plain language, run coding harnesses, schedule complex tasks, and watch executions in real time.

## What you can do

- **Natural-language agents** — describe an agent and Goble creates it. Example: *“Create a coding agent that reviews PRs, runs cargo test, and posts a summary.”*
- **Coding harness** — run multi-step coding tasks: edit files, run commands, use git, run tests, deploy.
- **Workflows & teams** — chain agents into workflows, assign them to teams, and schedule them by cron, HTTP, or heartbeat.
- **MCP connectors** — attach external tool servers (search, shell, APIs). Credentials are selected from the encrypted vault.
- **Observability** — live logs, execution traces, and worker status.
- **Cluster identity** — one cluster key derives mTLS CA, device identity, and encrypted backups. The worker discovers the desktop cluster automatically and pairs itself.

## Project layout

```
crates/goble-core/     shared types, protocol, crypto, LLM abstraction, MCP registry, store
crates/goble-desktop/    Tauri desktop app + React UI
crates/goblin-worker/    headless worker that runs on your VPS
crates/goble-cli/        utility CLI for worker operations
```

## Development

```bash
# Format and check the whole workspace
cargo fmt --all
cargo check --workspace --all-targets

# Run tests
cargo test --workspace

# Frontend only
cd crates/goble-desktop
npm test
npm run build

# Build a release bundle
npm run tauri build
```

### Native UI (wgpu/winit) with live hot reload

The product shell is the native UI in `app/` (`goble-app`) + `crates/goble-ui-hot`, built directly on `goble-ui` (wgpu/winit). View trees are built in `crates/goble-ui-hot/src/lib.rs`; state and actions live in `app/src/root_view.rs`. `crates/goble-desktop-native` is the backend-integration reference (state_api + views pattern) and is not the product shell. To iterate on the shell without rebuilding the whole binary every time:

```bash
./scripts/dev-ui.sh
```

The script builds `goble-app` once, starts it, and then watches `crates/goble-ui-hot`. Editing `crates/goble-ui-hot/src/lib.rs` rebuilds only the small hot-reload cdylib; the running app swaps it in live. Restart the script when you change `crates/goble-ui` itself (that is the ABI the executable is linked against).

## License

MIT — see `LICENSE`.
