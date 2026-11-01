# Goble downloads Worker

Cloudflare Worker that serves Goble's download endpoints. No build step: ESM
JavaScript with JSDoc types, no dependencies.

| Route | Serves |
| --- | --- |
| `GET /health` | `{ "ok": true }` |
| `GET /channel_versions.json[?channel=beta]` | the published update manifest for a channel (`stable` by default) |
| `GET /dl/:tag/:asset` | 302 to the GitHub release asset; `:tag` is a version tag like `v0.2.0` or `latest` |
| `POST /admin/publish` | stores a manifest, guarded by `Authorization: Bearer $ADMIN_TOKEN` |

An unknown path is a 404, a known path with the wrong method is a 405, and a
malformed tag or asset (including path traversal) is a 400.

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
  `https://downloads.goble.dev`.
- `GITHUB_REPO` — `owner/repo` whose releases hold the artifacts. The value in
  `wrangler.toml` is a placeholder; set the real repository.

## Publishing a manifest

The worker accepts schema 2 documents (`schema: 2`, and each channel carries
`version`, `signature`, `epoch` and `artifacts`) and refuses an epoch lower than
the stored one, so a replayed upload cannot roll a channel back.
`scripts/release.sh --manifest` writes that document, and
`scripts/publish-manifest.sh` posts it with the checks above done locally.

```sh
GOBLE_WORKER_URL=https://downloads.goble.dev \
GOBLE_WORKER_ADMIN_TOKEN=… \
  scripts/publish-manifest.sh --manifest dist/channel_versions.json

# or by hand
curl -X POST \
  -H "Authorization: Bearer $GOBLE_WORKER_ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data "$(jq -n --arg channel stable \
    --arg manifest "$(cat dist/channel_versions.json)" \
    '{channel: $channel, manifest: $manifest}')" \
  https://downloads.goble.dev/admin/publish
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
