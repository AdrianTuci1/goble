import { useRouter } from "expo-router";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  SettingsSwitchRow,
  demoModels,
  saveModelPreferences,
  useModelChoices,
} from "@goble/common";

export default function ModelsSettingsScreen() {
  const { allModels, defaultModel, models, preferences } = useModelChoices();
  const router = useRouter();
  const catalog = demoModels();

  function update(next: { defaultModelId?: string | null; disabled?: string[] }) {
    void saveModelPreferences({ ...preferences, ...next });
  }

  function toggle(id: string, enabled: boolean) {
    const disabled = new Set(preferences.disabled);
    if (enabled) disabled.delete(id);
    else disabled.add(id);
    const remaining = allModels.filter((model) => !disabled.has(model.id));
    if (remaining.length === 0) return;
    const defaultModelId =
      preferences.defaultModelId && disabled.has(preferences.defaultModelId)
        ? remaining[0]!.id
        : preferences.defaultModelId;
    update({ defaultModelId, disabled: [...disabled] });
  }

  return (
    <SettingsPage title="Models">
      <SettingsSection
        footer="New sessions start on this model. The composer can pick another one for a turn."
        title="Default model"
      >
        <SettingsRow
          chevron
          description={`${defaultModel.vendor} · ${defaultModel.detail}`}
          label={defaultModel.name}
          onPress={() => router.push("/settings/default-model")}
        />
      </SettingsSection>

      <SettingsSection
        footer="A model switched off leaves the menus on this phone; the gateway keeps it in the catalog."
        title="Available on this phone"
      >
        {catalog.map((model) => {
          const enabled = !preferences.disabled.includes(model.id);
          const lastEnabled = enabled && models.length === 1;
          return (
            <SettingsSwitchRow
              description={`${model.vendor} · ${model.detail}`}
              disabled={lastEnabled}
              key={model.id}
              label={model.name}
              onValueChange={(next) => toggle(model.id, next)}
              value={enabled}
            />
          );
        })}
      </SettingsSection>
    </SettingsPage>
  );
}
