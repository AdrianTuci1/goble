# `@goble/common` — the shared client core

The web shell (`apps/web`) and the mobile shell (`apps/mobile`) are thin: they
own routing, the platform implementations and nothing else. Everything a user
reads or taps lives here, so a fix lands once and both surfaces get it.

## The boundary rule

Both shells are Expo apps, so `@goble/common` may use the Expo packages that also
run in a browser — `expo-router`, `expo-linear-gradient`, `@expo/vector-icons` —
plus the `react-native` set `react-native-web` provides (`View`, `Text`,
`StyleSheet`, `Pressable`, `FlatList`, `Animated`, `Linking`, `Image`, …).

What may **not** appear here is a module that only exists on a phone. A
capability that needs one — the keychain, haptics, push, the camera, the
microphone, the filesystem — is declared in
[`src/platform/index.ts`](src/platform/index.ts) and handed over by the shell at
boot through `configurePlatform`. The defaults are `localStorage` when the host
has it and no-ops when it does not, which is exactly what the web export needs.

| Interface | Mobile shell | Web shell |
|---|---|---|
| `SecureStorage` | `expo-secure-store` | `localStorage` (the default) |
| `Haptics` | `expo-haptics` | no-op (the default) |
| `PushNotifications` | `expo-notifications` | — |
| `AttachmentPicker` | `expo-image-picker` | — |
| `AudioRecorder` | `expo-audio` | — |
| `FileSystem` | `expo-file-system` | — |

Storage and haptics are the two wired through the registry today; the rest are
declared and still live in the mobile shell, which is where the web shell will
pick them up when a screen needs them.

## Consuming it

It is an npm workspace package and is consumed as TypeScript source — no build
step, no `dist/`. `apps/mobile/metro.config.js` points Metro at the repository
root so the linked package resolves; a second shell needs the same three
settings (`watchFolders`, `resolver.nodeModulesPaths`).

```ts
import type { Platform } from "@goble/common";
```
