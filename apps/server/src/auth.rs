use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

/// Bearer-token gate. No-op only when no token is configured (the standalone
/// binary on loopback or with `--insecure-no-token`). `/health` is always
/// allowed so liveness checks work without credentials.
pub async fn require_bearer(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.token.as_deref() else {
        return Ok(next.run(request).await);
    };

    // The web client page and liveness check load without a token; the page's
    // own API calls still carry the bearer token. The terminal WebSocket upgrade
    // is also exempt here because a browser WebSocket can't send an Authorization
    // header — `terminal_attach` authenticates its `?token=` query param itself.
    let path = request.uri().path();
    if path == "/" || path == "/health" || is_terminal_ws_path(path) {
        return Ok(next.run(request).await);
    }

    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// `/remote/terminals/{id}/ws` — the terminal WebSocket upgrade. Exempt from the
/// header gate (browser WS can't send `Authorization`); `terminal_attach` checks
/// the `?token=` query param instead.
fn is_terminal_ws_path(path: &str) -> bool {
    path.starts_with("/remote/terminals/") && path.ends_with("/ws")
}

/// Origins that may open a terminal WebSocket: the Tauri app webviews (macOS
/// `tauri://localhost`; Windows/Android `http(s)://tauri.localhost`).
const APP_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Vite dev servers for the desktop (1420) and mobile (1430) apps. Only honoured
/// in debug builds.
const DEV_ORIGINS: &[&str] = &[
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "http://localhost:1430",
    "http://127.0.0.1:1430",
];

/// Whether a terminal WebSocket upgrade may proceed given its `Origin` header.
///
/// Browsers apply no CORS to WebSockets, so any page the user visits could
/// otherwise script a connection to a local listener. The token already stops
/// that; this is defence in depth. Native clients send no `Origin` and pass.
/// Otherwise the origin must be an app webview, a dev server (`allow_dev`), or
/// the server's own `/` page (origin authority == `Host`).
pub(crate) fn ws_origin_allowed(origin: Option<&str>, host: Option<&str>, allow_dev: bool) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim_end_matches('/').to_ascii_lowercase();
    if APP_ORIGINS.contains(&origin.as_str())
        || (allow_dev && DEV_ORIGINS.contains(&origin.as_str()))
    {
        return true;
    }
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    match (authority, host) {
        (Some(authority), Some(host)) => authority == host.to_ascii_lowercase(),
        _ => false,
    }
}

/// Length-independent constant-time comparison to avoid leaking the token via
/// timing.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, ws_origin_allowed};

    #[test]
    fn native_clients_without_an_origin_may_attach() {
        assert!(ws_origin_allowed(None, Some("127.0.0.1:4317"), false));
    }

    #[test]
    fn app_webview_origins_may_attach() {
        for origin in [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
        ] {
            assert!(
                ws_origin_allowed(Some(origin), Some("127.0.0.1:4317"), false),
                "{origin}"
            );
        }
    }

    #[test]
    fn the_servers_own_page_may_attach() {
        assert!(ws_origin_allowed(
            Some("http://100.64.0.1:4317"),
            Some("100.64.0.1:4317"),
            false
        ));
        assert!(ws_origin_allowed(
            Some("http://Box:4317/"),
            Some("box:4317"),
            false
        ));
    }

    #[test]
    fn foreign_pages_are_rejected() {
        let host = Some("127.0.0.1:4317");
        assert!(!ws_origin_allowed(Some("https://evil.example"), host, true));
        assert!(!ws_origin_allowed(
            Some("http://127.0.0.1:8080"),
            host,
            true
        ));
        assert!(!ws_origin_allowed(Some("null"), host, true));
        assert!(!ws_origin_allowed(
            Some("chrome-extension://abc"),
            host,
            true
        ));
        // A same-origin match needs a Host to compare against.
        assert!(!ws_origin_allowed(
            Some("http://127.0.0.1:4317"),
            None,
            true
        ));
    }

    #[test]
    fn dev_server_origins_only_when_allowed() {
        let host = Some("127.0.0.1:4317");
        assert!(ws_origin_allowed(Some("http://localhost:1420"), host, true));
        assert!(ws_origin_allowed(Some("http://localhost:1430"), host, true));
        assert!(!ws_origin_allowed(
            Some("http://localhost:1420"),
            host,
            false
        ));
    }

    #[test]
    fn equal_only_for_identical_bytes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secres"));
        assert!(!constant_time_eq(b"secret", b"secre")); // length mismatch
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
