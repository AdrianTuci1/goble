# Distribution

Goble's artifacts live on GitHub Releases. A Cloudflare Worker at
`downloads.goble.dev` holds the signed manifest that names the current release
and redirects downloads to GitHub, so the app trusts one small domain we
control while the bytes stay in a place we do not have to pay for.

Three pieces, three responsibilities:

| Piece | Holds | Changes |
| --- | --- | --- |
| GitHub Releases | the `.dmg`, `.AppImage`, `.deb`, `.tar.gz` and setup `.exe` files, plus the manifest they were built with | whenever a release is built |
| `deploy/downloads-worker` | one signed manifest per channel, in the `MANIFESTS` KV namespace | every publish, including a rollback |
| the app | the manifest URL and the public key it trusts, baked in at build time | on a domain move or a key rotation |

The manifest is the pointer, and only the pointer. Yanking a version, rolling a
channel back, or moving to another domain are all a manifest publish; no
artifact is rebuilt and no app binary changes.

## Artifacts and targets

The release workflow builds four desktop targets and names the files the same
way every time:

| Target | Label | Artifact |
| --- | --- | --- |
| `aarch64-apple-darwin` | macOS aarch64 | `Goble-<version>-arm64.dmg` |
| `x86_64-apple-darwin` | macOS x86_64 | `Goble-<version>-x64.dmg` |
| `x86_64-unknown-linux-gnu` | Linux x86_64 | `Goble-<version>-x86_64.AppImage`, `goble_<version>_amd64.deb`, `goble-<version>-linux-x86_64.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows x86_64 | `Goble-<version>-win-x64-setup.exe` |

The manifest's `artifacts` array names one file per `os`/`arch` pair, and
`scripts/release.sh` prefers in that order: for macOS the native slice then the
universal build; for Linux the AppImage, then the `.deb`, then the tarball. The
updater installs what the manifest names, which is why the AppImage is
preferred over the archive: it is the one file a running app can replace.

## The release pipeline

`.github/workflows/release.yml` has two doors:

- `workflow_dispatch` builds, packages, signs, creates the release, and
  publishes the manifest.
- The `release: published` event catches a draft promoted by a human later.

A draft release never publishes a manifest. The publish step is guarded on
`!inputs.draft` and the release event only fires for a published release, so
pushing a draft leaves the channel pointing at the previous version. That is
the review window: a broken build can be inspected as a draft and deleted
before any app sees it.

What each step produces:

1. The four app builds, signed with the Apple certificate when
   `APPLE_CERTIFICATE_P12` is set.
2. `dist/channel_versions.json`: schema 2, one channel, the version, the
   epoch, the yanked list, one artifact line per platform, and a signature.
3. The GitHub release, with the artifacts and the manifest attached.
4. The worker publish, through `scripts/publish-manifest.sh`.

The manifest is built by:

```sh
GOBLE_RELEASE_EPOCH=7 \
GOBLE_YANKED_VERSIONS=0.1.9 \
GOBLE_DOWNLOAD_BASE_URL=https://downloads.goble.dev \
  scripts/release.sh --manifest --version 0.2.0 --channel stable --outdir dist
```

`scripts/release.sh` validates before it signs: the epoch must be a plain
integer that fits `u64`, and the yanked list must be sorted and free of
duplicates (write `0.1.9,0.2.0`, not `0.2.0,0.1.9`). Artifact URLs come from
`GOBLE_DOWNLOAD_BASE_URL`, or from the GitHub release URL when it is unset, and
the script appends `v<version>` unless that segment is already present.

## The downloads worker

`deploy/downloads-worker` is an ESM Cloudflare Worker with no build step.

| Route | Serves |
| --- | --- |
| `GET /health` | `{ "ok": true }` |
| `GET /channel_versions.json[?channel=beta]` | the signed manifest, `no-cache` |
| `GET /dl/:tag/:asset` | 302 to the GitHub release asset |
| `POST /admin/publish` | stores a manifest behind `Authorization: Bearer $ADMIN_TOKEN` |

The manifest is served with `cache-control: no-cache` on purpose. A cached
manifest is a rollback that has not happened yet: the whole point of the
pointer is that a publish takes effect on the next check.

Set up once:

```sh
cd deploy/downloads-worker
npx wrangler kv namespace create MANIFESTS      # paste the id into wrangler.toml
npx wrangler secret put ADMIN_TOKEN             # the /admin/publish bearer token
npx wrangler deploy
```

`PUBLIC_BASE_URL` and `GITHUB_REPO` are plain vars in `wrangler.toml`. The
route block for `downloads.goble.dev` is commented out there; uncomment it once
the zone is pointed at Cloudflare. The admin token is a secret: it never
appears in `wrangler.toml`, in a manifest, or in anything a browser can fetch.

A dry run validates the bundle without touching the account:

```sh
npx --yes wrangler@3 deploy --dry-run --outdir /tmp/worker-out \
  --config deploy/downloads-worker/wrangler.toml
```

The worker's own tests need no runtime and no account:

```sh
node --test deploy/downloads-worker/test
```

## Runbook: publish a release

1. Merge to `main` and let CI go green.
2. Dispatch **Release Build** with the tag, `draft: true`, the
   `epoch` (higher than the last published one) and any `yanked` versions.
3. Open the draft release and check the attached manifest:

   ```sh
   gh release download v0.2.0 --pattern channel_versions.json -O - | jq .
   ```

   The `epoch` must be the one you passed, `yanked` must be sorted, and every
   artifact URL must resolve.
4. Publish the release. That is what moves the channel.
5. Confirm both copies agree:

   ```sh
   curl -s https://downloads.goble.dev/channel_versions.json | jq .channels.stable
   ```

   The worker's copy and the release asset should be identical.

If the publish step was skipped (forks without the secrets), publish by hand:

```sh
GOBLE_WORKER_URL=https://downloads.goble.dev \
GOBLE_WORKER_ADMIN_TOKEN=… \
  scripts/publish-manifest.sh --manifest dist/channel_versions.json
```

A `409` means the epoch you are publishing is not higher than the stored one.
Raise `GOBLE_RELEASE_EPOCH` and run again; do not lower it.

## Runbook: roll back a release

Rolling back means publishing an *older artifact set* with a *higher epoch*.
Nothing is deleted, nothing is rebuilt, and no installed app is touched until
it next checks and installs.

1. Pick the known-good tag, e.g. `v0.2.0`, and the versions being withdrawn,
   e.g. `0.2.1`.
2. Dispatch **Rollback Release**:
   - `tag`: `v0.2.0`
   - `channel`: `stable`
   - `epoch`: higher than the epoch currently published (check the live
     manifest)
   - `yanked`: `0.1.9,0.2.1` (sorted, comma separated)
3. The workflow downloads that tag's artifacts, regenerates the manifest
   against them with the new epoch and a fresh signature, uploads it to the
   release, and publishes it to the worker.
4. Confirm:

   ```sh
   curl -s https://downloads.goble.dev/channel_versions.json | jq .channels.stable
   ```

   The version is the older one, the epoch is the new one, and the withdrawn
   versions are in `yanked`.

What clients do with that document: a client whose installed epoch is lower
takes the document as authoritative and offers to install `0.2.0`, even though
it is older than what is running. A client already running `0.2.0` sees no
offer. This is described in [auto-update.md](auto-update.md).

If a rolled-back version is still running on a machine, the Settings page
shows a banner naming the replacement and offering the install.

## Domain switch

Every artifact URL in the manifest is generated from `PUBLIC_BASE_URL` when
`scripts/release.sh --manifest` runs. To move downloads from
`downloads.goble.dev` to another host:

1. Deploy the worker behind the new route and point the new host at it.
2. Publish a manifest generated with the new base (a release dispatch, or
   `rollback.yml` with the current tag, a higher epoch, and
   `download_base_url` set).
3. Keep the old host answering redirects until the installed base has moved.

No app build changes, because the app reads the manifest rather than a URL
inside the binary. Builds that were made before a move keep checking the old
manifest URL, so that URL has to keep working: publish to it, or serve it as a
redirect to the new one.

## Signing

The manifest is signed with ed25519 by `packaging/sign-manifest.sh`, keyed on
`GOBLE_UPDATE_SIGNING_KEY` (a 64-character hex seed). The public half is
compiled into the app. A build with `GOBLE_REQUIRE_SIGNING=1` fails rather than
producing an unsigned manifest; `scripts/release.sh --allow-unsigned` is the
deliberate way out for a local test build, never for a release. Key rotation and
the payload format are in [auto-update.md](auto-update.md).
