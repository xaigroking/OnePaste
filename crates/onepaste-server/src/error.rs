//! API errors: a stable code, an HTTP status and optional structured fields. No natural-language text.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub extra: Map<String, Value>,
    pub retry_after: Option<u64>,
}

/// Attached to an error response so the middleware can write the final body, which needs the request id.
#[derive(Clone, Debug)]
pub struct ErrParts {
    pub code: &'static str,
    pub extra: Map<String, Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str) -> ApiError {
        ApiError { status, code, extra: Map::new(), retry_after: None }
    }

    pub fn with(mut self, key: &str, value: Value) -> ApiError {
        self.extra.insert(key.to_string(), value);
        self
    }

    pub fn invalid() -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_argument")
    }

    pub fn unauthorized() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }

    pub fn registration_invalid() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "registration_invalid")
    }

    pub fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found")
    }

    pub fn too_large() -> ApiError {
        ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large")
    }

    pub fn internal() -> ApiError {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
    }

    pub fn rate_limited(retry_after: u64) -> ApiError {
        let mut e = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
        e.retry_after = Some(retry_after);
        e
    }

    pub fn from_core(e: &onepaste_core::Error) -> ApiError {
        use onepaste_core::Error as E;
        match e {
            E::TooLarge => ApiError::too_large(),
            E::MalformedEnvelope | E::UnsupportedVersion(_) => {
                ApiError::new(StatusCode::BAD_REQUEST, "malformed_envelope")
            }
            _ => ApiError::invalid(),
        }
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> ApiError {
        // The database error text is deliberately dropped: the response and the log carry only the stable code.
        tracing::error!(kind = "sqlite", "database error");
        ApiError::internal()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut resp = Response::new(axum::body::Body::empty());
        *resp.status_mut() = self.status;
        if let Some(secs) = self.retry_after {
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                resp.headers_mut().insert(header::RETRY_AFTER, v);
            }
        }
        resp.extensions_mut().insert(ErrParts { code: self.code, extra: self.extra });
        resp
    }
}
