use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use brain_protocol::{ApiError, codes::api};

pub struct HttpError(pub ApiError);

/// The HTTP status each API error code is answered with. Every code in the catalogue
/// has a row; an unknown code is a server bug and is answered as one.
pub fn status_for(code: &str) -> StatusCode {
    match code {
        api::INVALID_REQUEST => StatusCode::BAD_REQUEST,
        api::UNAUTHORIZED => StatusCode::UNAUTHORIZED,
        api::NOT_FOUND => StatusCode::NOT_FOUND,
        api::CONFLICT => StatusCode::CONFLICT,
        api::OVERLOADED => StatusCode::SERVICE_UNAVAILABLE,
        api::AMBIGUOUS | api::EXECUTOR_FAILED | api::MODEL_PROVIDER_FAILED | api::INTERNAL => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
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
