//! Static browser client, served from `/app/`.
//!
//! The assets are produced by `scripts/build-web.sh` into `crates/goble-gateway/
//! web/` and are checked in so a source checkout works without the wasm
//! toolchain. Rebuild them after changing `goble-web`. Serving still degrades
//! gracefully: when the directory or its `index.html` is missing, the gateway
//! answers with a self-contained page that explains how to build the client.
//!
//! Every path handed to [`serve_file`] has already passed
//! [`relative_asset_path`], and the resolved file is re-checked against the
//! canonical asset root, so symlinks cannot escape it either.

use std::path::{Path, PathBuf};

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::web;

/// Entry point served for `/app/`.
const INDEX_FILE: &str = "index.html";

/// wasm-bindgen writes stable filenames (`goble_web_bg.wasm`), so a long cache
/// would pin an old client after a rebuild. Revalidate every asset instead.
const NO_CACHE: &str = "no-cache";

/// Resolve the client asset directory.
///
/// Order: the explicit `--web-dir`/`GOBLE_WEB_DIR` value, then `web/` next to
/// the running executable, then the crate directory recorded at build time. An
/// explicit path is returned even when it does not exist, so a mistyped
/// `--web-dir` shows the build page instead of silently serving another tree.
pub(crate) fn resolve_web_dir(configured: Option<&Path>) -> PathBuf {
    if let Some(dir) = configured {
        return dir.to_path_buf();
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(candidate) = executable.parent().map(|dir| dir.join("web")) {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/web"))
}

/// Serve `/app/`, the single-page client entry point.
pub(crate) async fn serve_index(root: &Path) -> Response {
    serve_file(root, Path::new(INDEX_FILE)).await
}

/// Serve one file below `root`.
pub(crate) async fn serve_file(root: &Path, relative: &Path) -> Response {
    let Some(canonical_root) = ready_root(root).await else {
        return unavailable(root);
    };

    let resolved = match tokio::fs::canonicalize(canonical_root.join(relative)).await {
        Ok(path) => path,
        Err(_) => return not_found(),
    };
    if !resolved.starts_with(&canonical_root) {
        return not_found();
    }

    let bytes = match tokio::fs::read(&resolved).await {
        Ok(bytes) => bytes,
        // A directory has no body and is never listed.
        Err(_) => return not_found(),
    };

    let content_type = content_type(relative);
    let mut response = (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, NO_CACHE),
        ],
        bytes,
    )
        .into_response();
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// Canonical asset root, but only when the client has actually been built.
async fn ready_root(root: &Path) -> Option<PathBuf> {
    let canonical = tokio::fs::canonicalize(root).await.ok()?;
    if !tokio::fs::metadata(&canonical).await.ok()?.is_dir() {
        return None;
    }
    if !tokio::fs::metadata(canonical.join(INDEX_FILE))
        .await
        .ok()?
        .is_file()
    {
        return None;
    }
    Some(canonical)
}

/// Validate a wildcard capture as a relative asset path.
///
/// Rejects absolute paths, empty segments, `.`/`..`, NUL, Windows separators and
/// drive prefixes. Anything returned here is still confined by the canonical
/// root check in [`serve_file`].
pub(crate) fn relative_asset_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.starts_with('/') || raw.contains('\0') || raw.contains('\\') {
        return None;
    }

    let mut path = PathBuf::new();
    for segment in raw.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return None;
        }
        path.push(segment);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

pub(crate) fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Unknown file, or a request that tried to leave the asset root.
pub(crate) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        "not found\n",
    )
        .into_response()
}

fn unavailable(root: &Path) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        web::build_page(&root.display().to_string()),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_shapes_are_rejected() {
        for raw in [
            "../Cargo.toml",
            "pkg/../../Cargo.toml",
            "./index.html",
            "/etc/passwd",
            "pkg//app.js",
            "..",
            "",
            "C:/Windows/system32",
            "pkg\\app.js",
        ] {
            assert_eq!(relative_asset_path(raw), None, "{raw:?} must be rejected");
        }
        assert_eq!(
            relative_asset_path("pkg/app_bg.wasm"),
            Some(PathBuf::from("pkg/app_bg.wasm"))
        );
    }

    #[test]
    fn nul_bytes_are_rejected() {
        assert_eq!(relative_asset_path("index.html\0.png"), None);
    }

    #[test]
    fn content_types_cover_the_client_bundle() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("app_bg.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("app.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("app.json")), "application/json");
        assert_eq!(content_type(Path::new("icon.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("icon.png")), "image/png");
        assert_eq!(content_type(Path::new("favicon.ico")), "image/x-icon");
        assert_eq!(content_type(Path::new("app.js.map")), "application/json");
        assert_eq!(
            content_type(Path::new("robots.txt")),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("blob.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn explicit_web_dir_wins_over_the_builtin_fallback() {
        assert_eq!(
            resolve_web_dir(Some(Path::new("/tmp/does-not-exist"))),
            PathBuf::from("/tmp/does-not-exist")
        );
        assert!(resolve_web_dir(None).ends_with("web"));
    }
}
