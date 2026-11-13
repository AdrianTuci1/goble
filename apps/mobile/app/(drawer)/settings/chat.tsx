import { useEffect, useRef, useState } from "react";
import { StyleSheet, TextInput } from "react-native";

import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsSegmentedCard } from "../../../components/settings/settings-segmented";
import { SettingsSection } from "../../../components/settings/settings-section";
import {
  DEFAULT_MODEL_PREFERENCES,
  loadModelPreferences,
  loadPreferences,
  saveDefaultMode,
  saveInstructions,
  saveModelPreferences,
  type ModelPreferences,
} from "../../../lib/preferences";
import { palette, typography } from "../../../lib/theme";
import type { SessionMode, ThinkingLevel } from "../../../lib/types";

const modes: { id: SessionMode; label: string }[] = [
  { id: "agent", label: "Agent" },
  { id: "terminal", label: "Terminal" },
];

const levels: { id: ThinkingLevel; label: string }[] = [
  { id: "low", label: "Low" },
  { id: "standard", label: "Standard" },
  { id: "high", label: "High" },
];

export default function ChatSettingsScreen() {
  const [mode, setMode] = useState<SessionMode>("agent");
  const [models, setModels] = useState<ModelPreferences>(
    DEFAULT_MODEL_PREFERENCES,
  );
  const [instructions, setInstructions] = useState("");
  const loaded = useRef(false);

  useEffect(() => {
    void loadPreferences().then((preferences) => {
      setMode(preferences.mode);
      setInstructions(preferences.instructions);
      loaded.current = true;
    });
    void loadModelPreferences().then(setModels);
  }, []);

  // Instructions save as you type, a beat after the last keystroke.
  useEffect(() => {
    if (!loaded.current) return;
    const timer = setTimeout(() => {
      void saveInstructions(instructions);
    }, 500);
    return () => clearTimeout(timer);
  }, [instructions]);

  function updateMode(next: SessionMode) {
    setMode(next);
    void saveDefaultMode(next);
  }

  function updateThinking(next: ThinkingLevel) {
    const preferences = { ...models, defaultThinking: next };
    setModels(preferences);
    void saveModelPreferences(preferences);
  }

  return (
    <SettingsPage title="Chat">
      <SettingsSection
        footer="The composer on the home screen opens in this mode."
        title="New sessions"
      >
        <SettingsSegmentedCard
          label="Default mode"
          onChange={updateMode}
          options={modes}
          value={mode}
        />
      </SettingsSection>

      <SettingsSection
        footer="A turn can still change the level from the composer. Higher levels take longer and think further."
        title="Thinking"
      >
        <SettingsSegmentedCard
          label="Default level"
          onChange={updateThinking}
          options={levels}
          value={models.defaultThinking}
        />
      </SettingsSection>

      <SettingsSection
        footer="Saved on this phone and sent with every turn from it."
        title="Instructions"
      >
        <TextInput
          accessibilityLabel="Custom instructions"
          multiline
          onChangeText={setInstructions}
          placeholder="Tell the agent how you like to work: languages, tone, commands to avoid."
          placeholderTextColor={palette.subtle}
          style={styles.input}
          textAlignVertical="top"
          value={instructions}
        />
      </SettingsSection>
    </SettingsPage>
  );
}

const styles = StyleSheet.create({
  input: {
    ...typography.body,
    color: palette.text,
    fontSize: 15,
    lineHeight: 21,
    minHeight: 132,
    padding: 16,
  },
});
