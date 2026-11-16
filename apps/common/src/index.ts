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
