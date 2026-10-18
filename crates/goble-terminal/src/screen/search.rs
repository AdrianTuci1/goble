//! Searching a screen's history and its visible rows.
//!
//! A search is over the same content a block draws: the rows the grid holds,
//! oldest first, so a match in scrollback is found while the viewport is on the
//! live screen. The default is a literal search with smart case; a caller that
//! asks for regex mode gets the `regex` crate's syntax, and an invalid pattern
//! is reported instead of silently matching nothing.

use regex::RegexBuilder;

use super::cell::ScreenLine;

/// How a search compares its lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// Interpret the query as a regular expression (`regex` crate syntax).
    pub regex: bool,
    /// `None` is smart case: a query with an uppercase character is matched
    /// case-sensitively and one without is matched case-insensitively.
    pub case_sensitive: Option<bool>,
}

impl SearchOptions {
    /// A literal, smart-case search.
    pub const fn literal() -> Self {
        Self {
            regex: false,
            case_sensitive: None,
        }
    }

    /// A regular-expression, smart-case search.
    pub const fn regex() -> Self {
        Self {
            regex: true,
            case_sensitive: None,
        }
    }
}

/// One match in the screen's content.
///
/// `line` is the absolute row index over history followed by the visible
/// screen: `0` is the oldest row the grid still holds, and
/// [`super::Screen::history_len`] is the first visible row at the live bottom.
/// `col` and `len` are cell columns, so a wide character is two columns wide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub text: String,
}

/// Why a search could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// The query is not a valid regular expression.
    InvalidRegex(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::InvalidRegex(message) => write!(f, "invalid regex: {message}"),
        }
    }
}

impl std::error::Error for SearchError {}

/// Search `lines` (oldest first) for `query`.
pub(super) fn search_lines(
    lines: &[ScreenLine],
    query: &str,
    options: SearchOptions,
) -> Result<Vec<SearchMatch>, SearchError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let case_sensitive = options
        .case_sensitive
        .unwrap_or_else(|| query.chars().any(char::is_uppercase));
    let pattern = if options.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|error| SearchError::InvalidRegex(error.to_string()))?;

    let mut matches = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let (text, columns, widths) = char_columns(line);
        for found in regex.find_iter(&text) {
            if found.start() == found.end() {
                continue;
            }
            let first = text[..found.start()].chars().count();
            let last = text[..found.end()].chars().count();
            let Some(&col) = columns.get(first) else {
                continue;
            };
            let end = columns
                .get(last - 1)
                .copied()
                .unwrap_or(col)
                .saturating_add(widths.get(last - 1).copied().unwrap_or(1));
            matches.push(SearchMatch {
                line: line_index,
                col,
                len: end.saturating_sub(col),
                text: found.as_str().to_string(),
            });
        }
    }
    Ok(matches)
}

/// The text the search compares, and the cell column each character starts in.
///
/// The pairs skip wide-character spacers and keep combining marks on the cell
/// they belong to, exactly as [`ScreenLine::text`] does, while the columns stay
/// in the grid's own coordinate space so a match can be highlighted in cells.
fn char_columns(line: &ScreenLine) -> (String, Vec<usize>, Vec<usize>) {
    let mut text = String::new();
    let mut columns = Vec::new();
    let mut widths = Vec::new();
    for (column, cell) in line.cells.iter().enumerate() {
        if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
            continue;
        }
        text.push(cell.ch);
        columns.push(column);
        widths.push(if cell.attrs.wide { 2 } else { 1 });
        for mark in &cell.zerowidth {
            text.push(*mark);
            columns.push(column);
            widths.push(0);
        }
    }
    // `ScreenLine::text` drops the row's trailing blanks; a search does the same
    // so a query cannot match the empty tail of every row.
    let len = text.trim_end().len();
    text.truncate(len);
    let count = text.chars().count();
    columns.truncate(count);
    widths.truncate(count);
    (text, columns, widths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> ScreenLine {
        ScreenLine {
            cells: text
                .chars()
                .map(|ch| super::super::cell::ScreenCell {
                    ch,
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn literal_search_is_case_insensitive_without_an_uppercase_query() {
        let lines = vec![line("Hello world"), line("hello again")];
        let found = search_lines(&lines, "hello", SearchOptions::literal()).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].line, found[0].col, found[0].len), (0, 0, 5));
        assert_eq!((found[1].line, found[1].col), (1, 0));
    }

    #[test]
    fn smart_case_turns_sensitive_when_the_query_has_an_uppercase_character() {
        let lines = vec![line("Hello"), line("hello")];
        let found = search_lines(&lines, "Hello", SearchOptions::literal()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 0);
    }

    #[test]
    fn a_literal_query_does_not_become_a_pattern() {
        let lines = vec![line("a.c"), line("abc")];
        let found = search_lines(&lines, "a.c", SearchOptions::literal()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "a.c");
    }

    #[test]
    fn regex_mode_and_an_invalid_pattern_are_reported() {
        let lines = vec![line("one two three")];
        let found = search_lines(&lines, r"\w+", SearchOptions::regex()).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found[2].text, "three");

        let error = search_lines(&lines, "(", SearchOptions::regex()).unwrap_err();
        assert!(matches!(error, SearchError::InvalidRegex(_)));
        // The same text is a literal, so it is not an error there.
        assert!(search_lines(&lines, "(", SearchOptions::literal())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_missing_query_finds_nothing() {
        let lines = vec![line("anything")];
        assert!(search_lines(&lines, "absent", SearchOptions::literal())
            .unwrap()
            .is_empty());
        assert!(search_lines(&lines, "", SearchOptions::literal())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_wide_character_match_covers_both_of_its_columns() {
        let mut wide = line("日本");
        wide.cells[0].attrs.wide = true;
        wide.cells[1].attrs.wide_spacer = true;
        let lines = vec![wide];
        let found = search_lines(&lines, "日", SearchOptions::literal()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].col, 0);
        // A wide cell is two columns, and its spacer is not part of the text.
        assert_eq!(found[0].len, 2);
    }
}
