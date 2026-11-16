/**
 * `@goble/common` — the platform-neutral core of the Goble client.
 *
 * The web shell and the mobile shell are both thin: they own routing, the
 * platform implementations from `./platform` and nothing else. Everything a
 * user reads or taps lives here, so a fix lands once and both surfaces get it.
 */

export type {
  AttachmentPicker,
  AttachmentSource,
  AudioRecorder,
  FileSystem,
  Haptics,
  NotificationPermission,
  PickedAttachment,
  Platform,
  PushNotifications,
  SecureStorage,
} from "./platform";

export * from "./lib/format";
export * from "./lib/harness";
export * from "./lib/mock";
export * from "./lib/model-preferences";
export * from "./lib/navigation";
export * from "./lib/pending-attachment";
export * from "./lib/remote/attachments";
export * from "./lib/remote/chat";
export * from "./lib/remote/connection";
export * from "./lib/remote/entities";
export * from "./lib/remote/gateway";
export * from "./lib/remote/protocol";
export * from "./lib/remote/server-url";
export * from "./lib/remote/session-store";
export * from "./lib/remote/socket";
export * from "./lib/remote/terminal";
export * from "./lib/remote/transcribe";
export * from "./lib/theme";
export * from "./lib/threads";
export * from "./lib/types";
export * from "./lib/use-models";
export * from "./platform";
