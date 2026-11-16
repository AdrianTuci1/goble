#!/usr/bin/env node
/**
 * Fail the web export when its JavaScript outgrows the size budget.
 *
 * CI already builds `apps/web/dist`; this reads that export back and measures
 * what a browser downloads. Growth is the regression a typechecker cannot see:
 * one more screen, or one dependency pulled into the entry chunk, costs every
 * visitor. The budgets below are the 2026-11-16 export plus roughly 10% of
 * headroom, not aspirations:
 *
 *   JS raw     1717 KB   budget 1900 KB
 *   JS gzip     471 KB   budget  520 KB
 *   largest     1130 KB   budget 1250 KB
 *
 * A change that genuinely needs more raises the budget in the same commit, so
 * a reviewer sees the number move. A change that does not need it fails here.
 *
 *   node scripts/check-web-size.mjs [--dir <path>] [--raw-kb N] [--gzip-kb N] [--file-kb N]
 */

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

const DEFAULT_DIR = "apps/web/dist";
const RAW_BUDGET_KB = 1900;
const GZIP_BUDGET_KB = 520;
const FILE_BUDGET_KB = 1250;

function option(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  if (index === -1) return fallback;
  const value = process.argv[index + 1];
  if (value === undefined) {
    console.error(`--${name} needs a value`);
    process.exit(2);
  }
  return value;
}

function number(name, fallback) {
  const value = option(name, undefined);
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    console.error(`--${name} must be a positive number, got ${value}`);
    process.exit(2);
  }
  return parsed;
}

const directory = option("dir", DEFAULT_DIR);
const rawBudget = number("raw-kb", RAW_BUDGET_KB);
const gzipBudget = number("gzip-kb", GZIP_BUDGET_KB);
const fileBudget = number("file-kb", FILE_BUDGET_KB);

/** Every file below `root`, as paths relative to it. */
function walk(root, prefix = "") {
  const files = [];
  for (const entry of readdirSync(join(root, prefix), { withFileTypes: true })) {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...walk(root, path));
    else if (entry.isFile()) files.push(path);
  }
  return files;
}

let scripts;
try {
  scripts = walk(directory).filter((file) => file.endsWith(".js"));
} catch {
  console.error(`cannot read ${directory}: run \`npm run web:build\` first`);
  process.exit(2);
}
if (scripts.length === 0) {
  console.error(`${directory} has no JavaScript: the export is empty`);
  process.exit(1);
}

let raw = 0;
let gzip = 0;
let largest = { file: "", bytes: 0 };
for (const file of scripts) {
  const bytes = readFileSync(join(directory, file));
  raw += bytes.length;
  gzip += gzipSync(bytes, { level: 9 }).length;
  if (bytes.length > largest.bytes) {
    largest = { file, bytes: bytes.length };
  }
}

const kb = (bytes) => (bytes / 1024).toFixed(1);
const ok = (value, budget) => value <= budget * 1024;

const rows = [
  {
    label: `JS raw (${scripts.length} files)`,
    value: raw,
    budget: rawBudget,
    ok: ok(raw, rawBudget),
  },
  {
    label: "JS gzip",
    value: gzip,
    budget: gzipBudget,
    ok: ok(gzip, gzipBudget),
  },
  {
    label: `largest (${largest.file})`,
    value: largest.bytes,
    budget: fileBudget,
    ok: ok(largest.bytes, fileBudget),
  },
];

console.log(`web export: ${directory}`);
for (const row of rows) {
  const status = row.ok ? "ok" : "OVER";
  console.log(
    `  ${row.label.padEnd(32)} ${kb(row.value).padStart(8)} KB` +
      `  budget ${String(row.budget).padStart(5)} KB  ${status}`,
  );
}

const over = rows.filter((row) => !row.ok);
if (over.length > 0) {
  console.error("");
  console.error("the web export is over budget:");
  for (const row of over) {
    console.error(
      `  ${row.label}: ${kb(row.value)} KB against ${row.budget} KB` +
        ` (${kb(row.value - row.budget * 1024)} KB over)`,
    );
  }
  console.error(
    "Shrink the bundle, or raise the budget in this commit if the growth is intended.",
  );
  process.exit(1);
}
