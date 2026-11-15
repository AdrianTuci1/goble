import { useRouter } from "expo-router";

import {
  ChoicePage,
  type ChoiceGroup,
} from "../../../components/settings/choice-page";
import { saveModelPreferences } from "../../../lib/preferences";
import type { Model } from "../../../lib/types";
import { useModelChoices } from "../../../lib/use-models";

/** The catalog grouped by the vendor that serves it, in catalog order. */
function groupByVendor(models: Model[]): ChoiceGroup[] {
  const groups: ChoiceGroup[] = [];
  for (const model of models) {
    const option = {
      description: model.detail,
      id: model.id,
      label: model.name,
    };
    const group = groups.find((entry) => entry.title === model.vendor);
    if (group) group.options.push(option);
    else groups.push({ options: [option], title: model.vendor });
  }
  return groups;
}

export default function DefaultModelScreen() {
  const router = useRouter();
  const { defaultModel, models, preferences } = useModelChoices();

  return (
    <ChoicePage
      footer="New sessions start here. The composer can pick another model for a single turn."
      groups={groupByVendor(models)}
      onSelect={(id) => {
        void saveModelPreferences({
          ...preferences,
          defaultModelId: id,
          disabled: preferences.disabled.filter((entry) => entry !== id),
        });
        router.back();
      }}
      placeholder="Search models"
      title="Default model"
      value={defaultModel.id}
    />
  );
}
