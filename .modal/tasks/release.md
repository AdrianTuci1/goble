# Task: distribution, auto-update, rollback

Goble ships a native binary soon. Finish the delivery path: serve artifacts from
GitHub Releases behind our own domain, publish the newest signed manifest
automatically, let the app update itself, and make a bad release reversible.
Also stop Dependabot from opening a pull request every week.

Work in phases, one commit each. The manifest contract below is frozen: the
release script, the updater, and the worker must agree on it exactly.

## Read first

- `crates/goble-update/src/{lib,manifest,client,install}.rs`
- `scripts/release.sh` (the `--manifest` section)
- `packaging/sign-manifest.sh`, `packaging/README.md`
- `.github/workflows/release.yml`, `.github/dependabot.yml`
- `app/src/main.rs`, `app/src/ui/types.rs`, `app/src/ui/actions.rs`
- `.agents/07-observability/crash-reporting-and-updates.md`

## Frozen manifest contract (schema 2)

`goble-update/src/manifest.rs` gets `SCHEMA: u32 = 2`. Each channel release
carries two new fields:

```json
{
  "schema": 2,
  "channels": {
    "stable": {
      "version": "0.2.0",
      "epoch": 7,
      "yanked": ["0.1.9"],
      "artifacts": [ ... ],
      "signature": "hex"
    }
  }
}
```

- `epoch: u64`, serialized with `#[serde(default)]` so an old manifest parses as
  `0`. It increases on every publish, including a rollback.
- `yanked: Vec<String>`, `#[serde(default)]`, sorted and de-duplicated before
  signing. A yanked version must be replaced even by a lower version.

The signing payload becomes exactly this, with the domain changed to
`goble-update-v2`:

```text
goble-update-v2
<version>
epoch=<epoch>
yanked=<comma-joined sorted versions>
<os> <arch> <sha256> <size> <url>
...
```

Artifact lines stay sorted and lower-cased as they are today. Update both
`crates/goble-update/src/manifest.rs` and `packaging/sign-manifest.sh`; the
existing signer-contract test must keep passing.

## R1 — updater core

- Parse `epoch` and `yanked`; validate that yanked entries are well-formed
  versions and that the epoch fits `u64`.
- `ChannelRelease::signing_payload` emits the payload above; the verifier uses
  the same bytes.
- `UpdateClient::check` takes the installed epoch (config or argument, your
  call) and decides:
  - `remote.epoch > installed_epoch`: authoritative, offer it even when
    `remote.version < current` (this is the rollback path);
  - otherwise offer only when `remote.version > current`;
  - never offer the version already installed.
- `AvailableUpdate` reports `epoch`, `rollback: bool`, and the artifact.
- Persist the installed epoch and version in `~/.goble/update-state.json`; write
  it only after an install reaches the healthy marker (see R4).
- Tests: a decision table covering newer, same, older, equal epoch, higher
  epoch with older version, yanked current, empty yanked, and invalid epoch.

## R2 — release pipeline and rollback publishing

- `scripts/release.sh --manifest` writes schema 2, reads `epoch` from
  `GOBLE_RELEASE_EPOCH` (default `1`), `GOBLE_YANKED_VERSIONS` (comma list), and
  keeps artifact URLs under `GOBLE_DOWNLOAD_BASE_URL` (default stays the GitHub
  release URL). Validate that the epoch is an integer and the yanked list is
  sorted before signing.
- `.github/workflows/release.yml`: after a non-draft release is published, POST
  the signed `channel_versions.json` to the worker's `/admin/publish` when
  `GOBLE_WORKER_URL` and `GOBLE_WORKER_ADMIN_TOKEN` are set. A draft release must
  not publish.
- Add `.github/workflows/rollback.yml` (manual dispatch: `tag`, `channel`,
  `epoch`). It downloads that tag's artifacts, regenerates the manifest against
  them with the requested epoch and signing key, and publishes it. This is how a
  bad release is withdrawn: the old artifacts, a higher epoch, a fresh signature.
- Keep the existing artifact naming and the current four packaging targets.

## R3 — downloads worker

Add `deploy/downloads-worker/` as a Cloudflare Worker in JavaScript (ESM, no
build step; JSDoc types are enough):

- `wrangler.toml` with a KV binding `MANIFESTS`, `PUBLIC_BASE_URL`,
  `GITHUB_REPO`, and a commented route for `downloads.goble.dev`.
- `GET /health` → `{ ok: true }`.
- `GET /channel_versions.json` → the signed manifest for `stable` from KV,
  `content-type: application/json`, `cache-control: no-cache` (the manifest is
  the thing that must change immediately). `?channel=beta` selects another key.
- `GET /dl/:tag/:asset` → validate `tag` (`v...` or `latest`) and `asset`
  (`[A-Za-z0-9._-]+`), then 302 to
  `https://github.com/${GITHUB_REPO}/releases/download/${tag}/${asset}` or
  `.../releases/latest/download/${asset}`. Reject anything else with 400.
- `POST /admin/publish` with `Authorization: Bearer ${ADMIN_TOKEN}` and
  `{ "channel": "stable", "manifest": "<json string>" }`. Validate that the
  document is schema 2, has a signature, and its epoch is not lower than the
  stored one; store it in KV; return `{ ok, epoch }`. Reject a stale epoch with
  409.
- No secret ever reaches the browser or the manifest.
- Domain switch: all manifest asset URLs are generated from
  `PUBLIC_BASE_URL` at release time; changing the route and republishing the
  manifest moves the domain without touching the app.
- Tests with `node --test`: routing and validation of tags/assets, manifest
  validation, epoch monotonicity, and the redirect target. Keep the pure logic
  in `src/manifest.mjs` and `src/route.mjs`.
- Validate with `npx --yes wrangler@3 deploy --dry-run --outdir /tmp/worker-out
  --config deploy/downloads-worker/wrangler.toml`.

## R4 — client install and local rollback

- `install.rs` keeps the current `InstallPlan`/`InstallOutcome` shape but adds:
  - copy the current bundle/AppImage to `~/.goble/updates/previous/` before
    replacing it;
  - write `~/.goble/updates/pending` with the new version and epoch before the
    swap;
  - on the next launch, if `pending` exists and `healthy` does not, restore
    `previous` and record the failed version in the update state;
  - write `healthy` after 60 seconds of uptime, then clear `pending`.
- `InstallPlan::apply` must never delete the only copy of the binary.
- Tests use temp directories and a fake bundle layout; no real installation.
- Add a `rollback_local()` path usable from the app at startup before the UI
  starts, so a broken build does not trap the user.

## R5 — app wiring

- Add `app/src/update.rs` (or the smallest equivalent module) with a background
  checker: run at startup and at most every 6 hours, never on the UI thread.
  Persist the last check in the update state.
- Settings gets an Updates section: current version, last check, a "Check now"
  button, an available version with channel and size, and "Install and restart".
  Use the existing element primitives and action plumbing.
- A downloaded update must be signature- and digest-verified before it is
  staged. A failure is visible in the section, not a silent log line.
- If the current version is yanked, show a banner that names the replacement and
  offers the rollback install.
- Add tests for the view model and the state transitions (idle → checking →
  available → downloading → ready → error) without launching the GUI.

## R6 — Dependabot and docs

- `.github/dependabot.yml`: monthly for cargo and github-actions,
  `open-pull-requests-limit: 2`, minor+patch grouped, majors in their own group
  with a `dependencies-major` label, keep security updates through Dependabot
  alerts, keep ignoring the vendored `alacritty_terminal`.
- Add `docs/distribution.md` (GitHub Releases, worker, domain switch, publish and
  rollback runbooks) and `docs/auto-update.md` (state machine, epoch, yanked,
  local rollback, signing key rotation). Do not edit unrelated docs.

## Verification

Run, with `timeout 900` on every test command:

- `cargo test -p goble-update`
- `cargo check -p goble-app --all-targets`
- the targeted app tests you add or touch
- `cargo clippy -p goble-update --all-targets`
- `bash -n scripts/release.sh packaging/sign-manifest.sh`
- `node --test deploy/downloads-worker/test`
- the worker dry-run command above

Commit per phase (R1…R6) and write `{{job_dir}}/out/summary.md`. If a
phase cannot finish, stop at a compiling commit and say exactly what remains.
