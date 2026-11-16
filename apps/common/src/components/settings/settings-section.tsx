import { StyleSheet, Text, View } from "react-native";
import type { ReactNode } from "react";

import { palette, space, typography } from "../../lib/theme";

/**
 * A grouped card of settings rows: a muted title above, the card, an optional
 * footnote under it. Cards are the raised surface, the screen stays behind.
 */
export function SettingsSection({
  title,
  footer,
  children,
}: {
  title?: string;
  footer?: string;
  children: ReactNode;
}) {
  return (
    <View style={styles.section}>
      {title ? <Text style={styles.title}>{title}</Text> : null}
      <View style={styles.card}>{children}</View>
      {footer ? <Text style={styles.footer}>{footer}</Text> : null}
    </View>
  );
}

/** The muted label above a card, for rows that sit outside one. */
export function SettingsSectionLabel({ children }: { children: string }) {
  return <Text style={styles.title}>{children}</Text>;
}

const styles = StyleSheet.create({
  section: {
    gap: space.sm,
  },
  title: {
    color: palette.muted,
    fontSize: 14,
    fontWeight: "500",
    lineHeight: 19,
    paddingHorizontal: space.sm,
  },
  card: {
    backgroundColor: palette.surface,
    borderRadius: 24,
    overflow: "hidden",
  },
  footer: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 17,
    paddingHorizontal: space.lg,
  },
});
