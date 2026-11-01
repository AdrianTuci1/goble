# Auto-update

The app checks a signed manifest on `get.octomus.dev`, compares it to what
this installation already runs, and installs what the manifest names. The
artifacts themselves stay on GitHub Releases; see
[distribution.md](distribution.md) for the publishing side.

Two ideas carry the design:

- The manifest names the release a channel *should* run, not the newest version
  that exists. A higher epoch makes that statement authoritative, which is how a
  rollback reaches machines without anyone touching them.
- An install is not finished when the files land. It is finished when the new
  build has been up for a minute, and until then the previous copy is on disk
  and the next launch undoes the swap if it has to.

## State machine

`app/src/update.rs` holds the view model; the settings page renders it and the
service drives it off the UI thread.

```text
idle ──check──▶ checking ──nothing──▶ idle
                   │
                   ├──offer──▶ available ──install──▶ downloading ──▶ ready
                   │               ▲                      │
                   │               └──────error◀──────────┘
                   └──failure──▶ error ──reset/check──▶ checking
```

`ready` means the download is staged, verified, and the install script has been
run; the process exits and the new build starts. A restart that fails is caught
by the local rollback below, and the failure shows up in the settings page on
the next launch.

Transitions only move forward for one offer: a check that answers after the
user started a download is dropped, so a slow network cannot knock a ready
install back to available.

## When a check happens

- Once at startup, off the UI thread.
- Then at most once every six hours (`CHECK_INTERVAL`), measured from
  `last_check` in the state file.
- Immediately when the user presses **Check now**.

The clock is persisted, so restarting the app does not reset it.

## What a check decides

`decide_update` in `crates/goble-update/src/client.rs` compares the published
release with the running version and the installed epoch:

| Published | Installed epoch | Running | Offer |
| --- | --- | --- | --- |
| higher version | same | older | the newer version |
| same version | same | same | nothing |
| lower version | same | newer | nothing |
| any version | equal or lower | same version | nothing |
| lower version | higher | newer | the lower version (rollback) |
| higher version | higher | older | the newer version |
| lower version | higher | older | the lower version (rollback) |
| lower version | same | newer, and that version is yanked | the published version (rollback) |

The running version is never offered back to itself, whatever the epoch says:
reinstalling it would replace a working copy with itself and reset the healthy
marker.

A release with no artifact for this `os`/`arch` produces "no update for this
platform" rather than an offer that would fail at download time.

## Epoch

`epoch` is a `u64` that rises on every publish of a channel, including a
rollback. It is not a version: it says "this document is newer than anything
you have seen", and it is the only thing that lets the publisher move a channel
backwards.

Clients compare it against `installed_epoch` from
`~/.goble/update-state.json`, which is `0` for an installation that predates
the field or has never completed an install from a manifest. An old manifest
without `epoch` parses as `0`.

Rules the publisher has to honour:

- Raise the epoch on every publish. The worker refuses an epoch lower than the
  stored one with `409`.
- Never lower it to force a version out. Withdraw the version by yanking it and
  publishing a higher epoch.
- The epoch is per channel. `stable` and `beta` count separately.

## Yanked versions

`yanked` lists versions that must not stay installed. It is sorted and
de-duplicated before signing, so the payload is stable for a given set.

A running version that appears in `yanked` is offered the published version
even when that version is lower, which covers the case where a build is
withdrawn and the channel itself stays where it was. The settings page then
shows a banner naming the replacement and offering the install, and the app
does not wait for the six-hour interval to notice.

Nothing is force-installed. A yanked build keeps working; it is told it is
withdrawn and offered the way forward.

## Files this state lives in

All under `~/.goble` (`GOBLE_HOME` overrides the directory, which is how tests
and portable installs are run):

| Path | Meaning |
| --- | --- |
| `update-state.json` | installed version and epoch, last check, last failed version |
| `updates/pending` | the version and epoch being installed, written before the swap |
| `updates/healthy` | the staged build reached 60 seconds of uptime |
| `updates/previous/` | the bundle or AppImage that was replaced |
| `updates/downloads/` | verified downloads waiting to be installed |

`update-state.json` is written only after an install reaches the healthy
marker:

```json
{
  "installed_version": "0.2.0",
  "installed_epoch": 7,
  "last_check": "2026-10-01T09:00:00Z",
  "failed_version": "0.2.1",
  "failed_epoch": 8
}
```

A version that was installed and never became healthy is recorded as
`failed_version`, and the page says which version failed instead of silently
offering it again.

## Install and local rollback

`InstallPlan::apply` copies before it replaces:

1. Copy the current bundle or AppImage into `updates/previous/`.
2. Write `updates/pending` with the version and epoch being installed.
3. Replace the target with the staged build.
4. On the next launch, the app watches the clock: after 60 seconds of uptime it
   writes `updates/healthy`, clears `pending`, and records the version and epoch
   in `update-state.json`.
5. If the next launch finds `pending` without `healthy`, the previous copy goes
   back and the failed version is recorded.

The copy is never moved, so the backup exists before the replacement does. If
the backup cannot be made, `apply` refuses to touch the target. The rule the
tests pin is that an install never leaves the only copy of the binary in a
place a crash can lose it.

`rollback_local()` runs in `main.rs` before the window opens. If it restores a
build it relaunches that build and exits, because a broken binary cannot
usefully continue. This is the path that keeps a bad upgrade from trapping a
user in a window that never appears.

A `.deb`, `.rpm` or package-manager install is left alone: the updater reports
the command to run and moves no files.

## Signature and digest

Before anything is staged, the download is checked against the manifest: the
SHA-256 and size from the artifact line, then the manifest's signature over the
exact signing payload. A mismatch is an error shown in the settings page, not a
log line.

The signing payload is fixed:

```text
goble-update-v2
<version>
epoch=<epoch>
yanked=<comma-joined sorted versions>
<os> <arch> <sha256> <size> <url>
...
```

Artifact lines are sorted and lower-cased. The domain string separates this
format from any other signed document, so a signature can only be replayed as
this payload. `crates/goble-update/src/manifest.rs` and
`packaging/sign-manifest.sh` build the same bytes; a shared test
(`tests/signer_contract.rs`) signs a release with the script and verifies it
with the crate.

The app trusts one Ed25519 public key, compiled in at build time from
`GOBLE_UPDATE_PUBLIC_KEY`, and reads one manifest URL, from
`GOBLE_UPDATE_MANIFEST`. A build with neither set checks nothing and says so in
Settings. Both can be overridden at run time for a local test, and
`GOBLE_UPDATE_ALLOW_UNSIGNED=1` accepts an unsigned manifest for development
only.

## Rotating the signing key

A published build trusts exactly one key, baked into the binary at compile time,
so rotation is a two-release manoeuvre: the build that trusts the new key has
to arrive signed by the old one.

1. Generate the new seed and keep the old one.
2. Compile the new public key into the next build, and sign that release with
   the **old** key. Every installed app trusts the old key, so this is the only
   signature it will accept.
3. Once the fleet has that build, sign releases with the new key alone.
4. Destroy the old seed after a full update cycle, and after the release that no
   longer mentions it has been out long enough that stragglers have updated.

A compromised key skips the window, because there is no signature a fleet
trusting the old key will accept from anyone else. Then the path is out of band:
publish the new build on the download page, tell users to download it, and yank
every version signed with the compromised key so the stragglers are told what to
do instead of being offered something forged.
