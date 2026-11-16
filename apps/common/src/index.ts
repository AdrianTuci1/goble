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
export { harnessMark, nativeHarness } from "./lib/harness";
export type { HarnessMark as HarnessMarkSpec } from "./lib/harness";
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
export * from "./components/avatar";
export * from "./components/brand-mark";
export * from "./components/composer-sheet";
export * from "./components/composer";
export * from "./components/conversation-menu";
export * from "./components/detail-header";
export * from "./components/empty-state";
export * from "./components/harness-mark";
export * from "./components/menu-button";
export * from "./components/model-menu";
export * from "./components/popover";
export * from "./components/pulse-dot";
export * from "./components/quick-action";
export * from "./components/rich-text";
export * from "./components/screen";
export * from "./components/section-header";
export * from "./components/server-sheet";
export * from "./components/sheet";
export * from "./components/sidebar";
export * from "./components/status-pill";
export * from "./components/terminal-block";
export * from "./components/terminal-surface";
export * from "./components/top-bar";
export * from "./components/settings/choice-page";
export * from "./components/settings/settings-button";
export * from "./components/settings/settings-page";
export * from "./components/settings/settings-row";
export * from "./components/settings/settings-search-field";
export * from "./components/settings/settings-section";
export * from "./components/settings/settings-segmented";
export * from "./components/settings/settings-switch-row";
