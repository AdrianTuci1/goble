import { useMemo } from "react";
import {
  Linking,
  StyleSheet,
  Text,
  type StyleProp,
  type TextStyle,
} from "react-native";

import { mono, palette } from "../lib/theme";

/** The inline styles a single run can carry at once. */
export type RichStyle = {
  bold?: boolean;
  italic?: boolean;
  code?: boolean;
  color?: string;
  link?: string;
};

export type RichRun = {
  text: string;
  style: RichStyle;
};

/**
 * The named colours `{color:…}` accepts, shared with the desktop transcript.
 * They are the palette's own status colours, so a reply's colour reads as part
 * of the interface rather than as an arbitrary value.
 */
const NAMED_COLORS: Record<string, string> = {
  red: "#ef4444",
  orange: "#ff9e64",
  yellow: "#f59e0b",
  green: "#10b981",
  blue: "#19aad8",
  purple: "#a78bfa",
  cyan: "#22d3ee",
  gray: "#8e8e8e",
  grey: "#8e8e8e",
  white: "#f1f1ef",
  black: "#0e0e0e",
};

const EMPHASIS: Array<{
  token: string;
  bold?: boolean;
  italic?: boolean;
}> = [
  { token: "***", bold: true, italic: true },
  { token: "___", bold: true, italic: true },
  { token: "**", bold: true },
  { token: "__", bold: true },
  { token: "*", italic: true },
  { token: "_", italic: true },
];

/** Resolve a `{color:…}` value: `#rrggbb` first, then the shared names. */
export function resolveTextColor(value: string): string | null {
  const normalized = value.trim().toLowerCase();
  const named = NAMED_COLORS[normalized];
  if (named) return named;
  return /^#[0-9a-f]{6}$/.test(normalized) ? normalized : null;
}

/**
 * Split a reply into styled runs. The grammar mirrors the desktop parser:
 * `**bold**`, `_italic_`, `` `code` ``, `[label](url)` and
 * `{color:#rrggbb}text{/color}`. A marker with no closing delimiter stays
 * literal text.
 */
export function parseRichText(input: string): RichRun[] {
  const runs: RichRun[] = [];
  parseInline(input, {}, runs, 0);
  return runs;
}

function parseInline(
  input: string,
  style: RichStyle,
  out: RichRun[],
  depth: number,
): void {
  if (depth > 8) {
    if (input) out.push({ text: input, style });
    return;
  }

  let index = 0;
  let buffer = "";

  const flush = () => {
    if (!buffer) return;
    out.push({ text: buffer, style });
    buffer = "";
  };

  while (index < input.length) {
    const rest = input.slice(index);

    // A backslash escapes the character after it, so a reply can write a
    // literal marker.
    if (rest[0] === "\\" && rest.length > 1) {
      buffer += rest[1];
      index += 2;
      continue;
    }

    const colorMarker = /^\{color:([^}]+)\}/.exec(rest);
    if (colorMarker) {
      const color = resolveTextColor(colorMarker[1] ?? "");
      const contentStart = index + colorMarker[0].length;
      const close = color ? input.indexOf("{/color}", contentStart) : -1;
      if (color && close !== -1) {
        flush();
        parseInline(
          input.slice(contentStart, close),
          { ...style, color },
          out,
          depth + 1,
        );
        index = close + "{/color}".length;
      } else {
        // An unknown name or a missing close is what the source wrote, not a
        // style instruction.
        buffer += colorMarker[0];
        index += colorMarker[0].length;
      }
      continue;
    }

    if (rest[0] === "`") {
      const close = input.indexOf("`", index + 1);
      if (close > index + 1) {
        flush();
        out.push({
          text: input.slice(index + 1, close),
          style: { ...style, code: true },
        });
        index = close + 1;
        continue;
      }
    }

    const link = /^\[([^\]]*)\]\(([^)\s]+)\)/.exec(rest);
    if (link) {
      flush();
      parseInline(link[1] ?? "", { ...style, link: link[2] ?? "" }, out, depth + 1);
      index += link[0].length;
      continue;
    }

    let emphasized = false;
    for (const marker of EMPHASIS) {
      if (!rest.startsWith(marker.token)) continue;
      // An underscore inside a word is part of the word, as in `snake_case`.
      if (
        marker.token.startsWith("_") &&
        index > 0 &&
        /[\p{L}\p{N}]/u.test(input.charAt(index - 1))
      ) {
        continue;
      }
      const close = input.indexOf(marker.token, index + marker.token.length);
      if (close <= index + marker.token.length) {
        buffer += marker.token;
        index += marker.token.length;
        emphasized = true;
        break;
      }
      flush();
      parseInline(
        input.slice(index + marker.token.length, close),
        {
          ...style,
          bold: marker.bold || style.bold,
          italic: marker.italic || style.italic,
        },
        out,
        depth + 1,
      );
      index = close + marker.token.length;
      emphasized = true;
      break;
    }
    if (emphasized) continue;

    buffer += rest.charAt(0);
    index += 1;
  }

  flush();
}

type RichTextProps = {
  text: string;
  style?: StyleProp<TextStyle>;
};

/** An assistant reply with its inline emphasis, code, links and colours. */
export function RichText({ text, style }: RichTextProps) {
  const runs = useMemo(() => parseRichText(text), [text]);

  return (
    <Text style={style}>
      {runs.map((run, index) => {
        const link = run.style.link;
        return (
          <Text
            accessibilityRole={link ? "link" : undefined}
            key={`${index}-${run.text.length}`}
            onPress={
              link
                ? () => {
                    void Linking.openURL(link);
                  }
                : undefined
            }
            style={[
              run.style.bold ? styles.bold : null,
              run.style.italic ? styles.italic : null,
              run.style.code ? styles.code : null,
              link ? styles.link : null,
              run.style.color ? { color: run.style.color } : null,
            ]}
          >
            {run.text}
          </Text>
        );
      })}
    </Text>
  );
}

const styles = StyleSheet.create({
  bold: {
    fontWeight: "700",
  },
  italic: {
    fontStyle: "italic",
  },
  code: {
    color: palette.command,
    fontFamily: mono,
  },
  link: {
    color: palette.accent,
    textDecorationLine: "underline",
  },
});
