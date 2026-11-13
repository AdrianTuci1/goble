import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsRow } from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { SettingsSwitchRow } from "../../../components/settings/settings-switch-row";
import { demoModels } from "../../../lib/mock";
import { saveModelPreferences } from "../../../lib/preferences";
import { useModelChoices } from "../../../lib/use-models";

export default function ModelsSettingsScreen() {
  const { allModels, models, preferences } = useModelChoices();
  const catalog = demoModels();
  const defaultModelId =
    preferences.defaultModelId ?? models[0]?.id ?? catalog[0]?.id ?? null;

  function update(next: {
    defaultModelId?: string | null;
    disabled?: string[];
  }) {
    void saveModelPreferences({ ...preferences, ...next });
  }

  function makeDefault(id: string) {
    update({
      defaultModelId: id,
      disabled: preferences.disabled.filter((entry) => entry !== id),
    });
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
        footer="New sessions start on the default model. The composer can pick another one for a turn."
        title="Default model"
      >
        {models.map((model) => (
          <SettingsRow
            description={`${model.vendor} · ${model.detail}`}
            key={model.id}
            label={model.name}
            onPress={() => makeDefault(model.id)}
            selected={defaultModelId === model.id}
          />
        ))}
      </SettingsSection>

      <SettingsSection
        footer="Hidden models keep their place in the gateway catalog. They only leave this phone's menus."
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
