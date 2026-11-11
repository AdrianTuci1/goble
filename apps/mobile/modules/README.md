# Native modules

Expo modules that need Kotlin or Swift live here. Each module is a standalone
package with an `expo-module.config.json`, an Android Gradle target, an iOS
podspec, and a small TypeScript surface.

Planned first modules:

| Module | Android | iOS | Job |
|---|---|---|---|
| `goble-notifications` | Kotlin | Swift | Push token, notification actions, deep links |
| `goble-secure` | Kotlin | Swift | Biometric unlock and keychain access groups |
| `goble-terminal` | Kotlin | Swift | Native terminal renderer |
| `goble-live-run` | Kotlin | Swift | Live Activities / ongoing run notification |

The home screen uses Expo packages only, so none of these are needed to boot the
current shell. Add one module at a time and wire its config plugin before an EAS
build.
