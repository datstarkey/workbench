use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Wraps any `anyhow::Error` so handlers can use `?` on `workbench_core`'s
/// `anyhow::Result` and still return a JSON error body. Defaults to 500; callers
/// that need 400/404 can construct those variants explicitly.
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.into().to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anyhow_errors_map_to_500() {
        let err: ApiError = anyhow::anyhow!("boom").into();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.message, "boom");
    }

    #[test]
    fn bad_request_maps_to_400() {
        let err = ApiError::bad_request("nope");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.message, "nope");
    }

    #[test]
    fn into_response_uses_the_status() {
        let resp = ApiError::bad_request("x").into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
