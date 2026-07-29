use axum::{
    body::{Body, Bytes},
    extract::{rejection::BytesRejection, rejection::QueryRejection, Query, Request, State},
    http::{header::AUTHORIZATION, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use constant_time_eq::constant_time_eq;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    config::Gender,
    engine::EngineFailure,
    error::{ApiError, ApiErrorKind},
    AppState,
};

#[derive(Clone, Copy)]
pub struct RequestId(pub Uuid);

pub async fn request_context(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = Uuid::new_v4();
    let path = request.uri().path().to_owned();
    if path != "/" && path != "/healthz" && !authorized(&request, &state.config.api_token) {
        warn!(%request_id, %path, "unauthorized request");
        return ApiError::new(ApiErrorKind::Unauthorized, request_id).into_response();
    }
    request.extensions_mut().insert(RequestId(request_id));
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), value);
    }
    response
}

fn authorized(request: &Request, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    let Some(value) = request.headers().get(AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(token.as_bytes(), expected.as_bytes())
}

#[derive(Deserialize)]
pub struct VoiceQuery {
    language: Option<String>,
    gender: Option<Gender>,
}

pub async fn voices(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    query: Result<Query<VoiceQuery>, QueryRejection>,
) -> Result<Json<Vec<crate::config::Voice>>, ApiError> {
    let Query(query) = query.map_err(|error| {
        ApiError::new(ApiErrorKind::InvalidBody, request_id).detail(error.to_string())
    })?;

    let filtered: Vec<_> = state
        .voices
        .voices
        .iter()
        .filter(|voice| {
            query
                .language
                .as_ref()
                .is_none_or(|language| voice.language.eq_ignore_ascii_case(language))
                && query
                    .gender
                    .as_ref()
                    .is_none_or(|gender| &voice.gender == gender)
        })
        .cloned()
        .collect();

    Ok(Json(filtered))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TtsRequest {
    text: String,
    voice_id: String,
    #[serde(default = "default_one")]
    audio_volume: f64,
    #[serde(default = "default_one")]
    speaking_rate: f64,
    #[serde(default = "default_one")]
    audio_pitch: f64,
    #[serde(default = "default_format")]
    format: String,
}

fn default_one() -> f64 {
    1.0
}

fn default_format() -> String {
    "wav".to_owned()
}

pub async fn tts(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, ApiError> {
    let body = body.map_err(|error| {
        ApiError::new(ApiErrorKind::InvalidBody, request_id).detail(error.to_string())
    })?;
    let request: TtsRequest = serde_json::from_slice(&body).map_err(|error| {
        ApiError::new(ApiErrorKind::InvalidBody, request_id).detail(error.to_string())
    })?;
    validate(&request, request_id)?;
    let voice = state
        .voices
        .find(&request.voice_id)
        .ok_or_else(|| ApiError::new(ApiErrorKind::VoiceNotFound, request_id))?;
    let character_count = request.text.chars().count();
    let text_hash = hex::encode(Sha256::digest(request.text.as_bytes()));
    info!(
        %request_id,
        voice_id = %request.voice_id,
        character_count,
        %text_hash,
        "synthesis started"
    );
    let result = state
        .engine
        .synthesize(
            voice,
            &request.text,
            request.audio_volume,
            request.speaking_rate,
            request.audio_pitch,
        )
        .await;
    let audio = match result {
        Ok(audio) => audio,
        Err(EngineFailure::QueueFull) => {
            return Err(ApiError::new(ApiErrorKind::QueueFull, request_id));
        }
        Err(EngineFailure::Timeout) => {
            warn!(%request_id, "synthesis timed out");
            return Err(ApiError::new(ApiErrorKind::Timeout, request_id));
        }
        Err(EngineFailure::Failed(cause)) => {
            error!(%request_id, error = %cause, "synthesis failed");
            return Err(ApiError::new(ApiErrorKind::Synthesis, request_id));
        }
    };
    info!(%request_id, audio_bytes = audio.len(), "synthesis completed");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "audio/wav")
        .body(Body::from(audio))
        .map_err(|error| {
            error!(%request_id, %error, "failed to build response");
            ApiError::new(ApiErrorKind::Internal, request_id)
        })
}

fn validate(request: &TtsRequest, request_id: Uuid) -> Result<(), ApiError> {
    let characters = request.text.chars().count();
    if !(1..=2000).contains(&characters) {
        return Err(ApiError::new(
            ApiErrorKind::InvalidParameter("text".to_owned()),
            request_id,
        )
        .detail("text must contain between 1 and 2000 characters"));
    }
    validate_range("audio_volume", request.audio_volume, 0.0, 1.0, request_id)?;
    validate_range("speaking_rate", request.speaking_rate, 0.5, 6.0, request_id)?;
    validate_range("audio_pitch", request.audio_pitch, 0.0, 2.0, request_id)?;
    if request.format != "wav" {
        return Err(ApiError::new(
            ApiErrorKind::InvalidParameter("format".to_owned()),
            request_id,
        )
        .detail("format must be wav"));
    }
    Ok(())
}

fn validate_range(
    name: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
    request_id: Uuid,
) -> Result<(), ApiError> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(
            ApiError::new(ApiErrorKind::InvalidParameter(name.to_owned()), request_id)
                .detail(format!("{name} must be between {minimum} and {maximum}")),
        );
    }
    Ok(())
}

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    voices: usize,
    version: &'static str,
}

pub async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: if state.engine.healthy() {
            "ok"
        } else {
            "degraded"
        },
        voices: state.voices.voices.len(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_authentication_when_token_is_empty() {
        let request = Request::new(Body::empty());
        assert!(authorized(&request, ""));
    }

    #[test]
    fn requires_matching_bearer_token_when_configured() {
        let mut request = Request::new(Body::empty());
        assert!(!authorized(&request, "secret"));

        request
            .headers_mut()
            .insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert!(authorized(&request, "secret"));
        assert!(!authorized(&request, "different"));
    }

    #[test]
    fn validates_character_count_and_ranges() {
        let id = Uuid::new_v4();
        let valid = TtsRequest {
            text: "あ".repeat(2000),
            voice_id: "ja".to_owned(),
            audio_volume: 1.0,
            speaking_rate: 1.0,
            audio_pitch: 1.0,
            format: "wav".to_owned(),
        };
        assert!(validate(&valid, id).is_ok());
        let invalid = TtsRequest {
            speaking_rate: 99.0,
            ..valid
        };
        assert!(matches!(
            validate(&invalid, id),
            Err(ApiError {
                kind: ApiErrorKind::InvalidParameter(ref param),
                ..
            }) if param == "speaking_rate"
        ));
    }
}
