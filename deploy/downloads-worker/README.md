# Goble downloads Worker

Cloudflare Worker that serves Goble's download endpoints. No build step: ESM
JavaScript with JSDoc types, no dependencies.

## Deployed

Live on the `core` Cloudflare account:

- `https://get.octomus.dev` — the release host, served by a zone route on
  `octomus.dev` (a Cloudflare zone, so no DNS change was needed)
- `https://goble-downloads.adrian-tucicovenco.workers.dev` — the same Worker on
  its workers.dev name
- KV namespace `MANIFESTS` = `bd11e97c523842728c26840931f02c60`
- `GITHUB_REPO` = `AdrianTuci1/goble`, so `GET /dl/...` redirects into that
  repository's releases

`PUBLIC_BASE_URL` is `https://get.octomus.dev`, so the index links point at the
release host.

`ADMIN_TOKEN` is not set yet, so `POST /admin/publish` answers 500 until
`npx wrangler secret put ADMIN_TOKEN` runs. The same value goes into the
repository's `GOBLE_WORKER_ADMIN_TOKEN` Actions secret that
`scripts/publish-manifest.sh` reads.

| Route | Serves |
| --- | --- |
| `GET /health` | `{ "ok": true }` |
| `GET /channel_versions.json[?channel=beta]` | the published update manifest for a channel (`stable` by default) |
| `GET /install.sh` | the shell installer: `curl -fsSL https://get.octomus.dev/install.sh \| sh` |
| `GET /install.ps1` | the PowerShell installer for Windows |
| `GET /dl/:tag/:asset` | 302 to the GitHub release asset; `:tag` is a version tag like `v0.2.0` or `latest` |
| `POST /admin/publish` | stores a manifest, guarded by `Authorization: Bearer $ADMIN_TOKEN` |

An unknown path is a 404, a known path with the wrong method is a 405, and a
malformed tag or asset (including path traversal) is a 400.

The installers ask GitHub for the latest release tag, then download the asset
for the machine (`uname -m` / `PROCESSOR_ARCHITECTURE`) through `/dl/latest`.
macOS gets a `.dmg` copied into `~/Applications`, Linux an AppImage in
`~/.local/bin`, and Windows the per-user Inno Setup executable run silently.
No release means the installer stops with `no published release was found`.

## KV keys

One key per channel: `manifest:stable`, `manifest:beta`, `manifest:dev`. The
value is the manifest JSON document, stored exactly as published and returned
verbatim by `GET /channel_versions.json`.

Create the namespace once and paste the id into `wrangler.toml`:

```sh
npx wrangler kv namespace create MANIFESTS
```

## Secrets and variables

- `ADMIN_TOKEN` — secret, set with `npx wrangler secret put ADMIN_TOKEN`. It is
  compared against the request header only: it never appears in
  `wrangler.toml`, in a response body, or in a manifest, so nothing a browser
  fetches can carry it.
- `PUBLIC_BASE_URL` — absolute base used in the index links, e.g.
  `https://get.octomus.dev`.
- `GITHUB_REPO` — `owner/repo` whose releases hold the artifacts. The value in
  `wrangler.toml` is a placeholder; set the real repository.

## Publishing a manifest

The worker accepts schema 2 documents (`schema: 2`, and each channel carries
`version`, `signature`, `epoch` and `artifacts`) and refuses an epoch lower than
the stored one, so a replayed upload cannot roll a channel back.
`scripts/release.sh --manifest` writes that document, and
`scripts/publish-manifest.sh` posts it with the checks above done locally.

```sh
GOBLE_WORKER_URL=https://get.octomus.dev \
GOBLE_WORKER_ADMIN_TOKEN=… \
  scripts/publish-manifest.sh --manifest dist/channel_versions.json

# or by hand
curl -X POST \
  -H "Authorization: Bearer $GOBLE_WORKER_ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data "$(jq -n --arg channel stable \
    --arg manifest "$(cat dist/channel_versions.json)" \
    '{channel: $channel, manifest: $manifest}')" \
  https://get.octomus.dev/admin/publish
```

A successful publish answers `{ "ok": true, "epoch": <n> }`. A lower epoch is a
409, a bad body is a 400, and a missing or wrong token is a 401.

## Switching the domain

Change the `[[routes]]` pattern and `PUBLIC_BASE_URL` in `wrangler.toml`,
redeploy, then republish each channel's manifest. Clients fetch the URL compiled
into the build, so the manifest has to exist at the new host before a release
points at it.

## Local development and tests

```sh
npx wrangler dev     # local worker with emulated KV
node --test test     # unit and handler tests (node 18, no dependencies)
```
