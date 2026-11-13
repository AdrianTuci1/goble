# Remote Access

Goble can run a workspace on a machine that is not the one in front of you. The remote machine is a **worker**; your local app is the **client**. Because a machine is a workspace, exposing a machine makes that workspace reachable from anywhere Goble runs.

There are two ways to get a workspace on a remote machine:

1. **Self-as-worker** — expose this machine itself as a worker, so other Goble clients can point at it.
2. **Provisioned worker** — bring up a worker on a host (via the pairing/SSH/Helm path) and publish a workspace endpoint the router can target.

---

## Exposing This Machine Through Tailscale

Tailscale gives every machine a stable, private IP and connects them in an encrypted mesh — ideal for reaching a Goble worker directly.

1. Install Tailscale on this machine and the client you'll connect from, then bring them into the same tailnet:

   ```bash
   tailscale up --hostname=goble-worker
   ```

2. Confirm the machine's Tailscale IP (and that the tailnet accepts it):

   ```bash
   tailscale ip -4
   ```

3. On the client, add a remote workspace pointing at that address, and Goble routes turns to the worker while the rest of the state stays in the workspace's home. Because Tailscale credentials/IPs are point-in-time and network-specific, store them as a **credential** so the key never shows up in a transcript.

For a **provisioned worker**, the router consumes the endpoint (address, TLS, worker id) you published for the workspace and routes to it.

---

## What "Self-as-Worker" Means for This Machine

When this machine is exposed as a worker, it materializes the **workspace payload** of its home — bundled tooling, worktrees, threads, a local store — because the workspace runs here. Clients connect over the tailnet/network and drive turns; the home (and its principals and grants) travels with the workspace.

---

## Setting Remote Up With the Agent

Remote settings are a few tables in `~/.goble/config.toml`, and the agent can
collect and write them for you. Settings → Agent & Approval has **Set up Remote
with the agent** and **Set up the hub with the agent**; either opens a
conversation named `Remote setup` or `Hub setup` whose first message is a brief
built from what the config already has.

The agent asks one question at a time: whether you want a gateway server or a
sandbox platform, then only the values that choice needs — the provider, its
key or Google Cloud project and region, and the worker image. It writes through
`goble-cli setup remote …`, `goble-cli setup gateway …` or `goble-cli setup hub
…`, never by hand, and reads `goble-cli setup status` back to you after each
write. When you do not know a value, it names the page or command that shows it
instead of guessing.

You can run the same commands yourself:

```bash
goble-cli setup status                 # what is set, what is missing
goble-cli setup remote --name builder --provider e2b \
  --api-key "$E2B_API_KEY" --image ghcr.io/example/goble-worker:latest
goble-cli setup gateway --url wss://gateway.example/ws/worker --token …
```

The hub's settings go to `~/.goble/hub.toml` (database, relay URL, API token,
workflow directory). Goble does not ship the hub binary yet; `setup hub`
records what it will start with.

---

## Security

- Tailscale is encrypted mesh networking; still, only grant reach into the workspace to principals you trust (see [Principals and Access](05-principals-and-access.md)).
- Keep `auth.json` and credentials owner-only; use full-disk encryption.
- Route remote state over TLS; the endpoint you publish carries the worker's TLS identity.

---

## Related

- [Workspaces](02-workspaces.md) — local vs remote and the home layout.
- [Mobile Access](07-mobile-access.md) — reaching this machine from the Goble mobile app.
