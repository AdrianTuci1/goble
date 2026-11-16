import { getPlatformStorage } from "../platform";
import type { ThinkingLevel } from "./types";

const MODELS_KEY = "goble.model_preferences";

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
  let raw: string | null = null;
  try {
    raw = await getPlatformStorage().get(MODELS_KEY);
  } catch {
    return DEFAULT_MODEL_PREFERENCES;
  }
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
  try {
    await getPlatformStorage().set(MODELS_KEY, JSON.stringify(next));
  } catch {
    // Some test hosts have no storage; the screen still works in memory.
  }
}
