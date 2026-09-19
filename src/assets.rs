//! Embedded dashboard assets (rust-embed). The Svelte/React dashboard build is
//! copied into `dashboard/dist` before compiling so it ships in the binary.

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/dashboard/dist"]
struct Assets;

/// Serve an embedded asset, falling back to `index.html` for SPA routes.
pub async fn serve(uri: Uri) -> Response {
    // The dashboard is built with base '/admin/', so browser asset requests look
    // like '/admin/assets/...'. Strip that prefix to match the embedded folder.
    let raw = uri.path().trim_start_matches('/');
    let path = raw.strip_prefix("admin/").unwrap_or(raw);
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::CACHE_CONTROL, cache_control(path))
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }

    // SPA fallback.
    match Assets::get("index.html") {
        Some(content) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(content.data.into_owned()))
            .unwrap(),
        None => (StatusCode::NOT_FOUND, "dashboard not built").into_response(),
    }
}

fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}
