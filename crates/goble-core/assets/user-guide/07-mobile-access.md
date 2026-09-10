# Mobile Access

You can drive a Goble workspace from your phone. A machine running Goble is a workspace, so once it's reachable on your tailnet the mobile app can connect to it and run turns from anywhere.

The flow is the same self-as-worker / remote-workspace path described in [Remote Access](06-remote-access.md), just from a small client:

1. Expose the machine as a worker (see [Remote Access](06-remote-access.md#exposing-this-machine-through-tailscale)) and note its Tailscale address.
2. Open the Goble mobile app and add that remote workspace.
3. The app connects to the worker over the mesh; your conversation, its state and the workspace home stay consistent with what you see on the desktop.

---

## What You Can Do From Mobile

- Run agent turns and read the streamed reasoning and tool cards.
- Approve or reject a pending tool run when the workspace asks for permission.
- Follow executions, traces and logs for runs you kicked off remotely.

---

## Notes

- The mobile client and the desktop app share the same workspace/routing contract, so a conversation can be started on one device and continued on another.
- Keep the tailnet membership and endpoint credentials as a **credential** so the key never enters a transcript (see [Credentials](04-credentials.md)).

---

## Related

- [Remote Access](06-remote-access.md) — the underlying self-as-worker and Tailscale setup.
- [Executions and Trace](14-executions-and-trace.md) — what you can follow while a run is in progress.
