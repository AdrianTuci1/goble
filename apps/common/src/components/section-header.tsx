import { Pressable, StyleSheet, Text, View } from "react-native";

import { palette, space, typography } from "../lib/theme";

export function SectionHeader({
  title,
  action,
  onPress,
}: {
  title: string;
  action?: string;
  onPress?: () => void;
}) {
  return (
    <View style={styles.row}>
      <Text style={styles.title}>{title}</Text>
      {action && onPress ? (
        <Pressable
          accessibilityRole="button"
          hitSlop={8}
          onPress={onPress}
          style={({ pressed }) => [pressed && styles.pressed]}
        >
          <Text style={styles.action}>{action}</Text>
        </Pressable>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  row: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
    marginBottom: space.md,
  },
  title: {
    ...typography.eyebrow,
    color: palette.muted,
  },
  action: {
    ...typography.caption,
    color: palette.accent,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.6,
  },
});
