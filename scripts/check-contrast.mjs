#!/usr/bin/env node
/**
 * Gate the shared TS dark palette against WCAG 2.1 AA contrast.
 *
 * `apps/common/src/lib/theme.ts` is consumed by name across the web and mobile
 * clients, so a token can silently fall below the readable threshold while
 * still compiling. This script reads the palette, computes relative luminance
 * and contrast ratios, and fails when a text token is under 4.5:1 on any
 * background it can sit on, or when the interactive border is under 3:1.
 *
 * It is intentionally dependency-free and runs in CI's `js-typecheck` job.
 *
 *   node scripts/check-contrast.mjs
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const THEME_PATH = fileURLToPath(
  new URL("../apps/common/src/lib/theme.ts", import.meta.url),
);

// Backgrounds a text token can be rendered on.
const BACKGROUNDS = ["bg", "surface", "raised", "overlay"];

// Text tokens that must reach AA normal-text contrast (4.5:1).
const TEXT_TOKENS = [
  "text",
  "muted",
  "subtle",
  "accent",
  "success",
  "warning",
  "error",
];
const TEXT_MIN = 4.5;

// A border that is a control's only affordance is a UI component, so WCAG
// 1.4.11 requires 3:1. `border` is decorative and deliberately not checked.
const BORDER_TOKENS = ["borderInteractive"];
const BORDER_BACKGROUNDS = ["bg", "surface", "raised"];
const BORDER_MIN = 3.0;

// The tinted "soft" backgrounds also carry text, so check the pair that is
// actually rendered rather than assuming the neutral backgrounds cover them.
const SOFT_PAIRS = [
  ["error", "errorSoft"],
  ["success", "successSoft"],
  ["warning", "warningSoft"],
  ["accent", "accentSoft"],
  ["text", "accentSoft"],
];

// `palette.error` is also a fill behind the destructive button's label. That
// label is `palette.bg` (dark text on the light red), so measure the pair the
// component actually draws.
const FILL_PAIRS = [["bg", "error"]];

function fail(message) {
  console.error(`check-contrast: ${message}`);
  process.exit(1);
}

function readPalette(path) {
  let source;
  try {
    source = readFileSync(path, "utf8");
  } catch (error) {
    fail(`cannot read ${path}: ${error.message}`);
  }

  const body = source.match(
    /export\s+const\s+palette\s*=\s*\{([\s\S]*?)\}\s*as\s+const\s*;/,
  );
  if (!body) fail(`could not find the \`palette\` object in ${path}`);

  const palette = {};
  const entry = /([A-Za-z_$][\w$]*)\s*:\s*"#([0-9a-fA-F]{6})"/g;
  for (const match of body[1].matchAll(entry)) {
    const [, name, hex] = match;
    if (Object.hasOwn(palette, name)) fail(`duplicate palette token ${name}`);
    palette[name] = `#${hex.toLowerCase()}`;
  }
  return palette;
}

function parseHex(hex) {
  return [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16));
}

/** WCAG 2.1 relative luminance of an opaque #rrggbb color. */
function relativeLuminance(hex) {
  const [r, g, b] = parseHex(hex).map((value) => {
    const channel = value / 255;
    return channel <= 0.03928
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 2.1 contrast ratio between two opaque #rrggbb colors. */
function contrastRatio(a, b) {
  const first = relativeLuminance(a);
  const second = relativeLuminance(b);
  const lighter = Math.max(first, second);
  const darker = Math.min(first, second);
  return (lighter + 0.05) / (darker + 0.05);
}

const palette = readPalette(THEME_PATH);

// Required backgrounds. A missing token is a failure, never a silent skip.
const backgrounds = BACKGROUNDS.map((name) => {
  if (!palette[name]) fail(`missing expected palette token: ${name}`);
  return name;
});

// Required foregrounds, checked against every background.
const textTokens = TEXT_TOKENS.map((name) => {
  if (!palette[name]) fail(`missing expected palette token: ${name}`);
  return name;
});

const borderTokens = BORDER_TOKENS.map((name) => {
  if (!palette[name]) fail(`missing expected palette token: ${name}`);
  return name;
});

function requireToken(name) {
  if (!palette[name]) fail(`missing expected palette token: ${name}`);
  return name;
}

for (const pair of [...SOFT_PAIRS, ...FILL_PAIRS]) {
  for (const name of pair) requireToken(name);
}

const checks = [];
for (const token of textTokens) {
  for (const background of backgrounds) {
    checks.push({
      token,
      background,
      ratio: contrastRatio(palette[token], palette[background]),
      min: TEXT_MIN,
    });
  }
}
for (const token of borderTokens) {
  for (const background of BORDER_BACKGROUNDS) {
    checks.push({
      token,
      background,
      ratio: contrastRatio(palette[token], palette[background]),
      min: BORDER_MIN,
    });
  }
}
for (const [foreground, background] of SOFT_PAIRS) {
  checks.push({
    token: foreground,
    background,
    ratio: contrastRatio(palette[foreground], palette[background]),
    min: TEXT_MIN,
  });
}
for (const [foreground, background] of FILL_PAIRS) {
  checks.push({
    token: foreground,
    background,
    ratio: contrastRatio(palette[foreground], palette[background]),
    min: TEXT_MIN,
  });
}

const rows = checks.map((check) => ({
  ...check,
  ok: check.ratio >= check.min,
}));

const tokenWidth = Math.max("token".length, ...rows.map((row) => row.token.length));
const backgroundWidth = Math.max(
  "background".length,
  ...rows.map((row) => row.background.length),
);
const line = (row) =>
  `${row.token.padEnd(tokenWidth)}  ${row.background.padEnd(backgroundWidth)}  ` +
  `${row.ratio.toFixed(2).padStart(6)}:1  ${row.min.toFixed(1)}:1  ` +
  `${row.ok ? "ok" : "FAIL"}`;

console.log(
  `${"token".padEnd(tokenWidth)}  ${"background".padEnd(backgroundWidth)}  ` +
    `${"ratio".padStart(6)}    ${"min".padStart(4)}  result`,
);
for (const row of rows) console.log(line(row));

const failures = rows.filter((row) => !row.ok);
if (failures.length > 0) {
  for (const row of failures) {
    console.error(
      `FAIL ${row.token} on ${row.background}: ${row.ratio.toFixed(2)}:1 ` +
        `< ${row.min.toFixed(1)}:1`,
    );
  }
  fail(`${failures.length} of ${rows.length} contrast checks failed`);
}

console.log(`\nAll ${rows.length} contrast checks pass.`);
