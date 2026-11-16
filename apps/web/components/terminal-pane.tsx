import { useEffect } from "react";
import { StyleSheet, View } from "react-native";

import { TerminalSurface, useTerminal } from "@goble/common";

/**
 * The browser's shell: the shared terminal surface over one pty.
 *
 * The pty lives in the shared registry, not in this component, so leaving the
 * shell and coming back finds the same child and its scrollback instead of
 * opening a second one. `open()` is itself a no-op on a live pane, which is why
 * mounting is enough to start it and remounting is not a second shell.
 */
export function TerminalPane() {
  const { session, snapshot } = useTerminal("web:shell");

  useEffect(() => {
    session.open();
  }, [session]);

  return (
    <View style={styles.root}>
      <TerminalSurface session={session} snapshot={snapshot} />
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    paddingTop: 8,
  },
});
