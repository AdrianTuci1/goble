//! The unified diff a file mutation records on its tool call.
//!
//! The transcript draws a file change as a diff, but the arguments alone only
//! carry a replacement's old and new text ([`super::edit_hunks`]'s UI
//! counterpart): a write would draw nothing, and a replacement would have no
//! line numbers or context. The tool that changes the file is the only place
//! that sees both versions, so it records the diff there
//! ([`crate::harness::ChatToolCall::diff`]) and the row parses it back.
//!
//! The output is hunks only: no `---`/`+++` file header, because the row that
//! carries it already names the path. It is the same shape
//! `crates/goble-ui`'s `parse_unified_diff` reads.

/// The unchanged lines a hunk carries on each side of a change, git's default.
pub const DIFF_CONTEXT: usize = 3;

/// Beyond this many cells the line table would cost more than the diff is
/// worth: a whole-file replacement is emitted instead, which is still a correct
/// diff for a text that was completely rewritten.
const MAX_TABLE_CELLS: usize = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Remove,
    Add,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Context,
    Removed,
    Added,
}

struct Row<'a> {
    kind: RowKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: &'a str,
}

/// `old` and `new` as a unified diff with [`DIFF_CONTEXT`] lines of context.
/// Empty when the texts are equal.
pub fn unified_diff(old: &str, new: &str) -> String {
    unified_diff_with_context(old, new, DIFF_CONTEXT)
}

/// The same, with an explicit context width.
pub fn unified_diff_with_context(old: &str, new: &str, context: usize) -> String {
    if old == new {
        return String::new();
    }
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = diff_ops(&old_lines, &new_lines);
    let rows = rows(&old_lines, &new_lines, &ops);
    render(&rows, context)
}

/// The line edits between two sequences: unchanged lines outside the changed
/// middle, then the middle's own line table.
fn diff_ops(old: &[&str], new: &[&str]) -> Vec<Op> {
    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut ops = Vec::with_capacity(prefix + (old.len() - prefix) + (new.len() - prefix) + suffix);
    ops.extend(std::iter::repeat_n(Op::Equal, prefix));
    ops.extend(middle_ops(
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
    ));
    ops.extend(std::iter::repeat_n(Op::Equal, suffix));
    ops
}

/// The middle of two sequences: the longest common subsequence as a walk, or a
/// whole-block replacement when the table would be too large.
fn middle_ops(old: &[&str], new: &[&str]) -> Vec<Op> {
    if old.is_empty() {
        return vec![Op::Add; new.len()];
    }
    if new.is_empty() {
        return vec![Op::Remove; old.len()];
    }
    if old.len().saturating_mul(new.len()) > MAX_TABLE_CELLS {
        let mut ops = vec![Op::Remove; old.len()];
        ops.extend(std::iter::repeat_n(Op::Add, new.len()));
        return ops;
    }
    let width = new.len() + 1;
    let mut table = vec![0u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i * width + j] = if old[i] == new[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(old.len() + new.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            ops.push(Op::Equal);
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            ops.push(Op::Remove);
            i += 1;
        } else {
            ops.push(Op::Add);
            j += 1;
        }
    }
    ops.extend(std::iter::repeat_n(Op::Remove, old.len() - i));
    ops.extend(std::iter::repeat_n(Op::Add, new.len() - j));
    ops
}

/// The ops as numbered rows: each line knows the number it has on each side it
/// survives on.
fn rows<'a>(old: &[&'a str], new: &[&'a str], ops: &[Op]) -> Vec<Row<'a>> {
    let mut rows = Vec::with_capacity(ops.len());
    let (mut old_index, mut new_index) = (0usize, 0usize);
    for op in ops {
        let row = match op {
            Op::Equal => {
                let row = Row {
                    kind: RowKind::Context,
                    old_line: Some(old_index as u32 + 1),
                    new_line: Some(new_index as u32 + 1),
                    text: old[old_index],
                };
                old_index += 1;
                new_index += 1;
                row
            }
            Op::Remove => {
                let row = Row {
                    kind: RowKind::Removed,
                    old_line: Some(old_index as u32 + 1),
                    new_line: None,
                    text: old[old_index],
                };
                old_index += 1;
                row
            }
            Op::Add => {
                let row = Row {
                    kind: RowKind::Added,
                    old_line: None,
                    new_line: Some(new_index as u32 + 1),
                    text: new[new_index],
                };
                new_index += 1;
                row
            }
        };
        rows.push(row);
    }
    rows
}

/// The rows as hunk text: each group of changes with `context` unchanged rows
/// around it, merged when two groups would show the same rows twice.
fn render(rows: &[Row<'_>], context: usize) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.kind == RowKind::Context {
            continue;
        }
        let start = index.saturating_sub(context);
        let stop = (index + context + 1).min(rows.len());
        match ranges.last_mut() {
            Some((_, last_stop)) if start <= *last_stop => *last_stop = stop,
            _ => ranges.push((start, stop)),
        }
    }

    let mut text = String::new();
    for (start, stop) in ranges {
        let range = &rows[start..stop];
        let old_count = range.iter().filter(|row| row.old_line.is_some()).count();
        let new_count = range.iter().filter(|row| row.new_line.is_some()).count();
        // A hunk that only adds or only removes has no line of its own on the
        // untouched side: git names the line it sits after, or 0 at the top.
        let old_before = rows[..start]
            .iter()
            .rev()
            .find_map(|row| row.old_line)
            .unwrap_or(0);
        let new_before = rows[..start]
            .iter()
            .rev()
            .find_map(|row| row.new_line)
            .unwrap_or(0);
        let old_start = range
            .iter()
            .find_map(|row| row.old_line)
            .unwrap_or(old_before);
        let new_start = range
            .iter()
            .find_map(|row| row.new_line)
            .unwrap_or(new_before);
        text.push_str(&format!(
            "@@ -{} +{} @@\n",
            range_text(old_start, old_count),
            range_text(new_start, new_count)
        ));
        for row in range {
            text.push(match row.kind {
                RowKind::Context => ' ',
                RowKind::Removed => '-',
                RowKind::Added => '+',
            });
            text.push_str(row.text);
            text.push('\n');
        }
    }
    text
}

/// A hunk range as git spells it: a one-line range is its start alone, every
/// other range is `start,count`.
fn range_text(start: u32, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_texts_make_no_diff() {
        assert!(unified_diff("a\nb\n", "a\nb\n").is_empty());
        assert!(unified_diff("", "").is_empty());
    }

    #[test]
    fn a_one_line_change_carries_its_context_and_line_numbers() {
        let diff = unified_diff("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(diff, "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
    }

    #[test]
    fn a_new_file_is_all_added_from_line_one() {
        let diff = unified_diff("", "one\ntwo\n");
        assert_eq!(diff, "@@ -0,0 +1,2 @@\n+one\n+two\n");
    }

    #[test]
    fn a_deleted_file_is_all_removed_to_nothing() {
        let diff = unified_diff("one\ntwo\n", "");
        assert_eq!(diff, "@@ -1,2 +0,0 @@\n-one\n-two\n");
    }

    #[test]
    fn far_apart_changes_make_two_hunks() {
        let old = (1..=20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut new_lines: Vec<String> = (1..=20).map(|i| i.to_string()).collect();
        new_lines[1] = "two".to_string();
        new_lines[18] = "nineteen".to_string();
        let new = new_lines.join("\n");
        let diff = unified_diff(&old, &new);
        assert_eq!(
            diff.lines().filter(|line| line.starts_with("@@")).count(),
            2,
            "{diff}"
        );
        assert!(diff.contains("@@ -1,5 +1,5 @@"), "{diff}");
        assert!(diff.contains("-2\n+two\n"), "{diff}");
        assert!(diff.contains("-19\n+nineteen\n"), "{diff}");
    }

    #[test]
    fn an_insertion_names_the_line_it_follows() {
        let diff = unified_diff("a\nb\n", "a\nnew\nb\n");
        assert_eq!(diff, "@@ -1,2 +1,3 @@\n a\n+new\n b\n");
    }

    #[test]
    fn an_overwritten_file_reads_as_a_replacement() {
        let diff = unified_diff("old\n", "new\n");
        assert_eq!(diff, "@@ -1 +1 @@\n-old\n+new\n");
    }
}
