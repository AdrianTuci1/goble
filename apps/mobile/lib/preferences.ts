import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

import type { SessionMode, ThinkingLevel } from "./types";
import {
  DEFAULT_VOICE_PREFERENCES,
  normalizeVoicePreferences,
  type VoicePreferences,
} from "./voice";

const INSTRUCTIONS_KEY = "goble.custom_instructions";
const MODE_KEY = "goble.default_mode";
const VOICE_KEY = "goble.voice_preferences";
const MODELS_KEY = "goble.model_preferences";
const NOTIFICATIONS_KEY = "goble.notification_preferences";
const HAPTICS_KEY = "goble.haptics_enabled";

export interface Preferences {
  instructions: string;
  mode: SessionMode;
}

const modeListeners = new Set<(mode: SessionMode) => void>();

/** Keeps a mounted home screen in step when the default mode changes. */
export function subscribeDefaultMode(
  listener: (mode: SessionMode) => void,
): () => void {
  modeListeners.add(listener);
  return () => {
    modeListeners.delete(listener);
  };
}

function webStorage(): Storage | null {
  if (Platform.OS !== "web") return null;
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

async function read(key: string): Promise<string | null> {
  const web = webStorage();
  if (web) return web.getItem(key);
  try {
    return await SecureStore.getItemAsync(key);
  } catch {
    return null;
  }
}

async function write(key: string, value: string): Promise<void> {
  const web = webStorage();
  if (web) {
    web.setItem(key, value);
    return;
  }
  try {
    await SecureStore.setItemAsync(key, value);
  } catch {
    // Some test hosts have no keychain; the screen still works in memory.
  }
}

export async function loadPreferences(): Promise<Preferences> {
  const [instructions, mode] = await Promise.all([
    read(INSTRUCTIONS_KEY),
    read(MODE_KEY),
  ]);
  return {
    instructions: instructions ?? "",
    mode: mode === "terminal" ? "terminal" : "agent",
  };
}

export async function saveInstructions(instructions: string): Promise<void> {
  await write(INSTRUCTIONS_KEY, instructions);
}

export async function saveDefaultMode(mode: SessionMode): Promise<void> {
  for (const listener of modeListeners) listener(mode);
  await write(MODE_KEY, mode);
}

const voiceListeners = new Set<(preferences: VoicePreferences) => void>();

/** Keeps an open voice sheet or settings page in step with a new choice. */
export function subscribeVoicePreferences(
  listener: (preferences: VoicePreferences) => void,
): () => void {
  voiceListeners.add(listener);
  return () => {
    voiceListeners.delete(listener);
  };
}

export async function loadVoicePreferences(): Promise<VoicePreferences> {
  const raw = await read(VOICE_KEY);
  if (!raw) return DEFAULT_VOICE_PREFERENCES;
  try {
    return normalizeVoicePreferences(
      JSON.parse(raw) as Partial<VoicePreferences>,
    );
  } catch {
    return DEFAULT_VOICE_PREFERENCES;
  }
}

export async function saveVoicePreferences(
  preferences: VoicePreferences,
): Promise<void> {
  const next = normalizeVoicePreferences(preferences);
  for (const listener of voiceListeners) listener(next);
  await write(VOICE_KEY, JSON.stringify(next));
}

export interface ModelPreferences {
  /** Model used when the top bar has not picked one for the session. */
  defaultModelId: string | null;
  /** Thinking level a new session starts on. */
  defaultThinking: ThinkingLevel;
  /** Models hidden from the top-bar menu on every screen. */
  disabled: string[];
}

export const DEFAULT_MODEL_PREFERENCES: ModelPreferences = {
  defaultModelId: null,
  defaultThinking: "standard",
  disabled: [],
};

const thinkingLevels: ThinkingLevel[] = ["low", "standard", "high"];

function normalizeThinking(value: unknown): ThinkingLevel {
  return thinkingLevels.includes(value as ThinkingLevel)
    ? (value as ThinkingLevel)
    : "standard";
}

function normalizeModelPreferences(
  input: Partial<ModelPreferences> | null | undefined,
): ModelPreferences {
  const disabled = Array.isArray(input?.disabled)
    ? [...new Set(input.disabled.filter((id): id is string => typeof id === "string"))]
    : [];
  const defaultModelId =
    typeof input?.defaultModelId === "string" &&
    !disabled.includes(input.defaultModelId)
      ? input.defaultModelId
      : null;
  return {
    defaultModelId,
    defaultThinking: normalizeThinking(input?.defaultThinking),
    disabled,
  };
}

const modelListeners = new Set<(preferences: ModelPreferences) => void>();

/** Keeps the home screen and the model menu in step with Settings. */
export function subscribeModelPreferences(
  listener: (preferences: ModelPreferences) => void,
): () => void {
  modelListeners.add(listener);
  return () => {
    modelListeners.delete(listener);
  };
}

export async function loadModelPreferences(): Promise<ModelPreferences> {
  const raw = await read(MODELS_KEY);
  if (!raw) return DEFAULT_MODEL_PREFERENCES;
  try {
    return normalizeModelPreferences(
      JSON.parse(raw) as Partial<ModelPreferences>,
    );
  } catch {
    return DEFAULT_MODEL_PREFERENCES;
  }
}

export async function saveModelPreferences(
  preferences: ModelPreferences,
): Promise<void> {
  const next = normalizeModelPreferences(preferences);
  for (const listener of modelListeners) listener(next);
  await write(MODELS_KEY, JSON.stringify(next));
}

export interface NotificationPreferences {
  runFinished: boolean;
  agentNeedsHelp: boolean;
}

export const DEFAULT_NOTIFICATION_PREFERENCES: NotificationPreferences = {
  runFinished: true,
  agentNeedsHelp: true,
};

function normalizeNotificationPreferences(
  input: Partial<NotificationPreferences> | null | undefined,
): NotificationPreferences {
  return {
    runFinished:
      typeof input?.runFinished === "boolean" ? input.runFinished : true,
    agentNeedsHelp:
      typeof input?.agentNeedsHelp === "boolean" ? input.agentNeedsHelp : true,
  };
}

const notificationListeners = new Set<
  (preferences: NotificationPreferences) => void
>();

export function subscribeNotificationPreferences(
  listener: (preferences: NotificationPreferences) => void,
): () => void {
  notificationListeners.add(listener);
  return () => {
    notificationListeners.delete(listener);
  };
}

export async function loadNotificationPreferences(): Promise<NotificationPreferences> {
  const raw = await read(NOTIFICATIONS_KEY);
  if (!raw) return DEFAULT_NOTIFICATION_PREFERENCES;
  try {
    return normalizeNotificationPreferences(
      JSON.parse(raw) as Partial<NotificationPreferences>,
    );
  } catch {
    return DEFAULT_NOTIFICATION_PREFERENCES;
  }
}

export async function saveNotificationPreferences(
  preferences: NotificationPreferences,
): Promise<void> {
  const next = normalizeNotificationPreferences(preferences);
  for (const listener of notificationListeners) listener(next);
  await write(NOTIFICATIONS_KEY, JSON.stringify(next));
}

const hapticsListeners = new Set<(enabled: boolean) => void>();

export function subscribeHapticsPreference(
  listener: (enabled: boolean) => void,
): () => void {
  hapticsListeners.add(listener);
  return () => {
    hapticsListeners.delete(listener);
  };
}

/** Taps vibrate unless the phone turned them off in Settings. */
export async function loadHapticsPreference(): Promise<boolean> {
  return (await read(HAPTICS_KEY)) !== "off";
}

export async function saveHapticsPreference(enabled: boolean): Promise<void> {
  for (const listener of hapticsListeners) listener(enabled);
  await write(HAPTICS_KEY, enabled ? "on" : "off");
}
