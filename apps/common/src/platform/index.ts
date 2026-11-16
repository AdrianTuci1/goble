/**
 * The native capabilities the shared client needs but cannot own.
 *
 * `@goble/common` holds the protocol, the state and the screens that both the
 * web and the mobile shell draw. Those screens still need a handful of things
 * only a platform provides — keychain storage, haptics, push, the camera, the
 * microphone — so every one of them is an interface here and an implementation
 * in the shell (`*.native.ts` on mobile, `*.web.ts` on the web).
 *
 * The rule that keeps this file honest: nothing under `src/` imports `expo-*`
 * or `react-native` beyond the neutral `View`/`Text`/`StyleSheet` set. If a
 * screen needs a new capability, it lands here first.
 */

/** An image the composer holds, with its bytes ready for the worker. */
export interface PickedAttachment {
  /** Local URI, used for the preview in the transcript and the composer. */
  uri: string;
  name: string;
  mime: string;
  /** Base64 of the file's bytes. Empty when the platform gave no bytes. */
  base64: string;
}

export type AttachmentSource = "library" | "camera";

/** Small string values that must not land in plain application storage. */
export interface SecureStorage {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  remove(key: string): Promise<void>;
}

/** Short physical cues. The enable switch lives in the shared preferences. */
export interface Haptics {
  tap(): void;
  success(): void;
  error(): void;
}

export type NotificationPermission = "granted" | "denied" | "unsupported";

/** Push registration; the web shell answers `unsupported` and delivers nothing. */
export interface PushNotifications {
  permission(): Promise<NotificationPermission>;
  /** Registers for push and returns the device token, or null when it cannot. */
  register(): Promise<string | null>;
}

/** The composer's image picker, library or camera. */
export interface AttachmentPicker {
  pickImage(source: AttachmentSource): Promise<PickedAttachment | null>;
}

/** One recording session of the voice composer. */
export interface AudioRecorder {
  start(): Promise<void>;
  /** Stops and returns the recording plus how long it ran, in milliseconds. */
  stop(): Promise<{ uri: string; durationMs: number }>;
  cancel(): Promise<void>;
}

/** Files the client writes outside its own storage (share sheets, exports). */
export interface FileSystem {
  cacheDirectory(): string;
  remove(uri: string): Promise<void>;
}

/** Everything a shell hands the shared client at boot. */
export interface Platform {
  storage: SecureStorage;
  haptics: Haptics;
  notifications: PushNotifications;
  attachments: AttachmentPicker;
  recorder: AudioRecorder;
  files: FileSystem;
}
