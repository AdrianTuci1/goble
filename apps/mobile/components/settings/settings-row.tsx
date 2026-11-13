import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, Text, View } from "react-native";
import type { ComponentProps, ReactNode } from "react";

import { tapFeedback } from "../../lib/haptics";
import { palette, space, typography, withAlpha } from "../../lib/theme";

export type SettingsIconName = ComponentProps<typeof Ionicons>["name"];

export interface SettingsRowBodyProps {
  icon?: SettingsIconName;
  /** Drawn in the icon column for marks a single icon cannot express. */
  leading?: ReactNode;
  label: string;
  description?: string;
  /** Short value kept on the right of the row. */
  detail?: string;
  /** Tints the value, for states such as Active or Paused. */
  detailTone?: "muted" | "success" | "warning" | "error";
  accessory?: ReactNode;
  tone?: "default" | "danger";
  disabled?: boolean;
}

export interface SettingsRowProps extends SettingsRowBodyProps {
  /** Shows a checkmark on the right, for the row the list chooses. */
  selected?: boolean;
  chevron?: boolean;
  onPress?: () => void;
  testID?: string;
}

/** The row contents, shared by tappable rows, toggles and cards. */
export function SettingsRowBody({
  icon,
  leading,
  label,
  description,
  detail,
  detailTone = "muted",
  accessory,
  tone = "default",
  disabled,
}: SettingsRowBodyProps) {
  const danger = tone === "danger";
  const detailColor =
    detailTone === "success"
      ? palette.success
      : detailTone === "warning"
        ? palette.warning
        : detailTone === "error"
          ? palette.error
          : palette.muted;
  return (
    <View
      style={[
        styles.body,
        description ? styles.bodyTall : null,
        disabled ? styles.disabled : null,
      ]}
    >
      {icon || leading ? (
        <View style={styles.iconBox}>
          {icon ? (
            <Ionicons
              color={danger ? palette.error : palette.text}
              name={icon}
              size={21}
            />
          ) : (
            leading
          )}
        </View>
      ) : null}
      <View style={styles.text}>
        <Text
          numberOfLines={1}
          style={[styles.label, danger ? styles.labelDanger : null]}
        >
          {label}
        </Text>
        {description ? (
          <Text numberOfLines={2} style={styles.description}>
            {description}
          </Text>
        ) : null}
      </View>
      {detail ? (
        <Text
          ellipsizeMode="middle"
          numberOfLines={1}
          style={[styles.detail, { color: detailColor }]}
        >
          {detail}
        </Text>
      ) : null}
      {accessory}
    </View>
  );
}

export function SettingsRow({
  selected,
  chevron,
  onPress,
  testID,
  ...body
}: SettingsRowProps) {
  const showsChevron = chevron ?? (onPress !== undefined && !selected);
  const content = (
    <SettingsRowBody
      {...body}
      accessory={
        <>
          {body.accessory}
          {selected ? (
            <Ionicons color={palette.accent} name="checkmark" size={19} />
          ) : showsChevron ? (
            <Ionicons
              color={withAlpha(palette.text, 0.35)}
              name="chevron-forward"
              size={16}
            />
          ) : null}
        </>
      }
    />
  );

  if (!onPress) return content;

  return (
    <Pressable
      accessibilityLabel={[body.label, body.description, body.detail]
        .filter(Boolean)
        .join(", ")}
      accessibilityRole="button"
      accessibilityState={{ disabled: Boolean(body.disabled), selected }}
      disabled={body.disabled}
      onPress={() => {
        tapFeedback();
        onPress();
      }}
      style={({ pressed }) => [styles.pressable, pressed && styles.pressed]}
      testID={testID}
    >
      {content}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  body: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    minHeight: 56,
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  bodyTall: {
    minHeight: 68,
    paddingVertical: 10,
  },
  disabled: {
    opacity: 0.45,
  },
  iconBox: {
    alignItems: "center",
    height: 24,
    justifyContent: "center",
    width: 24,
  },
  text: {
    flex: 1,
    gap: 2,
    minWidth: 0,
  },
  label: {
    color: palette.text,
    fontSize: 16,
    lineHeight: 22,
  },
  labelDanger: {
    color: palette.error,
  },
  description: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 14,
    lineHeight: 19,
  },
  detail: {
    ...typography.caption,
    color: palette.muted,
    flexShrink: 1,
    fontSize: 14,
    lineHeight: 19,
    maxWidth: "42%",
    textAlign: "right",
  },
  pressable: {
    overflow: "hidden",
  },
  pressed: {
    backgroundColor: palette.raised,
  },
});
