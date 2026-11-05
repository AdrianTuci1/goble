# Config plugins

Config plugins add native entitlements and manifest entries without editing the
generated `ios/` or `android/` projects by hand.

Planned plugins:

- `with-notification-actions.ts` — iOS categories and Android channel metadata.
- `with-associated-domains.ts` — universal links for `goble://` and the gateway.
- `with-privacy-manifest.ts` — iOS privacy manifest entries.
- `with-android-permissions.ts` — only the permissions the app uses.

Keep each plugin small and covered by a unit test that runs the mods against a
fixture, following the pattern in `../app.json`.
