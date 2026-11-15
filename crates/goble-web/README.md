# goble-web — stale

**Stale as of 2026-11-15.** The browser and desktop surface is built from the
Expo app in [`apps/mobile`](../../apps/mobile), which shares the phone's UI and
is being optimized for desktop widths. This Rust/wasm client stays in the tree
for reference and receives no new work.

What follows from the decision:

- new browser work goes into `apps/mobile`, not here;
- the gateway's `/app/` is meant to serve the Expo web export
  (`npx expo export --platform web`) instead of this bundle;
- `scripts/build-web.sh` stays only to reproduce the old bundle;
- the viewer slice added here on 2026-11-14 was the last change, and the mobile
  app supersedes it.

The desktop application itself (`app/`, `crates/goble-ui`, `crates/goble-daemon`)
is unaffected.
