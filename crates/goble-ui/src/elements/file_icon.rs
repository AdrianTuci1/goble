//! The icon a file row draws: the language a file is written in when the tree
//! knows it, a plain document when it does not. The set is warp-new's, taken
//! with its bundled file-type SVGs.

use crate::elements::{Icon, IconName};

/// The icon name for the file called `name`, chosen by its extension.
pub fn file_icon_name(name: &str) -> IconName {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "rs" => "file-rust",
        "json" => "file-json",
        "ts" | "tsx" => "file-typescript",
        "js" | "jsx" => "file-javascript",
        "py" => "file-python",
        "cpp" | "hpp" => "file-cpp",
        "c" | "h" => "file-c",
        "go" => "file-go",
        "md" => "file-markdown",
        "kt" | "kts" => "file-kotlin",
        "php" => "file-php",
        "pl" | "pm" => "file-perl",
        "pyx" | "pxd" => "file-cython",
        "swf" => "file-flash",
        "wasm" => "file-wasm",
        "zig" => "file-zig",
        "sql" => "file-sql",
        "ng" | "ngml" => "file-angular",
        "tf" | "hcl" | "tfvars" => "file-terraform",
        "mmd" | "mermaid" => "file-mermaid",
        _ => "file",
    }
}

/// The icon for a file called `name`, ready to place in a row.
pub fn file_icon(name: &str) -> Icon {
    Icon::new(file_icon_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::icon_atlas::is_registered;

    #[test]
    fn a_known_extension_names_its_language() {
        assert_eq!(file_icon_name("main.rs"), "file-rust");
        assert_eq!(file_icon_name("package.json"), "file-json");
        assert_eq!(file_icon_name("view/View.TSX"), "file-typescript");
        assert_eq!(file_icon_name("mod.py"), "file-python");
        assert_eq!(file_icon_name("schema.sql"), "file-sql");
    }

    #[test]
    fn an_unknown_or_absent_extension_is_a_plain_document() {
        for name in ["Makefile", "LICENSE", "archive.tar.gz", "notes.txt", ".gitignore"] {
            assert_eq!(file_icon_name(name), "file", "{name}");
        }
    }

    /// Every icon a row can name is really in the atlas, so a file type the tree
    /// maps is never drawn as the fallback cross.
    #[test]
    fn every_file_icon_the_tree_can_choose_is_registered() {
        for name in [
            "a.rs", "a.json", "a.ts", "a.js", "a.py", "a.cpp", "a.c", "a.go", "a.md", "a.kt",
            "a.php", "a.pl", "a.pyx", "a.swf", "a.wasm", "a.zig", "a.sql", "a.ng", "a.tf", "a.mmd",
            "a.unknown",
        ] {
            let icon = file_icon_name(name);
            assert!(is_registered(icon), "{icon} is not in the icon atlas");
        }
    }
}
