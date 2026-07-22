use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub pairing_code_hash: String,
}

#[derive(Debug, Serialize)]
pub struct PairResponse {
    pub worker_id: String,
    pub paired: bool,
}

pub async fn pair_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<PairRequest>,
) -> impl IntoResponse {
    state.set_pairing_hash(req.pairing_code_hash);
    let response = PairResponse {
        worker_id: state.worker_id.to_string(),
        paired: true,
    };
    (StatusCode::OK, Json(response))
}

pub async fn status_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let response = PairResponse {
        worker_id: state.worker_id.to_string(),
        paired: state.pairing_hash.lock().is_some(),
    };
    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        axum::Router::new()
            .route("/pair", axum::routing::post(pair_handler))
            .route("/status", axum::routing::get(status_handler))
            .layer(axum::Extension(AppState::new(
                goble_core::worker::WorkerId::generate(),
            )))
    }

    #[tokio::test]
    async fn test_pair_and_status() {
        let app = app();
        let req = Request::builder()
            .uri("/pair")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"pairing_code_hash":"hash123"}"#))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/status")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
