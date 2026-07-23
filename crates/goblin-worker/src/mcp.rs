use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct McpListResponse {
    pub servers: Vec<McpSummary>,
}

#[derive(Debug, Serialize)]
pub struct McpSummary {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub capabilities: Vec<String>,
}

pub async fn list_mcp_handler(State(state): State<Arc<AppState>>) -> Json<McpListResponse> {
    let servers = state
        .mcp_servers
        .lock()
        .values()
        .map(|s| McpSummary {
            id: s.id.clone(),
            name: s.name.clone(),
            runtime: format!("{:?}", s.manifest.runtime),
            capabilities: s.manifest.capabilities.clone(),
        })
        .collect();
    Json(McpListResponse { servers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use goble_core::agent::{McpManifest, McpRuntime, McpServer, McpSource};
    use goble_core::worker::WorkerId;

    #[tokio::test]
    async fn test_list_mcp_empty() {
        let state = AppState::new(WorkerId::generate());
        let app = axum::Router::new()
            .route("/mcp", axum::routing::get(list_mcp_handler))
            .with_state(state);
        let req = Request::builder().uri("/mcp").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn test_list_mcp_with_server() {
        let state = AppState::new(WorkerId::generate());
        state.store_mcp(McpServer {
            id: "echo".to_string(),
            name: "Echo".to_string(),
            source: McpSource::Local {
                path: "/tmp".to_string(),
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "index.js".to_string(),
                runtime: McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec!["echo".to_string()],
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        let app = axum::Router::new()
            .route("/mcp", axum::routing::get(list_mcp_handler))
            .with_state(state);
        let req = Request::builder().uri("/mcp").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), 200);
    }
}
