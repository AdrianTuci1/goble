use super::*;
use crate::harness::*;

#[test]
fn test_list_tools() {
    let store = Store::open_in_memory().unwrap();
    let harness = Harness::new(store);
    let tools = harness.list_tools();
    assert!(tools.iter().any(|t| t.name == "create_agent"));
    assert!(tools.iter().any(|t| t.name == "search_store"));
    assert!(tools.iter().any(|t| t.name == "deploy_agent"));
    assert!(tools.iter().any(|t| t.name == "read_file"));
}

#[test]
fn spawn_subagent_is_declared_with_its_schema() {
    let definition = harness_tool_definitions()
        .into_iter()
        .find(|definition| definition.name == SPAWN_SUBAGENT_TOOL)
        .expect("spawn_subagent is defined");
    let properties = definition.parameters["properties"]
        .as_object()
        .expect("an object schema");
    for field in [
        "description",
        "prompt",
        "subagent_type",
        "run_in_background",
        "cwd",
    ] {
        assert!(properties.contains_key(field), "{field} is declared");
    }
    let required = definition.parameters["required"]
        .as_array()
        .expect("a required list");
    for field in [
        "description",
        "prompt",
        "subagent_type",
        "run_in_background",
    ] {
        assert!(
            required.iter().any(|r| r.as_str() == Some(field)),
            "{field} is required"
        );
    }
    assert!(
        !required.iter().any(|r| r.as_str() == Some("cwd")),
        "cwd is optional"
    );
}
