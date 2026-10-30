## Summary

<!-- What changed and why. Link the issue it closes: "Fixes #123". -->

## Type

- [ ] Feature
- [ ] Fix
- [ ] Refactor or performance
- [ ] Documentation
- [ ] Build, CI or release

## Test plan

<!-- The commands you ran and what they show. UI changes need a before/after
     screenshot or a short recording, and the viewports you checked. -->

- [ ] `cargo test -p goble-core --lib`
- [ ] `cargo test -p goble-ui --lib`
- [ ] `cargo test -p goble-app --lib`
- [ ] `cargo check --workspace`
- [ ] Exercised the affected flow in the running app (UI changes)

## Checklist

- [ ] One concern per PR; the description says what and why.
- [ ] New behavior has a test that fails without the change.
- [ ] User-facing changes are under `[Unreleased]` in `CHANGELOG.md`.
- [ ] No credentials, tokens, private keys or local-only paths are committed.
- [ ] Protocol or schema changes update `docs/` and ship with a migration.
- [ ] Screenshots and recordings contain no secrets or personal data.

## Breaking changes

<!-- Delete if none. What breaks, who is affected, and the upgrade path. -->
