use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::{AuthField, McpManifest, McpServer, McpSource};

/// Registry of known MCP servers. Desktop owns this and pushes to workers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpRegistry {
    entries: HashMap<String, McpServer>,
}

/// Search result returned to the LLM for a matching MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSearchResult {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub auth_required: bool,
    pub source_kind: String,
}



#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpRegistryEntry {
    id: String,
    name: String,
    #[serde(rename = "package")]
    package: String,
    version: String,
    description: String,
    category: String,
    auth: Option<McpRegistryAuth>,
    command: Vec<String>,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpRegistryAuth {
    env: String,
    label: String,
}

fn auth_field_type_for_env(env: &str) -> crate::agent::AuthFieldType {
    let lower = env.to_lowercase();
    if lower.contains("url") {
        crate::agent::AuthFieldType::Url
    } else if lower.contains("token") || lower.contains("key") {
        crate::agent::AuthFieldType::Token
    } else if lower.contains("password") || lower.contains("secret") {
        crate::agent::AuthFieldType::Password
    } else {
        crate::agent::AuthFieldType::Text
    }
}

impl From<McpRegistryEntry> for McpServer {
    fn from(entry: McpRegistryEntry) -> Self {
        let auth_schema = entry.auth.map(|a| {
            vec![AuthField {
                name: a.env.clone(),
                label: a.label,
                field_type: auth_field_type_for_env(&a.env),
                required: true,
                description: Some(format!("Set {} environment variable", a.env)),
            }]
        }).unwrap_or_default();
        let (command, args) = if let Some(first) = entry.command.first() {
            (first.clone(), entry.command.iter().skip(1).cloned().collect())
        } else {
            ("npx".to_string(), vec!["-y".to_string(), entry.package.clone()])
        };
        McpServer {
            id: format!("mcp-{}", entry.id),
            name: entry.name,
            source: McpSource::Npm {
                package: entry.package,
                version: entry.version,
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "dist/index.js".to_string(),
                runtime: crate::agent::McpRuntime::Binary { command, args },
                auth_schema,
                capabilities: entry.capabilities,
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed with the real MCP registry bundled as JSON.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        let data = include_str!("../assets/mcp_registry.json");
        let entries: Vec<McpRegistryEntry> =
            serde_json::from_str(data).unwrap_or_default();
        for entry in entries {
            registry.register(entry.into());
        }
        registry
    }

    pub fn register(&mut self, server: McpServer) -> Option<McpServer> {
        self.entries.insert(server.id.clone(), server)
    }

    pub fn get(&self, id: &str) -> Option<&McpServer> {
        self.entries.get(id)
    }

    pub fn search(&self, query: &str) -> Vec<&McpServer> {
        let q = query.to_lowercase();
        self.entries
            .values()
            .filter(|s| {
                s.name.to_lowercase().contains(&q)
                    || s.id.to_lowercase().contains(&q)
                    || s.manifest
                        .capabilities
                        .iter()
                        .any(|c| c.to_lowercase().contains(&q))
            })
            .collect()
    }

    pub fn list(&self) -> Vec<&McpServer> {
        self.entries.values().collect()
    }

    pub fn remove(&mut self, id: &str) -> Option<McpServer> {
        self.entries.remove(id)
    }

    /// Resolve a natural-language request to the best matching MCP server.
    pub fn resolve(&self, request: &str) -> Option<&McpServer> {
        self.search(request).into_iter().next()
    }

    /// Build an McpServer from a registry template and user-provided credentials.
    pub fn instantiate(
        &self,
        id: &str,
        credentials: Vec<crate::secret::Secret>,
    ) -> Option<McpServer> {
        let mut server = self.entries.get(id)?.clone();
        if !credentials.is_empty() {
            let key = uuid::Uuid::new_v4().to_string();
            server.credentials_key = Some(key);
        }
        server.updated_at = chrono::Utc::now();
        Some(server)
    }

    /// Search both the local registry and optionally the web for matching MCP servers.
    /// Web search is best-effort and returns npm-based MCP packages that match the query.
    pub async fn search_mcp_servers(&self, query: &str) -> Vec<McpSearchResult> {
        let mut results: Vec<McpSearchResult> = self
            .search(query)
            .into_iter()
            .map(|s| McpSearchResult {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s
                    .manifest
                    .auth_schema
                    .first()
                    .and_then(|f| f.description.clone())
                    .unwrap_or_else(|| {
                        format!(
                            "MCP server with capabilities: {:?}",
                            s.manifest.capabilities
                        )
                    }),
                capabilities: s.manifest.capabilities.clone(),
                auth_required: !s.manifest.auth_schema.is_empty(),
                source_kind: match &s.source {
                    McpSource::Npm { .. } => "npm".to_string(),
                    McpSource::Github { .. } => "github".to_string(),
                    McpSource::Local { .. } => "local".to_string(),
                    McpSource::Url { .. } => "url".to_string(),
                },
            })
            .collect();

        // Web search is optional; on failure we return only builtin results.
        if let Ok(web) = web_search_mcp_packages(query).await {
            for entry in web {
                if results.iter().any(|r| r.id == entry.id) {
                    continue;
                }
                results.push(entry);
            }
        }

        results
    }
}

/// Search npm and GitHub for public MCP packages matching the query.
/// This is intentionally lightweight and does not require authentication.
async fn web_search_mcp_packages(query: &str) -> anyhow::Result<Vec<McpSearchResult>> {
    let mut results = Vec::new();
    let npm_url = format!(
        "https://registry.npmjs.org/-/v1/search?text={}+mcp&size=10",
        urlencoding::encode(query)
    );
    let resp = reqwest::get(&npm_url).await?;
    if resp.status().is_success() {
        let body = resp.json::<serde_json::Value>().await?;
        if let Some(objects) = body.get("objects").and_then(|v| v.as_array()) {
            for obj in objects {
                let package = obj
                    .get("package")
                    .and_then(|p| p.as_object())
                    .cloned()
                    .unwrap_or_default();
                let name = package
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if !name.contains("mcp") && !name.contains("modelcontextprotocol") {
                    continue;
                }
                let id = name.replace('/', "-").replace('@', "");
                let description = package
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Public MCP server from npm")
                    .to_string();
                let keywords = package
                    .get("keywords")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                    .unwrap_or_default();
                let capabilities = infer_capabilities(&description, &keywords);
                let auth_required = likely_requires_auth(&name, &description);
                results.push(McpSearchResult {
                    id,
                    name: name.clone(),
                    description,
                    capabilities,
                    auth_required,
                    source_kind: "npm".to_string(),
                });
            }
        }
    }

    // GitHub search is a fallback for source repos; we don't parse it deeply.
    let github_url = format!(
        "https://api.github.com/search/repositories?q={}+mcp&per_page=5",
        urlencoding::encode(query)
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&github_url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "goble-mcp-search")
        .send()
        .await;
    if let Ok(resp) = resp {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
                    for item in items {
                        let name = item
                            .get("full_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let id = name.replace('/', "-");
                        let description = item
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Public MCP server from GitHub")
                            .to_string();
                        let auth_required = likely_requires_auth(&name, &description);
                        results.push(McpSearchResult {
                            id,
                            name: name.clone(),
                            description,
                            capabilities: vec!["tools".to_string()],
                            auth_required,
                            source_kind: "github".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

fn infer_capabilities(description: &str, keywords: &[String]) -> Vec<String> {
    let text = format!("{} {}", description, keywords.join(" ")).to_lowercase();
    let mut caps = Vec::new();
    if text.contains("file") { caps.push("filesystem".to_string()); }
    if text.contains("git") || text.contains("github") || text.contains("repository") { caps.push("git".to_string()); }
    if text.contains("database") || text.contains("sql") || text.contains("postgres") || text.contains("sqlite") { caps.push("database".to_string()); }
    if text.contains("browser") || text.contains("puppeteer") || text.contains("playwright") || text.contains("web") { caps.push("browser".to_string()); }
    if text.contains("slack") || text.contains("discord") || text.contains("teams") || text.contains("message") { caps.push("messaging".to_string()); }
    if text.contains("issue") || text.contains("ticket") || text.contains("linear") || text.contains("jira") { caps.push("issue-tracking".to_string()); }
    if caps.is_empty() { caps.push("tools".to_string()); }
    caps
}

fn likely_requires_auth(name: &str, description: &str) -> bool {
    let text = format!("{name} {description}").to_lowercase();
    text.contains("api key")
        || text.contains("api_key")
        || text.contains("token")
        || text.contains("secret")
        || text.contains("password")
        || text.contains("oauth")
        || text.contains("auth")
}
#[cfg(test)]

mod tests {
    use super::*;
    use crate::secret::Secret;

    #[test]
    fn test_builtin_has_postgres() {
        let registry = McpRegistry::builtin();
        assert!(registry.get("mcp-postgres").is_some());
    }

    #[test]
    fn test_search_by_name() {
        let registry = McpRegistry::builtin();
        let hits = registry.search("postgres");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "PostgreSQL");
    }

    #[test]
    fn test_resolve_natural_language() {
        let registry = McpRegistry::builtin();
        let server = registry.resolve("filesystem").unwrap();
        assert_eq!(server.id, "mcp-filesystem");
    }

    #[test]
    fn test_instantiate_credentials_key() {
        let registry = McpRegistry::builtin();
        let server = registry
            .instantiate(
                "mcp-postgres",
                vec![Secret::new(
                    "database_url",
                    "registry",
                    b"postgres://localhost/db".to_vec(),
                )],
            )
            .unwrap();
        assert!(server.credentials_key.is_some());
    }

    #[test]
    fn test_resolve_unknown_returns_none() {
        let registry = McpRegistry::builtin();
        assert!(registry.resolve("unknown-service-xyz").is_none());
    }

    #[test]
    fn test_register_and_remove() {
        let mut registry = McpRegistry::builtin();
        let before = registry.list().len();
        registry.remove("mcp-postgres");
        assert_eq!(registry.list().len(), before - 1);
        assert!(registry.get("mcp-postgres").is_none());
    }

    #[test]
    fn test_auth_field_schema_presence() {
        let registry = McpRegistry::builtin();
        let server = registry.get("mcp-postgres").unwrap();
        assert!(!server.manifest.auth_schema.is_empty());
        let field = &server.manifest.auth_schema[0];
        assert_eq!(field.name, "DATABASE_URL");
        assert!(field.required);
    }

    #[test]
    fn test_mcp_registry_serialization() {
        let registry = McpRegistry::builtin();
        let json = serde_json::to_string(&registry).unwrap();
        let decoded: McpRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.list().len(), registry.list().len());
    }

    #[tokio::test]
    async fn test_search_mcp_servers_returns_builtin() {
        let registry = McpRegistry::builtin();
        let results = registry.search_mcp_servers("filesystem").await;
        assert!(results.iter().any(|r| r.id == "mcp-filesystem"));
    }
}
