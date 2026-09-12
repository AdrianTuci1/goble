use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use crate::llm::ToolDefinition;
use crate::mcp_client::McpTool;
use crate::secret::Secret;
use anyhow::{Context, Result};

pub(super) fn build_server_from_user_input(
    id: &str,
    name: &str,
    source: &str,
    source_value: Option<&str>,
    credentials: Vec<Secret>,
    manifest: Option<McpManifest>,
) -> Result<McpServer> {
    let mcp_source = match source {
        "npm" => McpSource::Npm {
            package: source_value
                .context("npm source requires package name")?
                .to_string(),
            version: "latest".to_string(),
        },
        "github" => {
            let parts = source_value.context("github source requires owner/repo")?;
            let (repo, rev) = parts
                .split_once('#')
                .map(|(r, v)| (r, v))
                .unwrap_or((parts, "main"));
            McpSource::Github {
                repo: repo.to_string(),
                rev: rev.to_string(),
            }
        }
        "local" => McpSource::Local {
            path: source_value
                .context("local source requires path")?
                .to_string(),
        },
        "url" => McpSource::Url {
            url: source_value.context("url source requires url")?.to_string(),
        },
        "stdio" => McpSource::Npm {
            package: source_value.unwrap_or(id).to_string(),
            version: "latest".to_string(),
        },
        _ => anyhow::bail!("unknown source {source}"),
    };

    let manifest = manifest.unwrap_or_else(|| McpManifest {
        schema_version: "1".to_string(),
        entrypoint: "dist/index.js".to_string(),
        runtime: McpRuntime::Binary {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), source_value.unwrap_or(id).to_string()],
        },
        auth_schema: vec![],
        capabilities: vec!["tools".to_string()],
        config_schema: serde_json::json!({}),
    });

    let credentials_key = if credentials.is_empty() {
        None
    } else {
        Some(uuid::Uuid::new_v4().to_string())
    };

    let now = chrono::Utc::now();
    Ok(McpServer {
        id: id.to_string(),
        name: name.to_string(),
        source: mcp_source,
        manifest,
        credentials_key,
        installed_at: now,
        updated_at: now,
    })
}

pub(super) fn tool_definition_from_mcp_tool(
    server_id: &str,
    full_name: &str,
    tool: &McpTool,
) -> ToolDefinition {
    let description = tool.description.clone().unwrap_or_else(|| {
        format!(
            "MCP tool {tool_name} from server {server_id}",
            tool_name = tool.name
        )
    });
    let parameters = match &tool.input_schema {
        crate::mcp_client::McpToolInputSchema::Object {
            properties,
            required,
            ..
        } => {
            let mut schema = serde_json::json!({
                "type": "object",
                "properties": properties.clone().unwrap_or_default(),
            });
            if let Some(req) = required {
                schema["required"] = serde_json::json!(req.clone());
            }
            schema
        }
        crate::mcp_client::McpToolInputSchema::Other(v) => v.clone(),
    };
    ToolDefinition {
        name: full_name.to_string(),
        description,
        parameters,
    }
}
