import type { ReactNode } from "react";
import { View } from "react-native";

/**
 * A line icon drawn as a real DOM `<svg>`.
 *
 * `apps/web` is web-only (`app.json` platforms: ["web"]) and runs on
 * react-native-web, so the shipped DeepSeek-Harness SVG artwork can be drawn
 * directly instead of pulling in an SVG renderer: `color` sets the CSS
 * `color`, which the glyphs' `stroke="currentColor"`/`fill="currentColor"`
 * inherit, exactly like the Rust `Icon::with_color` contract. Rendering into
 * `apps/common` would break the native mobile app, so this stays in `apps/web`.
 */
export function SvgIcon({
  children,
  color,
  size = 16,
}: {
  children: ReactNode;
  color: string;
  size?: number;
}) {
  return (
    <View style={{ height: size, width: size }}>
      <svg
        aria-hidden="true"
        fill="none"
        height={size}
        stroke="currentColor"
        style={{ color, display: "block" }}
        viewBox="0 0 24 24"
        width={size}
      >
        {children}
      </svg>
    </View>
  );
}
