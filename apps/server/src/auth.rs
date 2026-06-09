use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

/// Optional bearer-token gate. No-op when no token is configured. `/health` is
/// always allowed so liveness checks work without credentials.
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
    use super::constant_time_eq;

    #[test]
    fn equal_only_for_identical_bytes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secres"));
        assert!(!constant_time_eq(b"secret", b"secre")); // length mismatch
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
