use crate::agent::McpSource;
use crate::mcp_installer::McpInstaller;
use crate::store::Store;

use super::*;

use crate::mcp_manager::server::build_server_from_user_input;

#[test]
fn test_mcp_manager_update_meta_and_enabled_tools_filter() {
    use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};

    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("index.js"),
        include_str!("../../tests/mcp_mock_server.js"),
    )
    .unwrap();

    let server = McpServer {
        id: "mock-echo".to_string(),
        name: "Mock Echo".to_string(),
        source: McpSource::Local {
            path: src.to_string_lossy().to_string(),
        },
        manifest: McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![],
            capabilities: vec!["tools".to_string()],
            config_schema: serde_json::json!({}),
        },
        credentials_key: None,
        installed_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let store = Store::open_in_memory().unwrap();
    let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(manager.install_mcp_server(
        &store,
        &server.id,
        &server.name,
        "local",
        Some(&src.to_string_lossy()),
        &[],
        Some(server.manifest.clone()),
    ))
    .unwrap();

    let listed = manager.list_mcp_servers(&store).unwrap();
    assert_eq!(listed[0].enabled_tools.len(), 0);

    // Discover defaults all tools as enabled.
    manager
        .discover_and_enable_all(&store, "mock-echo")
        .unwrap();
    let listed = manager.list_mcp_servers(&store).unwrap();
    assert_eq!(listed[0].enabled_tools, vec!["echo"]);

    // Disable the only tool.
    manager
        .update_mcp_server_meta(&store, "mock-echo", &[], &[])
        .unwrap();
    let tools = manager.refresh_from_store(&store).unwrap();
    let concrete: Vec<_> = tools
        .into_iter()
        .filter(|t| t.name == "mcp_mock_echo_echo")
        .collect();
    assert!(concrete.is_empty(), "disabled tool should not be exposed");

    // Re-enable and verify exposed again.
    manager
        .update_mcp_server_meta(&store, "mock-echo", &[], &["echo".to_string()])
        .unwrap();
    let tools = manager.refresh_from_store(&store).unwrap();
    assert!(
        tools.iter().any(|t| t.name == "mcp_mock_echo_echo"),
        "enabled tool should be exposed"
    );
}

#[test]
fn test_mcp_manager_secret_ids_persisted() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open_in_memory().unwrap();
    let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(manager.install_mcp_server(
        &store,
        "mcp-sequential-thinking",
        "Sequential Thinking",
        "npm",
        Some("@modelcontextprotocol/server-sequential-thinking"),
        &[],
        None,
    ))
    .unwrap();

    manager
        .update_mcp_server_meta(
            &store,
            "mcp-sequential-thinking",
            &["openai_api_key".to_string()],
            &[],
        )
        .unwrap();

    let listed = manager.list_mcp_servers(&store).unwrap();
    assert_eq!(listed[0].secret_ids, vec!["openai_api_key"]);
}

#[test]
fn test_mcp_manager_generic_tool() {
    let tool = McpManager::generic_mcp_call_tool();
    assert_eq!(tool.name, "mcp_call");
}

#[test]
fn test_build_server_from_user_input() {
    let server = build_server_from_user_input(
        "mcp-sequential-thinking",
        "Sequential Thinking",
        "npm",
        Some("@modelcontextprotocol/server-sequential-thinking"),
        vec![],
        None,
    )
    .unwrap();
    assert_eq!(
        server.source,
        McpSource::Npm {
            package: "@modelcontextprotocol/server-sequential-thinking".to_string(),
            version: "latest".to_string(),
        }
    );
}

#[test]
fn test_mcp_manager_install_and_discover_local_mock() {
    use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
    use chrono::Utc;

    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let server_path = src.join("index.js");
    let script = include_str!("../../tests/mcp_mock_server.js");
    std::fs::write(&server_path, script).unwrap();

    let server = McpServer {
        id: "mock-echo".to_string(),
        name: "Mock Echo".to_string(),
        source: McpSource::Local {
            path: src.to_string_lossy().to_string(),
        },
        manifest: McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![],
            capabilities: vec!["tools".to_string()],
            config_schema: serde_json::json!({}),
        },
        credentials_key: None,
        installed_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let store = Store::open_in_memory().unwrap();
    let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(manager.install_mcp_server(
        &store,
        &server.id,
        &server.name,
        "local",
        Some(&src.to_string_lossy()),
        &[],
        Some(server.manifest.clone()),
    ))
    .unwrap();

    let listed = manager.list_mcp_servers(&store).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "mock-echo");

    manager
        .discover_and_enable_all(&store, "mock-echo")
        .unwrap();

    let tools = manager.refresh_from_store(&store).unwrap();
    let concrete = tools
        .iter()
        .find(|t| t.name == "mcp_mock_echo_echo")
        .cloned();
    assert!(
        concrete.is_some(),
        "concrete mcp tool should be exposed: got {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let result = manager
        .call_tool(
            "mcp_mock_echo_echo",
            serde_json::json!({ "message": "hello" }),
        )
        .unwrap();
    let text = result
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(text, "echo: hello");

    manager.delete_mcp_server(&store, "mock-echo").unwrap();
    assert!(manager.list_mcp_servers(&store).unwrap().is_empty());
}
