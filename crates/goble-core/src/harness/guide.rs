use anyhow::Result;

/// `user_guide` tool handler. With no `topic`, lists the seeded guide topics; with
/// a `topic` (filename, `NN-topic`, or bare `topic` name), returns that entry in
/// full. Resolution is against the `.md` files already present in `docs_dir`, so it
/// never reads a path outside the guide directory.
pub(super) fn user_guide(args: &serde_json::Value, docs_dir: &std::path::Path) -> Result<String> {
    let topic = args["topic"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_default();

    if !docs_dir.is_dir() {
        return Ok(concat!(
            "The user guide is not available yet (no user-guide directory). ",
            "It is seeded into ~/.goble/docs/user-guide on first launch."
        )
        .to_string());
    }

    let mut names: Vec<String> = std::fs::read_dir(docs_dir)
        .map_err(|e| anyhow::anyhow!("read user guide dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();

    if topic.trim().is_empty() {
        let mut out = String::from("User guide topics:\n");
        for name in &names {
            out.push_str(&format!(
                "- {} — {}\n",
                user_guide_topic(name),
                user_guide_title(&docs_dir.join(name))
            ));
        }
        return Ok(out);
    }

    let want = topic.trim();
    let target = names.iter().find(|name| {
        name.as_str() == want
            || user_guide_topic(name).eq_ignore_ascii_case(want)
            || name
                .strip_suffix(".md")
                .is_some_and(|s| s.eq_ignore_ascii_case(want))
    });

    match target {
        Some(name) => std::fs::read_to_string(docs_dir.join(name))
            .map_err(|e| anyhow::anyhow!("read user guide doc {name}: {e}")),
        None => {
            let mut out = format!("No user guide topic ‘{want}’.\nAvailable topics:\n");
            for name in &names {
                out.push_str(&format!("- {}\n", user_guide_topic(name)));
            }
            Ok(out)
        }
    }
}

/// `06-remote-access.md` → `remote-access`. Only strips the `NN-` prefix when the
/// file is numbered (two leading digits + a dash).
fn user_guide_topic(name: &str) -> &str {
    let bare = name.strip_suffix(".md").unwrap_or(name);
    let bytes = bare.as_bytes();
    if bare.len() >= 3
        && bare.as_bytes().get(2) == Some(&b'-')
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
    {
        &bare[3..]
    } else {
        bare
    }
}

/// First `# Heading` line of a guide doc, if any.
fn user_guide_title(path: &std::path::Path) -> String {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    s.lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").trim().to_string())
        .unwrap_or_default()
}
