import { StyleSheet, Text, View } from "react-native";

import { palette, radius, space, typography } from "../lib/theme";

export type ConnectionState = "online" | "offline" | "demo";

const tone: Record<
  ConnectionState,
  { dot: string; text: string; background: string; border: string }
> = {
  online: {
    dot: palette.success,
    text: palette.success,
    background: palette.successSoft,
    border: "rgba(16, 185, 129, 0.24)",
  },
  offline: {
    dot: palette.error,
    text: palette.error,
    background: palette.errorSoft,
    border: "rgba(239, 68, 68, 0.24)",
  },
  demo: {
    dot: palette.warning,
    text: palette.warning,
    background: palette.warningSoft,
    border: "rgba(245, 158, 11, 0.24)",
  },
};

export function StatusPill({
  label,
  state,
}: {
  label: string;
  state: ConnectionState;
}) {
  const colors = tone[state];
  return (
    <View
      style={[
        styles.pill,
        { backgroundColor: colors.background, borderColor: colors.border },
      ]}
    >
      <View style={[styles.dot, { backgroundColor: colors.dot }]} />
      <Text style={[styles.label, { color: colors.text }]} numberOfLines={1}>
        {label}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  pill: {
    alignItems: "center",
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.sm,
    maxWidth: 190,
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  dot: {
    borderRadius: 3,
    height: 6,
    width: 6,
  },
  label: {
    ...typography.caption,
    fontWeight: "600",
  },
});
