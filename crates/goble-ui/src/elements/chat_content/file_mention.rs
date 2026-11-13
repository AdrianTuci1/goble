//! Recognise the file an inline code run names.
//!
//! A model writes a workspace file the way its tools spell it — `src/main.rs`,
//! `crates/goble-ui/src/lib.rs:42` — and the chat draws that run as a file
//! mention: a chip with the file's type icon that opens the file. A bare word
//! in code (`main.rs`, `package.json`) stays a code chip, because a file name
//! with no directory and no line number is as likely to be prose.

/// The workspace path an inline code run names, when it names one.
///
/// Returns the path with any `:line` / `:line:col` suffix removed, ready to
/// hand to the app's file opener. Run text that is not a path, a URL, a flag or
/// anything with whitespace returns `None`.
pub fn inline_code_file_path(code: &str) -> Option<&str> {
    let text = code.trim();
    if text.is_empty() || text.len() > 200 {
        return None;
    }
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    if text.contains("://")
        || text.starts_with('-')
        || text.starts_with('#')
        || text.starts_with('{')
    {
        return None;
    }
    let (path, had_position) = strip_position_suffix(text);
    // A mention names its place: a directory or a line. A lone file name is a
    // code run, the way a lone word is prose.
    if !had_position && !path.contains('/') {
        return None;
    }
    if path.is_empty() || path.ends_with('/') || path.starts_with("../") {
        return None;
    }
    // A leading `./` is how a model spells "the workspace root"; every other
    // `.`/`..` segment would name a place outside it.
    let mut segments = path.split('/');
    if path.starts_with("./") {
        segments.next();
    }
    if segments.any(|segment| {
        segment.is_empty() || segment == "." || segment == ".." || segment.contains('…')
    }) {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() || extension.len() > 8 {
        return None;
    }
    if !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    // `v1.2` and `0.13` are versions, not file types.
    if extension.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(path)
}

/// Split a `path:line` or `path:line:col` suffix off, reporting whether one was
/// there.
fn strip_position_suffix(text: &str) -> (&str, bool) {
    let mut path = text;
    let mut stripped = false;
    for _ in 0..2 {
        let Some((head, tail)) = path.rsplit_once(':') else {
            break;
        };
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) || !head.contains('.') {
            break;
        }
        path = head;
        stripped = true;
    }
    (path, stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_a_directory_is_a_mention() {
        assert_eq!(
            inline_code_file_path("crates/goble-ui/src/lib.rs"),
            Some("crates/goble-ui/src/lib.rs")
        );
        assert_eq!(inline_code_file_path("app/src/ui/chat/mod.rs"), Some("app/src/ui/chat/mod.rs"));
        assert_eq!(inline_code_file_path("./package.json"), Some("./package.json"));
    }

    #[test]
    fn a_line_suffix_is_dropped() {
        assert_eq!(inline_code_file_path("src/main.rs:42"), Some("src/main.rs"));
        assert_eq!(
            inline_code_file_path("crates/goble-ui/src/lib.rs:42:7"),
            Some("crates/goble-ui/src/lib.rs")
        );
    }

    #[test]
    fn a_lone_file_name_stays_code() {
        assert_eq!(inline_code_file_path("main.rs"), None);
        assert_eq!(inline_code_file_path("package.json"), None);
        assert_eq!(inline_code_file_path("README.md"), None);
    }

    #[test]
    fn prose_urls_and_flags_are_not_paths() {
        for text in [
            "example.com",
            "https://github.com/goble/goble",
            "npm install",
            "--force",
            "#12",
            "{color:red}",
            "crates/",
            "../outside.rs",
            "app/../outside.rs",
            "crates/…/lib.rs",
            "app/v1.2",
            "",
        ] {
            assert_eq!(inline_code_file_path(text), None, "{text:?}");
        }
    }
}
