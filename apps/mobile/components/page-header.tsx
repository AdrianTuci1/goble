import { StyleSheet, Text, View } from "react-native";
import type { ReactNode } from "react";

import { useOpenDrawer } from "../lib/navigation";
import { palette, space, typography } from "../lib/theme";
import { MenuButton } from "./menu-button";

export function PageHeader({
  eyebrow,
  title,
  right,
}: {
  eyebrow?: string;
  title: string;
  right?: ReactNode;
}) {
  const openDrawer = useOpenDrawer();
  return (
    <View style={styles.header}>
      <MenuButton onPress={openDrawer} />
      <View style={styles.text}>
        {eyebrow ? <Text style={styles.eyebrow}>{eyebrow}</Text> : null}
        <Text style={styles.title}>{title}</Text>
      </View>
      {right ?? <View style={styles.placeholder} />}
    </View>
  );
}

const styles = StyleSheet.create({
  header: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingBottom: space.xl,
    paddingTop: space.sm,
  },
  text: {
    flex: 1,
    gap: 1,
  },
  eyebrow: {
    ...typography.eyebrow,
    color: palette.muted,
    fontSize: 9,
  },
  title: {
    ...typography.title,
    color: palette.text,
    fontSize: 24,
    lineHeight: 28,
  },
  placeholder: {
    width: 38,
  },
});
