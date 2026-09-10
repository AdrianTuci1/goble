# Third-party notices

Goble is MIT licensed (see `LICENSE`). It embeds the following third-party
source. Everything listed here is permissive; nothing in the dependency tree is
copyleft.

## Vendored source

### alacritty_terminal 0.26.0

- Upstream: <https://github.com/alacritty/alacritty> (crate
  `alacritty_terminal`, published from that repository)
- License: Apache-2.0 — full text in
  `vendor/alacritty_terminal/LICENSE-APACHE`
- Published crate archive: `alacritty_terminal-0.26.0.crate`,
  SHA-256 `bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59`
- Location: `vendor/alacritty_terminal/`
- Used by: `crates/goble-terminal` (the VT emulator and its parser)

It is the terminal emulator core: the grid, cursor, modes, alternate screen,
reflow, and the `vte` escape-sequence parser. `goble-terminal` depends on it by
path and always reaches the parser through the crate's own `pub use vte`
re-export, so the parser and the handler cannot drift apart.

#### What was changed

The published archive is used as published, with two deletions and no edits to
any source file:

- `tests/` removed (about 46 MB of recorded reference fixtures). The crate is
  excluded from the workspace, so those tests never run from here. To run them
  against the published sources, extract a fresh copy of the archive:

  ```sh
  scripts/fetch-alacritty-terminal.sh /tmp/alacritty
  cargo test --manifest-path /tmp/alacritty/alacritty_terminal-0.26.0/Cargo.toml
  ```

- `Cargo.lock` removed, since the crate is built as part of our workspace.

`scripts/fetch-alacritty-terminal.sh` downloads the archive, checks its
SHA-256 against the value above, and diffs it against `vendor/` — so any
accidental edit to vendored source shows up as a reported difference.

#### Upgrading

The crate is pinned by path in the root `Cargo.toml`. Upgrading means
re-extracting the new archive into `vendor/alacritty_terminal`, updating the
version and SHA-256 here, and re-running `cargo test -p goble-terminal`.

## Reference-only, not linked or copied

Warp's terminal implementation (studied for its command-block model) is
AGPL-3.0-only. No code from it is used, linked, or vendored anywhere in this
repository; it was read to map behaviour, and the mapping is written up in
`.agents/06-renderer/terminal-blocks.md`.
