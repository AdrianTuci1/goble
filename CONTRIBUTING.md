# Contributing to Goble

Goble is a Rust workspace with one native app and a set of runtime crates. This guide
covers the local setup, the checks a change must pass, and the shape of a pull request.

## Prerequisites

- Rust 1.86 or newer (`rustup toolchain install stable`).
- macOS or Linux for the native app. Windows builds run through CI.
- A vendored terminal emulator: `vendor/alacritty_terminal` is a path dependency the
  workspace does not compile without.

```bash
./scripts/fetch-alacritty-terminal.sh --install
```

The script pins the archive by SHA-256; do not replace it with an unverified checkout.

## Build and run

```bash
cargo build --bin goble-app        # the native app
cargo run --bin goble-app          # run it
cargo build -p goblin-worker       # the headless worker
cargo build -p goble-cli           # the control CLI
```

The app writes its state under `~/.goble`. For a throwaway instance set `HOME` to a
temporary directory and copy the config you need into it; never point a test run at a
real store or a real API key.

## Tests

Integration tests that need a shell or a worker binary are marked `#[ignore]`; the
default suite is the unit suite.

```bash
cargo test -p goble-core --lib
cargo test -p goble-ui --lib
cargo test -p goble-app --lib
cargo nextest run --workspace --profile ci   # the full suite, if nextest is installed
cargo check --workspace
```

UI changes need a check in the running app, not only a test: exercise the flow, try the
empty and error states, and look at a narrow and a wide window.

## Formatting and lints

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets` are the target.
The tree is not rustfmt-clean yet; format the files you touch and do not reformat
unrelated code in the same PR. Clippy warnings are advisory today; fix the ones in code
you changed.

## Architecture rules

The layering rule is `types ← protocol ← runtime`. A crate may depend on the layer to
its left, never the reverse. Two rules that reviewers enforce:

- The GUI talks to a daemon through `goble-daemon-client`; it does not know whether the
  daemon is in-process or remote.
- Harnesses enter through `goble-harness-protocol`. A new harness is an adapter, not a
  branch inside the core.

See `docs/substrate-architecture.md` and `docs/PROJECT_STRUCTURE.md` before adding a
crate or moving a seam.

## Commits and pull requests

- One concern per PR. Split refactors from behavior changes.
- Conventional Commits: `feat(ui): ...`, `fix(pty): ...`, `docs: ...`, `chore: ...`.
  The PR title follows the same shape; CI checks it.
- The description answers what changed, why, and how it was tested. Link the issue.
- New behavior needs a test that fails without the change. Bug fixes need a regression
  test.
- User-facing changes go under `[Unreleased]` in `CHANGELOG.md`.
- UI work includes a screenshot or a short recording.
- A schema or protocol change updates `docs/` and includes a migration.

Do not commit credentials, tokens, private keys, local-only paths or generated stores.
Secret scanning and push protection run on the repository; a blocked push means the
value must be rotated, not bypassed.

## Review

- A PR needs a green CI run before merge.
- A reviewer may ask for a narrower change or a test before reading the rest.
- Security-sensitive changes (sandbox, approval, secrets, transports) need the maintainer
  named in `CODEOWNERS` and a note in the PR about the threat it addresses.

Report vulnerabilities through [SECURITY.md](SECURITY.md), never in a PR.

## License

By contributing you agree that your contribution is licensed under the MIT license in
[LICENSE](LICENSE).
