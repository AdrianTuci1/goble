import { Pressable, StyleSheet, Text, View } from "react-native";

import { tapFeedback } from "../../lib/haptics";
import { palette, radius, space } from "../../lib/theme";

/** A compact choice strip, used for modes and thinking levels. */
export function SettingsSegmented<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <View accessibilityLabel={label} style={styles.track}>
      {options.map((option) => {
        const active = option.id === value;
        return (
          <Pressable
            accessibilityLabel={`${label}: ${option.label}`}
            accessibilityRole="button"
            accessibilityState={{ selected: active }}
            key={option.id}
            onPress={() => {
              if (active) return;
              tapFeedback();
              onChange(option.id);
            }}
            style={({ pressed }) => [
              styles.option,
              active && styles.optionActive,
              pressed && styles.pressed,
            ]}
          >
            <Text style={[styles.optionLabel, active && styles.optionLabelActive]}>
              {option.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

/** A segmented control inside a padded settings card. */
export function SettingsSegmentedCard<T extends string>(props: {
  label: string;
  description?: string;
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <View style={styles.card}>
      <Text style={styles.cardLabel}>{props.label}</Text>
      <SettingsSegmented
        label={props.label}
        onChange={props.onChange}
        options={props.options}
        value={props.value}
      />
      {props.description ? (
        <Text style={styles.cardDescription}>{props.description}</Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  track: {
    backgroundColor: palette.bg,
    borderRadius: radius.md,
    flexDirection: "row",
    gap: 3,
    padding: 3,
  },
  option: {
    alignItems: "center",
    borderRadius: radius.md - 2,
    flex: 1,
    justifyContent: "center",
    minHeight: 34,
    paddingHorizontal: space.sm,
  },
  optionActive: {
    backgroundColor: palette.overlay,
  },
  optionLabel: {
    color: palette.muted,
    fontSize: 14,
    fontWeight: "500",
    lineHeight: 19,
  },
  optionLabelActive: {
    color: palette.text,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.7,
  },
  card: {
    gap: space.sm,
    padding: space.lg,
  },
  cardLabel: {
    color: palette.text,
    fontSize: 16,
    lineHeight: 22,
  },
  cardDescription: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 19,
  },
});
