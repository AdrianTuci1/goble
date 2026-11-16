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

/**
 * The registry a shell fills in at boot.
 *
 * The interfaces above describe every capability; a shell only has to hand over
 * the ones the shared code reaches for today. Anything it leaves out keeps the
 * default below, which is what the web export gets for free: `localStorage`
 * when the host has it, and no-ops when it does not.
 */
export type PlatformOverrides = Partial<Platform>;

function browserStorage(): Storage | null {
  try {
    return typeof globalThis.localStorage === "undefined"
      ? null
      : globalThis.localStorage;
  } catch {
    return null;
  }
}

const memory = new Map<string, string>();

const defaultStorage: SecureStorage = {
  async get(key) {
    const browser = browserStorage();
    if (browser) return browser.getItem(key);
    return memory.get(key) ?? null;
  },
  async set(key, value) {
    const browser = browserStorage();
    if (browser) {
      browser.setItem(key, value);
      return;
    }
    memory.set(key, value);
  },
  async remove(key) {
    const browser = browserStorage();
    if (browser) {
      browser.removeItem(key);
      return;
    }
    memory.delete(key);
  },
};

const defaultHaptics: Haptics = {
  tap() {},
  success() {},
  error() {},
};

let storage: SecureStorage = defaultStorage;
let haptics: Haptics = defaultHaptics;

/** Called once by a shell at boot, before anything reads a stored value. */
export function configurePlatform(overrides: PlatformOverrides): void {
  if (overrides.storage) storage = overrides.storage;
  if (overrides.haptics) haptics = overrides.haptics;
}

export function getPlatformStorage(): SecureStorage {
  return storage;
}

export function getPlatformHaptics(): Haptics {
  return haptics;
}
