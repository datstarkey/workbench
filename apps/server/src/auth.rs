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

    if request.uri().path() == "/health" {
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

/// Length-independent constant-time comparison to avoid leaking the token via
/// timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
