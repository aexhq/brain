use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request},
    http::StatusCode,
    http::request::Parts,
    response::{IntoResponse, Response},
};
use brain_protocol::{ApiError, codes::api};
use serde::de::DeserializeOwned;

pub struct HttpError(pub ApiError);

pub(crate) struct ApiJson<T>(pub T);
pub(crate) struct ApiQuery<T>(pub T);
pub(crate) struct ApiPath<T>(pub T);
pub(crate) struct ApiBytes(pub axum::body::Bytes);

fn rejection(status: StatusCode, message: String) -> Response {
    let error = if status.is_server_error() {
        ApiError::internal(message)
    } else {
        ApiError::invalid_request(message)
    };
    (status, Json(error)).into_response()
}

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for ApiJson<T> {
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|error| rejection(error.status(), error.body_text()))
    }
}

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Self(value))
            .map_err(|error| rejection(error.status(), error.body_text()))
    }
}

impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for ApiPath<T> {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Self(value))
            .map_err(|error| rejection(error.status(), error.body_text()))
    }
}

impl<S: Send + Sync> FromRequest<S> for ApiBytes {
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        axum::body::Bytes::from_request(request, state)
            .await
            .map(Self)
            .map_err(|error| rejection(error.status(), error.body_text()))
    }
}

/// The HTTP status each API error code is answered with. Every code in the catalogue
/// has a row; an unknown code is a server bug and is answered as one.
pub fn status_for(code: &str) -> StatusCode {
    match code {
        api::INVALID_REQUEST => StatusCode::BAD_REQUEST,
        api::UNAUTHORIZED => StatusCode::UNAUTHORIZED,
        api::NOT_FOUND => StatusCode::NOT_FOUND,
        api::CONFLICT => StatusCode::CONFLICT,
        api::PREPARATION_FAILED => StatusCode::CONFLICT,
        api::OVERLOADED => StatusCode::SERVICE_UNAVAILABLE,
        api::AMBIGUOUS
        | api::EXECUTOR_FAILED
        | api::MODEL_PROVIDER_FAILED
        | api::MODEL_OUTPUT_INVALID
        | api::MODEL_OUTPUT_INCOMPLETE
        | api::INTERNAL => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (status_for(&self.0.code), Json(self.0)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_facing_codes_do_not_answer_as_server_errors() {
        let server_side = [
            api::AMBIGUOUS,
            api::EXECUTOR_FAILED,
            api::MODEL_PROVIDER_FAILED,
            api::MODEL_OUTPUT_INVALID,
            api::MODEL_OUTPUT_INCOMPLETE,
            api::INTERNAL,
        ];
        for code in api::ALL {
            let status = status_for(code);
            if server_side.contains(code) {
                assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{code}");
            } else {
                assert!(
                    status.is_client_error() || status == StatusCode::SERVICE_UNAVAILABLE,
                    "{code} answers {status}"
                );
            }
        }
        assert_eq!(status_for("unknown"), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
