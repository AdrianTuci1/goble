import { useState } from "react";
import { StyleSheet, Text } from "react-native";

import { palette, space, typography } from "../../lib/theme";
import { SettingsPage } from "./settings-page";
import { SettingsRow } from "./settings-row";
import { SettingsSearchField } from "./settings-search-field";
import { SettingsSection } from "./settings-section";

export interface ChoiceOption {
  id: string;
  label: string;
  description?: string;
  detail?: string;
}

export interface ChoiceGroup {
  title: string;
  options: ChoiceOption[];
}

/**
 * A full-screen list that picks one value: a search field on top, one card per
 * group, the current value checked. Rows that do not match the search drop out
 * with their group, so a provider with no match leaves no empty card behind.
 */
export function ChoicePage({
  title,
  placeholder,
  groups,
  value,
  footer,
  onSelect,
}: {
  title: string;
  placeholder: string;
  groups: ChoiceGroup[];
  value: string | null;
  footer?: string;
  onSelect: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const matches = (option: ChoiceOption) =>
    [option.label, option.description, option.detail]
      .filter(Boolean)
      .join(" ")
      .toLowerCase()
      .includes(needle);
  const visible = groups
    .map((group) => ({ ...group, options: group.options.filter(matches) }))
    .filter((group) => group.options.length > 0);

  return (
    <SettingsPage title={title}>
      <SettingsSearchField
        onChangeText={setQuery}
        placeholder={placeholder}
        value={query}
      />
      {visible.map((group) => (
        <SettingsSection key={group.title} title={group.title}>
          {group.options.map((option) => (
            <SettingsRow
              chevron={false}
              description={option.description}
              detail={option.detail}
              key={option.id}
              label={option.label}
              onPress={() => onSelect(option.id)}
              selected={option.id === value}
            />
          ))}
        </SettingsSection>
      ))}
      {needle && visible.length === 0 ? (
        <Text style={styles.empty}>No match for “{query.trim()}”</Text>
      ) : null}
      {footer ? <Text style={styles.footer}>{footer}</Text> : null}
    </SettingsPage>
  );
}

const styles = StyleSheet.create({
  empty: {
    ...typography.body,
    color: palette.muted,
    paddingHorizontal: space.sm,
    textAlign: "center",
  },
  footer: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 17,
    paddingHorizontal: space.lg,
  },
});
