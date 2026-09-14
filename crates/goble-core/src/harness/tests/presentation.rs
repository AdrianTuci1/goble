use crate::harness::*;

/// Each of this harness's tools parses into the family whose shape it draws:
/// the reference pager's families, mapped onto the names the harness defines.
#[test]
fn every_harness_tool_parses_into_a_family() {
    let cases = [
        ("run_command", ToolKind::Execute),
        ("read_file", ToolKind::Read),
        ("edit_file", ToolKind::Edit),
        ("write_file", ToolKind::Create),
        ("codebase_search", ToolKind::Search),
        ("search_store", ToolKind::Search),
        ("web_search", ToolKind::WebSearch),
        ("read_url", ToolKind::WebFetch),
        ("search_mcp_servers", ToolKind::SearchTools),
        ("create_agent", ToolKind::SubAgent),
        ("run_agent", ToolKind::SubAgent),
        (SPAWN_SUBAGENT_TOOL, ToolKind::SubAgent),
        // A tool with no family of its own is named by its own name.
        ("credentials", ToolKind::Other),
        ("delete_file", ToolKind::Other),
        ("memory_read", ToolKind::Other),
        ("mcp__srv__tool", ToolKind::Other),
    ];
    for (name, kind) in cases {
        assert_eq!(tool_kind_for(name), kind, "{name}");
    }
}

/// Every name the harness defines is claimed by the parse — either by a family
/// or by the fall-through that names the tool — so no tool is unparseable.
#[test]
fn the_parse_answers_for_every_defined_tool() {
    for definition in harness_tool_definitions() {
        let row = tool_row(&definition.name, &serde_json::json!({}), None);
        // A sub-agent's row is the child's own, and its subject comes from the
        // live record rather than from the call's arguments.
        let expected = if row.kind == ToolKind::SubAgent {
            String::new()
        } else {
            definition.name.clone()
        };
        assert_eq!(
            row.subject, expected,
            "a call with no operand is named by its tool: {}",
            definition.name
        );
    }
}
