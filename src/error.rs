use axum::{
    http::{header::CONTENT_TYPE, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ApiErrorKind {
    #[error("authentication is required")]
    Unauthorized,
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("request body is invalid")]
    InvalidBody,
    #[error("voice was not found")]
    VoiceNotFound,
    #[error("synthesis queue is full")]
    QueueFull,
    #[error("synthesis timed out")]
    Timeout,
    #[error("synthesis failed")]
    Synthesis,
    #[error("internal server error")]
    Internal,
}

#[derive(Debug)]
pub struct ApiError {
    pub kind: ApiErrorKind,
    pub request_id: Uuid,
    pub detail: Option<String>,
}

impl ApiError {
    pub fn new(kind: ApiErrorKind, request_id: Uuid) -> Self {
        Self {
            kind,
            request_id,
            detail: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<String>,
    request_id: Uuid,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, param) = match &self.kind {
            ApiErrorKind::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Missing or invalid bearer token".to_owned(),
                None,
            ),
            ApiErrorKind::InvalidParameter(param) => (
                StatusCode::BAD_REQUEST,
                "invalid_parameter",
                self.detail
                    .unwrap_or_else(|| format!("Invalid value for {param}")),
                Some(param.clone()),
            ),
            ApiErrorKind::InvalidBody => (
                StatusCode::BAD_REQUEST,
                "invalid_body",
                self.detail
                    .unwrap_or_else(|| "Request body is not valid JSON".to_owned()),
                None,
            ),
            ApiErrorKind::VoiceNotFound => (
                StatusCode::NOT_FOUND,
                "voice_not_found",
                "Unknown voice_id".to_owned(),
                Some("voice_id".to_owned()),
            ),
            ApiErrorKind::QueueFull => (
                StatusCode::TOO_MANY_REQUESTS,
                "queue_full",
                "Synthesis queue limit exceeded".to_owned(),
                None,
            ),
            ApiErrorKind::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "synthesis_timeout",
                "Synthesis exceeded its time limit".to_owned(),
                None,
            ),
            ApiErrorKind::Synthesis => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "synthesis_failed",
                self.detail
                    .unwrap_or_else(|| "TTS engine failed".to_owned()),
                None,
            ),
            ApiErrorKind::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error".to_owned(),
                None,
            ),
        };
        let mut response = (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code,
                    message,
                    param,
                    request_id: self.request_id,
                },
            }),
        )
            .into_response();
        response.headers_mut().insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(&self.request_id.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("invalid-request-id")),
        );
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        response
    }
}
