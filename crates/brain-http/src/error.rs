use axum::{Json, http::StatusCode, response::{IntoResponse, Response}};
use brain_protocol::ApiError;

pub struct HttpError(pub ApiError);

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "not_found" => StatusCode::NOT_FOUND,
            "conflict" => StatusCode::CONFLICT,
            "overloaded" => StatusCode::SERVICE_UNAVAILABLE,
            "invalid_request" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self.0)).into_response()
    }
}
