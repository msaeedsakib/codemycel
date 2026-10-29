use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[cfg(feature = "bundle")]
static BUNDLE: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/dist");

#[cfg(not(feature = "bundle"))]
const PLACEHOLDER: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>codemycel</title></head>\n<body><p>This build of codemycel has no web view bundle. Build it with <code>scripts/build-ui.sh</code> and the <code>ui-bundle</code> feature. The API is under <a href=\"/api/status\">/api</a>.</p></body></html>\n";

/// The bundle file at the request path; any other path outside `/api` gets `index.html`, so
/// the front end's routes load on refresh.
pub async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match file(path).or_else(|| file("index.html")) {
        Some((body, mime)) => ([(header::CONTENT_TYPE, mime)], body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(feature = "bundle")]
fn file(path: &str) -> Option<(&'static [u8], &'static str)> {
    let file = BUNDLE.get_file(path)?;
    Some((file.contents(), mime(path)))
}

#[cfg(not(feature = "bundle"))]
fn file(path: &str) -> Option<(&'static [u8], &'static str)> {
    (path == "index.html").then(|| (PLACEHOLDER.as_bytes(), mime(path)))
}

fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}
