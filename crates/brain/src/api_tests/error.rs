use super::map_err;
use crate::BrainError;
use axum::http::StatusCode;

fn serde_error() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("{").unwrap_err()
}

#[test]
fn every_brain_error_has_an_explicit_safe_public_mapping() {
    const SECRET: &str = "dictionary-sentinel-secret";
    let cases = vec![
        (
            BrainError::PrefixSealed {
                digest: SECRET.into(),
                what: "model",
            },
            StatusCode::CONFLICT,
            "configuration_sealed",
        ),
        (
            BrainError::NoSuchSession(SECRET.into()),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            BrainError::TurnInFlight(SECRET.into()),
            StatusCode::CONFLICT,
            "session_busy",
        ),
        (
            BrainError::IdempotencyConflict,
            StatusCode::CONFLICT,
            "conflict",
        ),
        (
            BrainError::SessionDeleted(SECRET.into()),
            StatusCode::GONE,
            "session_deleted",
        ),
        (
            BrainError::SessionFailed(SECRET.into()),
            StatusCode::CONFLICT,
            "session_failed",
        ),
        (
            BrainError::Invalid(SECRET.into()),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            BrainError::FileNotFound(SECRET.into()),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            BrainError::FileTooLarge { limit: 1 },
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        ),
        (
            BrainError::StorageObjectTooLarge { limit: 1 },
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        ),
        (
            BrainError::SandboxNotMaterialized,
            StatusCode::CONFLICT,
            "sandbox_not_materialized",
        ),
        (BrainError::SandboxGone, StatusCode::GONE, "sandbox_gone"),
        (
            BrainError::SandboxGenerationConflict,
            StatusCode::CONFLICT,
            "generation_conflict",
        ),
        (
            BrainError::SandboxResourceExhausted,
            StatusCode::TOO_MANY_REQUESTS,
            "resource_exhausted",
        ),
        (
            BrainError::StorageQuotaExceeded {
                published: 1,
                reserved: 2,
                requested: 3,
                limit: 4,
            },
            StatusCode::PAYLOAD_TOO_LARGE,
            "storage_limit_exceeded",
        ),
        (
            BrainError::TenantStorageQuotaExceeded {
                requested: 1,
                limit: 2,
            },
            StatusCode::PAYLOAD_TOO_LARGE,
            "storage_limit_exceeded",
        ),
        (
            BrainError::SessionJournalQuotaExceeded {
                requested: 1,
                limit: 2,
            },
            StatusCode::TOO_MANY_REQUESTS,
            "resource_exhausted",
        ),
        (
            BrainError::TenantJournalQuotaExceeded {
                requested: 1,
                limit: 2,
            },
            StatusCode::TOO_MANY_REQUESTS,
            "resource_exhausted",
        ),
        (
            BrainError::TenantRetainedSessionQuotaExceeded { limit: 2 },
            StatusCode::TOO_MANY_REQUESTS,
            "resource_exhausted",
        ),
        (
            BrainError::StorageUploadInProgress {
                transfer_id: SECRET.into(),
            },
            StatusCode::CONFLICT,
            "storage_upload_in_progress",
        ),
        (
            BrainError::StorageUploadExpired(SECRET.into()),
            StatusCode::GONE,
            "storage_upload_expired",
        ),
        (
            BrainError::SandboxTransferUnknown(SECRET.into()),
            StatusCode::NOT_FOUND,
            "sandbox_transfer_unknown",
        ),
        (
            BrainError::SandboxTransferExpired(SECRET.into()),
            StatusCode::GONE,
            "sandbox_transfer_expired",
        ),
        (
            BrainError::SandboxTransferAmbiguous,
            StatusCode::CONFLICT,
            "sandbox_transfer_ambiguous",
        ),
        (
            BrainError::Overloaded,
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
        ),
        (
            BrainError::UndeclaredTool {
                name: SECRET.into(),
            },
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            BrainError::Tool {
                name: SECRET.into(),
                source: Box::new(std::io::Error::other(SECRET)),
            },
            StatusCode::BAD_GATEWAY,
            "tool_error",
        ),
        (
            BrainError::Transport(SECRET.into()),
            StatusCode::BAD_GATEWAY,
            "provider_error",
        ),
        (
            BrainError::Protocol(SECRET.into()),
            StatusCode::BAD_GATEWAY,
            "provider_error",
        ),
        (
            BrainError::ProviderStatus {
                status: 500,
                body: SECRET.into(),
                retry_after_ms: None,
            },
            StatusCode::BAD_GATEWAY,
            "provider_error",
        ),
        (
            BrainError::HandUnavailable(SECRET.into()),
            StatusCode::SERVICE_UNAVAILABLE,
            "hand_unavailable",
        ),
        (
            BrainError::Hand(SECRET.into()),
            StatusCode::SERVICE_UNAVAILABLE,
            "hand_unavailable",
        ),
        (
            BrainError::Journal(SECRET.into()),
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
        (
            BrainError::Fenced,
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
        (
            BrainError::Custody(SECRET.into()),
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
        (
            BrainError::RoundCap { cap: 1 },
            StatusCode::CONFLICT,
            "round_limit_exceeded",
        ),
        (BrainError::Cancelled, StatusCode::CONFLICT, "cancelled"),
        (
            BrainError::Serde(serde_error()),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
    ];

    for (error, expected_status, expected_code) in cases {
        let failure = map_err(error);
        assert_eq!(failure.status, expected_status);
        assert_eq!(failure.code.as_str(), expected_code);
        assert!(!failure.message.contains(SECRET));
        assert!(failure.request_id.starts_with("req_"));
    }
}
