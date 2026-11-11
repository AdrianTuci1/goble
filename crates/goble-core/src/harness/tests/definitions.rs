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

/// The harness exposes the tool set the transcript's grok-build families can
/// name. `execute_python_code` was a leftover outside that set: it rendered as
/// a raw tool name and failed when called.
#[test]
fn the_python_tool_is_not_defined() {
    assert!(
        harness_tool_definitions()
            .iter()
            .all(|definition| definition.name != "execute_python_code")
    );
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

/// Every tool the harness advertises reaches a handler. A definition without a
/// dispatch arm hands the model a tool that can only fail as "unknown tool",
/// which is the shape of a turn that goes nowhere: this test calls each
/// advertised name through the real dispatch and fails on that error alone
/// (arguments are empty, so every tool is free to reject them for its own
/// reason).
#[tokio::test]
async fn every_advertised_tool_reaches_a_handler() {
    use crate::harness::{execute_tool_call, MockCommandRunner, ToolTurnContext, WebSearchConfig};
    use crate::llm::{CompletionResponse, LlmToolCall, MockProvider};
    use crate::mcp_manager::McpManager;
    use crate::subagent_run::SubAgentHost;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::Duration;

    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    let chat_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    store
        .insert_chat(&chat_id, "tools", None, None, &now, &now)
        .expect("chat row");
    let host = SubAgentHost {
        store: store.clone(),
        runner: Arc::new(MockCommandRunner),
        llm: Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: Vec::new(),
                usage: None,
            },
        )),
        deploy_sender: None,
        mcp_manager: McpManager::new(),
        docs_dir: workspace.path().to_path_buf(),
        registry: Default::default(),
        parent_cancel: Arc::new(AtomicBool::new(false)),
        foreground_wait: Duration::from_secs(5),
    };
    let turn = ToolTurnContext {
        chat_id: &chat_id,
        provider: "mock",
        model: "mock",
        depth: 0,
        host: &host,
    };
    let mcp_manager = McpManager::new();

    for definition in harness_tool_definitions() {
        let name = definition.name.clone();
        let call = LlmToolCall {
            id: format!("tc-{name}"),
            name: name.clone(),
            arguments: serde_json::json!({}),
        };
        let result = execute_tool_call(
            &store,
            &MockCommandRunner,
            None,
            &mcp_manager,
            workspace.path(),
            workspace.path(),
            &call,
            &WebSearchConfig::default(),
            &turn,
        )
        .await;
        if let Err(error) = result {
            assert!(
                !error.to_string().contains("unknown tool"),
                "{name} is advertised but has no handler: {error}"
            );
        }
    }
}
