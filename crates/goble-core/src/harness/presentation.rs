use serde_json::Value;

use super::SPAWN_SUBAGENT_TOOL;

/// The family a tool call belongs to.
///
/// The variants are the kinds grok-build's `ToolCallBlock::from_name` parses a
/// call into, plus the two this harness has and that one does not: a spawned
/// sub-agent, and a call no family claims.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolKind {
    /// A shell command: the command, then its output.
    Execute,
    /// A file read: the path, then the file's lines as an editor excerpt.
    Read,
    /// An edit of an existing file: the path, then diff rows.
    Edit,
    /// A file being written: the path, then the write's confirmation.
    Create,
    /// A directory listing: the path, then the entries.
    List,
    /// A search: the pattern, its scope, then the matches.
    Search,
    /// A web search: the query, then the results and their sites.
    WebSearch,
    /// A URL fetch: the URL, then the content it returned.
    WebFetch,
    /// A search for tools to install: the query, then what it found.
    SearchTools,
    /// A call of a tool a server owns: the server and the tool, then its result.
    UseTool,
    /// A search of stored memory: the query, then the entries.
    MemorySearch,
    /// A read of a skill definition, named by the skill rather than the file.
    Skill,
    /// A sub-agent: the child's own record and rows.
    SubAgent,
    /// No family claims the call: the row names the tool itself.
    #[default]
    Other,
}

/// The family a tool name belongs to. The reference pager's own names come
/// first, so a call made with one of them parses the way it parses there even
/// when this harness would have spelled it differently.
pub fn tool_kind_for(name: &str) -> ToolKind {
    match name.to_lowercase().as_str() {
        "run_terminal_command" | "run_terminal_cmd" | "bash" | "shell" | "execute"
        | "run_command" => ToolKind::Execute,
        "read" | "read_file" => ToolKind::Read,
        "search_replace" | "edit" | "apply_patch" | "strreplace" | "edit_file" => ToolKind::Edit,
        "write" | "write_file" => ToolKind::Create,
        "list_dir" | "ls" => ToolKind::List,
        "grep" | "search" | "glob" | "codebase_search" | "search_store" => ToolKind::Search,
        "web_search" => ToolKind::WebSearch,
        "web_fetch" | "fetch" | "read_url" => ToolKind::WebFetch,
        "search_tool" | "search_mcp_servers" => ToolKind::SearchTools,
        "use_tool" => ToolKind::UseTool,
        "memory_search" => ToolKind::MemorySearch,
        "skill" => ToolKind::Skill,
        "create_agent" | "run_agent" | SPAWN_SUBAGENT_TOOL => ToolKind::SubAgent,
        _ => ToolKind::Other,
    }
}

/// One tool call parsed into the row the transcript draws: the bold verb its
/// family gives it, the operand its arguments carry, and the dim detail its
/// result lets us state. A call whose family is recognised is never drawn under
/// its tool's own name — `read_file` with a path draws `Read <path>` — and only
/// a call no family claims falls back to the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRow {
    pub kind: ToolKind,
    /// The bold leading verb, with its trailing space (`"Read "`), or empty for
    /// a row that names a tool instead.
    pub verb: &'static str,
    /// The operand the verb applies to: a path, a command, a query, a URL.
    pub subject: String,
    /// The dim trailing detail: a diffstat or a count, empty when the row
    /// states none.
    pub detail: String,
}

/// Parse one tool call into its row. `result` is the tool's own result, read
/// only for the counts a collapsed row states (`(N entries)`, `(N sites)`).
pub fn tool_row(name: &str, arguments: &Value, result: Option<&str>) -> ToolRow {
    let kind = tool_kind_for(name);
    // A sub-agent's row is the child's own: the caller fills its subject and
    // detail from the live record, which the arguments cannot carry.
    if kind == ToolKind::SubAgent {
        return ToolRow {
            kind,
            verb: "Subagent ",
            subject: String::new(),
            detail: String::new(),
        };
    }
    let Some(subject) = operand(kind, arguments) else {
        // The arguments carry no operand for this family (a call with no path,
        // a truncated payload): name the tool rather than draw an empty verb.
        return named_row(name, arguments);
    };
    match kind {
        ToolKind::Read => match skill_name(&subject) {
            Some(skill) => ToolRow {
                kind: ToolKind::Skill,
                verb: "Skill ",
                subject: skill,
                detail: String::new(),
            },
            None => ToolRow {
                kind,
                verb: "Read ",
                subject,
                detail: String::new(),
            },
        },
        ToolKind::Edit => {
            let detail = match (
                string_argument(arguments, "old_text"),
                string_argument(arguments, "new_text"),
            ) {
                (Some(old), Some(new)) => edit_diffstat(&old, &new),
                _ => String::new(),
            };
            ToolRow {
                kind,
                verb: "Edit ",
                subject,
                detail,
            }
        }
        ToolKind::Create => ToolRow {
            kind,
            verb: "Creating ",
            subject,
            detail: String::new(),
        },
        ToolKind::Execute => ToolRow {
            kind,
            verb: "Run ",
            subject: one_line(&subject),
            detail: String::new(),
        },
        ToolKind::List => {
            let detail = entry_count(result);
            ToolRow {
                kind,
                verb: "List ",
                subject,
                detail,
            }
        }
        ToolKind::Search => ToolRow {
            kind,
            verb: "Search ",
            subject: search_subject(arguments, &subject),
            detail: String::new(),
        },
        ToolKind::WebSearch => {
            let detail = site_count(result);
            ToolRow {
                kind,
                verb: "Web Search ",
                subject,
                detail,
            }
        }
        ToolKind::WebFetch => ToolRow {
            kind,
            verb: "Fetch ",
            subject,
            detail: String::new(),
        },
        ToolKind::SearchTools => ToolRow {
            kind,
            verb: "Search Tools ",
            subject,
            detail: String::new(),
        },
        ToolKind::MemorySearch => ToolRow {
            kind,
            verb: "Memory Search ",
            subject,
            detail: String::new(),
        },
        ToolKind::Skill => ToolRow {
            kind,
            verb: "Skill ",
            subject,
            detail: String::new(),
        },
        // A server's tool is named by the server and the tool, the way the
        // reference pager names it; the operand is the tool itself.
        ToolKind::UseTool => ToolRow {
            kind,
            verb: "",
            subject,
            detail: String::new(),
        },
        ToolKind::SubAgent => unreachable!("a sub-agent row is built above"),
        ToolKind::Other => named_row(name, arguments),
    }
}

/// What a sub-agent's row names the child by: its description in quotes, the
/// way grok-build's subagent blocks quote it.
pub fn subagent_subject(description: &str) -> String {
    format!("\u{201C}{}\u{201D}", one_line(description))
}

/// A row for a call no family claims: the tool's own name, and the one string
/// its arguments carry as the dim detail.
fn named_row(name: &str, arguments: &Value) -> ToolRow {
    ToolRow {
        kind: ToolKind::Other,
        verb: "",
        subject: name.to_string(),
        detail: first_string_argument(arguments),
    }
}

/// The argument a family's row is named by.
fn operand(kind: ToolKind, arguments: &Value) -> Option<String> {
    let key = match kind {
        ToolKind::Execute => "command",
        ToolKind::Read | ToolKind::Create | ToolKind::List => "path",
        ToolKind::Edit => "path",
        ToolKind::Search => "pattern",
        ToolKind::WebSearch | ToolKind::SearchTools | ToolKind::MemorySearch => "query",
        ToolKind::WebFetch => "url",
        ToolKind::UseTool => "name",
        ToolKind::Skill => "skill",
        ToolKind::SubAgent | ToolKind::Other => return None,
    };
    let value = string_argument(arguments, key)
        // The harness spells a content search's operand `query` where the
        // reference spells it `pattern`, and vice versa for a web search.
        .or_else(|| match kind {
            ToolKind::Search => string_argument(arguments, "query"),
            _ => None,
        })?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

/// A search's operand as the reference states it: the quoted pattern, then the
/// path it was scoped to.
fn search_subject(arguments: &Value, pattern: &str) -> String {
    match string_argument(arguments, "path").filter(|path| !path.trim().is_empty()) {
        Some(path) => format!("\"{pattern}\" in {path}"),
        None => format!("\"{pattern}\""),
    }
}

/// ` +N/-M` for a replacement: the lines it added and the lines it removed.
/// Empty when neither changed, so a no-op edit states no diffstat.
pub fn edit_diffstat(old_text: &str, new_text: &str) -> String {
    let (added, removed) = edit_change_counts(old_text, new_text);
    if added == 0 && removed == 0 {
        return String::new();
    }
    format!(" +{added}/-{removed}")
}

/// The lines a replacement added and removed.
pub fn edit_change_counts(old_text: &str, new_text: &str) -> (usize, usize) {
    (new_text.lines().count(), old_text.lines().count())
}

/// ` (N entries)` for a listing's result, counted the way the reference counts
/// it: its non-empty lines.
fn entry_count(result: Option<&str>) -> String {
    let Some(result) = result else {
        return String::new();
    };
    let count = result.lines().filter(|line| !line.trim().is_empty()).count();
    if count == 0 {
        return String::new();
    }
    let plural = if count == 1 { "y" } else { "ies" };
    format!(" ({count} entr{plural})")
}

/// ` (N sites)` for a web search's result: the distinct hosts of the URLs it
/// cites, the count the reference states while collapsed.
fn site_count(result: Option<&str>) -> String {
    let Some(result) = result else {
        return String::new();
    };
    let mut hosts: Vec<String> = web_search_sources(result)
        .iter()
        .filter_map(|url| host_of(url))
        .collect();
    hosts.sort();
    hosts.dedup();
    if hosts.is_empty() {
        return String::new();
    }
    let plural = if hosts.len() == 1 { "" } else { "s" };
    format!(" ({} site{plural})", hosts.len())
}

/// The URLs a web search's result cites, in result order.
pub fn web_search_sources(result: &str) -> Vec<String> {
    result
        .lines()
        .filter_map(|line| line.strip_prefix("URL: "))
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .collect()
}

/// The host of a URL, without its scheme, userinfo, port or path.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// A read of a skill definition is named by the skill: the directory a
/// `SKILL.md` sits in, the rule the reference reads it by.
pub fn skill_name(path: &str) -> Option<String> {
    let path = std::path::Path::new(path);
    if path.file_name()?.to_str()? != "SKILL.md" {
        return None;
    }
    Some(path.parent()?.file_name()?.to_str()?.to_string())
}

/// A string argument, when the call carries one.
pub fn string_argument<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}

/// The first string value an object carries, whatever its key: what a row with
/// no declared shape states as its detail.
fn first_string_argument(arguments: &Value) -> String {
    arguments
        .as_object()
        .and_then(|object| object.values().find_map(|value| value.as_str()))
        .map(one_line)
        .unwrap_or_default()
}

/// The row is a single line whatever the tool was handed, so no embedded
/// newline may break it out of it.
pub fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, arguments: &str) -> ToolRow {
        tool_row(name, &serde_json::from_str(arguments).unwrap(), None)
    }

    #[test]
    fn a_read_parses_into_the_verb_and_its_path() {
        let row = row("read_file", r#"{"path":"src/main.rs"}"#);
        assert_eq!(row.kind, ToolKind::Read);
        assert_eq!(row.verb, "Read ");
        assert_eq!(row.subject, "src/main.rs");
        assert_eq!(row.detail, "");
    }

    /// A read of a skill definition is the skill's row, named by the skill
    /// rather than by the file it lives in.
    #[test]
    fn a_skill_read_is_named_by_the_skill() {
        let row = row("read_file", r#"{"path":"/home/u/.grok/skills/deploy/SKILL.md"}"#);
        assert_eq!(row.kind, ToolKind::Skill);
        assert_eq!(row.verb, "Skill ");
        assert_eq!(row.subject, "deploy");
    }

    #[test]
    fn a_command_is_run_and_a_multi_line_one_stays_on_one_row() {
        let row = row("run_command", r#"{"command":"cargo test\n-p goble-ui"}"#);
        assert_eq!(row.kind, ToolKind::Execute);
        assert_eq!(row.verb, "Run ");
        assert_eq!(row.subject, "cargo test -p goble-ui");
    }

    #[test]
    fn an_edit_parses_into_its_path_and_diffstat() {
        let row = row(
            "edit_file",
            r#"{"path":"src/lib.rs","old_text":"one\ntwo","new_text":"one"}"#,
        );
        assert_eq!(row.kind, ToolKind::Edit);
        assert_eq!(row.verb, "Edit ");
        assert_eq!(row.subject, "src/lib.rs");
        assert_eq!(row.detail, " +1/-2");
    }

    #[test]
    fn a_write_creates_the_path() {
        let row = row("write_file", r#"{"path":"src/new.rs","content":"x"}"#);
        assert_eq!(row.kind, ToolKind::Create);
        assert_eq!(row.verb, "Creating ");
        assert_eq!(row.subject, "src/new.rs");
    }

    #[test]
    fn a_search_quotes_its_pattern_and_names_its_scope() {
        assert_eq!(
            row("codebase_search", r#"{"pattern":"fn main"}"#).subject,
            "\"fn main\""
        );
        let scoped = row(
            "codebase_search",
            r#"{"pattern":"fn main","path":"crates/goble-ui"}"#,
        );
        assert_eq!(scoped.verb, "Search ");
        assert_eq!(scoped.subject, "\"fn main\" in crates/goble-ui");
    }

    /// The harness spells a store search's operand `query`; it is the same
    /// search row, quoted the same way.
    #[test]
    fn a_store_search_reads_the_query_the_harness_spells_it() {
        let row = row("search_store", r#"{"query":"deploy"}"#);
        assert_eq!(row.kind, ToolKind::Search);
        assert_eq!(row.subject, "\"deploy\"");
    }

    #[test]
    fn a_web_search_counts_the_sites_it_cited() {
        let result = "Title\nURL: https://a.example/x\n\nOther\nURL: https://b.example/y\nURL: https://a.example/z";
        let row = tool_row(
            "web_search",
            &serde_json::json!({ "query": "rust gui" }),
            Some(result),
        );
        assert_eq!(row.kind, ToolKind::WebSearch);
        assert_eq!(row.verb, "Web Search ");
        assert_eq!(row.subject, "rust gui");
        assert_eq!(row.detail, " (2 sites)");
    }

    #[test]
    fn a_fetch_and_a_tool_search_name_their_operand() {
        assert_eq!(row("read_url", r#"{"url":"https://x.example"}"#).verb, "Fetch ");
        assert_eq!(
            row("read_url", r#"{"url":"https://x.example"}"#).subject,
            "https://x.example"
        );
        let row = row("search_mcp_servers", r#"{"query":"postgres"}"#);
        assert_eq!(row.kind, ToolKind::SearchTools);
        assert_eq!(row.verb, "Search Tools ");
        assert_eq!(row.subject, "postgres");
    }

    #[test]
    fn a_listing_counts_its_entries() {
        let row = tool_row(
            "list_dir",
            &serde_json::json!({ "path": "src" }),
            Some("main.rs\n\nlib.rs\n"),
        );
        assert_eq!(row.kind, ToolKind::List);
        assert_eq!(row.verb, "List ");
        assert_eq!(row.subject, "src");
        assert_eq!(row.detail, " (2 entries)");
    }

    /// A call no family claims keeps its own name, and states the one string
    /// its arguments carry.
    #[test]
    fn an_unclaimed_call_is_named_by_its_tool() {
        let row = row("credentials", r#"{"name":"openai"}"#);
        assert_eq!(row.kind, ToolKind::Other);
        assert_eq!(row.verb, "");
        assert_eq!(row.subject, "credentials");
        assert_eq!(row.detail, "openai");
    }

    /// A recognised family whose arguments carry no operand falls back to the
    /// name rather than drawing a verb with nothing after it.
    #[test]
    fn a_recognised_call_with_no_operand_is_named_by_its_tool() {
        let row = row("read_file", r#"{}"#);
        assert_eq!(row.kind, ToolKind::Other);
        assert_eq!(row.subject, "read_file");
    }

    #[test]
    fn a_subagent_row_is_the_childrens_own() {
        let row = row("spawn_subagent", r#"{"description":"check the tests"}"#);
        assert_eq!(row.kind, ToolKind::SubAgent);
        assert_eq!(row.verb, "Subagent ");
        assert_eq!(row.subject, "");
        assert_eq!(subagent_subject("check the tests"), "\u{201C}check the tests\u{201D}");
    }

    /// Every name family the reference parses is parsed here too.
    #[test]
    fn the_reference_name_families_are_parsed() {
        let cases = [
            ("run_terminal_command", ToolKind::Execute),
            ("bash", ToolKind::Execute),
            ("read", ToolKind::Read),
            ("search_replace", ToolKind::Edit),
            ("apply_patch", ToolKind::Edit),
            ("strreplace", ToolKind::Edit),
            ("write", ToolKind::Create),
            ("ls", ToolKind::List),
            ("grep", ToolKind::Search),
            ("glob", ToolKind::Search),
            ("fetch", ToolKind::WebFetch),
            ("search_tool", ToolKind::SearchTools),
            ("use_tool", ToolKind::UseTool),
            ("skill", ToolKind::Skill),
        ];
        for (name, kind) in cases {
            assert_eq!(tool_kind_for(name), kind, "{name}");
        }
    }

    /// An MCP tool arrives under a name nothing claims and keeps it.
    #[test]
    fn an_mcp_tool_is_other() {
        assert_eq!(tool_kind_for("mcp__postgres__query"), ToolKind::Other);
    }
}
