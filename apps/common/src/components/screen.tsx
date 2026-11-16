import { SafeAreaView } from "react-native-safe-area-context";
import {
  RefreshControl,
  ScrollView,
  StyleSheet,
  View,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import type { ReactNode } from "react";

import { palette, space } from "../lib/theme";

export function Screen({
  children,
  scroll = true,
  refreshing = false,
  onRefresh,
  contentContainerStyle,
}: {
  children: ReactNode;
  scroll?: boolean;
  refreshing?: boolean;
  onRefresh?: () => void;
  contentContainerStyle?: StyleProp<ViewStyle>;
}) {
  if (!scroll) {
    return (
      <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
        <View style={[styles.content, contentContainerStyle]}>{children}</View>
      </SafeAreaView>
    );
  }

  return (
    <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
      <ScrollView
        contentContainerStyle={[styles.scrollContent, contentContainerStyle]}
        contentInsetAdjustmentBehavior="automatic"
        keyboardShouldPersistTaps="handled"
        refreshControl={
          onRefresh ? (
            <RefreshControl
              refreshing={refreshing}
              onRefresh={onRefresh}
              tintColor={palette.muted}
            />
          ) : undefined
        }
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
  content: {
    flex: 1,
    paddingHorizontal: space.lg,
  },
  scrollContent: {
    paddingBottom: 120,
    paddingHorizontal: space.lg,
  },
});
