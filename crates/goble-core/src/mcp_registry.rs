use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::{AuthField, McpManifest, McpServer, McpSource};

/// Registry of known MCP servers. Desktop owns this and pushes to workers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpRegistry {
    entries: HashMap<String, McpServer>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed with a few well-known servers so natural language can map to them.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        registry.register(McpServer {
            id: "mcp-postgres".to_string(),
            name: "PostgreSQL".to_string(),
            source: McpSource::Npm {
                package: "@modelcontextprotocol/server-postgres".to_string(),
                version: "latest".to_string(),
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "dist/index.js".to_string(),
                runtime: crate::agent::McpRuntime::Binary {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@modelcontextprotocol/server-postgres".to_string(),
                    ],
                },
                auth_schema: vec![AuthField {
                    name: "database_url".to_string(),
                    label: "Database URL".to_string(),
                    field_type: crate::agent::AuthFieldType::Url,
                    required: true,
                    description: Some("postgres://user:pass@host:port/db".to_string()),
                }],
                capabilities: vec!["query".to_string(), "schema".to_string()],
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });

        registry.register(McpServer {
            id: "mcp-filesystem".to_string(),
            name: "Filesystem".to_string(),
            source: McpSource::Npm {
                package: "@modelcontextprotocol/server-filesystem".to_string(),
                version: "latest".to_string(),
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "dist/index.js".to_string(),
                runtime: crate::agent::McpRuntime::Binary {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@modelcontextprotocol/server-filesystem".to_string(),
                    ],
                },
                auth_schema: vec![AuthField {
                    name: "allowed_paths".to_string(),
                    label: "Allowed Paths".to_string(),
                    field_type: crate::agent::AuthFieldType::Text,
                    required: true,
                    description: Some("comma-separated paths".to_string()),
                }],
                capabilities: vec!["read".to_string(), "write".to_string()],
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });

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
        assert_eq!(field.name, "database_url");
        assert!(field.required);
    }

    #[test]
    fn test_mcp_registry_serialization() {
        let registry = McpRegistry::builtin();
        let json = serde_json::to_string(&registry).unwrap();
        let decoded: McpRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.list().len(), registry.list().len());
    }
}
