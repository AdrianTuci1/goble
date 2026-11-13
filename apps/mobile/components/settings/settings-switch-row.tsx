import { Switch } from "react-native";

import { tapFeedback } from "../../lib/haptics";
import { palette } from "../../lib/theme";
import { SettingsRowBody, type SettingsIconName } from "./settings-row";

/** A settings row with a switch on the right. */
export function SettingsSwitchRow({
  icon,
  label,
  description,
  value,
  onValueChange,
  disabled,
}: {
  icon?: SettingsIconName;
  label: string;
  description?: string;
  value: boolean;
  onValueChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <SettingsRowBody
      description={description}
      disabled={disabled}
      icon={icon}
      label={label}
      accessory={
        <Switch
          accessibilityLabel={label}
          disabled={disabled}
          ios_backgroundColor={palette.raised}
          onValueChange={(next) => {
            tapFeedback();
            onValueChange(next);
          }}
          thumbColor={value ? palette.white : palette.muted}
          trackColor={{ false: palette.raised, true: palette.accent }}
          value={value}
        />
      }
    />
  );
}
