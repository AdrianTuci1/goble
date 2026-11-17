#!/usr/bin/env node
/**
 * Generate `apps/web/components/sidebar-icons.tsx` from the SVG assets in
 * `apps/web/assets/icons/`.
 *
 * The web shell is web-only (`app.json` platforms: ["web"]) and runs on
 * react-native-web, so the shipped 24x24 line icons are rendered as real DOM
 * `<svg>` elements with the same `currentColor` contract the Rust sidebar uses.
 * This script copies the geometry out of the assets so the two apps cannot
 * drift: re-run it after touching a file in `assets/icons/`.
 *
 *     node apps/web/scripts/gen-sidebar-icons.mjs
 */
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const iconsDir = join(root, "assets", "icons");
const outFile = join(root, "components", "sidebar-icons.tsx");

/** JSX wants camelCase SVG attributes; the assets use the DOM spelling. */
const CAMEL = {
  "clip-rule": "clipRule",
  "fill-opacity": "fillOpacity",
  "fill-rule": "fillRule",
  "stroke-dasharray": "strokeDasharray",
  "stroke-dashoffset": "strokeDashoffset",
  "stroke-linecap": "strokeLinecap",
  "stroke-linejoin": "strokeLinejoin",
  "stroke-miterlimit": "strokeMiterlimit",
  "stroke-opacity": "strokeOpacity",
  "stroke-width": "strokeWidth",
};

function camelAttributes(markup) {
  return markup.replace(/\b([a-z]+(?:-[a-z]+)+)=/g, (match, name) => {
    const camel = CAMEL[name];
    return camel === undefined ? match : `${camel}=`;
  });
}

/** The inner geometry of one 24x24 asset, ready to drop into JSX. */
function geometry(file) {
  const svg = readFileSync(join(iconsDir, file), "utf8").trim();
  const openEnd = svg.indexOf(">");
  const closeStart = svg.lastIndexOf("</svg>");
  if (openEnd < 0 || closeStart < 0) {
    throw new Error(`${file}: not a well-formed <svg> document`);
  }
  const body = camelAttributes(svg.slice(openEnd + 1, closeStart).trim());
  // Keep the generated JSX readable: indent each geometry line one level.
  return body
    .split("\n")
    .map((line) => `    ${line.trim()}`)
    .join("\n");
}

const names = readdirSync(iconsDir)
  .filter((file) => file.endsWith(".svg"))
  .map((file) => file.slice(0, -4))
  .sort();

const entries = names.map((name) => `  ${JSON.stringify(name)}: (\n${geometry(`${name}.svg`)}\n  ),`);

const module = `/* eslint-disable */
/**
 * DSH sidebar icons, generated from \`apps/web/assets/icons/*.svg\`.
 *
 * Do not edit by hand: run \`node apps/web/scripts/gen-sidebar-icons.mjs\`.
 * The geometry is the same 24x24 artwork the Rust sidebar registers in
 * \`crates/goble-ui/assets/icons/\` (16px glyph scaled 1.5x, 1px stroke), so both
 * apps paint identical glyphs. Fill-only glyphs (triangle, ellipsis, pin) keep
 * their \`fill\`; the rest stroke \`currentColor\`.
 */
import type { ReactNode } from "react";

import { SvgIcon } from "./svg-icon";

export type SidebarIconName =
${names.map((name) => `  | ${JSON.stringify(name)}`).join("\n")};

const ART: Record<SidebarIconName, ReactNode> = {
${entries.join("\n")}
};

export function SidebarIcon({
  color,
  name,
  size = 16,
}: {
  color: string;
  name: SidebarIconName;
  size?: number;
}) {
  return (
    <SvgIcon color={color} size={size}>
      {ART[name]}
    </SvgIcon>
  );
}
`;

writeFileSync(outFile, module);
console.log(`wrote ${outFile} (${names.length} icons)`);
