use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

use crate::auth::verify_pin;
use crate::state::{GatewayState, SESSION_COOKIE};
use crate::web;

#[derive(Deserialize)]
struct LoginForm {
    pin: String,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    workers: usize,
}

pub(crate) fn router(state: Arc<GatewayState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/", get(index))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/panel", get(panel))
        .route("/ws/worker", get(crate::ws::worker_ws))
        .route("/ws/client", get(crate::ws::client_ws))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, host_guard))
        .layer(TraceLayer::new_for_http())
}

/// Reject requests whose Host header is neither loopback nor an explicitly
/// allowed name. This is the cheap half of DNS-rebinding defence; it does not
/// replace TLS or the PIN.
async fn host_guard(
    State(state): State<Arc<GatewayState>>,
    request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.to_string())
        });

    let allowed = match host {
        Some(host) => state.host_allowed(&host),
        // HTTP/2 carries the authority in the URI; a request with neither is
        // local tooling rather than a browser, which always sends one.
        None => true,
    };
    if allowed {
        next.run(request).await
    } else {
        (StatusCode::BAD_REQUEST, "unrecognized Host header\n").into_response()
    }
}

async fn health(State(state): State<Arc<GatewayState>>) -> impl IntoResponse {
    axum::Json(Health {
        status: "ok",
        workers: state.worker_count(),
    })
}

async fn index(State(state): State<Arc<GatewayState>>, headers: HeaderMap) -> Response {
    if authenticated(&state, &headers) {
        return Redirect::to("/panel").into_response();
    }
    Html(web::login_page(None)).into_response()
}

async fn login(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let Some(pin_hash) = state.config().pin_hash else {
        // Auth is disabled; there is nothing to verify.
        return Redirect::to("/panel").into_response();
    };

    let delay = state.login_delay();
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }

    if !verify_pin(&pin_hash, form.pin.trim()) {
        state.record_login_failure();
        return (
            StatusCode::UNAUTHORIZED,
            Html(web::login_page(Some("Wrong PIN."))),
        )
            .into_response();
    }

    state.record_login_success();
    let token = state.create_session();
    let cookie = session_cookie(
        &token,
        state.config().session_ttl,
        secure_transport(&state, &headers),
    );

    let mut response = Redirect::to("/panel").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn logout(State(state): State<Arc<GatewayState>>, headers: HeaderMap) -> Response {
    if let Some(token) = state.session_token(&headers) {
        state.destroy_session(&token);
    }

    let mut cookie = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    if secure_transport(&state, &headers) {
        cookie.push_str("; Secure");
    }

    let mut response = Redirect::to("/").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn panel(State(state): State<Arc<GatewayState>>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers) {
        return Redirect::to("/").into_response();
    }
    Html(web::panel_page(
        &state.workers(),
        state.config().public_url.as_deref(),
    ))
    .into_response()
}

fn authenticated(state: &GatewayState, headers: &HeaderMap) -> bool {
    if !state.requires_auth() {
        return true;
    }
    state
        .session_token(headers)
        .map(|token| state.session_valid(&token))
        .unwrap_or(false)
}

fn secure_transport(state: &GatewayState, headers: &HeaderMap) -> bool {
    let configured = state
        .config()
        .public_url
        .as_deref()
        .map(|url| url.starts_with("https://"))
        .unwrap_or(false);
    let forwarded = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|proto| proto.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    configured || forwarded
}

fn session_cookie(token: &str, ttl: Duration, secure: bool) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        ttl.as_secs()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}
