import { useEffect, useState } from "react";

import { demoModels } from "./mock";
import {
  DEFAULT_MODEL_PREFERENCES,
  loadModelPreferences,
  subscribeModelPreferences,
  type ModelPreferences,
} from "./preferences";
import type { Model } from "./types";

/**
 * The gateway model catalog minus the ones disabled in Settings. The default
 * model comes from Settings and falls back to the first available entry.
 */
export function useModelChoices(): {
  allModels: Model[];
  defaultModel: Model;
  models: Model[];
  preferences: ModelPreferences;
} {
  const allModels = demoModels();
  const [preferences, setPreferences] = useState<ModelPreferences>(
    DEFAULT_MODEL_PREFERENCES,
  );

  useEffect(() => {
    let alive = true;
    void loadModelPreferences().then((next) => {
      if (alive) setPreferences(next);
    });
    const unsubscribe = subscribeModelPreferences(setPreferences);
    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);

  const enabled = allModels.filter(
    (model) => !preferences.disabled.includes(model.id),
  );
  const models = enabled.length > 0 ? enabled : allModels;
  const defaultModel =
    models.find((model) => model.id === preferences.defaultModelId) ??
    models[0]!;

  return { allModels, defaultModel, models, preferences };
}
