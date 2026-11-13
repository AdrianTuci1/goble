import { Ionicons } from "@expo/vector-icons";
import { useRouter } from "expo-router";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import type { ReactNode } from "react";

import { tapFeedback } from "../../lib/haptics";
import { useOpenDrawer } from "../../lib/navigation";
import { palette, radius, space } from "../../lib/theme";
import { MenuButton } from "../menu-button";

const SCREEN_PADDING = 20;

/** The list the drawer opens: a menu button and the screen title. */
export function SettingsRootPage({
  title,
  right,
  children,
}: {
  title: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  const openDrawer = useOpenDrawer();
  return (
    <SettingsShell
      leading={<MenuButton onPress={openDrawer} />}
      right={right}
      title={title}
    >
      {children}
    </SettingsShell>
  );
}

/** A settings page opened from the list: a back button and the page title. */
export function SettingsPage({
  title,
  right,
  children,
}: {
  title: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  const router = useRouter();
  return (
    <SettingsShell
      leading={
        <Pressable
          accessibilityLabel="Back"
          accessibilityRole="button"
          hitSlop={8}
          onPress={() => {
            tapFeedback();
            if (router.canGoBack()) router.back();
            else router.replace("/settings");
          }}
          style={({ pressed }) => [styles.back, pressed && styles.pressed]}
        >
          <Ionicons color={palette.text} name="chevron-back" size={20} />
        </Pressable>
      }
      right={right}
      title={title}
    >
      {children}
    </SettingsShell>
  );
}

function SettingsShell({
  title,
  leading,
  right,
  children,
}: {
  title: string;
  leading: ReactNode;
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <SafeAreaView edges={["top", "left", "right"]} style={styles.safe}>
      <View style={styles.header}>
        <View style={styles.slot}>{leading}</View>
        <Text numberOfLines={1} style={styles.title}>
          {title}
        </Text>
        <View style={[styles.slot, styles.slotEnd]}>{right}</View>
      </View>
      <ScrollView
        contentContainerStyle={styles.content}
        contentInsetAdjustmentBehavior="automatic"
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        {children}
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  header: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
    minHeight: 48,
    paddingHorizontal: SCREEN_PADDING,
    paddingVertical: space.sm,
  },
  slot: {
    alignItems: "flex-start",
    width: 38,
  },
  slotEnd: {
    alignItems: "flex-end",
  },
  title: {
    color: palette.text,
    flex: 1,
    fontSize: 17,
    fontWeight: "600",
    lineHeight: 22,
    textAlign: "center",
  },
  back: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderRadius: radius.pill,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  pressed: {
    opacity: 0.65,
  },
  content: {
    gap: space.xl,
    paddingBottom: 120,
    paddingHorizontal: SCREEN_PADDING,
    paddingTop: space.lg,
  },
});
