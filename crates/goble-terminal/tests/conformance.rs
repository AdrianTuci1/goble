//! Phase 6 conformance harness: a fixture corpus that pins the emulator's grid.
//!
//! The corpus lives in `tests/conformance/<fixture-name>/`, one directory per
//! fixture, and is enumerated from disk at run time. There is no macro list and
//! no hard-coded fixture name: a directory that is not well-formed fails the
//! run, so a fixture cannot be silently orphaned the way upstream's
//! `row_reset` is. An optional extra corpus kept outside the repository is read
//! from `GOBLE_TERMINAL_CONFORMANCE_DIR` (a `;`- or `:`-separated list of
//! directories with the same layout).
//!
//! # Fixture files
//!
//! Each fixture directory holds exactly four files:
//!
//! | file | contents |
//! |---|---|
//! | `recording.bin` | the raw PTY byte stream |
//! | `size.json` | `{"columns": N, "screen_lines": N}`; other keys (`cell_width_px`, …) are ignored |
//! | `config.json` | `{"history_size": N}`; other keys are ignored |
//! | `grid.json` | the expected result, below |
//!
//! # `grid.json`
//!
//! ```json
//! {
//!   "rows": ["first row", "", "    indented"],
//!   "cursor": { "line": 0, "column": 5, "visible": true, "shape": "block", "wrap_pending": false },
//!   "cells": [
//!     { "row": 0, "column": 0, "fg": "red", "bold": true },
//!     { "row": 0, "column": 1, "bg": "#102030", "italic": true, "underline": "single" }
//!   ],
//!   "history_size": 7,
//!   "alt_screen": false,
//!   "content_rows": ["oldest", "…", "newest"]
//! }
//! ```
//!
//! Unknown keys are rejected by every nested object, so a typo cannot silently
//! pass.
//!
//! * `rows` is the complete list of visible rows. Its length must equal
//!   `size.screen_lines` and every row's `ScreenLine::text()` must match
//!   exactly (trailing blanks trimmed by `text()`). The vendored emulator's
//!   `put_tab` writes a literal `\t` into the cell a tab starts from, so a tab
//!   fixture's row string carries `\t` at the origin and blanks up to the stop.
//! * `cursor` is asserted in full — line, column, visibility, shape and the
//!   deferred-wrap flag (`wrap_pending` is `Screen::wrap_pending()`).
//! * `cells` is an optional sparse list of exact cell assertions. A listed cell
//!   asserts the value of every field it names and the **default** of every
//!   field it does not (`false` / `Underline::None` / `fg = default` /
//!   `bg = default` / no underline colour / no zero-width marks / no
//!   hyperlink). Its `ch` is pinned by the `rows` string. Supported keys:
//!   `row`, `column`, `fg`, `bg`, `bold`, `dim`, `italic`, `inverse`, `hidden`,
//!   `strikeout`, `wide`, `wide_spacer`, `leading_wide_spacer`, `wrapline`,
//!   `underline`, `underline_color`, `zerowidth`, `hyperlink`.
//! * `history_size` is optional: when present, `Screen::history_size()` must
//!   equal it. It is how a fixture proves the alt screen keeps no scrollback.
//! * `alt_screen` is optional: when present, `Screen::is_alt_screen()` must
//!   equal it.
//! * `content_rows` is optional: when present, the text of every row returned
//!   by `Screen::content_lines()` (scrollback history followed by the visible
//!   rows) must equal it, in order.
//!
//! # Colours
//!
//! `"default"` is `Named(Foreground)` for `fg` and `Named(Background)` for
//! `bg`; `"cursor"` is `Named(Cursor)`; a lowercase ANSI name (`black`,
//! `bright_red`, `dim_black`, …) is the matching `NamedColor`; `"index:208"` is
//! `Indexed(208)`; `"#112233"` is `Rgb(0x11, 0x22, 0x33)`. An omitted `fg` or
//! `bg` asserts the default `Foreground`/`Background` colour.
//!
//! # Why the cursor, the shape and `wrap_pending` are asserted
//!
//! The reference corpus this format is adapted from compares cells only, and
//! leaves the cursor out entirely (`Cursor` is `#[serde(skip)]`). That makes the
//! whole "where is the cursor and is a wrap pending" class of bugs — the most
//! commonly botched part of terminal semantics — invisible to the suite, so a
//! passing fixture would not have noticed a right-hand border drawn one column
//! off. This runner asserts them, which is the structural fix §5 of
//! `.agents/06-renderer/terminal-emulator.md` calls for.

use std::path::{Path, PathBuf};

use goble_terminal::{Screen, ScreenCell, ScreenColor, ScreenConfig, ScreenSize, Underline};
use serde::Deserialize;

/// The four files every fixture directory must hold, and nothing else.
const FIXTURE_FILES: [&str; 4] = ["recording.bin", "size.json", "config.json", "grid.json"];

/// The corpus checked into the repository.
fn builtin_corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance")
}

/// Extra corpora from `GOBLE_TERMINAL_CONFORMANCE_DIR`, `;`- or
/// `:`-separated. `std::env::split_paths` handles the platform separator, and
/// splitting on `;` first also accepts a semicolon list on Unix.
fn external_corpora() -> Vec<PathBuf> {
    let Ok(value) = std::env::var("GOBLE_TERMINAL_CONFORMANCE_DIR") else {
        return Vec::new();
    };
    let mut corpora = Vec::new();
    for piece in value.split(';') {
        for path in std::env::split_paths(piece) {
            if !path.as_os_str().is_empty() {
                corpora.push(path);
            }
        }
    }
    corpora
}

#[derive(Debug, Deserialize)]
struct SizeSpec {
    columns: usize,
    screen_lines: usize,
}

#[derive(Debug, Deserialize)]
struct ConfigSpec {
    history_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GridSpec {
    rows: Vec<String>,
    cursor: CursorSpec,
    #[serde(default)]
    cells: Vec<CellSpec>,
    history_size: Option<usize>,
    alt_screen: Option<bool>,
    content_rows: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorSpec {
    line: usize,
    column: usize,
    visible: bool,
    shape: String,
    wrap_pending: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CellSpec {
    row: usize,
    column: usize,
    fg: Option<String>,
    bg: Option<String>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    inverse: Option<bool>,
    hidden: Option<bool>,
    strikeout: Option<bool>,
    wide: Option<bool>,
    wide_spacer: Option<bool>,
    leading_wide_spacer: Option<bool>,
    wrapline: Option<bool>,
    underline: Option<String>,
    underline_color: Option<String>,
    zerowidth: Option<String>,
    hyperlink: Option<String>,
}

/// A colour in the form the fixture names it. `ScreenColor::Named` is reduced
/// to the lowercase debug name of the `NamedColor` variant, because
/// `NamedColor` is not nameable from an integration test (it is not re-exported
/// and `alacritty_terminal` is not a dev-dependency). Comparing debug names is
/// enough to pin a colour, and works for every variant including `Cursor` and
/// the `Dim*` set.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectedColor {
    Named(String),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy)]
enum ColorRole {
    Foreground,
    Background,
}

impl ColorRole {
    fn default_name(self) -> &'static str {
        match self {
            ColorRole::Foreground => "foreground",
            ColorRole::Background => "background",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ColorRole::Foreground => "fg",
            ColorRole::Background => "bg",
        }
    }
}

#[test]
fn conformance() {
    let mut failures = Vec::new();
    let builtin = builtin_corpus();
    run_corpus("", &builtin, &mut failures);

    for corpus in external_corpora() {
        let name = format!("[$GOBLE_TERMINAL_CONFORMANCE_DIR] {}", corpus.display());
        if !corpus.is_dir() {
            failures.push(format!("{name}: not a directory"));
            continue;
        }
        run_corpus(&name, &corpus, &mut failures);
    }

    assert!(
        failures.is_empty(),
        "{} conformance problem(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Enumerate a corpus directory and check every fixture in it. `name` prefixes
/// every message (empty for the built-in corpus) so an external fixture is
/// distinguishable from a checked-in one.
fn run_corpus(name: &str, corpus: &Path, failures: &mut Vec<String>) {
    let prefix = |message: String| {
        if name.is_empty() {
            message
        } else {
            format!("{name}: {message}")
        }
    };

    let entries = match std::fs::read_dir(corpus) {
        Ok(entries) => entries,
        Err(err) => {
            failures.push(prefix(format!(
                "cannot read corpus {}: {err}",
                corpus.display()
            )));
            return;
        }
    };

    let mut fixtures: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        match entry.path().is_dir() {
            true => fixtures.push(entry.path()),
            false => failures.push(prefix(format!(
                "unexpected file `{}` in the corpus root; fixtures are directories",
                entry.path().display()
            ))),
        }
    }
    fixtures.sort();

    if fixtures.is_empty() {
        failures.push(prefix(format!(
            "corpus {} is empty: a conformance run that checks nothing must not pass",
            corpus.display()
        )));
        return;
    }

    for fixture in fixtures {
        let fixture_name = fixture
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| fixture.display().to_string());
        let mut errors = check_fixture(&fixture);
        if errors.is_empty() {
            continue;
        }
        errors.insert(
            0,
            format!("fixture `{fixture_name}` at {}", fixture.display()),
        );
        for error in errors {
            failures.push(prefix(error));
        }
    }
}

/// Run one fixture, returning every mismatch it produced. An empty vector means
/// the fixture passed.
fn check_fixture(dir: &Path) -> Vec<String> {
    let mut errors = Vec::new();

    check_layout(dir, &mut errors);
    if !errors.is_empty() {
        return errors;
    }

    let size: SizeSpec = match read_json(&dir.join("size.json")) {
        Ok(size) => size,
        Err(err) => return vec![err],
    };
    let config: ConfigSpec = match read_json(&dir.join("config.json")) {
        Ok(config) => config,
        Err(err) => return vec![err],
    };
    let grid: GridSpec = match read_json(&dir.join("grid.json")) {
        Ok(grid) => grid,
        Err(err) => return vec![err],
    };
    let recording = match std::fs::read(dir.join("recording.bin")) {
        Ok(recording) => recording,
        Err(err) => return vec![format!("cannot read recording.bin: {err}")],
    };

    let mut screen = Screen::new(
        ScreenSize::new(size.columns, size.screen_lines),
        ScreenConfig::scrolling(config.history_size),
    );
    screen.feed(&recording);

    check_rows(&screen, &size, &grid, &mut errors);
    check_cursor(&screen, &grid, &mut errors);
    check_cells(&screen, &grid, &mut errors);
    check_optional_state(&screen, &grid, &mut errors);

    errors
}

/// A fixture holds exactly the four required files and nothing else. This is
/// what makes an unregistered or half-created fixture impossible to miss.
fn check_layout(dir: &Path, errors: &mut Vec<String>) {
    let mut present: Vec<String> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            errors.push(format!("cannot read fixture directory: {err}"));
            return;
        }
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            errors.push(format!(
                "unexpected directory `{file_name}`; a fixture holds exactly {FIXTURE_FILES:?}"
            ));
        } else if FIXTURE_FILES.contains(&file_name.as_str()) {
            present.push(file_name);
        } else {
            errors.push(format!(
                "unexpected file `{file_name}`; a fixture holds exactly {FIXTURE_FILES:?}"
            ));
        }
    }
    for required in FIXTURE_FILES {
        if !present.iter().any(|name| name == required) {
            errors.push(format!("missing `{required}`"));
        }
    }
}

fn check_rows(screen: &Screen, size: &SizeSpec, grid: &GridSpec, errors: &mut Vec<String>) {
    let lines = screen.lines();
    if lines.len() != size.screen_lines {
        errors.push(format!(
            "lines(): expected {} visible rows, got {}",
            size.screen_lines,
            lines.len()
        ));
    }
    if grid.rows.len() != size.screen_lines {
        errors.push(format!(
            "grid.json `rows` has {} entries but size.json says {} screen_lines",
            grid.rows.len(),
            size.screen_lines
        ));
    }
    for (index, expected) in grid.rows.iter().enumerate() {
        let Some(line) = lines.get(index) else {
            continue;
        };
        let actual = line.text();
        if actual != *expected {
            errors.push(format!(
                "row {index}: expected {expected:?}, got {actual:?}"
            ));
        }
    }
}

fn check_cursor(screen: &Screen, grid: &GridSpec, errors: &mut Vec<String>) {
    let cursor = screen.cursor();
    if cursor.line != grid.cursor.line {
        errors.push(format!(
            "cursor.line: expected {}, got {}",
            grid.cursor.line, cursor.line
        ));
    }
    if cursor.column != grid.cursor.column {
        errors.push(format!(
            "cursor.column: expected {}, got {}",
            grid.cursor.column, cursor.column
        ));
    }
    if cursor.visible != grid.cursor.visible {
        errors.push(format!(
            "cursor.visible: expected {}, got {}",
            grid.cursor.visible, cursor.visible
        ));
    }
    // `CursorShape` is not nameable here, so the shape is compared through its
    // debug name, which is stable and matches the lowercase names in grid.json
    // ("block", "underline", "beam", "hollowblock", "hidden").
    let actual_shape = format!("{:?}", cursor.shape).to_lowercase();
    let expected_shape = grid.cursor.shape.to_lowercase();
    if actual_shape != expected_shape {
        errors.push(format!(
            "cursor.shape: expected {expected_shape:?}, got {actual_shape:?}"
        ));
    }
    if screen.wrap_pending() != grid.cursor.wrap_pending {
        errors.push(format!(
            "cursor.wrap_pending: expected {}, got {}",
            grid.cursor.wrap_pending,
            screen.wrap_pending()
        ));
    }
}

fn check_cells(screen: &Screen, grid: &GridSpec, errors: &mut Vec<String>) {
    let lines = screen.lines();
    for spec in &grid.cells {
        let Some(line) = lines.get(spec.row) else {
            errors.push(format!(
                "cell (row={}, column={}): row is out of range",
                spec.row, spec.column
            ));
            continue;
        };
        let Some(cell) = line.cells.get(spec.column) else {
            errors.push(format!(
                "cell (row={}, column={}): column is out of range",
                spec.row, spec.column
            ));
            continue;
        };
        check_cell(spec, cell, errors);
    }
}

fn check_cell(spec: &CellSpec, cell: &ScreenCell, errors: &mut Vec<String>) {
    let at = format!("cell (row={}, column={})", spec.row, spec.column);
    let attrs = &cell.attrs;

    check_color(&at, &spec.fg, ColorRole::Foreground, cell.fg, errors);
    check_color(&at, &spec.bg, ColorRole::Background, cell.bg, errors);

    check_flag(&at, "bold", spec.bold, attrs.bold, errors);
    check_flag(&at, "dim", spec.dim, attrs.dim, errors);
    check_flag(&at, "italic", spec.italic, attrs.italic, errors);
    check_flag(&at, "inverse", spec.inverse, attrs.inverse, errors);
    check_flag(&at, "hidden", spec.hidden, attrs.hidden, errors);
    check_flag(&at, "strikeout", spec.strikeout, attrs.strikeout, errors);
    check_flag(&at, "wide", spec.wide, attrs.wide, errors);
    check_flag(
        &at,
        "wide_spacer",
        spec.wide_spacer,
        attrs.wide_spacer,
        errors,
    );
    check_flag(
        &at,
        "leading_wide_spacer",
        spec.leading_wide_spacer,
        attrs.leading_wide_spacer,
        errors,
    );
    check_flag(&at, "wrapline", spec.wrapline, attrs.wrapline, errors);

    let expected_underline = spec.underline.as_deref().unwrap_or("none").to_lowercase();
    let actual_underline = underline_name(attrs.underline);
    if actual_underline != expected_underline {
        errors.push(format!(
            "{at} underline: expected {expected_underline:?}, got {actual_underline:?}"
        ));
    }

    match &spec.underline_color {
        None => {
            if attrs.underline_color.is_some() {
                errors.push(format!(
                    "{at} underline_color: expected none, got {:?}",
                    attrs.underline_color
                ));
            }
        }
        Some(raw) => match parse_color(raw, ColorRole::Foreground) {
            Ok(expected) => {
                let actual = attrs.underline_color.map(actual_color);
                if actual.as_ref() != Some(&expected) {
                    errors.push(format!(
                        "{at} underline_color: expected {expected:?}, got {actual:?}"
                    ));
                }
            }
            Err(err) => errors.push(format!("{at} underline_color: {err}")),
        },
    }

    match &spec.zerowidth {
        None => {
            if !cell.zerowidth.is_empty() {
                errors.push(format!(
                    "{at} zerowidth: expected none, got {:?}",
                    cell.zerowidth
                ));
            }
        }
        Some(expected) => {
            let expected: Vec<char> = expected.chars().collect();
            if cell.zerowidth != expected {
                errors.push(format!(
                    "{at} zerowidth: expected {expected:?}, got {:?}",
                    cell.zerowidth
                ));
            }
        }
    }

    match &spec.hyperlink {
        None => {
            if cell.hyperlink.is_some() {
                errors.push(format!(
                    "{at} hyperlink: expected none, got {:?}",
                    cell.hyperlink
                ));
            }
        }
        Some(expected) => {
            if cell.hyperlink.as_deref() != Some(expected.as_str()) {
                errors.push(format!(
                    "{at} hyperlink: expected {expected:?}, got {:?}",
                    cell.hyperlink
                ));
            }
        }
    }
}

fn check_flag(
    at: &str,
    name: &str,
    expected: Option<bool>,
    actual: bool,
    errors: &mut Vec<String>,
) {
    let expected = expected.unwrap_or(false);
    if actual != expected {
        errors.push(format!("{at} {name}: expected {expected}, got {actual}"));
    }
}

fn check_color(
    at: &str,
    expected: &Option<String>,
    role: ColorRole,
    actual: ScreenColor,
    errors: &mut Vec<String>,
) {
    let expected = match expected {
        None => ExpectedColor::Named(role.default_name().to_string()),
        Some(raw) => match parse_color(raw, role) {
            Ok(color) => color,
            Err(err) => {
                errors.push(format!("{at} {}: {err}", role.label()));
                return;
            }
        },
    };
    let actual = actual_color(actual);
    if actual != expected {
        errors.push(format!(
            "{at} {}: expected {expected:?}, got {actual:?}",
            role.label()
        ));
    }
}

fn check_optional_state(screen: &Screen, grid: &GridSpec, errors: &mut Vec<String>) {
    if let Some(expected) = grid.history_size {
        let actual = screen.history_size();
        if actual != expected {
            errors.push(format!("history_size: expected {expected}, got {actual}"));
        }
    }
    if let Some(expected) = grid.alt_screen {
        let actual = screen.is_alt_screen();
        if actual != expected {
            errors.push(format!("alt_screen: expected {expected}, got {actual}"));
        }
    }
    if let Some(expected) = &grid.content_rows {
        let actual: Vec<String> = screen
            .content_lines()
            .iter()
            .map(|line| line.text())
            .collect();
        if actual.len() != expected.len() {
            errors.push(format!(
                "content_rows: expected {} rows, got {}",
                expected.len(),
                actual.len()
            ));
        }
        for (index, (expected, actual)) in expected.iter().zip(actual.iter()).enumerate() {
            if expected != actual {
                errors.push(format!(
                    "content_rows[{index}]: expected {expected:?}, got {actual:?}"
                ));
            }
        }
    }
}

fn parse_color(raw: &str, role: ColorRole) -> Result<ExpectedColor, String> {
    if raw == "default" {
        return Ok(ExpectedColor::Named(role.default_name().to_string()));
    }
    if let Some(index) = raw.strip_prefix("index:") {
        let index = index
            .parse::<u8>()
            .map_err(|_| format!("`{raw}` is not a valid `index:N` colour"))?;
        return Ok(ExpectedColor::Indexed(index));
    }
    if let Some(hex) = raw.strip_prefix('#') {
        let (r, g, b) =
            parse_hex(hex).ok_or_else(|| format!("`{raw}` is not a `#RRGGBB` colour"))?;
        return Ok(ExpectedColor::Rgb(r, g, b));
    }
    // A lowercase ANSI name; compared against the variant's debug name, which
    // is camel case with no separator (`BrightRed`, `DimBlack`, …), so the
    // fixture's `bright_red` / `dim_black` spellings are normalised to match.
    Ok(ExpectedColor::Named(raw.to_lowercase().replace('_', "")))
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

fn actual_color(color: ScreenColor) -> ExpectedColor {
    match color {
        ScreenColor::Named(named) => ExpectedColor::Named(format!("{named:?}").to_lowercase()),
        ScreenColor::Indexed(index) => ExpectedColor::Indexed(index),
        ScreenColor::Rgb(r, g, b) => ExpectedColor::Rgb(r, g, b),
    }
}

fn underline_name(underline: Underline) -> &'static str {
    match underline {
        Underline::None => "none",
        Underline::Single => "single",
        Underline::Double => "double",
        Underline::Curly => "curly",
        Underline::Dotted => "dotted",
        Underline::Dashed => "dashed",
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}
