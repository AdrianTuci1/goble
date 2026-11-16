# `@goble/common` — the shared client core

The web shell (`apps/web`) and the mobile shell (`apps/mobile`) are thin: they
own routing, the platform implementations and nothing else. Everything a user
reads or taps lives here, so a fix lands once and both surfaces get it.

## The boundary rule

Nothing under `src/` may import an `expo-*` package, and `react-native` is
limited to the neutral set that `react-native-web` also provides (`View`, `Text`,
`StyleSheet`, `Pressable`, `FlatList`, `Animated`, `useWindowDimensions`, …).

A capability that only a platform has — the keychain, haptics, push, the
camera, the microphone, the filesystem — is declared as an interface in
[`src/platform/index.ts`](src/platform/index.ts) first. The shells implement it:

| Interface | Mobile | Web |
|---|---|---|
| `SecureStorage` | `expo-secure-store` | `localStorage` |
| `Haptics` | `expo-haptics` | no-op |
| `PushNotifications` | `expo-notifications` | `unsupported` |
| `AttachmentPicker` | `expo-image-picker` | `<input type="file">` |
| `AudioRecorder` | `expo-audio` | `MediaRecorder` |
| `FileSystem` | `expo-file-system` | Blob/download |

`expo-router` stays in the shells, never here: the web layout is a sidebar, the
mobile one a drawer, and both need their own route tree over the same screens.

## Consuming it

It is an npm workspace package and is consumed as TypeScript source — no build
step, no `dist/`. `apps/mobile/metro.config.js` points Metro at the repository
root so the linked package resolves; a second shell needs the same three
settings (`watchFolders`, `resolver.nodeModulesPaths`).

```ts
import type { Platform } from "@goble/common";
```
