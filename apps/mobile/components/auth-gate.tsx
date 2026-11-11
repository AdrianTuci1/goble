import { useRouter } from "expo-router";
import { useEffect, useState } from "react";
import { StyleSheet, View } from "react-native";
import type { ReactNode } from "react";

import { loadAuthMode } from "../lib/api";
import { palette } from "../lib/theme";

/**
 * Sends a fresh install to sign-in. Once a server or the demo workspace has
 * been chosen the app opens straight into the empty session.
 */
export function AuthGate({ children }: { children: ReactNode }) {
  const router = useRouter();
  const [checked, setChecked] = useState(false);

  useEffect(() => {
    let active = true;
    void loadAuthMode().then((mode) => {
      if (!active) return;
      setChecked(true);
      if (!mode) router.replace("/sign-in");
    });
    return () => {
      active = false;
    };
  }, [router]);

  if (!checked) return <View style={styles.blank} />;
  return <>{children}</>;
}

const styles = StyleSheet.create({
  blank: {
    backgroundColor: palette.bg,
    flex: 1,
  },
});
