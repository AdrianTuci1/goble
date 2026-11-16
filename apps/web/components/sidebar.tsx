import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, Text, View } from "react-native";

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
  canShell,
  shellOpen,
  onNewShell,
  onShowSessions,
}: {
  label: string;
  state: ConnectionState;
  server: string | null;
  canShell: boolean;
  shellOpen: boolean;
  onNewShell: () => void;
  onShowSessions: () => void;
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

      <Pressable
        accessibilityRole="button"
        disabled={!canShell}
        onPress={onNewShell}
        style={({ pressed }) => [
          styles.newShell,
          !canShell && styles.newShellDisabled,
          pressed && canShell && styles.pressed,
        ]}
      >
        <Ionicons
          color={canShell ? palette.bg : palette.subtle}
          name="add"
          size={15}
        />
        <Text
          style={[
            styles.newShellLabel,
            !canShell && styles.newShellLabelDisabled,
          ]}
        >
          New shell
        </Text>
      </Pressable>
      {!canShell ? (
        <Text style={styles.newShellHint}>No worker connected</Text>
      ) : null}

      <View style={styles.nav}>
        <Pressable
          accessibilityRole="button"
          onPress={onShowSessions}
          style={({ pressed }) => [
            styles.navRow,
            !shellOpen && styles.navRowActive,
            pressed && styles.pressed,
          ]}
        >
          <View style={styles.navDot} />
          <Text style={styles.navLabel}>Sessions</Text>
        </Pressable>
        {shellOpen ? (
          <View style={[styles.navRow, styles.navRowActive]}>
            <View style={styles.navDot} />
            <Text style={styles.navLabel}>Shell</Text>
          </View>
        ) : null}
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
  newShell: {
    alignItems: "center",
    backgroundColor: palette.accent,
    borderRadius: radius.md,
    flexDirection: "row",
    gap: 6,
    justifyContent: "center",
    marginTop: space.xl,
    paddingVertical: 9,
  },
  newShellDisabled: {
    backgroundColor: palette.raised,
  },
  newShellLabel: {
    ...typography.callout,
    color: palette.bg,
    fontSize: 13,
    fontWeight: "600",
  },
  newShellLabelDisabled: {
    color: palette.subtle,
  },
  newShellHint: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
    marginTop: 6,
    textAlign: "center",
  },
  nav: {
    marginTop: space.xl,
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
  pressed: {
    opacity: 0.7,
  },
});
