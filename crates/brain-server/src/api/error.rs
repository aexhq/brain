use super::*;

#[derive(Debug)]
pub(super) struct Failure {
    pub(super) status: StatusCode,
    pub(super) code: ApiErrorCode,
    pub(super) message: String,
    pub(super) request_id: String,
}

// Keep the compact constructor used by handlers while storing the request id in the failure at
// creation time. That gives the response and any structured server-side diagnostic one identity.
#[allow(non_snake_case)]
pub(super) fn Failure(status: StatusCode, code: ApiErrorCode, message: String) -> Failure {
    Failure {
        status,
        code,
        message,
        request_id: mint_id("req", 16),
    }
}

pub(super) fn api_code(value: &str) -> ApiErrorCode {
    value.parse().expect("static API error code")
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            error: ApiError {
                code: self.code,
                details: None,
                message: self.message,
                param: None,
                request_id: Some(self.request_id),
            },
        };
        (self.status, Json(body)).into_response()
    }
}

pub(super) fn map_err(e: BrainError) -> Failure {
    use StatusCode as S;
    let (status, code, message, log_internal) = match &e {
        BrainError::PrefixSealed { .. } => (
            S::CONFLICT,
            "configuration_sealed",
            "session configuration is immutable",
            false,
        ),
        BrainError::NoSuchSession(_) | BrainError::FileNotFound(_) => {
            (S::NOT_FOUND, "not_found", "resource not found", false)
        }
        BrainError::TurnInFlight(_) => (
            S::CONFLICT,
            "session_busy",
            "the session already has a running turn",
            false,
        ),
        BrainError::IdempotencyConflict => (
            S::CONFLICT,
            "conflict",
            "the idempotency key was used for a different request",
            false,
        ),
        BrainError::SessionDeleted(_) => (
            S::GONE,
            "session_deleted",
            "the session has been deleted",
            false,
        ),
        BrainError::SessionFailed(_) => (
            S::CONFLICT,
            "session_failed",
            "the session is failed",
            false,
        ),
        BrainError::Invalid(_) => (
            S::BAD_REQUEST,
            "invalid_request",
            "the request is invalid",
            false,
        ),
        BrainError::FileTooLarge { .. } => (
            S::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "the file payload exceeds the route limit",
            false,
        ),
        BrainError::StorageObjectTooLarge { .. } => (
            S::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "the storage object exceeds the session limit",
            false,
        ),
        BrainError::SandboxNotMaterialized => (
            S::CONFLICT,
            "sandbox_not_materialized",
            "the environment has not been materialized",
            false,
        ),
        BrainError::SandboxGone => (
            S::GONE,
            "sandbox_gone",
            "the requested sandbox generation is gone",
            false,
        ),
        BrainError::SandboxGenerationConflict => (
            S::CONFLICT,
            "generation_conflict",
            "the sandbox generation does not match",
            false,
        ),
        BrainError::SandboxResourceExhausted => (
            S::TOO_MANY_REQUESTS,
            "resource_exhausted",
            "sandbox capacity is exhausted",
            false,
        ),
        BrainError::StorageQuotaExceeded { .. } | BrainError::TenantStorageQuotaExceeded { .. } => {
            (
                S::PAYLOAD_TOO_LARGE,
                "storage_limit_exceeded",
                "the storage quota would be exceeded",
                false,
            )
        }
        BrainError::StorageUploadInProgress { .. } => (
            S::CONFLICT,
            "storage_upload_in_progress",
            "another storage upload is already in progress",
            false,
        ),
        BrainError::StorageUploadExpired(_) => (
            S::GONE,
            "storage_upload_expired",
            "the storage upload has expired",
            false,
        ),
        BrainError::SandboxTransferUnknown(_) => (
            S::NOT_FOUND,
            "sandbox_transfer_unknown",
            "the sandbox transfer is unknown; prepare a fresh transfer",
            false,
        ),
        BrainError::SandboxTransferExpired(_) => (
            S::GONE,
            "sandbox_transfer_expired",
            "the sandbox transfer expired; inspect the file and prepare a fresh transfer",
            false,
        ),
        BrainError::SandboxTransferAmbiguous => (
            S::CONFLICT,
            "sandbox_transfer_ambiguous",
            "the sandbox transfer outcome is ambiguous; inspect the file before retrying",
            false,
        ),
        BrainError::SessionJournalQuotaExceeded { .. }
        | BrainError::TenantJournalQuotaExceeded { .. }
        | BrainError::TenantRetainedSessionQuotaExceeded { .. } => (
            S::TOO_MANY_REQUESTS,
            "resource_exhausted",
            "the retained-session quota would be exceeded",
            false,
        ),
        BrainError::Overloaded => (
            S::TOO_MANY_REQUESTS,
            "rate_limited",
            "Brain is at capacity; retry with backoff",
            false,
        ),
        BrainError::Draining => (
            S::SERVICE_UNAVAILABLE,
            "draining",
            "this instance is draining for shutdown; retry against a replacement",
            false,
        ),
        BrainError::UndeclaredTool { .. } => (
            S::BAD_REQUEST,
            "invalid_request",
            "the requested Tool is not declared for this session",
            false,
        ),
        BrainError::Tool { .. } => (S::BAD_GATEWAY, "tool_error", "Tool execution failed", false),
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => (
            S::BAD_GATEWAY,
            "provider_error",
            "the upstream provider request failed",
            false,
        ),
        BrainError::EnvironmentUnavailable(_) | BrainError::Environment(_) => (
            S::SERVICE_UNAVAILABLE,
            "environment_unavailable",
            "the execution runtime is unavailable",
            false,
        ),
        BrainError::Agentloop(_) => (
            S::INTERNAL_SERVER_ERROR,
            "agentloop_error",
            "the session's agentloop failed",
            true,
        ),
        BrainError::Journal(_) | BrainError::Fenced | BrainError::Custody(_) => (
            S::INTERNAL_SERVER_ERROR,
            "internal",
            "the request could not be completed",
            true,
        ),
        BrainError::RoundCap { .. } => (
            S::CONFLICT,
            "round_limit_exceeded",
            "the turn exceeded its Tool-round limit",
            false,
        ),
        BrainError::Cancelled => (
            S::CONFLICT,
            "cancelled",
            "the operation was cancelled",
            false,
        ),
        BrainError::Serde(_) => (
            S::BAD_REQUEST,
            "invalid_request",
            "the request body is invalid",
            false,
        ),
    };
    let request_id = mint_id("req", 16);
    if log_internal {
        let error_kind = match &e {
            BrainError::Journal(_) => "journal",
            BrainError::Fenced => "lease_fenced",
            BrainError::Custody(_) => "custody",
            _ => "internal",
        };
        tracing::error!(%request_id, error_kind, "internal Brain request failure");
    }
    Failure {
        status,
        code: api_code(code),
        message: message.into(),
        request_id,
    }
}
