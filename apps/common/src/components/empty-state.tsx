import { Ionicons } from "@expo/vector-icons";
import { StyleSheet, Text, View } from "react-native";
import type { ComponentProps } from "react";

import { palette, radius, space, typography } from "../lib/theme";

type IconName = ComponentProps<typeof Ionicons>["name"];

export function EmptyState({
  icon,
  title,
  body,
}: {
  icon: IconName;
  title: string;
  body: string;
}) {
  return (
    <View style={styles.wrap}>
      <View style={styles.icon}>
        <Ionicons name={icon} size={22} color={palette.muted} />
      </View>
      <Text style={styles.title}>{title}</Text>
      <Text style={styles.body}>{body}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderStyle: "dashed",
    borderWidth: StyleSheet.hairlineWidth,
    gap: space.sm,
    paddingHorizontal: space.xl,
    paddingVertical: space.xxl,
  },
  icon: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderRadius: radius.pill,
    height: 44,
    justifyContent: "center",
    marginBottom: space.xs,
    width: 44,
  },
  title: {
    ...typography.callout,
    color: palette.text,
    textAlign: "center",
  },
  body: {
    ...typography.caption,
    color: palette.muted,
    lineHeight: 18,
    textAlign: "center",
  },
});
