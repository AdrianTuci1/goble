import "react-native-gesture-handler";
import "../lib/platform";

import { Stack } from "expo-router";
import { useEffect } from "react";
import { StatusBar } from "expo-status-bar";
import { StyleSheet } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { AuthGate } from "../components/auth-gate";
import { setHapticsEnabled } from "../lib/haptics";
import {
  notifyAgentNeedsHelp,
  notifyRunFinished,
  syncPushRegistration,
} from "../lib/notifications";
import { loadHapticsPreference } from "../lib/preferences";
import { configureTurnNotifier, connection, palette } from "@goble/common";

configureTurnNotifier({
  runFinished(title) {
    void notifyRunFinished(title);
  },
  agentNeedsHelp(title) {
    void notifyAgentNeedsHelp(title);
  },
});

export default function RootLayout() {
  useEffect(() => {
    void syncPushRegistration();
    void loadHapticsPreference().then(setHapticsEnabled);
    void connection.hydrate();
  }, []);

  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <StatusBar style="light" />
        <AuthGate>
          <Stack
            screenOptions={{
              headerShown: false,
              contentStyle: { backgroundColor: palette.bg },
              animation: "slide_from_right",
            }}
          >
            <Stack.Screen name="(drawer)" />
            <Stack.Screen name="sign-in" options={{ animation: "fade" }} />
            <Stack.Screen name="+not-found" />
          </Stack>
        </AuthGate>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    flex: 1,
  },
});
