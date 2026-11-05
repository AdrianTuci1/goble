import { Image, StyleSheet, Text, View } from "react-native";

import { palette, typography } from "../lib/theme";
import type { ChatMessage } from "../lib/types";

const outputColor = "#d6d6d6";

/**
 * One command and its output, drawn as the desktop's terminal block: a
 * hairline, a muted context row, the `❯` command line and the output rows. The
 * block carries no card chrome; running and failed blocks stand on a left pole.
 */
export function TerminalBlock({ message }: { message: ChatMessage }) {
  const command = message.command ?? message.text;
  const output = message.output ?? "";
  const lines = output ? output.split("\n") : [];
  const running =
    message.status === "running" || message.status === "waiting_approval";
  const failed = message.status === "error";
  const meta = [
    message.cwd,
    message.branch ? `git:(${message.branch})` : null,
    message.duration,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <View
      style={[
        styles.block,
        failed && styles.blockFailed,
        (running || failed) && styles.blockPole,
        running && { borderLeftColor: palette.accent },
        failed && { borderLeftColor: palette.error },
      ]}
    >
      {message.image ? (
        <Image source={{ uri: message.image }} style={styles.image} />
      ) : null}
      <View style={styles.rule} />
      {meta ? <Text style={styles.meta}>{meta}</Text> : null}
      <View style={styles.commandRow}>
        <Text style={styles.prompt}>❯ </Text>
        <Text style={styles.command}>{command}</Text>
      </View>
      {lines.map((line, index) => (
        <Text key={index} style={styles.output}>
          {line || " "}
        </Text>
      ))}
    </View>
  );
}

const styles = StyleSheet.create({
  block: {
    gap: 1,
    paddingVertical: 2,
  },
  blockFailed: {
    backgroundColor: "rgba(239, 68, 68, 0.07)",
  },
  blockPole: {
    borderLeftWidth: 2,
    paddingLeft: 10,
  },
  rule: {
    backgroundColor: palette.border,
    height: StyleSheet.hairlineWidth,
    marginBottom: 7,
  },
  meta: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 10,
    lineHeight: 15,
    marginBottom: 5,
  },
  commandRow: {
    flexDirection: "row",
  },
  prompt: {
    ...typography.mono,
    color: palette.accent,
    fontSize: 12.5,
    lineHeight: 19,
  },
  command: {
    ...typography.mono,
    color: palette.command,
    flex: 1,
    fontSize: 12.5,
    lineHeight: 19,
  },
  output: {
    ...typography.mono,
    color: outputColor,
    fontSize: 12.5,
    lineHeight: 19,
  },
  image: {
    backgroundColor: palette.raised,
    borderRadius: 12,
    height: 148,
    marginBottom: 10,
    width: 148,
  },
});
