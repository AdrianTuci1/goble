import { useEffect, useState } from "react";

import {
  loadVoicePreferences,
  saveVoicePreferences,
  subscribeVoicePreferences,
} from "./preferences";
import {
  DEFAULT_VOICE_PREFERENCES,
  normalizeVoicePreferences,
  type VoicePreferences,
} from "./voice";

/** The voice choices of this phone, kept in step with every open screen. */
export function useVoicePreferences(): {
  preferences: VoicePreferences;
  save: (next: VoicePreferences) => void;
} {
  const [preferences, setPreferences] = useState<VoicePreferences>(
    DEFAULT_VOICE_PREFERENCES,
  );

  useEffect(() => {
    void loadVoicePreferences().then(setPreferences);
    return subscribeVoicePreferences(setPreferences);
  }, []);

  function save(next: VoicePreferences) {
    const normalized = normalizeVoicePreferences(next);
    setPreferences(normalized);
    void saveVoicePreferences(normalized);
  }

  return { preferences, save };
}
