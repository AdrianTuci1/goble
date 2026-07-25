use std::collections::HashMap;

use chrono::Utc;
use goble_core::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use goble_core::mcp_installer::McpInstaller;

#[tokio::test]
async fn test_install_and_connect_local_mcp_mock() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let server_path = src.join("index.js");
    std::fs::write(
        &server_path,
        include_str!("mcp_mock_server.js"),
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
        installed_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let installer = McpInstaller::new(tmp.path().join("cache"));
    let installed = installer.install(&server).await.expect("install");
    assert!(installer.is_installed("mock-echo"));

    let client = installed
        .start_client(HashMap::new())
        .expect("start client");
    client.initialize().expect("initialize");
    let tools = client.list_tools().expect("list tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    let result = client
        .call_tool("echo", serde_json::json!({ "message": "hello mcp" }))
        .expect("call tool");
    let text = result
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(text, "echo: hello mcp");
}
