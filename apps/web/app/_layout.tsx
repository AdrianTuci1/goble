import "../lib/platform";

import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { useEffect } from "react";
import { StyleSheet, View } from "react-native";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { connection, palette } from "@goble/common";

import { useFocusVisible } from "../lib/focus";

export default function RootLayout() {
  useFocusVisible();

  useEffect(() => {
    void connection.hydrate().then(() => {
      // Served by the gateway at `/app/`, the browser is already behind its
      // login: adopt this origin instead of asking for a PIN the page has
      // already answered. Anywhere else the stored pairing is the only one.
      if (connection.getSnapshot().phase !== "demo") return;
      const { origin, pathname } = window.location;
      if (pathname !== "/app" && !pathname.startsWith("/app/")) return;
      void connection.adoptBrowserSession(origin);
    });
  }, []);

  return (
    <SafeAreaProvider>
      <View style={styles.root}>
        <StatusBar style="light" />
        <Stack
          screenOptions={{
            headerShown: false,
            contentStyle: { backgroundColor: palette.bg },
          }}
        />
      </View>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    flex: 1,
  },
});
