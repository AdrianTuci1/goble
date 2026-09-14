/// The three line kinds a unified diff can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One body line of a hunk, with the line number each side of the diff shows.
///
/// A removed line has only an old number, an added line only a new one, and a
/// context line both — so which gutter column is populated *is* the change type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

/// One `@@ -old,count +new,count @@ section` block of a unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    /// The function/context label the header carries after the closing `@@`.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// The hunk's `@@ … @@` header, exactly as it appears in a unified diff.
    pub fn header(&self) -> String {
        let mut text = format!(
            "@@ -{} +{} @@",
            format_range(self.old_start, self.old_count),
            format_range(self.new_start, self.new_count)
        );
        if !self.section.is_empty() {
            text.push(' ');
            text.push_str(&self.section);
        }
        text
    }

    pub fn added(&self) -> usize {
        self.count(DiffLineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(DiffLineKind::Removed)
    }

    pub fn context(&self) -> usize {
        self.count(DiffLineKind::Context)
    }

    fn count(&self, kind: DiffLineKind) -> usize {
        self.lines.iter().filter(|line| line.kind == kind).count()
    }
}

/// Added/removed totals across a whole diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

/// A row the diff element draws. Hunks contribute a header row and their body
/// lines; the context skipped between two hunks contributes a gap separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    HunkHeader {
        old_start: u32,
        old_count: u32,
        new_start: u32,
        new_count: u32,
        section: String,
    },
    /// A separator standing in for the unchanged lines neither hunk shows.
    Gap {
        skipped: usize,
    },
    Line(DiffLine),
}

/// Parse a unified diff into hunks.
///
/// `---`/`+++` file headers, `diff --git`/`index` lines and
/// `\ No newline at end of file` markers are skipped; a hunk ends once it has
/// consumed the old and new line counts its header declares.
pub fn parse_unified_diff(input: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    let mut consumed_old = 0u32;
    let mut consumed_new = 0u32;

    for raw in input.lines() {
        if raw.starts_with("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if let Some(hunk) = parse_hunk_header(raw) {
                consumed_old = 0;
                consumed_new = 0;
                current = Some(hunk);
            }
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        // The header declares how many lines the hunk covers; once both sides
        // are satisfied, a further line starts a new region.
        if consumed_old >= hunk.old_count && consumed_new >= hunk.new_count {
            let finished = current.take().expect("current is Some");
            hunks.push(finished);
            continue;
        }

        let (kind, text) = match raw.as_bytes().first() {
            Some(b' ') => (DiffLineKind::Context, &raw[1..]),
            Some(b'+') => (DiffLineKind::Added, &raw[1..]),
            Some(b'-') => (DiffLineKind::Removed, &raw[1..]),
            Some(b'\\') => continue,
            _ => continue,
        };

        let old_line = (kind != DiffLineKind::Added).then(|| hunk.old_start + consumed_old);
        let new_line = (kind != DiffLineKind::Removed).then(|| hunk.new_start + consumed_new);
        if kind != DiffLineKind::Added {
            consumed_old += 1;
        }
        if kind != DiffLineKind::Removed {
            consumed_new += 1;
        }
        hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text: text.to_string(),
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@")?;
    let end = rest.find("@@")?;
    let ranges = &rest[..end];
    let section = rest[end + 2..].trim().to_string();
    let mut tokens = ranges.split_whitespace();
    let (old_start, old_count) = parse_range(tokens.next()?)?;
    let (new_start, new_count) = parse_range(tokens.next()?)?;
    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        section,
        lines: Vec::new(),
    })
}

fn parse_range(token: &str) -> Option<(u32, u32)> {
    let token = token.trim_start_matches(['-', '+']);
    let mut parts = token.split(',');
    let start = parts.next()?.trim().parse().ok()?;
    let count = match parts.next() {
        Some(count) => count.trim().parse().ok()?,
        None => 1,
    };
    Some((start, count))
}

/// `count` of 1 is elided, matching git's `-1 +1` form.
fn format_range(start: u32, count: u32) -> String {
    if count == 1 {
        format!("{start}")
    } else {
        format!("{start},{count}")
    }
}

pub(super) fn build_rows(hunks: &[Hunk]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for (index, hunk) in hunks.iter().enumerate() {
        if index > 0 {
            rows.push(DiffRow::Gap {
                skipped: skipped_between(&hunks[index - 1], hunk),
            });
        }
        rows.push(DiffRow::HunkHeader {
            old_start: hunk.old_start,
            old_count: hunk.old_count,
            new_start: hunk.new_start,
            new_count: hunk.new_count,
            section: hunk.section.clone(),
        });
        rows.extend(hunk.lines.iter().cloned().map(DiffRow::Line));
    }
    rows
}

/// Unchanged lines between the end of `prev` and the start of `next`.
fn skipped_between(prev: &Hunk, next: &Hunk) -> usize {
    let prev_end = prev.old_start.saturating_add(prev.old_count);
    next.old_start.saturating_sub(prev_end) as usize
}

pub(super) fn header_text(row: &DiffRow) -> String {
    match row {
        DiffRow::HunkHeader {
            old_start,
            old_count,
            new_start,
            new_count,
            section,
        } => {
            let mut text = format!(
                "@@ -{} +{} @@",
                format_range(*old_start, *old_count),
                format_range(*new_start, *new_count)
            );
            if !section.is_empty() {
                text.push(' ');
                text.push_str(section);
            }
            text
        }
        _ => String::new(),
    }
}

pub(super) fn gap_text(skipped: usize) -> String {
    if skipped == 1 {
        "... 1 unchanged line skipped".to_string()
    } else {
        format!("... {skipped} unchanged lines skipped")
    }
}
