import { Ionicons } from "@expo/vector-icons";
import type { ComponentProps } from "react";

import { palette } from "./theme";

type IconName = ComponentProps<typeof Ionicons>["name"];

/** The harness the app runs on, and the one a conversation started here gets. */
export const nativeHarness = "goble";

/** What a sidebar row draws for a conversation's harness. */
export interface HarnessMark {
  /** The family's name, as the row's accessible label reads it. */
  label: string;
  /** The single tint the mark is drawn in. */
  accent: string;
  /** The family's glyph; `null` draws Goble's own mark. */
  glyph: IconName | null;
}

const nativeMark: HarnessMark = {
  label: "Goble",
  accent: palette.accent,
  glyph: null,
};

/**
 * The mark each harness family draws, keyed by its catalog id
 * (`goble_core::harness`). The accents are the family colours the desktop
 * sidebar uses; every mark is one tint, so the accent is the only colour it
 * takes.
 */
const marks: Record<string, HarnessMark> = {
  [nativeHarness]: nativeMark,
  claude: { label: "Claude Code", accent: "#d97757", glyph: "sparkles" },
  codex: { label: "Codex", accent: "#74aa9c", glyph: "infinite" },
  gemini: { label: "Gemini CLI", accent: "#4285f4", glyph: "diamond" },
  grok: { label: "Grok", accent: palette.text, glyph: "flash" },
  cursor: { label: "Cursor", accent: "#b4b4b4", glyph: "navigate" },
  copilot: { label: "GitHub Copilot", accent: "#8b5cf6", glyph: "glasses" },
};

/**
 * The mark for a harness id. An id this build does not know draws the native
 * mark, the way the desktop sidebar falls back to it.
 */
export function harnessMark(harness: string): HarnessMark {
  return marks[harness] ?? nativeMark;
}
