import type { ComponentProps } from "react";
import { Platform, type TextStyle } from "react-native";
import { Ionicons } from "@expo/vector-icons";

import type { SessionStatus } from "./types";

type IconName = ComponentProps<typeof Ionicons>["name"];

/** Dark palette shared with the desktop shell. */
export const palette = {
  bg: "#0e0e0e",
  surface: "#161616",
  raised: "#1e1e1e",
  overlay: "#242424",
  border: "#262626",
  borderStrong: "#343434",
  text: "#e8e8e8",
  muted: "#8e8e8e",
  subtle: "#666666",
  primary: "#f1f1ef",
  accent: "#19aad8",
  accentSoft: "#10262d",
  success: "#10b981",
  successSoft: "#0d2a20",
  warning: "#f59e0b",
  warningSoft: "#2b2111",
  error: "#ef4444",
  errorSoft: "#2b1416",
  path: "#ff9e64",
  command: "#e0af68",
  white: "#ffffff",
  black: "#000000",
} as const;

export const space = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
  xxl: 32,
  huge: 48,
} as const;

export const radius = {
  sm: 8,
  md: 12,
  lg: 18,
  xl: 24,
  pill: 999,
} as const;

export const mono = Platform.select({
  ios: "Menlo",
  android: "monospace",
  default: "monospace",
});

export const typography = {
  hero: {
    color: palette.text,
    fontSize: 34,
    fontWeight: "700",
    letterSpacing: -1.1,
    lineHeight: 38,
  } satisfies TextStyle,
  title: {
    color: palette.text,
    fontSize: 20,
    fontWeight: "600",
    letterSpacing: -0.3,
    lineHeight: 26,
  } satisfies TextStyle,
  body: {
    color: palette.text,
    fontSize: 15,
    lineHeight: 21,
  } satisfies TextStyle,
  callout: {
    color: palette.text,
    fontSize: 14,
    fontWeight: "600",
    lineHeight: 19,
  } satisfies TextStyle,
  caption: {
    color: palette.muted,
    fontSize: 12,
    lineHeight: 16,
  } satisfies TextStyle,
  eyebrow: {
    color: palette.muted,
    fontSize: 10,
    fontWeight: "700",
    letterSpacing: 1.6,
    lineHeight: 14,
  } satisfies TextStyle,
  mono: {
    color: palette.muted,
    fontFamily: mono,
    fontSize: 11,
    lineHeight: 16,
  } satisfies TextStyle,
};

export const sessionStatusColor: Record<SessionStatus, string> = {
  live: palette.success,
  waiting: palette.warning,
  done: palette.accent,
};

export function withAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  if (value.length !== 6) return hex;
  const r = Number.parseInt(value.slice(0, 2), 16);
  const g = Number.parseInt(value.slice(2, 4), 16);
  const b = Number.parseInt(value.slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
