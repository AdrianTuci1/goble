import { Ionicons } from "@expo/vector-icons";
import { useEffect, useRef, useState, type ComponentRef } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
  type LayoutChangeEvent,
  type ScrollViewInstance,
} from "react-native";

import { getPlatformHaptics } from "../platform";
import { palette, radius, space, typography } from "../lib/theme";
import type { TerminalSession, TerminalSnapshot } from "../lib/remote/terminal";

import { PulseDot } from "./pulse-dot";

/**
 * The phone's terminal: monospace output, a key bar and a line input.
 *
 * The desktop draws a full terminal grid; the phone keeps the same shell and
 * the same bytes, and shows them as text. Escape sequences are consumed before
 * they reach this view (see `TerminalBuffer`), so what is drawn is the shell's
 * words. Keys a touch keyboard cannot type live on the bar above the input.
 */
export function TerminalSurface({
  session,
  snapshot,
  onSwitchToAgent,
}: {
  session: TerminalSession;
  snapshot: TerminalSnapshot;
  onSwitchToAgent: () => void;
}) {
  const scrollRef = useRef<ScrollViewInstance>(null);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<ComponentRef<typeof TextInput>>(null);
  const fontSize = 12;
  const lineHeight = 17;

  useEffect(() => {
    const timer = setTimeout(
      () => scrollRef.current?.scrollToEnd({ animated: false }),
      40,
    );
    return () => clearTimeout(timer);
  }, [snapshot.output]);

  /** Fit the shell to the space the output has, the way the desktop fits a pane. */
  function measure(event: LayoutChangeEvent) {
    const { height, width } = event.nativeEvent.layout;
    session.resize(
      Math.floor(width / (fontSize * 0.62)),
      Math.floor(height / lineHeight),
    );
  }

  const live = snapshot.state === "live";
  const tone =
    snapshot.state === "error"
      ? palette.error
      : live
        ? palette.success
        : snapshot.state === "opening"
          ? palette.warning
          : palette.muted;
  const status =
    snapshot.state === "live"
      ? "Live"
      : snapshot.state === "opening"
        ? "Opening…"
        : snapshot.state === "exited"
          ? snapshot.exitCode === null
            ? "Closed"
            : `Exited ${snapshot.exitCode}`
          : snapshot.state === "error"
            ? "Not connected"
            : "No worker link";

  const keys: { label: string; send: string }[] = [
    { label: "esc", send: "\u001b" },
    { label: "tab", send: "\t" },
    { label: "ctrl-c", send: "\u0003" },
    { label: "↑", send: "\u001b[A" },
    { label: "↓", send: "\u001b[B" },
  ];

  return (
    <View style={styles.root} testID="terminal-surface">
      <View style={styles.bar}>
        <View style={styles.status}>
          {live ? (
            <PulseDot color={tone} size={6} />
          ) : (
            <View style={[styles.dot, { backgroundColor: tone }]} />
          )}
          <Text style={styles.statusText}>{status}</Text>
        </View>
        <View style={styles.barActions}>
          <Pressable
            accessibilityLabel="Restart terminal"
            accessibilityRole="button"
            hitSlop={8}
            onPress={() => {
              getPlatformHaptics().tap();
              session.restart();
            }}
            style={({ pressed }) => [styles.barButton, pressed && styles.pressed]}
          >
            <Ionicons color={palette.muted} name="refresh" size={15} />
          </Pressable>
          <Pressable
            accessibilityLabel="Switch to the agent"
            accessibilityRole="button"
            hitSlop={8}
            onPress={() => {
              getPlatformHaptics().tap();
              onSwitchToAgent();
            }}
            style={({ pressed }) => [styles.barButton, pressed && styles.pressed]}
            testID="terminal-to-agent"
          >
            <Ionicons color={palette.muted} name="chatbubble-outline" size={15} />
          </Pressable>
        </View>
      </View>

      <ScrollView
        contentContainerStyle={styles.outputPad}
        keyboardShouldPersistTaps="handled"
        onLayout={measure}
        ref={scrollRef}
        showsVerticalScrollIndicator={false}
        style={styles.output}
      >
        <Text
          selectable
          style={[styles.mono, { fontSize, lineHeight }]}
          testID="terminal-output"
        >
          {snapshot.output ||
            (snapshot.state === "opening"
              ? "Opening a shell on the worker…"
              : (snapshot.error ??
                "A shell opens on the worker behind the gateway. Pair in Settings, then come back: the shell runs there and this screen is its window."))}
        </Text>
      </ScrollView>

      <View style={styles.keys}>
        {keys.map((key) => (
          <Pressable
            accessibilityLabel={`Send ${key.label}`}
            accessibilityRole="button"
            disabled={!live}
            key={key.label}
            onPress={() => {
              getPlatformHaptics().tap();
              session.write(key.send);
            }}
            style={({ pressed }) => [
              styles.key,
              !live && styles.keyDisabled,
              pressed && styles.pressed,
            ]}
          >
            <Text style={styles.keyLabel}>{key.label}</Text>
          </Pressable>
        ))}
        <Pressable
          accessibilityLabel="Focus the terminal input"
          accessibilityRole="button"
          onPress={() => inputRef.current?.focus()}
          style={({ pressed }) => [styles.key, pressed && styles.pressed]}
        >
          <Ionicons color={palette.muted} name="keypad-outline" size={14} />
        </Pressable>
      </View>

      <View style={styles.inputRow}>
        <TextInput
          accessibilityLabel="Terminal command"
          autoCapitalize="none"
          autoCorrect={false}
          blurOnSubmit={false}
          editable={live}
          onChangeText={setDraft}
          onSubmitEditing={() => {
            session.submit(draft);
            setDraft("");
          }}
          placeholder={live ? "Run a command" : "No live shell"}
          placeholderTextColor={palette.subtle}
          ref={inputRef}
          returnKeyType="send"
          selectionColor={palette.accent}
          style={styles.input}
          testID="terminal-input"
          value={draft}
        />
        <Pressable
          accessibilityLabel="Run command"
          accessibilityRole="button"
          disabled={!live || !draft.trim()}
          onPress={() => {
            getPlatformHaptics().tap();
            session.submit(draft);
            setDraft("");
          }}
          style={({ pressed }) => [
            styles.send,
            (!live || !draft.trim()) && styles.sendDisabled,
            pressed && styles.pressed,
          ]}
        >
          <Ionicons
            color={live && draft.trim() ? palette.bg : palette.subtle}
            name="arrow-up"
            size={16}
          />
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
  bar: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
    paddingHorizontal: space.lg,
    paddingVertical: space.sm,
  },
  status: {
    alignItems: "center",
    flexDirection: "row",
    gap: 6,
  },
  dot: {
    borderRadius: 3,
    height: 6,
    width: 6,
  },
  statusText: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 11,
  },
  barActions: {
    flexDirection: "row",
    gap: 4,
  },
  barButton: {
    alignItems: "center",
    height: 28,
    justifyContent: "center",
    width: 28,
  },
  output: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flex: 1,
    marginHorizontal: space.lg,
  },
  outputPad: {
    padding: space.md,
  },
  mono: {
    color: palette.text,
    fontFamily: typography.mono.fontFamily,
  },
  keys: {
    flexDirection: "row",
    gap: 6,
    paddingHorizontal: space.lg,
    paddingTop: space.md,
  },
  key: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.sm,
    borderWidth: StyleSheet.hairlineWidth,
    flex: 1,
    justifyContent: "center",
    minHeight: 32,
    paddingHorizontal: 6,
  },
  keyDisabled: {
    opacity: 0.4,
  },
  keyLabel: {
    ...typography.mono,
    color: palette.text,
    fontSize: 11,
  },
  inputRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
    paddingHorizontal: space.lg,
    paddingTop: space.sm,
  },
  input: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    color: palette.text,
    flex: 1,
    fontFamily: typography.mono.fontFamily,
    fontSize: 13,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 12,
    paddingVertical: 10,
  },
  send: {
    alignItems: "center",
    backgroundColor: palette.accent,
    borderRadius: radius.md,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  sendDisabled: {
    backgroundColor: palette.raised,
  },
  pressed: {
    opacity: 0.65,
  },
});
