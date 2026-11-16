import { StyleSheet, Text, View } from "react-native";

import {
  BrandMark,
  StatusPill,
  palette,
  radius,
  space,
  typography,
  type ConnectionState,
} from "@goble/common";

export function Sidebar({
  label,
  state,
  server,
}: {
  label: string;
  state: ConnectionState;
  server: string | null;
}) {
  return (
    <View style={styles.root}>
      <View style={styles.brandRow}>
        <BrandMark size={30} />
        <View style={styles.brandText}>
          <Text style={styles.brand}>Goble</Text>
          <Text style={styles.brandMeta}>Desktop</Text>
        </View>
      </View>

      <View style={styles.nav}>
        <View style={[styles.navRow, styles.navRowActive]}>
          <View style={styles.navDot} />
          <Text style={styles.navLabel}>Sessions</Text>
        </View>
      </View>

      <View style={styles.spacer} />

      <View style={styles.footer}>
        <Text style={styles.footerLabel}>Gateway</Text>
        <StatusPill label={label} state={state} />
        <Text numberOfLines={1} style={styles.server}>
          {server ?? "No gateway paired"}
        </Text>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    borderRightColor: palette.border,
    borderRightWidth: StyleSheet.hairlineWidth,
    flexDirection: "column",
    paddingHorizontal: space.lg,
    paddingVertical: space.lg,
    width: 280,
  },
  brandRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
  },
  brandText: {
    gap: 1,
  },
  brand: {
    ...typography.title,
    color: palette.text,
    fontSize: 21,
    letterSpacing: -0.5,
  },
  brandMeta: {
    ...typography.eyebrow,
    color: palette.subtle,
  },
  nav: {
    marginTop: space.xxl,
  },
  navRow: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
    paddingVertical: 10,
  },
  navRowActive: {
    backgroundColor: palette.raised,
  },
  navDot: {
    backgroundColor: palette.accent,
    borderRadius: 3,
    height: 6,
    width: 6,
  },
  navLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  spacer: {
    flex: 1,
  },
  footer: {
    borderTopColor: palette.border,
    borderTopWidth: StyleSheet.hairlineWidth,
    gap: space.sm,
    paddingHorizontal: space.sm,
    paddingTop: space.lg,
  },
  footerLabel: {
    ...typography.eyebrow,
    color: palette.muted,
  },
  server: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
  },
});
