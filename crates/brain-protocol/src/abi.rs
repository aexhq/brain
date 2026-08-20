#![allow(clippy::redundant_closure_call)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::match_single_binding)]
#![allow(clippy::clone_on_copy)]

#[doc = r" Error types."]
pub mod error {
    #[doc = r" Error from a `TryFrom` or `FromStr` implementation."]
    pub struct ConversionError(::std::borrow::Cow<'static, str>);
    impl ::std::error::Error for ConversionError {}
    impl ::std::fmt::Display for ConversionError {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Display::fmt(&self.0, f)
        }
    }
    impl ::std::fmt::Debug for ConversionError {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Debug::fmt(&self.0, f)
        }
    }
    impl From<&'static str> for ConversionError {
        fn from(value: &'static str) -> Self {
            Self(value.into())
        }
    }
    impl From<String> for ConversionError {
        fn from(value: String) -> Self {
            Self(value.into())
        }
    }
}
#[doc = "`AbiError`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"code\","]
#[doc = "    \"message\","]
#[doc = "    \"retryable\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"code\": {"]
#[doc = "      \"$ref\": \"#/$defs/ErrorCode\""]
#[doc = "    },"]
#[doc = "    \"details\": {"]
#[doc = "      \"description\": \"Code-specific structured detail, e.g. {\\\"schema_path\\\": ...} for tool_input_invalid.\","]
#[doc = "      \"type\": \"object\""]
#[doc = "    },"]
#[doc = "    \"message\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"retryable\": {"]
#[doc = "      \"description\": \"True only when repeating the identical request later is safe and may succeed (e.g. resource_exhausted). Never true for opaque work.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct AbiError {
    pub code: ErrorCode,
    #[doc = "Code-specific structured detail, e.g. {\"schema_path\": ...} for tool_input_invalid."]
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub details: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub message: ::std::string::String,
    #[doc = "True only when repeating the identical request later is safe and may succeed (e.g. resource_exhausted). Never true for opaque work."]
    pub retryable: bool,
}
impl AbiError {
    pub fn builder() -> builder::AbiError {
        Default::default()
    }
}
#[doc = "Wire contract between the brain (LLM harness) and a hand (tool executor in a microVM). One multiplexed WebSocket per hand carries JSON text frames: the brain sends `Request` frames, the hand sends `HandFrame` frames (responses correlated by `id`, plus unsolicited `hand_status` events). Major version must match; unknown fields are ignored. Semantics live in contracts/abi/v1/README.md."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://aex.dev/contracts/abi/v1/abi.json\","]
#[doc = "  \"title\": \"Aex brain-hand ABI v1\","]
#[doc = "  \"description\": \"Wire contract between the brain (LLM harness) and a hand (tool executor in a microVM). One multiplexed WebSocket per hand carries JSON text frames: the brain sends `Request` frames, the hand sends `HandFrame` frames (responses correlated by `id`, plus unsolicited `hand_status` events). Major version must match; unknown fields are ignored. Semantics live in contracts/abi/v1/README.md.\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct AexBrainHandAbiV1(pub ::serde_json::Value);
impl ::std::ops::Deref for AexBrainHandAbiV1 {
    type Target = ::serde_json::Value;
    fn deref(&self) -> &::serde_json::Value {
        &self.0
    }
}
impl ::std::convert::From<AexBrainHandAbiV1> for ::serde_json::Value {
    fn from(value: AexBrainHandAbiV1) -> Self {
        value.0
    }
}
impl ::std::convert::From<::serde_json::Value> for AexBrainHandAbiV1 {
    fn from(value: ::serde_json::Value) -> Self {
        Self(value)
    }
}
#[doc = "Groups the parallel tool calls of one assistant message."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Groups the parallel tool calls of one assistant message.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct BatchId(::std::string::String);
impl ::std::ops::Deref for BatchId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BatchId> for ::std::string::String {
    fn from(value: BatchId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BatchId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for BatchId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BatchId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BatchId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BatchId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Changes on every VM boot, including resume from a released state. Same generation_id + same boot_id on reconnect means in-guest state survived."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Changes on every VM boot, including resume from a released state. Same generation_id + same boot_id on reconnect means in-guest state survived.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct BootId(::std::string::String);
impl ::std::ops::Deref for BootId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BootId> for ::std::string::String {
    fn from(value: BootId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BootId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for BootId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BootId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BootId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BootId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Per-operation limits enforced by the hand. Missing fields take `limits.default_bounds`."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Per-operation limits enforced by the hand. Missing fields take `limits.default_bounds`.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"grace_ms\": {"]
#[doc = "      \"description\": \"SIGTERM -> grace -> SIGKILL.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_retained_bytes\": {"]
#[doc = "      \"description\": \"Per stream. Beyond this the oldest bytes are evicted from the spill file (tail retention).\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"timeout_ms\": {"]
#[doc = "      \"description\": \"Relative deadline on the guest monotonic clock. null = no hand-side timeout.\","]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Bounds {
    #[doc = "SIGTERM -> grace -> SIGKILL."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub grace_ms: ::std::option::Option<u64>,
    #[doc = "Per stream. Beyond this the oldest bytes are evicted from the spill file (tail retention)."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_retained_bytes: ::std::option::Option<u64>,
    #[doc = "Relative deadline on the guest monotonic clock. null = no hand-side timeout."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub timeout_ms: ::std::option::Option<::std::num::NonZeroU64>,
}
impl ::std::default::Default for Bounds {
    fn default() -> Self {
        Self {
            grace_ms: Default::default(),
            max_retained_bytes: Default::default(),
            timeout_ms: Default::default(),
        }
    }
}
impl Bounds {
    pub fn builder() -> builder::Bounds {
        Default::default()
    }
}
#[doc = "SIGTERM to the operation's process group -> grace_ms -> SIGKILL -> terminal(cancelled). Cancelling a terminal operation is not an error (accepted = false)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"SIGTERM to the operation's process group -> grace_ms -> SIGKILL -> terminal(cancelled). Cancelling a terminal operation is not an error (accepted = false).\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"operation_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"grace_ms\": {"]
#[doc = "      \"description\": \"Overrides bounds.grace_ms.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct CancelRequest {
    #[doc = "Overrides bounds.grace_ms."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub grace_ms: ::std::option::Option<u64>,
    pub operation_id: OperationId,
}
impl CancelRequest {
    pub fn builder() -> builder::CancelRequest {
        Default::default()
    }
}
#[doc = "`CancelResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"accepted\","]
#[doc = "    \"view\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"accepted\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"view\": {"]
#[doc = "      \"description\": \"The view at the time of the answer; may still be running during the grace period.\","]
#[doc = "      \"$ref\": \"#/$defs/OperationView\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct CancelResponse {
    pub accepted: bool,
    #[doc = "The view at the time of the answer; may still be running during the grace period."]
    pub view: OperationView,
}
impl CancelResponse {
    pub fn builder() -> builder::CancelResponse {
        Default::default()
    }
}
#[doc = "`Clock`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"monotonic_ms\","]
#[doc = "    \"wall_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"monotonic_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/MonotonicMs\""]
#[doc = "    },"]
#[doc = "    \"wall_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/WallMs\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Clock {
    pub monotonic_ms: MonotonicMs,
    pub wall_ms: WallMs,
}
impl Clock {
    pub fn builder() -> builder::Clock {
        Default::default()
    }
}
#[doc = "`Cursor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"offset\","]
#[doc = "    \"stream\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"offset\": {"]
#[doc = "      \"description\": \"Byte offset into the full stream as produced (not as retained).\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"stream\": {"]
#[doc = "      \"$ref\": \"#/$defs/Stream\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Cursor {
    #[doc = "Byte offset into the full stream as produced (not as retained)."]
    pub offset: u64,
    pub stream: Stream,
}
impl Cursor {
    pub fn builder() -> builder::Cursor {
        Default::default()
    }
}
#[doc = "The complete bounds a hand applies when `start.bounds` omits a field."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The complete bounds a hand applies when `start.bounds` omits a field.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"grace_ms\","]
#[doc = "    \"max_retained_bytes\","]
#[doc = "    \"timeout_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"grace_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_retained_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"timeout_ms\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct EffectiveBounds {
    pub grace_ms: u64,
    pub max_retained_bytes: u64,
    pub timeout_ms: ::std::option::Option<::std::num::NonZeroU64>,
}
impl EffectiveBounds {
    pub fn builder() -> builder::EffectiveBounds {
        Default::default()
    }
}
#[doc = "`ErrorCode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"malformed_request\","]
#[doc = "    \"protocol_unsupported\","]
#[doc = "    \"unauthorized\","]
#[doc = "    \"tool_manifest_mismatch\","]
#[doc = "    \"generation_mismatch\","]
#[doc = "    \"fence_stale\","]
#[doc = "    \"tool_not_found\","]
#[doc = "    \"tool_input_invalid\","]
#[doc = "    \"tool_output_invalid\","]
#[doc = "    \"lane_gone\","]
#[doc = "    \"lane_busy\","]
#[doc = "    \"lane_limit_exceeded\","]
#[doc = "    \"lane_not_closable\","]
#[doc = "    \"operation_not_found\","]
#[doc = "    \"operation_idempotency_conflict\","]
#[doc = "    \"operation_output_evicted\","]
#[doc = "    \"path_not_found\","]
#[doc = "    \"path_outside_scope\","]
#[doc = "    \"checksum_mismatch\","]
#[doc = "    \"transfer_failed\","]
#[doc = "    \"too_large\","]
#[doc = "    \"resource_exhausted\","]
#[doc = "    \"restore_failed\","]
#[doc = "    \"internal\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum ErrorCode {
    #[serde(rename = "malformed_request")]
    MalformedRequest,
    #[serde(rename = "protocol_unsupported")]
    ProtocolUnsupported,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "tool_manifest_mismatch")]
    ToolManifestMismatch,
    #[serde(rename = "generation_mismatch")]
    GenerationMismatch,
    #[serde(rename = "fence_stale")]
    FenceStale,
    #[serde(rename = "tool_not_found")]
    ToolNotFound,
    #[serde(rename = "tool_input_invalid")]
    ToolInputInvalid,
    #[serde(rename = "tool_output_invalid")]
    ToolOutputInvalid,
    #[serde(rename = "lane_gone")]
    LaneGone,
    #[serde(rename = "lane_busy")]
    LaneBusy,
    #[serde(rename = "lane_limit_exceeded")]
    LaneLimitExceeded,
    #[serde(rename = "lane_not_closable")]
    LaneNotClosable,
    #[serde(rename = "operation_not_found")]
    OperationNotFound,
    #[serde(rename = "operation_idempotency_conflict")]
    OperationIdempotencyConflict,
    #[serde(rename = "operation_output_evicted")]
    OperationOutputEvicted,
    #[serde(rename = "path_not_found")]
    PathNotFound,
    #[serde(rename = "path_outside_scope")]
    PathOutsideScope,
    #[serde(rename = "checksum_mismatch")]
    ChecksumMismatch,
    #[serde(rename = "transfer_failed")]
    TransferFailed,
    #[serde(rename = "too_large")]
    TooLarge,
    #[serde(rename = "resource_exhausted")]
    ResourceExhausted,
    #[serde(rename = "restore_failed")]
    RestoreFailed,
    #[serde(rename = "internal")]
    Internal,
}
impl ::std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MalformedRequest => f.write_str("malformed_request"),
            Self::ProtocolUnsupported => f.write_str("protocol_unsupported"),
            Self::Unauthorized => f.write_str("unauthorized"),
            Self::ToolManifestMismatch => f.write_str("tool_manifest_mismatch"),
            Self::GenerationMismatch => f.write_str("generation_mismatch"),
            Self::FenceStale => f.write_str("fence_stale"),
            Self::ToolNotFound => f.write_str("tool_not_found"),
            Self::ToolInputInvalid => f.write_str("tool_input_invalid"),
            Self::ToolOutputInvalid => f.write_str("tool_output_invalid"),
            Self::LaneGone => f.write_str("lane_gone"),
            Self::LaneBusy => f.write_str("lane_busy"),
            Self::LaneLimitExceeded => f.write_str("lane_limit_exceeded"),
            Self::LaneNotClosable => f.write_str("lane_not_closable"),
            Self::OperationNotFound => f.write_str("operation_not_found"),
            Self::OperationIdempotencyConflict => f.write_str("operation_idempotency_conflict"),
            Self::OperationOutputEvicted => f.write_str("operation_output_evicted"),
            Self::PathNotFound => f.write_str("path_not_found"),
            Self::PathOutsideScope => f.write_str("path_outside_scope"),
            Self::ChecksumMismatch => f.write_str("checksum_mismatch"),
            Self::TransferFailed => f.write_str("transfer_failed"),
            Self::TooLarge => f.write_str("too_large"),
            Self::ResourceExhausted => f.write_str("resource_exhausted"),
            Self::RestoreFailed => f.write_str("restore_failed"),
            Self::Internal => f.write_str("internal"),
        }
    }
}
impl ::std::str::FromStr for ErrorCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "malformed_request" => Ok(Self::MalformedRequest),
            "protocol_unsupported" => Ok(Self::ProtocolUnsupported),
            "unauthorized" => Ok(Self::Unauthorized),
            "tool_manifest_mismatch" => Ok(Self::ToolManifestMismatch),
            "generation_mismatch" => Ok(Self::GenerationMismatch),
            "fence_stale" => Ok(Self::FenceStale),
            "tool_not_found" => Ok(Self::ToolNotFound),
            "tool_input_invalid" => Ok(Self::ToolInputInvalid),
            "tool_output_invalid" => Ok(Self::ToolOutputInvalid),
            "lane_gone" => Ok(Self::LaneGone),
            "lane_busy" => Ok(Self::LaneBusy),
            "lane_limit_exceeded" => Ok(Self::LaneLimitExceeded),
            "lane_not_closable" => Ok(Self::LaneNotClosable),
            "operation_not_found" => Ok(Self::OperationNotFound),
            "operation_idempotency_conflict" => Ok(Self::OperationIdempotencyConflict),
            "operation_output_evicted" => Ok(Self::OperationOutputEvicted),
            "path_not_found" => Ok(Self::PathNotFound),
            "path_outside_scope" => Ok(Self::PathOutsideScope),
            "checksum_mismatch" => Ok(Self::ChecksumMismatch),
            "transfer_failed" => Ok(Self::TransferFailed),
            "too_large" => Ok(Self::TooLarge),
            "resource_exhausted" => Ok(Self::ResourceExhausted),
            "restore_failed" => Ok(Self::RestoreFailed),
            "internal" => Ok(Self::Internal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Minted by the hand per incarnation (per boot with fresh state). Lanes, operations and spill files are scoped to a generation."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Minted by the hand per incarnation (per boot with fresh state). Lanes, operations and spill files are scoped to a generation.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct GenerationId(::std::string::String);
impl ::std::ops::Deref for GenerationId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<GenerationId> for ::std::string::String {
    fn from(value: GenerationId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for GenerationId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for GenerationId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GenerationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GenerationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for GenerationId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Hand -> brain frame, tagged by `kind`."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Hand -> brain frame, tagged by `kind`.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"frame\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"frame\": {"]
#[doc = "          \"$ref\": \"#/$defs/Response\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"response\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"frame\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"frame\": {"]
#[doc = "          \"$ref\": \"#/$defs/HandStatusEvent\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"hand_status\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", content = "frame")]
pub enum HandFrame {
    #[serde(rename = "response")]
    Response(Response),
    #[serde(rename = "hand_status")]
    HandStatus(HandStatusEvent),
}
impl ::std::convert::From<Response> for HandFrame {
    fn from(value: Response) -> Self {
        Self::Response(value)
    }
}
impl ::std::convert::From<HandStatusEvent> for HandFrame {
    fn from(value: HandStatusEvent) -> Self {
        Self::HandStatus(value)
    }
}
#[doc = "Hand -> brain, unsolicited: on every idle/busy transition, when a detached job ends, and every heartbeat_ms. The idle signal."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Hand -> brain, unsolicited: on every idle/busy transition, when a detached job ends, and every heartbeat_ms. The idle signal.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"at_monotonic_ms\","]
#[doc = "    \"at_wall_ms\","]
#[doc = "    \"boot_id\","]
#[doc = "    \"generation_id\","]
#[doc = "    \"idle_for_ms\","]
#[doc = "    \"inflight\","]
#[doc = "    \"lanes_live\","]
#[doc = "    \"live_jobs\","]
#[doc = "    \"operations_retained\","]
#[doc = "    \"retained_bytes\","]
#[doc = "    \"seq\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"at_monotonic_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/MonotonicMs\""]
#[doc = "    },"]
#[doc = "    \"at_wall_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/WallMs\""]
#[doc = "    },"]
#[doc = "    \"boot_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/BootId\""]
#[doc = "    },"]
#[doc = "    \"generation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/GenerationId\""]
#[doc = "    },"]
#[doc = "    \"idle_for_ms\": {"]
#[doc = "      \"description\": \"0 while inflight or live_jobs is non-empty.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"inflight\": {"]
#[doc = "      \"description\": \"Attached operations not yet terminal.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"lanes_live\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"live_jobs\": {"]
#[doc = "      \"description\": \"Detached operations not yet terminal.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"operations_retained\": {"]
#[doc = "      \"description\": \"Terminal but not yet released.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"pressure\": {"]
#[doc = "      \"$ref\": \"#/$defs/Pressure\""]
#[doc = "    },"]
#[doc = "    \"retained_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"seq\": {"]
#[doc = "      \"description\": \"Per boot, monotonic.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct HandStatusEvent {
    pub at_monotonic_ms: MonotonicMs,
    pub at_wall_ms: WallMs,
    pub boot_id: BootId,
    pub generation_id: GenerationId,
    #[doc = "0 while inflight or live_jobs is non-empty."]
    pub idle_for_ms: u64,
    #[doc = "Attached operations not yet terminal."]
    pub inflight: ::std::vec::Vec<OperationId>,
    pub lanes_live: u64,
    #[doc = "Detached operations not yet terminal."]
    pub live_jobs: ::std::vec::Vec<OperationId>,
    #[doc = "Terminal but not yet released."]
    pub operations_retained: u64,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub pressure: ::std::option::Option<Pressure>,
    pub retained_bytes: u64,
    #[doc = "Per boot, monotonic."]
    pub seq: u64,
}
impl HandStatusEvent {
    pub fn builder() -> builder::HandStatusEvent {
        Default::default()
    }
}
#[doc = "First request on every connection. Seals the tool manifest (I1), authenticates the brain to the hand, restores the workspace on a fresh generation, re-attaches on reconnect."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"First request on every connection. Seals the tool manifest (I1), authenticates the brain to the hand, restores the workspace on a fresh generation, re-attaches on reconnect.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"env\","]
#[doc = "    \"heartbeat_ms\","]
#[doc = "    \"protocol\","]
#[doc = "    \"session_id\","]
#[doc = "    \"session_token\","]
#[doc = "    \"sync\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"env\": {"]
#[doc = "      \"description\": \"Environment for lane 0 (inherited by every lane). Customer-supplied; never platform credentials.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"expected_generation_id\": {"]
#[doc = "      \"description\": \"Set on reconnect. If the hand's generation differs, the response still succeeds and the brain treats every prior operation as lost.\","]
#[doc = "      \"$ref\": \"#/$defs/GenerationId\""]
#[doc = "    },"]
#[doc = "    \"heartbeat_ms\": {"]
#[doc = "      \"description\": \"Interval for unsolicited hand_status events.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1000.0"]
#[doc = "    },"]
#[doc = "    \"protocol\": {"]
#[doc = "      \"$ref\": \"#/$defs/ProtocolVersion\""]
#[doc = "    },"]
#[doc = "    \"restore\": {"]
#[doc = "      \"description\": \"Present when the hand is a fresh generation of a session that has synced before. The hand restores before answering.\","]
#[doc = "      \"$ref\": \"#/$defs/RestoreSource\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"session_token\": {"]
#[doc = "      \"description\": \"Per-session secret the hand was launched with. Mismatch = unauthorized and the connection is closed.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"sync\": {"]
#[doc = "      \"$ref\": \"#/$defs/SyncScope\""]
#[doc = "    },"]
#[doc = "    \"tool_manifest_digest\": {"]
#[doc = "      \"description\": \"The digest sealed at session create. Omitted only on the very first hello of a session, when the brain adopts the hand's manifest. A hand that cannot serve this digest answers tool_manifest_mismatch.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct HelloRequest {
    #[doc = "Environment for lane 0 (inherited by every lane). Customer-supplied; never platform credentials."]
    pub env: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    #[doc = "Set on reconnect. If the hand's generation differs, the response still succeeds and the brain treats every prior operation as lost."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub expected_generation_id: ::std::option::Option<GenerationId>,
    #[doc = "Interval for unsolicited hand_status events."]
    pub heartbeat_ms: i64,
    pub protocol: ProtocolVersion,
    #[doc = "Present when the hand is a fresh generation of a session that has synced before. The hand restores before answering."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub restore: ::std::option::Option<RestoreSource>,
    pub session_id: SessionId,
    #[doc = "Per-session secret the hand was launched with. Mismatch = unauthorized and the connection is closed."]
    pub session_token: ::std::string::String,
    pub sync: SyncScope,
    #[doc = "The digest sealed at session create. Omitted only on the very first hello of a session, when the brain adopts the hand's manifest. A hand that cannot serve this digest answers tool_manifest_mismatch."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tool_manifest_digest: ::std::option::Option<Sha256Hex>,
}
impl HelloRequest {
    pub fn builder() -> builder::HelloRequest {
        Default::default()
    }
}
#[doc = "`HelloResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"boot_id\","]
#[doc = "    \"clock\","]
#[doc = "    \"generation_id\","]
#[doc = "    \"lanes\","]
#[doc = "    \"limits\","]
#[doc = "    \"operations\","]
#[doc = "    \"paths\","]
#[doc = "    \"protocol\","]
#[doc = "    \"tool_manifest_digest\","]
#[doc = "    \"tools\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"boot_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/BootId\""]
#[doc = "    },"]
#[doc = "    \"clock\": {"]
#[doc = "      \"$ref\": \"#/$defs/Clock\""]
#[doc = "    },"]
#[doc = "    \"generation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/GenerationId\""]
#[doc = "    },"]
#[doc = "    \"lanes\": {"]
#[doc = "      \"description\": \"Every lane the hand knows (live and closed tombstones). Non-empty only on reconnect to a surviving generation.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/LaneSummary\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"limits\": {"]
#[doc = "      \"$ref\": \"#/$defs/Limits\""]
#[doc = "    },"]
#[doc = "    \"operations\": {"]
#[doc = "      \"description\": \"Every non-released operation. Empty on a fresh generation.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationView\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"paths\": {"]
#[doc = "      \"$ref\": \"#/$defs/Paths\""]
#[doc = "    },"]
#[doc = "    \"protocol\": {"]
#[doc = "      \"$ref\": \"#/$defs/ProtocolVersion\""]
#[doc = "    },"]
#[doc = "    \"restore\": {"]
#[doc = "      \"description\": \"Present when the request carried `restore`.\","]
#[doc = "      \"$ref\": \"#/$defs/RestoreReport\""]
#[doc = "    },"]
#[doc = "    \"tool_manifest_digest\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"tools\": {"]
#[doc = "      \"description\": \"The manifest's tools, sorted by name.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolSpec\""]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct HelloResponse {
    pub boot_id: BootId,
    pub clock: Clock,
    pub generation_id: GenerationId,
    #[doc = "Every lane the hand knows (live and closed tombstones). Non-empty only on reconnect to a surviving generation."]
    pub lanes: ::std::vec::Vec<LaneSummary>,
    pub limits: Limits,
    #[doc = "Every non-released operation. Empty on a fresh generation."]
    pub operations: ::std::vec::Vec<OperationView>,
    pub paths: Paths,
    pub protocol: ProtocolVersion,
    #[doc = "Present when the request carried `restore`."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub restore: ::std::option::Option<RestoreReport>,
    pub tool_manifest_digest: Sha256Hex,
    #[doc = "The manifest's tools, sorted by name."]
    pub tools: ::std::vec::Vec<ToolSpec>,
}
impl HelloResponse {
    pub fn builder() -> builder::HelloResponse {
        Default::default()
    }
}
#[doc = "Destroy a lane and tombstone its id. Cancels its attached in-flight operation. Does not kill detached jobs started from it (they belong to the operation registry). Lane 0 is not closable."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Destroy a lane and tombstone its id. Cancels its attached in-flight operation. Does not kill detached jobs started from it (they belong to the operation registry). Lane 0 is not closable.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"lane_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"grace_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"lane_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct LaneCloseRequest {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub grace_ms: ::std::option::Option<u64>,
    pub lane_id: LaneId,
}
impl LaneCloseRequest {
    pub fn builder() -> builder::LaneCloseRequest {
        Default::default()
    }
}
#[doc = "`LaneCloseResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"cancelled_operations\","]
#[doc = "    \"closed\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"cancelled_operations\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"closed\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct LaneCloseResponse {
    pub cancelled_operations: ::std::vec::Vec<OperationId>,
    pub closed: bool,
}
impl LaneCloseResponse {
    pub fn builder() -> builder::LaneCloseResponse {
        Default::default()
    }
}
#[doc = "Brain-minted lane identifier. \"0\" is the root lane and always exists."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted lane identifier. \\\"0\\\" is the root lane and always exists.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct LaneId(::std::string::String);
impl ::std::ops::Deref for LaneId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<LaneId> for ::std::string::String {
    fn from(value: LaneId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for LaneId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for LaneId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LaneId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LaneId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for LaneId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "`LaneMode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"persistent\","]
#[doc = "    \"ephemeral\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum LaneMode {
    #[serde(rename = "persistent")]
    Persistent,
    #[serde(rename = "ephemeral")]
    Ephemeral,
}
impl ::std::fmt::Display for LaneMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Persistent => f.write_str("persistent"),
            Self::Ephemeral => f.write_str("ephemeral"),
        }
    }
}
impl ::std::str::FromStr for LaneMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "persistent" => Ok(Self::Persistent),
            "ephemeral" => Ok(Self::Ephemeral),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LaneMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LaneMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LaneMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`LaneRef`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"id\","]
#[doc = "    \"mode\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    },"]
#[doc = "    \"mode\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneMode\""]
#[doc = "    },"]
#[doc = "    \"parent\": {"]
#[doc = "      \"description\": \"Required when mode = ephemeral: the lane whose environment is inherited. An ephemeral lane's env mutations are discarded when it is closed or its operation ends.\","]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct LaneRef {
    pub id: LaneId,
    pub mode: LaneMode,
    #[doc = "Required when mode = ephemeral: the lane whose environment is inherited. An ephemeral lane's env mutations are discarded when it is closed or its operation ends."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub parent: ::std::option::Option<LaneId>,
}
impl LaneRef {
    pub fn builder() -> builder::LaneRef {
        Default::default()
    }
}
#[doc = "`LaneSummary`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"id\","]
#[doc = "    \"mode\","]
#[doc = "    \"state\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"created_at_monotonic_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/MonotonicMs\""]
#[doc = "    },"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    },"]
#[doc = "    \"inflight\": {"]
#[doc = "      \"description\": \"The attached operation currently holding the lane, if any.\","]
#[doc = "      \"$ref\": \"#/$defs/OperationId\""]
#[doc = "    },"]
#[doc = "    \"mode\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneMode\""]
#[doc = "    },"]
#[doc = "    \"parent\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"live\","]
#[doc = "        \"closed\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct LaneSummary {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub created_at_monotonic_ms: ::std::option::Option<MonotonicMs>,
    pub id: LaneId,
    #[doc = "The attached operation currently holding the lane, if any."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub inflight: ::std::option::Option<OperationId>,
    pub mode: LaneMode,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub parent: ::std::option::Option<LaneId>,
    pub state: LaneSummaryState,
}
impl LaneSummary {
    pub fn builder() -> builder::LaneSummary {
        Default::default()
    }
}
#[doc = "`LaneSummaryState`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"live\","]
#[doc = "    \"closed\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum LaneSummaryState {
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "closed")]
    Closed,
}
impl ::std::fmt::Display for LaneSummaryState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Live => f.write_str("live"),
            Self::Closed => f.write_str("closed"),
        }
    }
}
impl ::std::str::FromStr for LaneSummaryState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "live" => Ok(Self::Live),
            "closed" => Ok(Self::Closed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LaneSummaryState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LaneSummaryState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LaneSummaryState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Declared by the hand in `hello`, not assumed by the brain."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Declared by the hand in `hello`, not assumed by the brain.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"default_bounds\","]
#[doc = "    \"max_concurrent_operations\","]
#[doc = "    \"max_frame_bytes\","]
#[doc = "    \"max_inline_put_bytes\","]
#[doc = "    \"max_lanes\","]
#[doc = "    \"max_persist_bytes\","]
#[doc = "    \"max_poll_wait_ms\","]
#[doc = "    \"max_slice_bytes\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"default_bounds\": {"]
#[doc = "      \"$ref\": \"#/$defs/EffectiveBounds\""]
#[doc = "    },"]
#[doc = "    \"max_concurrent_operations\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"max_frame_bytes\": {"]
#[doc = "      \"description\": \"Largest WebSocket text frame either side will send.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 65536.0"]
#[doc = "    },"]
#[doc = "    \"max_inline_put_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_lanes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"max_persist_bytes\": {"]
#[doc = "      \"description\": \"Per persisted item.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_poll_wait_ms\": {"]
#[doc = "      \"description\": \"Cap on `wait_ms` for start/poll.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_slice_bytes\": {"]
#[doc = "      \"description\": \"Largest decoded slice returned per stream per response.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 4096.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Limits {
    pub default_bounds: EffectiveBounds,
    pub max_concurrent_operations: ::std::num::NonZeroU64,
    #[doc = "Largest WebSocket text frame either side will send."]
    pub max_frame_bytes: i64,
    pub max_inline_put_bytes: u64,
    pub max_lanes: ::std::num::NonZeroU64,
    #[doc = "Per persisted item."]
    pub max_persist_bytes: u64,
    #[doc = "Cap on `wait_ms` for start/poll."]
    pub max_poll_wait_ms: u64,
    #[doc = "Largest decoded slice returned per stream per response."]
    pub max_slice_bytes: i64,
}
impl Limits {
    pub fn builder() -> builder::Limits {
        Default::default()
    }
}
#[doc = "Identifies one workspace sync manifest. Brain-minted."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Identifies one workspace sync manifest. Brain-minted.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ManifestId(::std::string::String);
impl ::std::ops::Deref for ManifestId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ManifestId> for ::std::string::String {
    fn from(value: ManifestId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ManifestId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ManifestId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ManifestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ManifestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ManifestId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Milliseconds on the guest's monotonic clock. Jumps across a restore; compare only within one boot_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Milliseconds on the guest's monotonic clock. Jumps across a restore; compare only within one boot_id.\","]
#[doc = "  \"type\": \"integer\","]
#[doc = "  \"minimum\": 0.0"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct MonotonicMs(pub u64);
impl ::std::ops::Deref for MonotonicMs {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}
impl ::std::convert::From<MonotonicMs> for u64 {
    fn from(value: MonotonicMs) -> Self {
        value.0
    }
}
impl ::std::convert::From<u64> for MonotonicMs {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for MonotonicMs {
    type Err = <u64 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for MonotonicMs {
    type Error = <u64 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for MonotonicMs {
    type Error = <u64 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for MonotonicMs {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
#[doc = "Brain-minted, durable, unique per tool-call attempt. Not the provider's tool_call id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted, durable, unique per tool-call attempt. Not the provider's tool_call id.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OperationId(::std::string::String);
impl ::std::ops::Deref for OperationId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OperationId> for ::std::string::String {
    fn from(value: OperationId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OperationId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OperationId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OperationId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "missing = the hand knows the id but lost its record (treated as interrupted)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"missing = the hand knows the id but lost its record (treated as interrupted).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"running\","]
#[doc = "    \"terminal\","]
#[doc = "    \"missing\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum OperationStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "missing")]
    Missing,
}
impl ::std::fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Running => f.write_str("running"),
            Self::Terminal => f.write_str("terminal"),
            Self::Missing => f.write_str("missing"),
        }
    }
}
impl ::std::str::FromStr for OperationStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "running" => Ok(Self::Running),
            "terminal" => Ok(Self::Terminal),
            "missing" => Ok(Self::Missing),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OperationStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`OperationView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"detach\","]
#[doc = "    \"lane_id\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"started_at_monotonic_ms\","]
#[doc = "    \"status\","]
#[doc = "    \"streams\","]
#[doc = "    \"tool\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"correlation\": {"]
#[doc = "      \"description\": \"Echoed verbatim from `start`.\","]
#[doc = "      \"type\": \"object\""]
#[doc = "    },"]
#[doc = "    \"detach\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"lane_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneId\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationId\""]
#[doc = "    },"]
#[doc = "    \"started_at_monotonic_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/MonotonicMs\""]
#[doc = "    },"]
#[doc = "    \"status\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationStatus\""]
#[doc = "    },"]
#[doc = "    \"streams\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/StreamInfo\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"terminal\": {"]
#[doc = "      \"description\": \"Present iff status = terminal.\","]
#[doc = "      \"$ref\": \"#/$defs/TerminalInfo\""]
#[doc = "    },"]
#[doc = "    \"tool\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct OperationView {
    #[doc = "Echoed verbatim from `start`."]
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub correlation: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub detach: bool,
    pub lane_id: LaneId,
    pub operation_id: OperationId,
    pub started_at_monotonic_ms: MonotonicMs,
    pub status: OperationStatus,
    pub streams: ::std::vec::Vec<StreamInfo>,
    #[doc = "Present iff status = terminal."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub terminal: ::std::option::Option<TerminalInfo>,
    pub tool: ::std::string::String,
}
impl OperationView {
    pub fn builder() -> builder::OperationView {
        Default::default()
    }
}
#[doc = "completed = the tool ran to its end (a non-zero exit_code is still `completed`; the failure is data for the model). failed = the hand could not run it. interrupted = outcome unknown; never replayed."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"completed = the tool ran to its end (a non-zero exit_code is still `completed`; the failure is data for the model). failed = the hand could not run it. interrupted = outcome unknown; never replayed.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"completed\","]
#[doc = "    \"failed\","]
#[doc = "    \"cancelled\","]
#[doc = "    \"deadline_exceeded\","]
#[doc = "    \"interrupted\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum Outcome {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "deadline_exceeded")]
    DeadlineExceeded,
    #[serde(rename = "interrupted")]
    Interrupted,
}
impl ::std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::DeadlineExceeded => f.write_str("deadline_exceeded"),
            Self::Interrupted => f.write_str("interrupted"),
        }
    }
}
impl ::std::str::FromStr for Outcome {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "deadline_exceeded" => Ok(Self::DeadlineExceeded),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Outcome {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Outcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Outcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`OutputSlice`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"data_base64\","]
#[doc = "    \"eof\","]
#[doc = "    \"offset\","]
#[doc = "    \"stream\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"data_base64\": {"]
#[doc = "      \"description\": \"Standard base64 (RFC 4648 §4, with padding). Bounded by `max_bytes` of the request and `limits.max_slice_bytes`.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"contentEncoding\": \"base64\""]
#[doc = "    },"]
#[doc = "    \"eof\": {"]
#[doc = "      \"description\": \"True when this slice reaches the current end of the stream. Only meaningful as 'no more bytes right now'; the stream may still grow while the operation is running.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"offset\": {"]
#[doc = "      \"description\": \"Byte offset of the first byte of `data_base64` in the full stream.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"stream\": {"]
#[doc = "      \"$ref\": \"#/$defs/Stream\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct OutputSlice {
    #[doc = "Standard base64 (RFC 4648 §4, with padding). Bounded by `max_bytes` of the request and `limits.max_slice_bytes`."]
    pub data_base64: ::std::string::String,
    #[doc = "True when this slice reaches the current end of the stream. Only meaningful as 'no more bytes right now'; the stream may still grow while the operation is running."]
    pub eof: bool,
    #[doc = "Byte offset of the first byte of `data_base64` in the full stream."]
    pub offset: u64,
    pub stream: Stream,
}
impl OutputSlice {
    pub fn builder() -> builder::OutputSlice {
        Default::default()
    }
}
#[doc = "Identifies one sync pack object (tar+zstd of changed files). Brain-minted."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Identifies one sync pack object (tar+zstd of changed files). Brain-minted.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct PackId(::std::string::String);
impl ::std::ops::Deref for PackId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<PackId> for ::std::string::String {
    fn from(value: PackId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for PackId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for PackId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PackId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PackId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for PackId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "`Paths`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"home\","]
#[doc = "    \"spill_dir\","]
#[doc = "    \"workspace\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"home\": {"]
#[doc = "      \"description\": \"$HOME of the agent user. Synced.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"spill_dir\": {"]
#[doc = "      \"description\": \"Where operation output files live. Not synced.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"workspace\": {"]
#[doc = "      \"description\": \"Default cwd. Synced.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Paths {
    #[doc = "$HOME of the agent user. Synced."]
    pub home: ::std::string::String,
    #[doc = "Where operation output files live. Not synced."]
    pub spill_dir: ::std::string::String,
    #[doc = "Default cwd. Synced."]
    pub workspace: ::std::string::String,
}
impl Paths {
    pub fn builder() -> builder::Paths {
        Default::default()
    }
}
#[doc = "`PersistItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"name\","]
#[doc = "    \"put_url\","]
#[doc = "    \"source\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"description\": \"Sent as Content-Type. Sniffed if absent.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Session-unique artifact name, chosen by the brain/customer.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 255,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"put_url\": {"]
#[doc = "      \"description\": \"Short-lived presigned PUT for exactly this artifact object.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"source\": {"]
#[doc = "      \"$ref\": \"#/$defs/PersistSource\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PersistItem {
    #[doc = "Sent as Content-Type. Sniffed if absent."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub media_type: ::std::option::Option<::std::string::String>,
    #[doc = "Session-unique artifact name, chosen by the brain/customer."]
    pub name: PersistItemName,
    #[doc = "Short-lived presigned PUT for exactly this artifact object."]
    pub put_url: ::std::string::String,
    pub source: PersistSource,
}
impl PersistItem {
    pub fn builder() -> builder::PersistItem {
        Default::default()
    }
}
#[doc = "Session-unique artifact name, chosen by the brain/customer."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Session-unique artifact name, chosen by the brain/customer.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 255,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct PersistItemName(::std::string::String);
impl ::std::ops::Deref for PersistItemName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<PersistItemName> for ::std::string::String {
    fn from(value: PersistItemName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for PersistItemName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 255usize {
            return Err("longer than 255 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for PersistItemName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PersistItemName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PersistItemName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for PersistItemName {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Files out. The hand uploads each item to its presigned URL and reports size + digest; the trusted side records the artifact. Bounded by limits.max_persist_bytes per item."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Files out. The hand uploads each item to its presigned URL and reports size + digest; the trusted side records the artifact. Bounded by limits.max_persist_bytes per item.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"items\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"items\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/PersistItem\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PersistRequest {
    pub items: ::std::vec::Vec<PersistItem>,
}
impl PersistRequest {
    pub fn builder() -> builder::PersistRequest {
        Default::default()
    }
}
#[doc = "`PersistResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"persisted\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"persisted\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"object\","]
#[doc = "        \"required\": ["]
#[doc = "          \"bytes\","]
#[doc = "          \"media_type\","]
#[doc = "          \"name\","]
#[doc = "          \"sha256\""]
#[doc = "        ],"]
#[doc = "        \"properties\": {"]
#[doc = "          \"bytes\": {"]
#[doc = "            \"type\": \"integer\","]
#[doc = "            \"minimum\": 0.0"]
#[doc = "          },"]
#[doc = "          \"media_type\": {"]
#[doc = "            \"type\": \"string\""]
#[doc = "          },"]
#[doc = "          \"name\": {"]
#[doc = "            \"type\": \"string\""]
#[doc = "          },"]
#[doc = "          \"sha256\": {"]
#[doc = "            \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "          }"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PersistResponse {
    pub persisted: ::std::vec::Vec<PersistResponsePersistedItem>,
}
impl PersistResponse {
    pub fn builder() -> builder::PersistResponse {
        Default::default()
    }
}
#[doc = "`PersistResponsePersistedItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"media_type\","]
#[doc = "    \"name\","]
#[doc = "    \"sha256\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PersistResponsePersistedItem {
    pub bytes: u64,
    pub media_type: ::std::string::String,
    pub name: ::std::string::String,
    pub sha256: Sha256Hex,
}
impl PersistResponsePersistedItem {
    pub fn builder() -> builder::PersistResponsePersistedItem {
        Default::default()
    }
}
#[doc = "`PersistSource`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"path\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"path\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"path\": {"]
#[doc = "          \"description\": \"Absolute guest path inside the sync scope, resolved through symlinks.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"operation_id\","]
#[doc = "        \"stream\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"operation_stream\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"operation_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/OperationId\""]
#[doc = "        },"]
#[doc = "        \"stream\": {"]
#[doc = "          \"$ref\": \"#/$defs/Stream\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind")]
pub enum PersistSource {
    #[serde(rename = "path")]
    Path {
        #[doc = "Absolute guest path inside the sync scope, resolved through symlinks."]
        path: ::std::string::String,
    },
    #[serde(rename = "operation_stream")]
    OperationStream {
        operation_id: OperationId,
        stream: Stream,
    },
}
#[doc = "Status plus incremental output from byte cursors. Byte offsets are the authority: no gaps, no duplicates, up to retention. Waits up to wait_ms for the operation to become terminal or for any cursor to have new bytes."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Status plus incremental output from byte cursors. Byte offsets are the authority: no gaps, no duplicates, up to retention. Waits up to wait_ms for the operation to become terminal or for any cursor to have new bytes.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"cursors\","]
#[doc = "    \"max_bytes\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"wait_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"cursors\": {"]
#[doc = "      \"description\": \"Empty = status only.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/Cursor\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"max_bytes\": {"]
#[doc = "      \"description\": \"Total decoded bytes across all returned slices.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationId\""]
#[doc = "    },"]
#[doc = "    \"wait_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PollRequest {
    #[doc = "Empty = status only."]
    pub cursors: ::std::vec::Vec<Cursor>,
    #[doc = "Total decoded bytes across all returned slices."]
    pub max_bytes: u64,
    pub operation_id: OperationId,
    pub wait_ms: u64,
}
impl PollRequest {
    pub fn builder() -> builder::PollRequest {
        Default::default()
    }
}
#[doc = "`PollResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"slices\","]
#[doc = "    \"view\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"slices\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OutputSlice\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"view\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationView\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PollResponse {
    pub slices: ::std::vec::Vec<OutputSlice>,
    pub view: OperationView,
}
impl PollResponse {
    pub fn builder() -> builder::PollResponse {
        Default::default()
    }
}
#[doc = "`Pressure`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"mem_available_bytes\","]
#[doc = "    \"swap_used_bytes\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"mem_available_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"psi_some_avg10\": {"]
#[doc = "      \"description\": \"PSI memory 'some' avg10, when the guest kernel exposes it.\","]
#[doc = "      \"type\": \"number\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"swap_used_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Pressure {
    pub mem_available_bytes: u64,
    #[doc = "PSI memory 'some' avg10, when the guest kernel exposes it."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub psi_some_avg10: ::std::option::Option<f64>,
    pub swap_used_bytes: u64,
}
impl Pressure {
    pub fn builder() -> builder::Pressure {
        Default::default()
    }
}
#[doc = "`ProtocolVersion`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"major\","]
#[doc = "    \"minor\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"major\": {"]
#[doc = "      \"description\": \"Must match exactly between brain and hand.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"minor\": {"]
#[doc = "      \"description\": \"Informational. Additive changes only.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ProtocolVersion {
    #[doc = "Must match exactly between brain and hand."]
    pub major: ::std::num::NonZeroU64,
    #[doc = "Informational. Additive changes only."]
    pub minor: u64,
}
impl ProtocolVersion {
    pub fn builder() -> builder::ProtocolVersion {
        Default::default()
    }
}
#[doc = "`PutFile`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"path\","]
#[doc = "    \"source\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"mode\": {"]
#[doc = "      \"description\": \"Unix permission bits. Default 0644.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 4095.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"description\": \"Absolute guest path inside the sync scope; parents are created. Symlinks are resolved before the scope check.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"source\": {"]
#[doc = "      \"$ref\": \"#/$defs/PutSource\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PutFile {
    #[doc = "Unix permission bits. Default 0644."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub mode: ::std::option::Option<i64>,
    #[doc = "Absolute guest path inside the sync scope; parents are created. Symlinks are resolved before the scope check."]
    pub path: ::std::string::String,
    pub source: PutSource,
}
impl PutFile {
    pub fn builder() -> builder::PutFile {
        Default::default()
    }
}
#[doc = "Files in. Bytes never travel as a tool result (I7): the hand downloads from presigned URLs, or accepts small inline payloads."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Files in. Bytes never travel as a tool result (I7): the hand downloads from presigned URLs, or accepts small inline payloads.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"files\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"files\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/PutFile\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PutRequest {
    pub files: ::std::vec::Vec<PutFile>,
}
impl PutRequest {
    pub fn builder() -> builder::PutRequest {
        Default::default()
    }
}
#[doc = "`PutResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"written\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"written\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"object\","]
#[doc = "        \"required\": ["]
#[doc = "          \"bytes\","]
#[doc = "          \"path\","]
#[doc = "          \"sha256\""]
#[doc = "        ],"]
#[doc = "        \"properties\": {"]
#[doc = "          \"bytes\": {"]
#[doc = "            \"type\": \"integer\","]
#[doc = "            \"minimum\": 0.0"]
#[doc = "          },"]
#[doc = "          \"path\": {"]
#[doc = "            \"type\": \"string\""]
#[doc = "          },"]
#[doc = "          \"sha256\": {"]
#[doc = "            \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "          }"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PutResponse {
    pub written: ::std::vec::Vec<PutResponseWrittenItem>,
}
impl PutResponse {
    pub fn builder() -> builder::PutResponse {
        Default::default()
    }
}
#[doc = "`PutResponseWrittenItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"path\","]
#[doc = "    \"sha256\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct PutResponseWrittenItem {
    pub bytes: u64,
    pub path: ::std::string::String,
    pub sha256: Sha256Hex,
}
impl PutResponseWrittenItem {
    pub fn builder() -> builder::PutResponseWrittenItem {
        Default::default()
    }
}
#[doc = "`PutSource`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"bytes\","]
#[doc = "        \"get_url\","]
#[doc = "        \"kind\","]
#[doc = "        \"sha256\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"bytes\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"get_url\": {"]
#[doc = "          \"description\": \"Short-lived presigned GET minted by the trusted side.\","]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"format\": \"uri\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"url\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"sha256\": {"]
#[doc = "          \"description\": \"Verified after download; mismatch = checksum_mismatch and the file is not written.\","]
#[doc = "          \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"data_base64\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"data_base64\": {"]
#[doc = "          \"description\": \"Small files only (limits.max_inline_put_bytes decoded).\","]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"contentEncoding\": \"base64\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"inline\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind")]
pub enum PutSource {
    #[serde(rename = "url")]
    Url {
        bytes: u64,
        #[doc = "Short-lived presigned GET minted by the trusted side."]
        get_url: ::std::string::String,
        #[doc = "Verified after download; mismatch = checksum_mismatch and the file is not written."]
        sha256: Sha256Hex,
    },
    #[serde(rename = "inline")]
    Inline {
        #[doc = "Small files only (limits.max_inline_put_bytes decoded)."]
        data_base64: ::std::string::String,
    },
}
#[doc = "The brain has durably committed these results; the hand deletes their spill files and forgets them. After release, poll returns operation_not_found and a replayed start would run again — so release only after commit."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The brain has durably committed these results; the hand deletes their spill files and forgets them. After release, poll returns operation_not_found and a replayed start would run again — so release only after commit.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"operation_ids\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"operation_ids\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ReleaseRequest {
    pub operation_ids: ::std::vec::Vec<OperationId>,
}
impl ReleaseRequest {
    pub fn builder() -> builder::ReleaseRequest {
        Default::default()
    }
}
#[doc = "`ReleaseResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"released\","]
#[doc = "    \"unknown\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"released\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"unknown\": {"]
#[doc = "      \"description\": \"Ids the hand did not know (already released or never started). Not an error.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OperationId\""]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ReleaseResponse {
    pub released: ::std::vec::Vec<OperationId>,
    #[doc = "Ids the hand did not know (already released or never started). Not an error."]
    pub unknown: ::std::vec::Vec<OperationId>,
}
impl ReleaseResponse {
    pub fn builder() -> builder::ReleaseResponse {
        Default::default()
    }
}
#[doc = "One successful reply, tagged by `op`; `body` is that op's response type."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One successful reply, tagged by `op`; `body` is that op's response type.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/HelloResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"hello\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/StartResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"start\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/PollResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"poll\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/CancelResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"cancel\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/ReleaseResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"release\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/LaneCloseResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"lane_close\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/PutResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"put\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/PersistResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"persist\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"body\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"body\": {"]
#[doc = "          \"$ref\": \"#/$defs/SyncResponse\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"sync\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", content = "body")]
pub enum Reply {
    #[serde(rename = "hello")]
    Hello(HelloResponse),
    #[serde(rename = "start")]
    Start(StartResponse),
    #[serde(rename = "poll")]
    Poll(PollResponse),
    #[serde(rename = "cancel")]
    Cancel(CancelResponse),
    #[serde(rename = "release")]
    Release(ReleaseResponse),
    #[serde(rename = "lane_close")]
    LaneClose(LaneCloseResponse),
    #[serde(rename = "put")]
    Put(PutResponse),
    #[serde(rename = "persist")]
    Persist(PersistResponse),
    #[serde(rename = "sync")]
    Sync(SyncResponse),
}
impl ::std::convert::From<HelloResponse> for Reply {
    fn from(value: HelloResponse) -> Self {
        Self::Hello(value)
    }
}
impl ::std::convert::From<StartResponse> for Reply {
    fn from(value: StartResponse) -> Self {
        Self::Start(value)
    }
}
impl ::std::convert::From<PollResponse> for Reply {
    fn from(value: PollResponse) -> Self {
        Self::Poll(value)
    }
}
impl ::std::convert::From<CancelResponse> for Reply {
    fn from(value: CancelResponse) -> Self {
        Self::Cancel(value)
    }
}
impl ::std::convert::From<ReleaseResponse> for Reply {
    fn from(value: ReleaseResponse) -> Self {
        Self::Release(value)
    }
}
impl ::std::convert::From<LaneCloseResponse> for Reply {
    fn from(value: LaneCloseResponse) -> Self {
        Self::LaneClose(value)
    }
}
impl ::std::convert::From<PutResponse> for Reply {
    fn from(value: PutResponse) -> Self {
        Self::Put(value)
    }
}
impl ::std::convert::From<PersistResponse> for Reply {
    fn from(value: PersistResponse) -> Self {
        Self::Persist(value)
    }
}
impl ::std::convert::From<SyncResponse> for Reply {
    fn from(value: SyncResponse) -> Self {
        Self::Sync(value)
    }
}
#[doc = "Brain -> hand. Every request carries the brain's ownership fence; a fence lower than the highest the hand has accepted is refused with fence_stale (no side effect). generation_id is required on every request except hello; a mismatch is generation_mismatch."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain -> hand. Every request carries the brain's ownership fence; a fence lower than the highest the hand has accepted is refused with fence_stale (no side effect). generation_id is required on every request except hello; a mismatch is generation_mismatch.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"call\","]
#[doc = "    \"fence\","]
#[doc = "    \"id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"call\": {"]
#[doc = "      \"$ref\": \"#/$defs/RequestCall\""]
#[doc = "    },"]
#[doc = "    \"fence\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"generation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/GenerationId\""]
#[doc = "    },"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/RequestId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub call: RequestCall,
    pub fence: u64,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub generation_id: ::std::option::Option<GenerationId>,
    pub id: RequestId,
}
impl Request {
    pub fn builder() -> builder::Request {
        Default::default()
    }
}
#[doc = "One brain->hand call, tagged by `op`; `args` is that op's request type."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One brain->hand call, tagged by `op`; `args` is that op's request type.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/HelloRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"hello\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/StartRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"start\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/PollRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"poll\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/CancelRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"cancel\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/ReleaseRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"release\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/LaneCloseRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"lane_close\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/PutRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"put\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/PersistRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"persist\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"args\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"args\": {"]
#[doc = "          \"$ref\": \"#/$defs/SyncRequest\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"sync\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", content = "args")]
pub enum RequestCall {
    #[serde(rename = "hello")]
    Hello(HelloRequest),
    #[serde(rename = "start")]
    Start(StartRequest),
    #[serde(rename = "poll")]
    Poll(PollRequest),
    #[serde(rename = "cancel")]
    Cancel(CancelRequest),
    #[serde(rename = "release")]
    Release(ReleaseRequest),
    #[serde(rename = "lane_close")]
    LaneClose(LaneCloseRequest),
    #[serde(rename = "put")]
    Put(PutRequest),
    #[serde(rename = "persist")]
    Persist(PersistRequest),
    #[serde(rename = "sync")]
    Sync(SyncRequest),
}
impl ::std::convert::From<HelloRequest> for RequestCall {
    fn from(value: HelloRequest) -> Self {
        Self::Hello(value)
    }
}
impl ::std::convert::From<StartRequest> for RequestCall {
    fn from(value: StartRequest) -> Self {
        Self::Start(value)
    }
}
impl ::std::convert::From<PollRequest> for RequestCall {
    fn from(value: PollRequest) -> Self {
        Self::Poll(value)
    }
}
impl ::std::convert::From<CancelRequest> for RequestCall {
    fn from(value: CancelRequest) -> Self {
        Self::Cancel(value)
    }
}
impl ::std::convert::From<ReleaseRequest> for RequestCall {
    fn from(value: ReleaseRequest) -> Self {
        Self::Release(value)
    }
}
impl ::std::convert::From<LaneCloseRequest> for RequestCall {
    fn from(value: LaneCloseRequest) -> Self {
        Self::LaneClose(value)
    }
}
impl ::std::convert::From<PutRequest> for RequestCall {
    fn from(value: PutRequest) -> Self {
        Self::Put(value)
    }
}
impl ::std::convert::From<PersistRequest> for RequestCall {
    fn from(value: PersistRequest) -> Self {
        Self::Persist(value)
    }
}
impl ::std::convert::From<SyncRequest> for RequestCall {
    fn from(value: SyncRequest) -> Self {
        Self::Sync(value)
    }
}
#[doc = "Brain-minted, unique per request on one connection. Responses echo it."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted, unique per request on one connection. Responses echo it.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct RequestId(::std::string::String);
impl ::std::ops::Deref for RequestId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<RequestId> for ::std::string::String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for RequestId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Hand -> brain answer to one Request, correlated by `id`."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Hand -> brain answer to one Request, correlated by `id`.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"id\","]
#[doc = "    \"result\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/RequestId\""]
#[doc = "    },"]
#[doc = "    \"result\": {"]
#[doc = "      \"$ref\": \"#/$defs/ResponseResult\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Response {
    pub id: RequestId,
    pub result: ResponseResult,
}
impl Response {
    pub fn builder() -> builder::Response {
        Default::default()
    }
}
#[doc = "`ResponseResult`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"reply\","]
#[doc = "        \"status\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"reply\": {"]
#[doc = "          \"$ref\": \"#/$defs/Reply\""]
#[doc = "        },"]
#[doc = "        \"status\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"ok\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"error\","]
#[doc = "        \"status\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"error\": {"]
#[doc = "          \"$ref\": \"#/$defs/AbiError\""]
#[doc = "        },"]
#[doc = "        \"status\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"error\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "status")]
pub enum ResponseResult {
    #[serde(rename = "ok")]
    Ok { reply: Reply },
    #[serde(rename = "error")]
    Error { error: AbiError },
}
#[doc = "`RestoreReport`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"duration_ms\","]
#[doc = "    \"files\","]
#[doc = "    \"manifest_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"duration_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"files\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"manifest_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct RestoreReport {
    pub bytes: u64,
    pub duration_ms: u64,
    pub files: u64,
    pub manifest_id: ManifestId,
}
impl RestoreReport {
    pub fn builder() -> builder::RestoreReport {
        Default::default()
    }
}
#[doc = "How a fresh hand re-materialises the workspace. All URLs are short-lived presigned GETs minted by the trusted side; the hand holds no credential (I8)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"How a fresh hand re-materialises the workspace. All URLs are short-lived presigned GETs minted by the trusted side; the hand holds no credential (I8).\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"manifest_get_url\","]
#[doc = "    \"manifest_id\","]
#[doc = "    \"packs\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"manifest_get_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"manifest_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    },"]
#[doc = "    \"packs\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"object\","]
#[doc = "        \"required\": ["]
#[doc = "          \"get_url\","]
#[doc = "          \"pack_id\""]
#[doc = "        ],"]
#[doc = "        \"properties\": {"]
#[doc = "          \"get_url\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"format\": \"uri\""]
#[doc = "          },"]
#[doc = "          \"pack_id\": {"]
#[doc = "            \"$ref\": \"#/$defs/PackId\""]
#[doc = "          }"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct RestoreSource {
    pub manifest_get_url: ::std::string::String,
    pub manifest_id: ManifestId,
    pub packs: ::std::vec::Vec<RestoreSourcePacksItem>,
}
impl RestoreSource {
    pub fn builder() -> builder::RestoreSource {
        Default::default()
    }
}
#[doc = "`RestoreSourcePacksItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"get_url\","]
#[doc = "    \"pack_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"get_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"pack_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/PackId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct RestoreSourcePacksItem {
    pub get_url: ::std::string::String,
    pub pack_id: PackId,
}
impl RestoreSourcePacksItem {
    pub fn builder() -> builder::RestoreSourcePacksItem {
        Default::default()
    }
}
#[doc = "`SessionId`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SessionId(::std::string::String);
impl ::std::ops::Deref for SessionId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SessionId> for ::std::string::String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SessionId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SessionId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Lower-case hex SHA-256."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Lower-case hex SHA-256.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[0-9a-f]{64}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct Sha256Hex(::std::string::String);
impl ::std::ops::Deref for Sha256Hex {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<Sha256Hex> for ::std::string::String {
    fn from(value: Sha256Hex) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for Sha256Hex {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[0-9a-f]{64}$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[0-9a-f]{64}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for Sha256Hex {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Sha256Hex {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Sha256Hex {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for Sha256Hex {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Begin a tool call. Idempotent within a generation by (operation_id, call_hash): a replay returns the existing operation without running it again. Validation precedes side effects."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Begin a tool call. Idempotent within a generation by (operation_id, call_hash): a replay returns the existing operation without running it again. Validation precedes side effects.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"call_hash\","]
#[doc = "    \"detach\","]
#[doc = "    \"input\","]
#[doc = "    \"lane\","]
#[doc = "    \"max_bytes\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"tool\","]
#[doc = "    \"wait_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"batch_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/BatchId\""]
#[doc = "    },"]
#[doc = "    \"bounds\": {"]
#[doc = "      \"$ref\": \"#/$defs/Bounds\""]
#[doc = "    },"]
#[doc = "    \"call_hash\": {"]
#[doc = "      \"description\": \"SHA-256 over JCS of {tool, input, lane, cwd, detach, bounds}. Same operation_id with a different call_hash = operation_idempotency_conflict.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"correlation\": {"]
#[doc = "      \"description\": \"Opaque to the hand (e.g. agent_id, provider tool_call id). Echoed on every view.\","]
#[doc = "      \"type\": \"object\""]
#[doc = "    },"]
#[doc = "    \"cwd\": {"]
#[doc = "      \"description\": \"Working directory for this call. Default = paths.workspace. Per call; the lane's cwd is never mutated by the ABI.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"detach\": {"]
#[doc = "      \"description\": \"true = background job: return as soon as the operation is recorded and spawned; the lane is not held; poll/cancel later. false = attached: holds the lane until terminal.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"input\": {"]
#[doc = "      \"description\": \"Validated against the manifest input_schema before anything runs.\""]
#[doc = "    },"]
#[doc = "    \"lane\": {"]
#[doc = "      \"$ref\": \"#/$defs/LaneRef\""]
#[doc = "    },"]
#[doc = "    \"max_bytes\": {"]
#[doc = "      \"description\": \"Bytes of stdout/stderr (from offset 0) to include in the response slices. 0 = none.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationId\""]
#[doc = "    },"]
#[doc = "    \"tool\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"wait_ms\": {"]
#[doc = "      \"description\": \"Attached only: wait up to this long for the operation to become terminal before answering, so short calls cost one round trip. Capped by limits.max_poll_wait_ms. Ignored when detach = true.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct StartRequest {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub batch_id: ::std::option::Option<BatchId>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub bounds: ::std::option::Option<Bounds>,
    #[doc = "SHA-256 over JCS of {tool, input, lane, cwd, detach, bounds}. Same operation_id with a different call_hash = operation_idempotency_conflict."]
    pub call_hash: Sha256Hex,
    #[doc = "Opaque to the hand (e.g. agent_id, provider tool_call id). Echoed on every view."]
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub correlation: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    #[doc = "Working directory for this call. Default = paths.workspace. Per call; the lane's cwd is never mutated by the ABI."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cwd: ::std::option::Option<::std::string::String>,
    #[doc = "true = background job: return as soon as the operation is recorded and spawned; the lane is not held; poll/cancel later. false = attached: holds the lane until terminal."]
    pub detach: bool,
    #[doc = "Validated against the manifest input_schema before anything runs."]
    pub input: ::serde_json::Value,
    pub lane: LaneRef,
    #[doc = "Bytes of stdout/stderr (from offset 0) to include in the response slices. 0 = none."]
    pub max_bytes: u64,
    pub operation_id: OperationId,
    pub tool: ::std::string::String,
    #[doc = "Attached only: wait up to this long for the operation to become terminal before answering, so short calls cost one round trip. Capped by limits.max_poll_wait_ms. Ignored when detach = true."]
    pub wait_ms: u64,
}
impl StartRequest {
    pub fn builder() -> builder::StartRequest {
        Default::default()
    }
}
#[doc = "`StartResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"replayed\","]
#[doc = "    \"slices\","]
#[doc = "    \"view\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"replayed\": {"]
#[doc = "      \"description\": \"True when this start matched an existing (operation_id, call_hash) and nothing new was executed.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"slices\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/OutputSlice\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"view\": {"]
#[doc = "      \"$ref\": \"#/$defs/OperationView\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct StartResponse {
    #[doc = "True when this start matched an existing (operation_id, call_hash) and nothing new was executed."]
    pub replayed: bool,
    pub slices: ::std::vec::Vec<OutputSlice>,
    pub view: OperationView,
}
impl StartResponse {
    pub fn builder() -> builder::StartResponse {
        Default::default()
    }
}
#[doc = "Every operation owns two byte streams. Typed tools write their human-readable result to stdout and diagnostics to stderr."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Every operation owns two byte streams. Typed tools write their human-readable result to stdout and diagnostics to stderr.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"stdout\","]
#[doc = "    \"stderr\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum Stream {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "stderr")]
    Stderr,
}
impl ::std::fmt::Display for Stream {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Stdout => f.write_str("stdout"),
            Self::Stderr => f.write_str("stderr"),
        }
    }
}
impl ::std::str::FromStr for Stream {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "stdout" => Ok(Self::Stdout),
            "stderr" => Ok(Self::Stderr),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Stream {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Stream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Stream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`StreamInfo`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"produced_bytes\","]
#[doc = "    \"retained_from\","]
#[doc = "    \"stream\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"produced_bytes\": {"]
#[doc = "      \"description\": \"Total bytes the child has written to this stream so far.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"retained_from\": {"]
#[doc = "      \"description\": \"Bytes before this offset are evicted (bounds.max_retained_bytes). A poll below this offset returns `operation_output_evicted`; absent is not zero.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"description\": \"Over the full stream. Present only once the operation is terminal and nothing was evicted.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"spill_path\": {"]
#[doc = "      \"description\": \"Guest path of the file holding the retained bytes, readable by the agent's own tools (e.g. `grep`). Deleted on `release`.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"stream\": {"]
#[doc = "      \"$ref\": \"#/$defs/Stream\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct StreamInfo {
    #[doc = "Total bytes the child has written to this stream so far."]
    pub produced_bytes: u64,
    #[doc = "Bytes before this offset are evicted (bounds.max_retained_bytes). A poll below this offset returns `operation_output_evicted`; absent is not zero."]
    pub retained_from: u64,
    #[doc = "Over the full stream. Present only once the operation is terminal and nothing was evicted."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub sha256: ::std::option::Option<Sha256Hex>,
    #[doc = "Guest path of the file holding the retained bytes, readable by the agent's own tools (e.g. `grep`). Deleted on `release`."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub spill_path: ::std::option::Option<::std::string::String>,
    pub stream: Stream,
}
impl StreamInfo {
    pub fn builder() -> builder::StreamInfo {
        Default::default()
    }
}
#[doc = "`SyncEntry`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"mode\","]
#[doc = "        \"mtime_ns\","]
#[doc = "        \"pack_id\","]
#[doc = "        \"path\","]
#[doc = "        \"sha256\","]
#[doc = "        \"size\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"file\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"mode\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"maximum\": 4095.0,"]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"mtime_ns\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"pack_id\": {"]
#[doc = "          \"description\": \"The pack whose entry `path` holds this exact content.\","]
#[doc = "          \"$ref\": \"#/$defs/PackId\""]
#[doc = "        },"]
#[doc = "        \"path\": {"]
#[doc = "          \"description\": \"Absolute guest path.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"sha256\": {"]
#[doc = "          \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "        },"]
#[doc = "        \"size\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"path\","]
#[doc = "        \"target\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"symlink\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"path\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"target\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"mode\","]
#[doc = "        \"path\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"dir\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"mode\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"maximum\": 4095.0,"]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"path\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind")]
pub enum SyncEntry {
    #[serde(rename = "file")]
    File {
        mode: i64,
        mtime_ns: u64,
        #[doc = "The pack whose entry `path` holds this exact content."]
        pack_id: PackId,
        #[doc = "Absolute guest path."]
        path: ::std::string::String,
        sha256: Sha256Hex,
        size: u64,
    },
    #[serde(rename = "symlink")]
    Symlink {
        path: ::std::string::String,
        target: ::std::string::String,
    },
    #[serde(rename = "dir")]
    Dir {
        mode: i64,
        path: ::std::string::String,
    },
}
#[doc = "The object a sync writes to manifest_put_url. Restore = fetch this, then the referenced packs, then extract each file entry from its pack. Empty directories and symlinks are recreated from entries."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The object a sync writes to manifest_put_url. Restore = fetch this, then the referenced packs, then extract each file entry from its pack. Empty directories and symlinks are recreated from entries.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"created_at_wall_ms\","]
#[doc = "    \"entries\","]
#[doc = "    \"generation_id\","]
#[doc = "    \"manifest_id\","]
#[doc = "    \"pack_format\","]
#[doc = "    \"packs\","]
#[doc = "    \"roots\","]
#[doc = "    \"version\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"created_at_wall_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/WallMs\""]
#[doc = "    },"]
#[doc = "    \"entries\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/SyncEntry\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"generation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/GenerationId\""]
#[doc = "    },"]
#[doc = "    \"manifest_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    },"]
#[doc = "    \"pack_format\": {"]
#[doc = "      \"description\": \"POSIX pax tar, zstd-compressed; entry names are absolute guest paths without the leading slash.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"tar+zstd\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"packs\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"object\","]
#[doc = "        \"required\": ["]
#[doc = "          \"bytes\","]
#[doc = "          \"pack_id\""]
#[doc = "        ],"]
#[doc = "        \"properties\": {"]
#[doc = "          \"bytes\": {"]
#[doc = "            \"description\": \"Compressed size.\","]
#[doc = "            \"type\": \"integer\","]
#[doc = "            \"minimum\": 0.0"]
#[doc = "          },"]
#[doc = "          \"pack_id\": {"]
#[doc = "            \"$ref\": \"#/$defs/PackId\""]
#[doc = "          },"]
#[doc = "          \"sha256\": {"]
#[doc = "            \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "          }"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"parent_manifest_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    },"]
#[doc = "    \"roots\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"version\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"const\": 1"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SyncManifest {
    pub created_at_wall_ms: WallMs,
    pub entries: ::std::vec::Vec<SyncEntry>,
    pub generation_id: GenerationId,
    pub manifest_id: ManifestId,
    #[doc = "POSIX pax tar, zstd-compressed; entry names are absolute guest paths without the leading slash."]
    pub pack_format: SyncManifestPackFormat,
    pub packs: ::std::vec::Vec<SyncManifestPacksItem>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub parent_manifest_id: ::std::option::Option<ManifestId>,
    pub roots: ::std::vec::Vec<::std::string::String>,
    pub version: i64,
}
impl SyncManifest {
    pub fn builder() -> builder::SyncManifest {
        Default::default()
    }
}
#[doc = "POSIX pax tar, zstd-compressed; entry names are absolute guest paths without the leading slash."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"POSIX pax tar, zstd-compressed; entry names are absolute guest paths without the leading slash.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"tar+zstd\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum SyncManifestPackFormat {
    #[serde(rename = "tar+zstd")]
    TarZstd,
}
impl ::std::fmt::Display for SyncManifestPackFormat {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::TarZstd => f.write_str("tar+zstd"),
        }
    }
}
impl ::std::str::FromStr for SyncManifestPackFormat {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "tar+zstd" => Ok(Self::TarZstd),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SyncManifestPackFormat {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SyncManifestPackFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SyncManifestPackFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`SyncManifestPacksItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"pack_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"description\": \"Compressed size.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"pack_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/PackId\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SyncManifestPacksItem {
    #[doc = "Compressed size."]
    pub bytes: u64,
    pub pack_id: PackId,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub sha256: ::std::option::Option<Sha256Hex>,
}
impl SyncManifestPacksItem {
    pub fn builder() -> builder::SyncManifestPacksItem {
        Default::default()
    }
}
#[doc = "`SyncReason`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"turn_end\","]
#[doc = "    \"interval\","]
#[doc = "    \"before_release\","]
#[doc = "    \"explicit\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum SyncReason {
    #[serde(rename = "turn_end")]
    TurnEnd,
    #[serde(rename = "interval")]
    Interval,
    #[serde(rename = "before_release")]
    BeforeRelease,
    #[serde(rename = "explicit")]
    Explicit,
}
impl ::std::fmt::Display for SyncReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::TurnEnd => f.write_str("turn_end"),
            Self::Interval => f.write_str("interval"),
            Self::BeforeRelease => f.write_str("before_release"),
            Self::Explicit => f.write_str("explicit"),
        }
    }
}
impl ::std::str::FromStr for SyncReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "turn_end" => Ok(Self::TurnEnd),
            "interval" => Ok(Self::Interval),
            "before_release" => Ok(Self::BeforeRelease),
            "explicit" => Ok(Self::Explicit),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SyncReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SyncReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SyncReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Workspace sync: diff the sync scope against the last manifest, upload changed files as one pack plus a new manifest. Brain-driven (turn end, every sync_interval, before release/termination). Nothing is uploaded when nothing changed."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Workspace sync: diff the sync scope against the last manifest, upload changed files as one pack plus a new manifest. Brain-driven (turn end, every sync_interval, before release/termination). Nothing is uploaded when nothing changed.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"full\","]
#[doc = "    \"manifest_id\","]
#[doc = "    \"manifest_put_url\","]
#[doc = "    \"pack_id\","]
#[doc = "    \"pack_put_url\","]
#[doc = "    \"reason\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"full\": {"]
#[doc = "      \"description\": \"true = pack every file (compaction), not only changed ones. The new manifest then references a single pack.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"manifest_id\": {"]
#[doc = "      \"description\": \"Id for the manifest this sync would produce.\","]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    },"]
#[doc = "    \"manifest_put_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"pack_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/PackId\""]
#[doc = "    },"]
#[doc = "    \"pack_put_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"reason\": {"]
#[doc = "      \"$ref\": \"#/$defs/SyncReason\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SyncRequest {
    #[doc = "true = pack every file (compaction), not only changed ones. The new manifest then references a single pack."]
    pub full: bool,
    #[doc = "Id for the manifest this sync would produce."]
    pub manifest_id: ManifestId,
    pub manifest_put_url: ::std::string::String,
    pub pack_id: PackId,
    pub pack_put_url: ::std::string::String,
    pub reason: SyncReason,
}
impl SyncRequest {
    pub fn builder() -> builder::SyncRequest {
        Default::default()
    }
}
#[doc = "`SyncResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes_total\","]
#[doc = "    \"bytes_uploaded\","]
#[doc = "    \"changed\","]
#[doc = "    \"duration_ms\","]
#[doc = "    \"files_added\","]
#[doc = "    \"files_deleted\","]
#[doc = "    \"files_modified\","]
#[doc = "    \"files_total\","]
#[doc = "    \"manifest_id\","]
#[doc = "    \"packs_referenced\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes_total\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"bytes_uploaded\": {"]
#[doc = "      \"description\": \"Compressed pack bytes actually uploaded.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"changed\": {"]
#[doc = "      \"description\": \"false = nothing differed from the last manifest; no upload happened and manifest_id is the previous one.\","]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"duration_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"files_added\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"files_deleted\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"files_modified\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"files_total\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"manifest_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/ManifestId\""]
#[doc = "    },"]
#[doc = "    \"packs_referenced\": {"]
#[doc = "      \"description\": \"How many packs the new manifest points at. The brain compacts (full = true) when this grows large.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SyncResponse {
    pub bytes_total: u64,
    #[doc = "Compressed pack bytes actually uploaded."]
    pub bytes_uploaded: u64,
    #[doc = "false = nothing differed from the last manifest; no upload happened and manifest_id is the previous one."]
    pub changed: bool,
    pub duration_ms: u64,
    pub files_added: u64,
    pub files_deleted: u64,
    pub files_modified: u64,
    pub files_total: u64,
    pub manifest_id: ManifestId,
    #[doc = "How many packs the new manifest points at. The brain compacts (full = true) when this grows large."]
    pub packs_referenced: u64,
}
impl SyncResponse {
    pub fn builder() -> builder::SyncResponse {
        Default::default()
    }
}
#[doc = "`SyncScope`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"roots\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"exclude\": {"]
#[doc = "      \"description\": \"Gitignore-style patterns relative to each root.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"roots\": {"]
#[doc = "      \"description\": \"Absolute directories included in workspace sync (default: workspace and home).\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SyncScope {
    #[doc = "Gitignore-style patterns relative to each root."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub exclude: ::std::vec::Vec<::std::string::String>,
    #[doc = "Absolute directories included in workspace sync (default: workspace and home)."]
    pub roots: ::std::vec::Vec<::std::string::String>,
}
impl SyncScope {
    pub fn builder() -> builder::SyncScope {
        Default::default()
    }
}
#[doc = "`TerminalInfo`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"ended_at_monotonic_ms\","]
#[doc = "    \"outcome\","]
#[doc = "    \"usage\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"ended_at_monotonic_ms\": {"]
#[doc = "      \"$ref\": \"#/$defs/MonotonicMs\""]
#[doc = "    },"]
#[doc = "    \"error\": {"]
#[doc = "      \"description\": \"Present when outcome = failed.\","]
#[doc = "      \"$ref\": \"#/$defs/AbiError\""]
#[doc = "    },"]
#[doc = "    \"exit_code\": {"]
#[doc = "      \"description\": \"Process exit code for command-like tools; null when killed by a signal. Typed tools use 0 = ok, 1 = tool-level failure explained on stderr.\","]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"outcome\": {"]
#[doc = "      \"$ref\": \"#/$defs/Outcome\""]
#[doc = "    },"]
#[doc = "    \"output\": {"]
#[doc = "      \"description\": \"Typed result validated against the tool's `output_schema`. Present when outcome = completed.\""]
#[doc = "    },"]
#[doc = "    \"signal\": {"]
#[doc = "      \"description\": \"Terminating signal name (e.g. \\\"SIGKILL\\\") when applicable.\","]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"usage\": {"]
#[doc = "      \"$ref\": \"#/$defs/Usage\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct TerminalInfo {
    pub ended_at_monotonic_ms: MonotonicMs,
    #[doc = "Present when outcome = failed."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub error: ::std::option::Option<AbiError>,
    #[doc = "Process exit code for command-like tools; null when killed by a signal. Typed tools use 0 = ok, 1 = tool-level failure explained on stderr."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub exit_code: ::std::option::Option<i64>,
    pub outcome: Outcome,
    #[doc = "Typed result validated against the tool's `output_schema`. Present when outcome = completed."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub output: ::std::option::Option<::serde_json::Value>,
    #[doc = "Terminating signal name (e.g. \"SIGKILL\") when applicable."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub signal: ::std::option::Option<::std::string::String>,
    pub usage: Usage,
}
impl TerminalInfo {
    pub fn builder() -> builder::TerminalInfo {
        Default::default()
    }
}
#[doc = "The sealed tool set. Digest = SHA-256 over the RFC 8785 (JCS) canonical JSON of this object. Tools are sorted by name."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The sealed tool set. Digest = SHA-256 over the RFC 8785 (JCS) canonical JSON of this object. Tools are sorted by name.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"tools\","]
#[doc = "    \"version\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"tools\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolSpec\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"version\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"1\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ToolManifest {
    pub tools: ::std::vec::Vec<ToolSpec>,
    pub version: ToolManifestVersion,
}
impl ToolManifest {
    pub fn builder() -> builder::ToolManifest {
        Default::default()
    }
}
#[doc = "`ToolManifestVersion`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"1\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum ToolManifestVersion {
    #[serde(rename = "1")]
    X1,
}
impl ::std::fmt::Display for ToolManifestVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::X1 => f.write_str("1"),
        }
    }
}
impl ::std::str::FromStr for ToolManifestVersion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "1" => Ok(Self::X1),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolManifestVersion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolManifestVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolManifestVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ToolSpec`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"description\","]
#[doc = "    \"input_schema\","]
#[doc = "    \"name\","]
#[doc = "    \"output_schema\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"description\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"input_schema\": {"]
#[doc = "      \"description\": \"JSON Schema 2020-12 for `start.input`.\","]
#[doc = "      \"type\": \"object\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[a-z][a-z0-9_]{0,63}$\""]
#[doc = "    },"]
#[doc = "    \"output_schema\": {"]
#[doc = "      \"description\": \"JSON Schema 2020-12 for `TerminalInfo.output`.\","]
#[doc = "      \"type\": \"object\""]
#[doc = "    },"]
#[doc = "    \"streams\": {"]
#[doc = "      \"description\": \"Hint for the brain: whether stdout is UTF-8 text. Default text.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"text\","]
#[doc = "        \"binary\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ToolSpec {
    pub description: ::std::string::String,
    #[doc = "JSON Schema 2020-12 for `start.input`."]
    pub input_schema: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub name: ToolSpecName,
    #[doc = "JSON Schema 2020-12 for `TerminalInfo.output`."]
    pub output_schema: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    #[doc = "Hint for the brain: whether stdout is UTF-8 text. Default text."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub streams: ::std::option::Option<ToolSpecStreams>,
}
impl ToolSpec {
    pub fn builder() -> builder::ToolSpec {
        Default::default()
    }
}
#[doc = "`ToolSpecName`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[a-z][a-z0-9_]{0,63}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolSpecName(::std::string::String);
impl ::std::ops::Deref for ToolSpecName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolSpecName> for ::std::string::String {
    fn from(value: ToolSpecName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolSpecName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[a-z][a-z0-9_]{0,63}$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[a-z][a-z0-9_]{0,63}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolSpecName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolSpecName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolSpecName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolSpecName {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
#[doc = "Hint for the brain: whether stdout is UTF-8 text. Default text."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Hint for the brain: whether stdout is UTF-8 text. Default text.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"text\","]
#[doc = "    \"binary\""]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(
    :: serde :: Deserialize,
    :: serde :: Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum ToolSpecStreams {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "binary")]
    Binary,
}
impl ::std::fmt::Display for ToolSpecStreams {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Text => f.write_str("text"),
            Self::Binary => f.write_str("binary"),
        }
    }
}
impl ::std::str::FromStr for ToolSpecStreams {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "text" => Ok(Self::Text),
            "binary" => Ok(Self::Binary),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolSpecStreams {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolSpecStreams {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolSpecStreams {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Observation only; customer-controlled data; never billing authority (I9)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Observation only; customer-controlled data; never billing authority (I9).\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"wall_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"cpu_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_rss_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"wall_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cpu_ms: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_rss_bytes: ::std::option::Option<u64>,
    pub wall_ms: u64,
}
impl Usage {
    pub fn builder() -> builder::Usage {
        Default::default()
    }
}
#[doc = "Milliseconds since the Unix epoch, guest wall clock."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Milliseconds since the Unix epoch, guest wall clock.\","]
#[doc = "  \"type\": \"integer\","]
#[doc = "  \"minimum\": 0.0"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct WallMs(pub u64);
impl ::std::ops::Deref for WallMs {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}
impl ::std::convert::From<WallMs> for u64 {
    fn from(value: WallMs) -> Self {
        value.0
    }
}
impl ::std::convert::From<u64> for WallMs {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for WallMs {
    type Err = <u64 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for WallMs {
    type Error = <u64 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for WallMs {
    type Error = <u64 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for WallMs {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
#[doc = r" Types for composing complex structures."]
pub mod builder {
    #[derive(Clone, Debug)]
    pub struct AbiError {
        code: ::std::result::Result<super::ErrorCode, ::std::string::String>,
        details: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        message: ::std::result::Result<::std::string::String, ::std::string::String>,
        retryable: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for AbiError {
        fn default() -> Self {
            Self {
                code: Err("no value supplied for code".to_string()),
                details: Ok(Default::default()),
                message: Err("no value supplied for message".to_string()),
                retryable: Err("no value supplied for retryable".to_string()),
            }
        }
    }
    impl AbiError {
        pub fn code<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ErrorCode>,
            T::Error: ::std::fmt::Display,
        {
            self.code = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for code: {e}"));
            self
        }
        pub fn details<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::serde_json::Map<::std::string::String, ::serde_json::Value>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.details = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for details: {e}"));
            self
        }
        pub fn message<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.message = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for message: {e}"));
            self
        }
        pub fn retryable<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.retryable = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for retryable: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<AbiError> for super::AbiError {
        type Error = super::error::ConversionError;
        fn try_from(value: AbiError) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                code: value.code?,
                details: value.details?,
                message: value.message?,
                retryable: value.retryable?,
            })
        }
    }
    impl ::std::convert::From<super::AbiError> for AbiError {
        fn from(value: super::AbiError) -> Self {
            Self {
                code: Ok(value.code),
                details: Ok(value.details),
                message: Ok(value.message),
                retryable: Ok(value.retryable),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Bounds {
        grace_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        max_retained_bytes:
            ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        timeout_ms: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for Bounds {
        fn default() -> Self {
            Self {
                grace_ms: Ok(Default::default()),
                max_retained_bytes: Ok(Default::default()),
                timeout_ms: Ok(Default::default()),
            }
        }
    }
    impl Bounds {
        pub fn grace_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.grace_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for grace_ms: {e}"));
            self
        }
        pub fn max_retained_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.max_retained_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_retained_bytes: {e}")
            });
            self
        }
        pub fn timeout_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.timeout_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for timeout_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Bounds> for super::Bounds {
        type Error = super::error::ConversionError;
        fn try_from(value: Bounds) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                grace_ms: value.grace_ms?,
                max_retained_bytes: value.max_retained_bytes?,
                timeout_ms: value.timeout_ms?,
            })
        }
    }
    impl ::std::convert::From<super::Bounds> for Bounds {
        fn from(value: super::Bounds) -> Self {
            Self {
                grace_ms: Ok(value.grace_ms),
                max_retained_bytes: Ok(value.max_retained_bytes),
                timeout_ms: Ok(value.timeout_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CancelRequest {
        grace_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        operation_id: ::std::result::Result<super::OperationId, ::std::string::String>,
    }
    impl ::std::default::Default for CancelRequest {
        fn default() -> Self {
            Self {
                grace_ms: Ok(Default::default()),
                operation_id: Err("no value supplied for operation_id".to_string()),
            }
        }
    }
    impl CancelRequest {
        pub fn grace_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.grace_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for grace_ms: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationId>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CancelRequest> for super::CancelRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CancelRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                grace_ms: value.grace_ms?,
                operation_id: value.operation_id?,
            })
        }
    }
    impl ::std::convert::From<super::CancelRequest> for CancelRequest {
        fn from(value: super::CancelRequest) -> Self {
            Self {
                grace_ms: Ok(value.grace_ms),
                operation_id: Ok(value.operation_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CancelResponse {
        accepted: ::std::result::Result<bool, ::std::string::String>,
        view: ::std::result::Result<super::OperationView, ::std::string::String>,
    }
    impl ::std::default::Default for CancelResponse {
        fn default() -> Self {
            Self {
                accepted: Err("no value supplied for accepted".to_string()),
                view: Err("no value supplied for view".to_string()),
            }
        }
    }
    impl CancelResponse {
        pub fn accepted<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.accepted = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for accepted: {e}"));
            self
        }
        pub fn view<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationView>,
            T::Error: ::std::fmt::Display,
        {
            self.view = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for view: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CancelResponse> for super::CancelResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CancelResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                accepted: value.accepted?,
                view: value.view?,
            })
        }
    }
    impl ::std::convert::From<super::CancelResponse> for CancelResponse {
        fn from(value: super::CancelResponse) -> Self {
            Self {
                accepted: Ok(value.accepted),
                view: Ok(value.view),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Clock {
        monotonic_ms: ::std::result::Result<super::MonotonicMs, ::std::string::String>,
        wall_ms: ::std::result::Result<super::WallMs, ::std::string::String>,
    }
    impl ::std::default::Default for Clock {
        fn default() -> Self {
            Self {
                monotonic_ms: Err("no value supplied for monotonic_ms".to_string()),
                wall_ms: Err("no value supplied for wall_ms".to_string()),
            }
        }
    }
    impl Clock {
        pub fn monotonic_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::MonotonicMs>,
            T::Error: ::std::fmt::Display,
        {
            self.monotonic_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for monotonic_ms: {e}"));
            self
        }
        pub fn wall_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::WallMs>,
            T::Error: ::std::fmt::Display,
        {
            self.wall_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wall_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Clock> for super::Clock {
        type Error = super::error::ConversionError;
        fn try_from(value: Clock) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                monotonic_ms: value.monotonic_ms?,
                wall_ms: value.wall_ms?,
            })
        }
    }
    impl ::std::convert::From<super::Clock> for Clock {
        fn from(value: super::Clock) -> Self {
            Self {
                monotonic_ms: Ok(value.monotonic_ms),
                wall_ms: Ok(value.wall_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Cursor {
        offset: ::std::result::Result<u64, ::std::string::String>,
        stream: ::std::result::Result<super::Stream, ::std::string::String>,
    }
    impl ::std::default::Default for Cursor {
        fn default() -> Self {
            Self {
                offset: Err("no value supplied for offset".to_string()),
                stream: Err("no value supplied for stream".to_string()),
            }
        }
    }
    impl Cursor {
        pub fn offset<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.offset = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for offset: {e}"));
            self
        }
        pub fn stream<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Stream>,
            T::Error: ::std::fmt::Display,
        {
            self.stream = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for stream: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Cursor> for super::Cursor {
        type Error = super::error::ConversionError;
        fn try_from(value: Cursor) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                offset: value.offset?,
                stream: value.stream?,
            })
        }
    }
    impl ::std::convert::From<super::Cursor> for Cursor {
        fn from(value: super::Cursor) -> Self {
            Self {
                offset: Ok(value.offset),
                stream: Ok(value.stream),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EffectiveBounds {
        grace_ms: ::std::result::Result<u64, ::std::string::String>,
        max_retained_bytes: ::std::result::Result<u64, ::std::string::String>,
        timeout_ms: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for EffectiveBounds {
        fn default() -> Self {
            Self {
                grace_ms: Err("no value supplied for grace_ms".to_string()),
                max_retained_bytes: Err("no value supplied for max_retained_bytes".to_string()),
                timeout_ms: Err("no value supplied for timeout_ms".to_string()),
            }
        }
    }
    impl EffectiveBounds {
        pub fn grace_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.grace_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for grace_ms: {e}"));
            self
        }
        pub fn max_retained_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_retained_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_retained_bytes: {e}")
            });
            self
        }
        pub fn timeout_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.timeout_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for timeout_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<EffectiveBounds> for super::EffectiveBounds {
        type Error = super::error::ConversionError;
        fn try_from(
            value: EffectiveBounds,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                grace_ms: value.grace_ms?,
                max_retained_bytes: value.max_retained_bytes?,
                timeout_ms: value.timeout_ms?,
            })
        }
    }
    impl ::std::convert::From<super::EffectiveBounds> for EffectiveBounds {
        fn from(value: super::EffectiveBounds) -> Self {
            Self {
                grace_ms: Ok(value.grace_ms),
                max_retained_bytes: Ok(value.max_retained_bytes),
                timeout_ms: Ok(value.timeout_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HandStatusEvent {
        at_monotonic_ms: ::std::result::Result<super::MonotonicMs, ::std::string::String>,
        at_wall_ms: ::std::result::Result<super::WallMs, ::std::string::String>,
        boot_id: ::std::result::Result<super::BootId, ::std::string::String>,
        generation_id: ::std::result::Result<super::GenerationId, ::std::string::String>,
        idle_for_ms: ::std::result::Result<u64, ::std::string::String>,
        inflight: ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
        lanes_live: ::std::result::Result<u64, ::std::string::String>,
        live_jobs:
            ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
        operations_retained: ::std::result::Result<u64, ::std::string::String>,
        pressure:
            ::std::result::Result<::std::option::Option<super::Pressure>, ::std::string::String>,
        retained_bytes: ::std::result::Result<u64, ::std::string::String>,
        seq: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for HandStatusEvent {
        fn default() -> Self {
            Self {
                at_monotonic_ms: Err("no value supplied for at_monotonic_ms".to_string()),
                at_wall_ms: Err("no value supplied for at_wall_ms".to_string()),
                boot_id: Err("no value supplied for boot_id".to_string()),
                generation_id: Err("no value supplied for generation_id".to_string()),
                idle_for_ms: Err("no value supplied for idle_for_ms".to_string()),
                inflight: Err("no value supplied for inflight".to_string()),
                lanes_live: Err("no value supplied for lanes_live".to_string()),
                live_jobs: Err("no value supplied for live_jobs".to_string()),
                operations_retained: Err("no value supplied for operations_retained".to_string()),
                pressure: Ok(Default::default()),
                retained_bytes: Err("no value supplied for retained_bytes".to_string()),
                seq: Err("no value supplied for seq".to_string()),
            }
        }
    }
    impl HandStatusEvent {
        pub fn at_monotonic_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::MonotonicMs>,
            T::Error: ::std::fmt::Display,
        {
            self.at_monotonic_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for at_monotonic_ms: {e}"));
            self
        }
        pub fn at_wall_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::WallMs>,
            T::Error: ::std::fmt::Display,
        {
            self.at_wall_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for at_wall_ms: {e}"));
            self
        }
        pub fn boot_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::BootId>,
            T::Error: ::std::fmt::Display,
        {
            self.boot_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for boot_id: {e}"));
            self
        }
        pub fn generation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::GenerationId>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_id: {e}"));
            self
        }
        pub fn idle_for_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.idle_for_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for idle_for_ms: {e}"));
            self
        }
        pub fn inflight<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.inflight = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for inflight: {e}"));
            self
        }
        pub fn lanes_live<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.lanes_live = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for lanes_live: {e}"));
            self
        }
        pub fn live_jobs<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.live_jobs = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for live_jobs: {e}"));
            self
        }
        pub fn operations_retained<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.operations_retained = value.try_into().map_err(|e| {
                format!("error converting supplied value for operations_retained: {e}")
            });
            self
        }
        pub fn pressure<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Pressure>>,
            T::Error: ::std::fmt::Display,
        {
            self.pressure = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pressure: {e}"));
            self
        }
        pub fn retained_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.retained_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for retained_bytes: {e}"));
            self
        }
        pub fn seq<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.seq = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for seq: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<HandStatusEvent> for super::HandStatusEvent {
        type Error = super::error::ConversionError;
        fn try_from(
            value: HandStatusEvent,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                at_monotonic_ms: value.at_monotonic_ms?,
                at_wall_ms: value.at_wall_ms?,
                boot_id: value.boot_id?,
                generation_id: value.generation_id?,
                idle_for_ms: value.idle_for_ms?,
                inflight: value.inflight?,
                lanes_live: value.lanes_live?,
                live_jobs: value.live_jobs?,
                operations_retained: value.operations_retained?,
                pressure: value.pressure?,
                retained_bytes: value.retained_bytes?,
                seq: value.seq?,
            })
        }
    }
    impl ::std::convert::From<super::HandStatusEvent> for HandStatusEvent {
        fn from(value: super::HandStatusEvent) -> Self {
            Self {
                at_monotonic_ms: Ok(value.at_monotonic_ms),
                at_wall_ms: Ok(value.at_wall_ms),
                boot_id: Ok(value.boot_id),
                generation_id: Ok(value.generation_id),
                idle_for_ms: Ok(value.idle_for_ms),
                inflight: Ok(value.inflight),
                lanes_live: Ok(value.lanes_live),
                live_jobs: Ok(value.live_jobs),
                operations_retained: Ok(value.operations_retained),
                pressure: Ok(value.pressure),
                retained_bytes: Ok(value.retained_bytes),
                seq: Ok(value.seq),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HelloRequest {
        env: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        expected_generation_id: ::std::result::Result<
            ::std::option::Option<super::GenerationId>,
            ::std::string::String,
        >,
        heartbeat_ms: ::std::result::Result<i64, ::std::string::String>,
        protocol: ::std::result::Result<super::ProtocolVersion, ::std::string::String>,
        restore: ::std::result::Result<
            ::std::option::Option<super::RestoreSource>,
            ::std::string::String,
        >,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        session_token: ::std::result::Result<::std::string::String, ::std::string::String>,
        sync: ::std::result::Result<super::SyncScope, ::std::string::String>,
        tool_manifest_digest:
            ::std::result::Result<::std::option::Option<super::Sha256Hex>, ::std::string::String>,
    }
    impl ::std::default::Default for HelloRequest {
        fn default() -> Self {
            Self {
                env: Err("no value supplied for env".to_string()),
                expected_generation_id: Ok(Default::default()),
                heartbeat_ms: Err("no value supplied for heartbeat_ms".to_string()),
                protocol: Err("no value supplied for protocol".to_string()),
                restore: Ok(Default::default()),
                session_id: Err("no value supplied for session_id".to_string()),
                session_token: Err("no value supplied for session_token".to_string()),
                sync: Err("no value supplied for sync".to_string()),
                tool_manifest_digest: Ok(Default::default()),
            }
        }
    }
    impl HelloRequest {
        pub fn env<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.env = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for env: {e}"));
            self
        }
        pub fn expected_generation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::GenerationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation_id = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation_id: {e}")
            });
            self
        }
        pub fn heartbeat_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.heartbeat_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for heartbeat_ms: {e}"));
            self
        }
        pub fn protocol<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ProtocolVersion>,
            T::Error: ::std::fmt::Display,
        {
            self.protocol = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for protocol: {e}"));
            self
        }
        pub fn restore<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::RestoreSource>>,
            T::Error: ::std::fmt::Display,
        {
            self.restore = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for restore: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionId>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
        pub fn session_token<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.session_token = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_token: {e}"));
            self
        }
        pub fn sync<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SyncScope>,
            T::Error: ::std::fmt::Display,
        {
            self.sync = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sync: {e}"));
            self
        }
        pub fn tool_manifest_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Sha256Hex>>,
            T::Error: ::std::fmt::Display,
        {
            self.tool_manifest_digest = value.try_into().map_err(|e| {
                format!("error converting supplied value for tool_manifest_digest: {e}")
            });
            self
        }
    }
    impl ::std::convert::TryFrom<HelloRequest> for super::HelloRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: HelloRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                env: value.env?,
                expected_generation_id: value.expected_generation_id?,
                heartbeat_ms: value.heartbeat_ms?,
                protocol: value.protocol?,
                restore: value.restore?,
                session_id: value.session_id?,
                session_token: value.session_token?,
                sync: value.sync?,
                tool_manifest_digest: value.tool_manifest_digest?,
            })
        }
    }
    impl ::std::convert::From<super::HelloRequest> for HelloRequest {
        fn from(value: super::HelloRequest) -> Self {
            Self {
                env: Ok(value.env),
                expected_generation_id: Ok(value.expected_generation_id),
                heartbeat_ms: Ok(value.heartbeat_ms),
                protocol: Ok(value.protocol),
                restore: Ok(value.restore),
                session_id: Ok(value.session_id),
                session_token: Ok(value.session_token),
                sync: Ok(value.sync),
                tool_manifest_digest: Ok(value.tool_manifest_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HelloResponse {
        boot_id: ::std::result::Result<super::BootId, ::std::string::String>,
        clock: ::std::result::Result<super::Clock, ::std::string::String>,
        generation_id: ::std::result::Result<super::GenerationId, ::std::string::String>,
        lanes: ::std::result::Result<::std::vec::Vec<super::LaneSummary>, ::std::string::String>,
        limits: ::std::result::Result<super::Limits, ::std::string::String>,
        operations:
            ::std::result::Result<::std::vec::Vec<super::OperationView>, ::std::string::String>,
        paths: ::std::result::Result<super::Paths, ::std::string::String>,
        protocol: ::std::result::Result<super::ProtocolVersion, ::std::string::String>,
        restore: ::std::result::Result<
            ::std::option::Option<super::RestoreReport>,
            ::std::string::String,
        >,
        tool_manifest_digest: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        tools: ::std::result::Result<::std::vec::Vec<super::ToolSpec>, ::std::string::String>,
    }
    impl ::std::default::Default for HelloResponse {
        fn default() -> Self {
            Self {
                boot_id: Err("no value supplied for boot_id".to_string()),
                clock: Err("no value supplied for clock".to_string()),
                generation_id: Err("no value supplied for generation_id".to_string()),
                lanes: Err("no value supplied for lanes".to_string()),
                limits: Err("no value supplied for limits".to_string()),
                operations: Err("no value supplied for operations".to_string()),
                paths: Err("no value supplied for paths".to_string()),
                protocol: Err("no value supplied for protocol".to_string()),
                restore: Ok(Default::default()),
                tool_manifest_digest: Err("no value supplied for tool_manifest_digest".to_string()),
                tools: Err("no value supplied for tools".to_string()),
            }
        }
    }
    impl HelloResponse {
        pub fn boot_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::BootId>,
            T::Error: ::std::fmt::Display,
        {
            self.boot_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for boot_id: {e}"));
            self
        }
        pub fn clock<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Clock>,
            T::Error: ::std::fmt::Display,
        {
            self.clock = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for clock: {e}"));
            self
        }
        pub fn generation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::GenerationId>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_id: {e}"));
            self
        }
        pub fn lanes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::LaneSummary>>,
            T::Error: ::std::fmt::Display,
        {
            self.lanes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for lanes: {e}"));
            self
        }
        pub fn limits<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Limits>,
            T::Error: ::std::fmt::Display,
        {
            self.limits = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for limits: {e}"));
            self
        }
        pub fn operations<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationView>>,
            T::Error: ::std::fmt::Display,
        {
            self.operations = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operations: {e}"));
            self
        }
        pub fn paths<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Paths>,
            T::Error: ::std::fmt::Display,
        {
            self.paths = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for paths: {e}"));
            self
        }
        pub fn protocol<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ProtocolVersion>,
            T::Error: ::std::fmt::Display,
        {
            self.protocol = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for protocol: {e}"));
            self
        }
        pub fn restore<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::RestoreReport>>,
            T::Error: ::std::fmt::Display,
        {
            self.restore = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for restore: {e}"));
            self
        }
        pub fn tool_manifest_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.tool_manifest_digest = value.try_into().map_err(|e| {
                format!("error converting supplied value for tool_manifest_digest: {e}")
            });
            self
        }
        pub fn tools<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::ToolSpec>>,
            T::Error: ::std::fmt::Display,
        {
            self.tools = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tools: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<HelloResponse> for super::HelloResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: HelloResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                boot_id: value.boot_id?,
                clock: value.clock?,
                generation_id: value.generation_id?,
                lanes: value.lanes?,
                limits: value.limits?,
                operations: value.operations?,
                paths: value.paths?,
                protocol: value.protocol?,
                restore: value.restore?,
                tool_manifest_digest: value.tool_manifest_digest?,
                tools: value.tools?,
            })
        }
    }
    impl ::std::convert::From<super::HelloResponse> for HelloResponse {
        fn from(value: super::HelloResponse) -> Self {
            Self {
                boot_id: Ok(value.boot_id),
                clock: Ok(value.clock),
                generation_id: Ok(value.generation_id),
                lanes: Ok(value.lanes),
                limits: Ok(value.limits),
                operations: Ok(value.operations),
                paths: Ok(value.paths),
                protocol: Ok(value.protocol),
                restore: Ok(value.restore),
                tool_manifest_digest: Ok(value.tool_manifest_digest),
                tools: Ok(value.tools),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct LaneCloseRequest {
        grace_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        lane_id: ::std::result::Result<super::LaneId, ::std::string::String>,
    }
    impl ::std::default::Default for LaneCloseRequest {
        fn default() -> Self {
            Self {
                grace_ms: Ok(Default::default()),
                lane_id: Err("no value supplied for lane_id".to_string()),
            }
        }
    }
    impl LaneCloseRequest {
        pub fn grace_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.grace_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for grace_ms: {e}"));
            self
        }
        pub fn lane_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneId>,
            T::Error: ::std::fmt::Display,
        {
            self.lane_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for lane_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<LaneCloseRequest> for super::LaneCloseRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: LaneCloseRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                grace_ms: value.grace_ms?,
                lane_id: value.lane_id?,
            })
        }
    }
    impl ::std::convert::From<super::LaneCloseRequest> for LaneCloseRequest {
        fn from(value: super::LaneCloseRequest) -> Self {
            Self {
                grace_ms: Ok(value.grace_ms),
                lane_id: Ok(value.lane_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct LaneCloseResponse {
        cancelled_operations:
            ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
        closed: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for LaneCloseResponse {
        fn default() -> Self {
            Self {
                cancelled_operations: Err("no value supplied for cancelled_operations".to_string()),
                closed: Err("no value supplied for closed".to_string()),
            }
        }
    }
    impl LaneCloseResponse {
        pub fn cancelled_operations<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.cancelled_operations = value.try_into().map_err(|e| {
                format!("error converting supplied value for cancelled_operations: {e}")
            });
            self
        }
        pub fn closed<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.closed = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for closed: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<LaneCloseResponse> for super::LaneCloseResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: LaneCloseResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                cancelled_operations: value.cancelled_operations?,
                closed: value.closed?,
            })
        }
    }
    impl ::std::convert::From<super::LaneCloseResponse> for LaneCloseResponse {
        fn from(value: super::LaneCloseResponse) -> Self {
            Self {
                cancelled_operations: Ok(value.cancelled_operations),
                closed: Ok(value.closed),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct LaneRef {
        id: ::std::result::Result<super::LaneId, ::std::string::String>,
        mode: ::std::result::Result<super::LaneMode, ::std::string::String>,
        parent: ::std::result::Result<::std::option::Option<super::LaneId>, ::std::string::String>,
    }
    impl ::std::default::Default for LaneRef {
        fn default() -> Self {
            Self {
                id: Err("no value supplied for id".to_string()),
                mode: Err("no value supplied for mode".to_string()),
                parent: Ok(Default::default()),
            }
        }
    }
    impl LaneRef {
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
        pub fn mode<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneMode>,
            T::Error: ::std::fmt::Display,
        {
            self.mode = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for mode: {e}"));
            self
        }
        pub fn parent<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::LaneId>>,
            T::Error: ::std::fmt::Display,
        {
            self.parent = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for parent: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<LaneRef> for super::LaneRef {
        type Error = super::error::ConversionError;
        fn try_from(value: LaneRef) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                id: value.id?,
                mode: value.mode?,
                parent: value.parent?,
            })
        }
    }
    impl ::std::convert::From<super::LaneRef> for LaneRef {
        fn from(value: super::LaneRef) -> Self {
            Self {
                id: Ok(value.id),
                mode: Ok(value.mode),
                parent: Ok(value.parent),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct LaneSummary {
        created_at_monotonic_ms:
            ::std::result::Result<::std::option::Option<super::MonotonicMs>, ::std::string::String>,
        id: ::std::result::Result<super::LaneId, ::std::string::String>,
        inflight:
            ::std::result::Result<::std::option::Option<super::OperationId>, ::std::string::String>,
        mode: ::std::result::Result<super::LaneMode, ::std::string::String>,
        parent: ::std::result::Result<::std::option::Option<super::LaneId>, ::std::string::String>,
        state: ::std::result::Result<super::LaneSummaryState, ::std::string::String>,
    }
    impl ::std::default::Default for LaneSummary {
        fn default() -> Self {
            Self {
                created_at_monotonic_ms: Ok(Default::default()),
                id: Err("no value supplied for id".to_string()),
                inflight: Ok(Default::default()),
                mode: Err("no value supplied for mode".to_string()),
                parent: Ok(Default::default()),
                state: Err("no value supplied for state".to_string()),
            }
        }
    }
    impl LaneSummary {
        pub fn created_at_monotonic_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::MonotonicMs>>,
            T::Error: ::std::fmt::Display,
        {
            self.created_at_monotonic_ms = value.try_into().map_err(|e| {
                format!("error converting supplied value for created_at_monotonic_ms: {e}")
            });
            self
        }
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
        pub fn inflight<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.inflight = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for inflight: {e}"));
            self
        }
        pub fn mode<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneMode>,
            T::Error: ::std::fmt::Display,
        {
            self.mode = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for mode: {e}"));
            self
        }
        pub fn parent<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::LaneId>>,
            T::Error: ::std::fmt::Display,
        {
            self.parent = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for parent: {e}"));
            self
        }
        pub fn state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneSummaryState>,
            T::Error: ::std::fmt::Display,
        {
            self.state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for state: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<LaneSummary> for super::LaneSummary {
        type Error = super::error::ConversionError;
        fn try_from(
            value: LaneSummary,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                created_at_monotonic_ms: value.created_at_monotonic_ms?,
                id: value.id?,
                inflight: value.inflight?,
                mode: value.mode?,
                parent: value.parent?,
                state: value.state?,
            })
        }
    }
    impl ::std::convert::From<super::LaneSummary> for LaneSummary {
        fn from(value: super::LaneSummary) -> Self {
            Self {
                created_at_monotonic_ms: Ok(value.created_at_monotonic_ms),
                id: Ok(value.id),
                inflight: Ok(value.inflight),
                mode: Ok(value.mode),
                parent: Ok(value.parent),
                state: Ok(value.state),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Limits {
        default_bounds: ::std::result::Result<super::EffectiveBounds, ::std::string::String>,
        max_concurrent_operations:
            ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        max_frame_bytes: ::std::result::Result<i64, ::std::string::String>,
        max_inline_put_bytes: ::std::result::Result<u64, ::std::string::String>,
        max_lanes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        max_persist_bytes: ::std::result::Result<u64, ::std::string::String>,
        max_poll_wait_ms: ::std::result::Result<u64, ::std::string::String>,
        max_slice_bytes: ::std::result::Result<i64, ::std::string::String>,
    }
    impl ::std::default::Default for Limits {
        fn default() -> Self {
            Self {
                default_bounds: Err("no value supplied for default_bounds".to_string()),
                max_concurrent_operations: Err(
                    "no value supplied for max_concurrent_operations".to_string()
                ),
                max_frame_bytes: Err("no value supplied for max_frame_bytes".to_string()),
                max_inline_put_bytes: Err("no value supplied for max_inline_put_bytes".to_string()),
                max_lanes: Err("no value supplied for max_lanes".to_string()),
                max_persist_bytes: Err("no value supplied for max_persist_bytes".to_string()),
                max_poll_wait_ms: Err("no value supplied for max_poll_wait_ms".to_string()),
                max_slice_bytes: Err("no value supplied for max_slice_bytes".to_string()),
            }
        }
    }
    impl Limits {
        pub fn default_bounds<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::EffectiveBounds>,
            T::Error: ::std::fmt::Display,
        {
            self.default_bounds = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for default_bounds: {e}"));
            self
        }
        pub fn max_concurrent_operations<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_concurrent_operations = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_concurrent_operations: {e}")
            });
            self
        }
        pub fn max_frame_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_frame_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_frame_bytes: {e}"));
            self
        }
        pub fn max_inline_put_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_inline_put_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_inline_put_bytes: {e}")
            });
            self
        }
        pub fn max_lanes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_lanes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_lanes: {e}"));
            self
        }
        pub fn max_persist_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_persist_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_persist_bytes: {e}"));
            self
        }
        pub fn max_poll_wait_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_poll_wait_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_poll_wait_ms: {e}"));
            self
        }
        pub fn max_slice_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_slice_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_slice_bytes: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Limits> for super::Limits {
        type Error = super::error::ConversionError;
        fn try_from(value: Limits) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                default_bounds: value.default_bounds?,
                max_concurrent_operations: value.max_concurrent_operations?,
                max_frame_bytes: value.max_frame_bytes?,
                max_inline_put_bytes: value.max_inline_put_bytes?,
                max_lanes: value.max_lanes?,
                max_persist_bytes: value.max_persist_bytes?,
                max_poll_wait_ms: value.max_poll_wait_ms?,
                max_slice_bytes: value.max_slice_bytes?,
            })
        }
    }
    impl ::std::convert::From<super::Limits> for Limits {
        fn from(value: super::Limits) -> Self {
            Self {
                default_bounds: Ok(value.default_bounds),
                max_concurrent_operations: Ok(value.max_concurrent_operations),
                max_frame_bytes: Ok(value.max_frame_bytes),
                max_inline_put_bytes: Ok(value.max_inline_put_bytes),
                max_lanes: Ok(value.max_lanes),
                max_persist_bytes: Ok(value.max_persist_bytes),
                max_poll_wait_ms: Ok(value.max_poll_wait_ms),
                max_slice_bytes: Ok(value.max_slice_bytes),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OperationView {
        correlation: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        detach: ::std::result::Result<bool, ::std::string::String>,
        lane_id: ::std::result::Result<super::LaneId, ::std::string::String>,
        operation_id: ::std::result::Result<super::OperationId, ::std::string::String>,
        started_at_monotonic_ms: ::std::result::Result<super::MonotonicMs, ::std::string::String>,
        status: ::std::result::Result<super::OperationStatus, ::std::string::String>,
        streams: ::std::result::Result<::std::vec::Vec<super::StreamInfo>, ::std::string::String>,
        terminal: ::std::result::Result<
            ::std::option::Option<super::TerminalInfo>,
            ::std::string::String,
        >,
        tool: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for OperationView {
        fn default() -> Self {
            Self {
                correlation: Ok(Default::default()),
                detach: Err("no value supplied for detach".to_string()),
                lane_id: Err("no value supplied for lane_id".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                started_at_monotonic_ms: Err(
                    "no value supplied for started_at_monotonic_ms".to_string()
                ),
                status: Err("no value supplied for status".to_string()),
                streams: Err("no value supplied for streams".to_string()),
                terminal: Ok(Default::default()),
                tool: Err("no value supplied for tool".to_string()),
            }
        }
    }
    impl OperationView {
        pub fn correlation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::serde_json::Map<::std::string::String, ::serde_json::Value>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.correlation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for correlation: {e}"));
            self
        }
        pub fn detach<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.detach = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for detach: {e}"));
            self
        }
        pub fn lane_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneId>,
            T::Error: ::std::fmt::Display,
        {
            self.lane_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for lane_id: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationId>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn started_at_monotonic_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::MonotonicMs>,
            T::Error: ::std::fmt::Display,
        {
            self.started_at_monotonic_ms = value.try_into().map_err(|e| {
                format!("error converting supplied value for started_at_monotonic_ms: {e}")
            });
            self
        }
        pub fn status<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationStatus>,
            T::Error: ::std::fmt::Display,
        {
            self.status = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for status: {e}"));
            self
        }
        pub fn streams<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::StreamInfo>>,
            T::Error: ::std::fmt::Display,
        {
            self.streams = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for streams: {e}"));
            self
        }
        pub fn terminal<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::TerminalInfo>>,
            T::Error: ::std::fmt::Display,
        {
            self.terminal = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for terminal: {e}"));
            self
        }
        pub fn tool<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.tool = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tool: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OperationView> for super::OperationView {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OperationView,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                correlation: value.correlation?,
                detach: value.detach?,
                lane_id: value.lane_id?,
                operation_id: value.operation_id?,
                started_at_monotonic_ms: value.started_at_monotonic_ms?,
                status: value.status?,
                streams: value.streams?,
                terminal: value.terminal?,
                tool: value.tool?,
            })
        }
    }
    impl ::std::convert::From<super::OperationView> for OperationView {
        fn from(value: super::OperationView) -> Self {
            Self {
                correlation: Ok(value.correlation),
                detach: Ok(value.detach),
                lane_id: Ok(value.lane_id),
                operation_id: Ok(value.operation_id),
                started_at_monotonic_ms: Ok(value.started_at_monotonic_ms),
                status: Ok(value.status),
                streams: Ok(value.streams),
                terminal: Ok(value.terminal),
                tool: Ok(value.tool),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputSlice {
        data_base64: ::std::result::Result<::std::string::String, ::std::string::String>,
        eof: ::std::result::Result<bool, ::std::string::String>,
        offset: ::std::result::Result<u64, ::std::string::String>,
        stream: ::std::result::Result<super::Stream, ::std::string::String>,
    }
    impl ::std::default::Default for OutputSlice {
        fn default() -> Self {
            Self {
                data_base64: Err("no value supplied for data_base64".to_string()),
                eof: Err("no value supplied for eof".to_string()),
                offset: Err("no value supplied for offset".to_string()),
                stream: Err("no value supplied for stream".to_string()),
            }
        }
    }
    impl OutputSlice {
        pub fn data_base64<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.data_base64 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for data_base64: {e}"));
            self
        }
        pub fn eof<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.eof = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for eof: {e}"));
            self
        }
        pub fn offset<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.offset = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for offset: {e}"));
            self
        }
        pub fn stream<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Stream>,
            T::Error: ::std::fmt::Display,
        {
            self.stream = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for stream: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OutputSlice> for super::OutputSlice {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputSlice,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                data_base64: value.data_base64?,
                eof: value.eof?,
                offset: value.offset?,
                stream: value.stream?,
            })
        }
    }
    impl ::std::convert::From<super::OutputSlice> for OutputSlice {
        fn from(value: super::OutputSlice) -> Self {
            Self {
                data_base64: Ok(value.data_base64),
                eof: Ok(value.eof),
                offset: Ok(value.offset),
                stream: Ok(value.stream),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Paths {
        home: ::std::result::Result<::std::string::String, ::std::string::String>,
        spill_dir: ::std::result::Result<::std::string::String, ::std::string::String>,
        workspace: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for Paths {
        fn default() -> Self {
            Self {
                home: Err("no value supplied for home".to_string()),
                spill_dir: Err("no value supplied for spill_dir".to_string()),
                workspace: Err("no value supplied for workspace".to_string()),
            }
        }
    }
    impl Paths {
        pub fn home<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.home = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for home: {e}"));
            self
        }
        pub fn spill_dir<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.spill_dir = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for spill_dir: {e}"));
            self
        }
        pub fn workspace<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.workspace = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for workspace: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Paths> for super::Paths {
        type Error = super::error::ConversionError;
        fn try_from(value: Paths) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                home: value.home?,
                spill_dir: value.spill_dir?,
                workspace: value.workspace?,
            })
        }
    }
    impl ::std::convert::From<super::Paths> for Paths {
        fn from(value: super::Paths) -> Self {
            Self {
                home: Ok(value.home),
                spill_dir: Ok(value.spill_dir),
                workspace: Ok(value.workspace),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PersistItem {
        media_type: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::PersistItemName, ::std::string::String>,
        put_url: ::std::result::Result<::std::string::String, ::std::string::String>,
        source: ::std::result::Result<super::PersistSource, ::std::string::String>,
    }
    impl ::std::default::Default for PersistItem {
        fn default() -> Self {
            Self {
                media_type: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                put_url: Err("no value supplied for put_url".to_string()),
                source: Err("no value supplied for source".to_string()),
            }
        }
    }
    impl PersistItem {
        pub fn media_type<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.media_type = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for media_type: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PersistItemName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn put_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.put_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for put_url: {e}"));
            self
        }
        pub fn source<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PersistSource>,
            T::Error: ::std::fmt::Display,
        {
            self.source = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for source: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PersistItem> for super::PersistItem {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PersistItem,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                media_type: value.media_type?,
                name: value.name?,
                put_url: value.put_url?,
                source: value.source?,
            })
        }
    }
    impl ::std::convert::From<super::PersistItem> for PersistItem {
        fn from(value: super::PersistItem) -> Self {
            Self {
                media_type: Ok(value.media_type),
                name: Ok(value.name),
                put_url: Ok(value.put_url),
                source: Ok(value.source),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PersistRequest {
        items: ::std::result::Result<::std::vec::Vec<super::PersistItem>, ::std::string::String>,
    }
    impl ::std::default::Default for PersistRequest {
        fn default() -> Self {
            Self {
                items: Err("no value supplied for items".to_string()),
            }
        }
    }
    impl PersistRequest {
        pub fn items<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::PersistItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.items = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for items: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PersistRequest> for super::PersistRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PersistRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                items: value.items?,
            })
        }
    }
    impl ::std::convert::From<super::PersistRequest> for PersistRequest {
        fn from(value: super::PersistRequest) -> Self {
            Self {
                items: Ok(value.items),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PersistResponse {
        persisted: ::std::result::Result<
            ::std::vec::Vec<super::PersistResponsePersistedItem>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for PersistResponse {
        fn default() -> Self {
            Self {
                persisted: Err("no value supplied for persisted".to_string()),
            }
        }
    }
    impl PersistResponse {
        pub fn persisted<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::PersistResponsePersistedItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.persisted = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for persisted: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PersistResponse> for super::PersistResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PersistResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                persisted: value.persisted?,
            })
        }
    }
    impl ::std::convert::From<super::PersistResponse> for PersistResponse {
        fn from(value: super::PersistResponse) -> Self {
            Self {
                persisted: Ok(value.persisted),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PersistResponsePersistedItem {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        media_type: ::std::result::Result<::std::string::String, ::std::string::String>,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        sha256: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
    }
    impl ::std::default::Default for PersistResponsePersistedItem {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                media_type: Err("no value supplied for media_type".to_string()),
                name: Err("no value supplied for name".to_string()),
                sha256: Err("no value supplied for sha256".to_string()),
            }
        }
    }
    impl PersistResponsePersistedItem {
        pub fn bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes: {e}"));
            self
        }
        pub fn media_type<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.media_type = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for media_type: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PersistResponsePersistedItem> for super::PersistResponsePersistedItem {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PersistResponsePersistedItem,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                media_type: value.media_type?,
                name: value.name?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::PersistResponsePersistedItem> for PersistResponsePersistedItem {
        fn from(value: super::PersistResponsePersistedItem) -> Self {
            Self {
                bytes: Ok(value.bytes),
                media_type: Ok(value.media_type),
                name: Ok(value.name),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PollRequest {
        cursors: ::std::result::Result<::std::vec::Vec<super::Cursor>, ::std::string::String>,
        max_bytes: ::std::result::Result<u64, ::std::string::String>,
        operation_id: ::std::result::Result<super::OperationId, ::std::string::String>,
        wait_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for PollRequest {
        fn default() -> Self {
            Self {
                cursors: Err("no value supplied for cursors".to_string()),
                max_bytes: Err("no value supplied for max_bytes".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                wait_ms: Err("no value supplied for wait_ms".to_string()),
            }
        }
    }
    impl PollRequest {
        pub fn cursors<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::Cursor>>,
            T::Error: ::std::fmt::Display,
        {
            self.cursors = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for cursors: {e}"));
            self
        }
        pub fn max_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_bytes: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationId>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn wait_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.wait_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wait_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PollRequest> for super::PollRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PollRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                cursors: value.cursors?,
                max_bytes: value.max_bytes?,
                operation_id: value.operation_id?,
                wait_ms: value.wait_ms?,
            })
        }
    }
    impl ::std::convert::From<super::PollRequest> for PollRequest {
        fn from(value: super::PollRequest) -> Self {
            Self {
                cursors: Ok(value.cursors),
                max_bytes: Ok(value.max_bytes),
                operation_id: Ok(value.operation_id),
                wait_ms: Ok(value.wait_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PollResponse {
        slices: ::std::result::Result<::std::vec::Vec<super::OutputSlice>, ::std::string::String>,
        view: ::std::result::Result<super::OperationView, ::std::string::String>,
    }
    impl ::std::default::Default for PollResponse {
        fn default() -> Self {
            Self {
                slices: Err("no value supplied for slices".to_string()),
                view: Err("no value supplied for view".to_string()),
            }
        }
    }
    impl PollResponse {
        pub fn slices<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OutputSlice>>,
            T::Error: ::std::fmt::Display,
        {
            self.slices = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for slices: {e}"));
            self
        }
        pub fn view<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationView>,
            T::Error: ::std::fmt::Display,
        {
            self.view = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for view: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PollResponse> for super::PollResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PollResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                slices: value.slices?,
                view: value.view?,
            })
        }
    }
    impl ::std::convert::From<super::PollResponse> for PollResponse {
        fn from(value: super::PollResponse) -> Self {
            Self {
                slices: Ok(value.slices),
                view: Ok(value.view),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Pressure {
        mem_available_bytes: ::std::result::Result<u64, ::std::string::String>,
        psi_some_avg10: ::std::result::Result<::std::option::Option<f64>, ::std::string::String>,
        swap_used_bytes: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for Pressure {
        fn default() -> Self {
            Self {
                mem_available_bytes: Err("no value supplied for mem_available_bytes".to_string()),
                psi_some_avg10: Ok(Default::default()),
                swap_used_bytes: Err("no value supplied for swap_used_bytes".to_string()),
            }
        }
    }
    impl Pressure {
        pub fn mem_available_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.mem_available_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for mem_available_bytes: {e}")
            });
            self
        }
        pub fn psi_some_avg10<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<f64>>,
            T::Error: ::std::fmt::Display,
        {
            self.psi_some_avg10 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for psi_some_avg10: {e}"));
            self
        }
        pub fn swap_used_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.swap_used_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for swap_used_bytes: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Pressure> for super::Pressure {
        type Error = super::error::ConversionError;
        fn try_from(value: Pressure) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                mem_available_bytes: value.mem_available_bytes?,
                psi_some_avg10: value.psi_some_avg10?,
                swap_used_bytes: value.swap_used_bytes?,
            })
        }
    }
    impl ::std::convert::From<super::Pressure> for Pressure {
        fn from(value: super::Pressure) -> Self {
            Self {
                mem_available_bytes: Ok(value.mem_available_bytes),
                psi_some_avg10: Ok(value.psi_some_avg10),
                swap_used_bytes: Ok(value.swap_used_bytes),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ProtocolVersion {
        major: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        minor: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for ProtocolVersion {
        fn default() -> Self {
            Self {
                major: Err("no value supplied for major".to_string()),
                minor: Err("no value supplied for minor".to_string()),
            }
        }
    }
    impl ProtocolVersion {
        pub fn major<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.major = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for major: {e}"));
            self
        }
        pub fn minor<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.minor = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for minor: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ProtocolVersion> for super::ProtocolVersion {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ProtocolVersion,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                major: value.major?,
                minor: value.minor?,
            })
        }
    }
    impl ::std::convert::From<super::ProtocolVersion> for ProtocolVersion {
        fn from(value: super::ProtocolVersion) -> Self {
            Self {
                major: Ok(value.major),
                minor: Ok(value.minor),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PutFile {
        mode: ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
        source: ::std::result::Result<super::PutSource, ::std::string::String>,
    }
    impl ::std::default::Default for PutFile {
        fn default() -> Self {
            Self {
                mode: Ok(Default::default()),
                path: Err("no value supplied for path".to_string()),
                source: Err("no value supplied for source".to_string()),
            }
        }
    }
    impl PutFile {
        pub fn mode<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<i64>>,
            T::Error: ::std::fmt::Display,
        {
            self.mode = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for mode: {e}"));
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn source<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PutSource>,
            T::Error: ::std::fmt::Display,
        {
            self.source = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for source: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PutFile> for super::PutFile {
        type Error = super::error::ConversionError;
        fn try_from(value: PutFile) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                mode: value.mode?,
                path: value.path?,
                source: value.source?,
            })
        }
    }
    impl ::std::convert::From<super::PutFile> for PutFile {
        fn from(value: super::PutFile) -> Self {
            Self {
                mode: Ok(value.mode),
                path: Ok(value.path),
                source: Ok(value.source),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PutRequest {
        files: ::std::result::Result<::std::vec::Vec<super::PutFile>, ::std::string::String>,
    }
    impl ::std::default::Default for PutRequest {
        fn default() -> Self {
            Self {
                files: Err("no value supplied for files".to_string()),
            }
        }
    }
    impl PutRequest {
        pub fn files<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::PutFile>>,
            T::Error: ::std::fmt::Display,
        {
            self.files = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PutRequest> for super::PutRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PutRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                files: value.files?,
            })
        }
    }
    impl ::std::convert::From<super::PutRequest> for PutRequest {
        fn from(value: super::PutRequest) -> Self {
            Self {
                files: Ok(value.files),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PutResponse {
        written: ::std::result::Result<
            ::std::vec::Vec<super::PutResponseWrittenItem>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for PutResponse {
        fn default() -> Self {
            Self {
                written: Err("no value supplied for written".to_string()),
            }
        }
    }
    impl PutResponse {
        pub fn written<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::PutResponseWrittenItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.written = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for written: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PutResponse> for super::PutResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PutResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                written: value.written?,
            })
        }
    }
    impl ::std::convert::From<super::PutResponse> for PutResponse {
        fn from(value: super::PutResponse) -> Self {
            Self {
                written: Ok(value.written),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PutResponseWrittenItem {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
        sha256: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
    }
    impl ::std::default::Default for PutResponseWrittenItem {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                path: Err("no value supplied for path".to_string()),
                sha256: Err("no value supplied for sha256".to_string()),
            }
        }
    }
    impl PutResponseWrittenItem {
        pub fn bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes: {e}"));
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PutResponseWrittenItem> for super::PutResponseWrittenItem {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PutResponseWrittenItem,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                path: value.path?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::PutResponseWrittenItem> for PutResponseWrittenItem {
        fn from(value: super::PutResponseWrittenItem) -> Self {
            Self {
                bytes: Ok(value.bytes),
                path: Ok(value.path),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ReleaseRequest {
        operation_ids:
            ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
    }
    impl ::std::default::Default for ReleaseRequest {
        fn default() -> Self {
            Self {
                operation_ids: Err("no value supplied for operation_ids".to_string()),
            }
        }
    }
    impl ReleaseRequest {
        pub fn operation_ids<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_ids = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_ids: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ReleaseRequest> for super::ReleaseRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ReleaseRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                operation_ids: value.operation_ids?,
            })
        }
    }
    impl ::std::convert::From<super::ReleaseRequest> for ReleaseRequest {
        fn from(value: super::ReleaseRequest) -> Self {
            Self {
                operation_ids: Ok(value.operation_ids),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ReleaseResponse {
        released: ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
        unknown: ::std::result::Result<::std::vec::Vec<super::OperationId>, ::std::string::String>,
    }
    impl ::std::default::Default for ReleaseResponse {
        fn default() -> Self {
            Self {
                released: Err("no value supplied for released".to_string()),
                unknown: Err("no value supplied for unknown".to_string()),
            }
        }
    }
    impl ReleaseResponse {
        pub fn released<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.released = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for released: {e}"));
            self
        }
        pub fn unknown<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OperationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.unknown = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for unknown: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ReleaseResponse> for super::ReleaseResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ReleaseResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                released: value.released?,
                unknown: value.unknown?,
            })
        }
    }
    impl ::std::convert::From<super::ReleaseResponse> for ReleaseResponse {
        fn from(value: super::ReleaseResponse) -> Self {
            Self {
                released: Ok(value.released),
                unknown: Ok(value.unknown),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Request {
        call: ::std::result::Result<super::RequestCall, ::std::string::String>,
        fence: ::std::result::Result<u64, ::std::string::String>,
        generation_id: ::std::result::Result<
            ::std::option::Option<super::GenerationId>,
            ::std::string::String,
        >,
        id: ::std::result::Result<super::RequestId, ::std::string::String>,
    }
    impl ::std::default::Default for Request {
        fn default() -> Self {
            Self {
                call: Err("no value supplied for call".to_string()),
                fence: Err("no value supplied for fence".to_string()),
                generation_id: Ok(Default::default()),
                id: Err("no value supplied for id".to_string()),
            }
        }
    }
    impl Request {
        pub fn call<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::RequestCall>,
            T::Error: ::std::fmt::Display,
        {
            self.call = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for call: {e}"));
            self
        }
        pub fn fence<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.fence = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for fence: {e}"));
            self
        }
        pub fn generation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::GenerationId>>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_id: {e}"));
            self
        }
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::RequestId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Request> for super::Request {
        type Error = super::error::ConversionError;
        fn try_from(value: Request) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                call: value.call?,
                fence: value.fence?,
                generation_id: value.generation_id?,
                id: value.id?,
            })
        }
    }
    impl ::std::convert::From<super::Request> for Request {
        fn from(value: super::Request) -> Self {
            Self {
                call: Ok(value.call),
                fence: Ok(value.fence),
                generation_id: Ok(value.generation_id),
                id: Ok(value.id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Response {
        id: ::std::result::Result<super::RequestId, ::std::string::String>,
        result: ::std::result::Result<super::ResponseResult, ::std::string::String>,
    }
    impl ::std::default::Default for Response {
        fn default() -> Self {
            Self {
                id: Err("no value supplied for id".to_string()),
                result: Err("no value supplied for result".to_string()),
            }
        }
    }
    impl Response {
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::RequestId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
        pub fn result<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResponseResult>,
            T::Error: ::std::fmt::Display,
        {
            self.result = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for result: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Response> for super::Response {
        type Error = super::error::ConversionError;
        fn try_from(value: Response) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                id: value.id?,
                result: value.result?,
            })
        }
    }
    impl ::std::convert::From<super::Response> for Response {
        fn from(value: super::Response) -> Self {
            Self {
                id: Ok(value.id),
                result: Ok(value.result),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct RestoreReport {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        duration_ms: ::std::result::Result<u64, ::std::string::String>,
        files: ::std::result::Result<u64, ::std::string::String>,
        manifest_id: ::std::result::Result<super::ManifestId, ::std::string::String>,
    }
    impl ::std::default::Default for RestoreReport {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                duration_ms: Err("no value supplied for duration_ms".to_string()),
                files: Err("no value supplied for files".to_string()),
                manifest_id: Err("no value supplied for manifest_id".to_string()),
            }
        }
    }
    impl RestoreReport {
        pub fn bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes: {e}"));
            self
        }
        pub fn duration_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.duration_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for duration_ms: {e}"));
            self
        }
        pub fn files<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.files = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files: {e}"));
            self
        }
        pub fn manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ManifestId>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<RestoreReport> for super::RestoreReport {
        type Error = super::error::ConversionError;
        fn try_from(
            value: RestoreReport,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                duration_ms: value.duration_ms?,
                files: value.files?,
                manifest_id: value.manifest_id?,
            })
        }
    }
    impl ::std::convert::From<super::RestoreReport> for RestoreReport {
        fn from(value: super::RestoreReport) -> Self {
            Self {
                bytes: Ok(value.bytes),
                duration_ms: Ok(value.duration_ms),
                files: Ok(value.files),
                manifest_id: Ok(value.manifest_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct RestoreSource {
        manifest_get_url: ::std::result::Result<::std::string::String, ::std::string::String>,
        manifest_id: ::std::result::Result<super::ManifestId, ::std::string::String>,
        packs: ::std::result::Result<
            ::std::vec::Vec<super::RestoreSourcePacksItem>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for RestoreSource {
        fn default() -> Self {
            Self {
                manifest_get_url: Err("no value supplied for manifest_get_url".to_string()),
                manifest_id: Err("no value supplied for manifest_id".to_string()),
                packs: Err("no value supplied for packs".to_string()),
            }
        }
    }
    impl RestoreSource {
        pub fn manifest_get_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_get_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_get_url: {e}"));
            self
        }
        pub fn manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ManifestId>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_id: {e}"));
            self
        }
        pub fn packs<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::RestoreSourcePacksItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.packs = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for packs: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<RestoreSource> for super::RestoreSource {
        type Error = super::error::ConversionError;
        fn try_from(
            value: RestoreSource,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                manifest_get_url: value.manifest_get_url?,
                manifest_id: value.manifest_id?,
                packs: value.packs?,
            })
        }
    }
    impl ::std::convert::From<super::RestoreSource> for RestoreSource {
        fn from(value: super::RestoreSource) -> Self {
            Self {
                manifest_get_url: Ok(value.manifest_get_url),
                manifest_id: Ok(value.manifest_id),
                packs: Ok(value.packs),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct RestoreSourcePacksItem {
        get_url: ::std::result::Result<::std::string::String, ::std::string::String>,
        pack_id: ::std::result::Result<super::PackId, ::std::string::String>,
    }
    impl ::std::default::Default for RestoreSourcePacksItem {
        fn default() -> Self {
            Self {
                get_url: Err("no value supplied for get_url".to_string()),
                pack_id: Err("no value supplied for pack_id".to_string()),
            }
        }
    }
    impl RestoreSourcePacksItem {
        pub fn get_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.get_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for get_url: {e}"));
            self
        }
        pub fn pack_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PackId>,
            T::Error: ::std::fmt::Display,
        {
            self.pack_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pack_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<RestoreSourcePacksItem> for super::RestoreSourcePacksItem {
        type Error = super::error::ConversionError;
        fn try_from(
            value: RestoreSourcePacksItem,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                get_url: value.get_url?,
                pack_id: value.pack_id?,
            })
        }
    }
    impl ::std::convert::From<super::RestoreSourcePacksItem> for RestoreSourcePacksItem {
        fn from(value: super::RestoreSourcePacksItem) -> Self {
            Self {
                get_url: Ok(value.get_url),
                pack_id: Ok(value.pack_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StartRequest {
        batch_id:
            ::std::result::Result<::std::option::Option<super::BatchId>, ::std::string::String>,
        bounds: ::std::result::Result<::std::option::Option<super::Bounds>, ::std::string::String>,
        call_hash: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        correlation: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        cwd: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        detach: ::std::result::Result<bool, ::std::string::String>,
        input: ::std::result::Result<::serde_json::Value, ::std::string::String>,
        lane: ::std::result::Result<super::LaneRef, ::std::string::String>,
        max_bytes: ::std::result::Result<u64, ::std::string::String>,
        operation_id: ::std::result::Result<super::OperationId, ::std::string::String>,
        tool: ::std::result::Result<::std::string::String, ::std::string::String>,
        wait_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for StartRequest {
        fn default() -> Self {
            Self {
                batch_id: Ok(Default::default()),
                bounds: Ok(Default::default()),
                call_hash: Err("no value supplied for call_hash".to_string()),
                correlation: Ok(Default::default()),
                cwd: Ok(Default::default()),
                detach: Err("no value supplied for detach".to_string()),
                input: Err("no value supplied for input".to_string()),
                lane: Err("no value supplied for lane".to_string()),
                max_bytes: Err("no value supplied for max_bytes".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                tool: Err("no value supplied for tool".to_string()),
                wait_ms: Err("no value supplied for wait_ms".to_string()),
            }
        }
    }
    impl StartRequest {
        pub fn batch_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::BatchId>>,
            T::Error: ::std::fmt::Display,
        {
            self.batch_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for batch_id: {e}"));
            self
        }
        pub fn bounds<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Bounds>>,
            T::Error: ::std::fmt::Display,
        {
            self.bounds = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bounds: {e}"));
            self
        }
        pub fn call_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.call_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for call_hash: {e}"));
            self
        }
        pub fn correlation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::serde_json::Map<::std::string::String, ::serde_json::Value>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.correlation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for correlation: {e}"));
            self
        }
        pub fn cwd<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.cwd = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for cwd: {e}"));
            self
        }
        pub fn detach<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.detach = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for detach: {e}"));
            self
        }
        pub fn input<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::serde_json::Value>,
            T::Error: ::std::fmt::Display,
        {
            self.input = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input: {e}"));
            self
        }
        pub fn lane<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::LaneRef>,
            T::Error: ::std::fmt::Display,
        {
            self.lane = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for lane: {e}"));
            self
        }
        pub fn max_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_bytes: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationId>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn tool<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.tool = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tool: {e}"));
            self
        }
        pub fn wait_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.wait_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wait_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<StartRequest> for super::StartRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: StartRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                batch_id: value.batch_id?,
                bounds: value.bounds?,
                call_hash: value.call_hash?,
                correlation: value.correlation?,
                cwd: value.cwd?,
                detach: value.detach?,
                input: value.input?,
                lane: value.lane?,
                max_bytes: value.max_bytes?,
                operation_id: value.operation_id?,
                tool: value.tool?,
                wait_ms: value.wait_ms?,
            })
        }
    }
    impl ::std::convert::From<super::StartRequest> for StartRequest {
        fn from(value: super::StartRequest) -> Self {
            Self {
                batch_id: Ok(value.batch_id),
                bounds: Ok(value.bounds),
                call_hash: Ok(value.call_hash),
                correlation: Ok(value.correlation),
                cwd: Ok(value.cwd),
                detach: Ok(value.detach),
                input: Ok(value.input),
                lane: Ok(value.lane),
                max_bytes: Ok(value.max_bytes),
                operation_id: Ok(value.operation_id),
                tool: Ok(value.tool),
                wait_ms: Ok(value.wait_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StartResponse {
        replayed: ::std::result::Result<bool, ::std::string::String>,
        slices: ::std::result::Result<::std::vec::Vec<super::OutputSlice>, ::std::string::String>,
        view: ::std::result::Result<super::OperationView, ::std::string::String>,
    }
    impl ::std::default::Default for StartResponse {
        fn default() -> Self {
            Self {
                replayed: Err("no value supplied for replayed".to_string()),
                slices: Err("no value supplied for slices".to_string()),
                view: Err("no value supplied for view".to_string()),
            }
        }
    }
    impl StartResponse {
        pub fn replayed<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.replayed = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for replayed: {e}"));
            self
        }
        pub fn slices<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OutputSlice>>,
            T::Error: ::std::fmt::Display,
        {
            self.slices = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for slices: {e}"));
            self
        }
        pub fn view<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationView>,
            T::Error: ::std::fmt::Display,
        {
            self.view = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for view: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<StartResponse> for super::StartResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: StartResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                replayed: value.replayed?,
                slices: value.slices?,
                view: value.view?,
            })
        }
    }
    impl ::std::convert::From<super::StartResponse> for StartResponse {
        fn from(value: super::StartResponse) -> Self {
            Self {
                replayed: Ok(value.replayed),
                slices: Ok(value.slices),
                view: Ok(value.view),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamInfo {
        produced_bytes: ::std::result::Result<u64, ::std::string::String>,
        retained_from: ::std::result::Result<u64, ::std::string::String>,
        sha256:
            ::std::result::Result<::std::option::Option<super::Sha256Hex>, ::std::string::String>,
        spill_path: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        stream: ::std::result::Result<super::Stream, ::std::string::String>,
    }
    impl ::std::default::Default for StreamInfo {
        fn default() -> Self {
            Self {
                produced_bytes: Err("no value supplied for produced_bytes".to_string()),
                retained_from: Err("no value supplied for retained_from".to_string()),
                sha256: Ok(Default::default()),
                spill_path: Ok(Default::default()),
                stream: Err("no value supplied for stream".to_string()),
            }
        }
    }
    impl StreamInfo {
        pub fn produced_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.produced_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for produced_bytes: {e}"));
            self
        }
        pub fn retained_from<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.retained_from = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for retained_from: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Sha256Hex>>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
        pub fn spill_path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.spill_path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for spill_path: {e}"));
            self
        }
        pub fn stream<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Stream>,
            T::Error: ::std::fmt::Display,
        {
            self.stream = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for stream: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<StreamInfo> for super::StreamInfo {
        type Error = super::error::ConversionError;
        fn try_from(
            value: StreamInfo,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                produced_bytes: value.produced_bytes?,
                retained_from: value.retained_from?,
                sha256: value.sha256?,
                spill_path: value.spill_path?,
                stream: value.stream?,
            })
        }
    }
    impl ::std::convert::From<super::StreamInfo> for StreamInfo {
        fn from(value: super::StreamInfo) -> Self {
            Self {
                produced_bytes: Ok(value.produced_bytes),
                retained_from: Ok(value.retained_from),
                sha256: Ok(value.sha256),
                spill_path: Ok(value.spill_path),
                stream: Ok(value.stream),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SyncManifest {
        created_at_wall_ms: ::std::result::Result<super::WallMs, ::std::string::String>,
        entries: ::std::result::Result<::std::vec::Vec<super::SyncEntry>, ::std::string::String>,
        generation_id: ::std::result::Result<super::GenerationId, ::std::string::String>,
        manifest_id: ::std::result::Result<super::ManifestId, ::std::string::String>,
        pack_format: ::std::result::Result<super::SyncManifestPackFormat, ::std::string::String>,
        packs: ::std::result::Result<
            ::std::vec::Vec<super::SyncManifestPacksItem>,
            ::std::string::String,
        >,
        parent_manifest_id:
            ::std::result::Result<::std::option::Option<super::ManifestId>, ::std::string::String>,
        roots: ::std::result::Result<::std::vec::Vec<::std::string::String>, ::std::string::String>,
        version: ::std::result::Result<i64, ::std::string::String>,
    }
    impl ::std::default::Default for SyncManifest {
        fn default() -> Self {
            Self {
                created_at_wall_ms: Err("no value supplied for created_at_wall_ms".to_string()),
                entries: Err("no value supplied for entries".to_string()),
                generation_id: Err("no value supplied for generation_id".to_string()),
                manifest_id: Err("no value supplied for manifest_id".to_string()),
                pack_format: Err("no value supplied for pack_format".to_string()),
                packs: Err("no value supplied for packs".to_string()),
                parent_manifest_id: Ok(Default::default()),
                roots: Err("no value supplied for roots".to_string()),
                version: Err("no value supplied for version".to_string()),
            }
        }
    }
    impl SyncManifest {
        pub fn created_at_wall_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::WallMs>,
            T::Error: ::std::fmt::Display,
        {
            self.created_at_wall_ms = value.try_into().map_err(|e| {
                format!("error converting supplied value for created_at_wall_ms: {e}")
            });
            self
        }
        pub fn entries<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::SyncEntry>>,
            T::Error: ::std::fmt::Display,
        {
            self.entries = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for entries: {e}"));
            self
        }
        pub fn generation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::GenerationId>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_id: {e}"));
            self
        }
        pub fn manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ManifestId>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_id: {e}"));
            self
        }
        pub fn pack_format<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SyncManifestPackFormat>,
            T::Error: ::std::fmt::Display,
        {
            self.pack_format = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pack_format: {e}"));
            self
        }
        pub fn packs<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::SyncManifestPacksItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.packs = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for packs: {e}"));
            self
        }
        pub fn parent_manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ManifestId>>,
            T::Error: ::std::fmt::Display,
        {
            self.parent_manifest_id = value.try_into().map_err(|e| {
                format!("error converting supplied value for parent_manifest_id: {e}")
            });
            self
        }
        pub fn roots<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.roots = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for roots: {e}"));
            self
        }
        pub fn version<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.version = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for version: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SyncManifest> for super::SyncManifest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SyncManifest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                created_at_wall_ms: value.created_at_wall_ms?,
                entries: value.entries?,
                generation_id: value.generation_id?,
                manifest_id: value.manifest_id?,
                pack_format: value.pack_format?,
                packs: value.packs?,
                parent_manifest_id: value.parent_manifest_id?,
                roots: value.roots?,
                version: value.version?,
            })
        }
    }
    impl ::std::convert::From<super::SyncManifest> for SyncManifest {
        fn from(value: super::SyncManifest) -> Self {
            Self {
                created_at_wall_ms: Ok(value.created_at_wall_ms),
                entries: Ok(value.entries),
                generation_id: Ok(value.generation_id),
                manifest_id: Ok(value.manifest_id),
                pack_format: Ok(value.pack_format),
                packs: Ok(value.packs),
                parent_manifest_id: Ok(value.parent_manifest_id),
                roots: Ok(value.roots),
                version: Ok(value.version),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SyncManifestPacksItem {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        pack_id: ::std::result::Result<super::PackId, ::std::string::String>,
        sha256:
            ::std::result::Result<::std::option::Option<super::Sha256Hex>, ::std::string::String>,
    }
    impl ::std::default::Default for SyncManifestPacksItem {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                pack_id: Err("no value supplied for pack_id".to_string()),
                sha256: Ok(Default::default()),
            }
        }
    }
    impl SyncManifestPacksItem {
        pub fn bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes: {e}"));
            self
        }
        pub fn pack_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PackId>,
            T::Error: ::std::fmt::Display,
        {
            self.pack_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pack_id: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Sha256Hex>>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SyncManifestPacksItem> for super::SyncManifestPacksItem {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SyncManifestPacksItem,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                pack_id: value.pack_id?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::SyncManifestPacksItem> for SyncManifestPacksItem {
        fn from(value: super::SyncManifestPacksItem) -> Self {
            Self {
                bytes: Ok(value.bytes),
                pack_id: Ok(value.pack_id),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SyncRequest {
        full: ::std::result::Result<bool, ::std::string::String>,
        manifest_id: ::std::result::Result<super::ManifestId, ::std::string::String>,
        manifest_put_url: ::std::result::Result<::std::string::String, ::std::string::String>,
        pack_id: ::std::result::Result<super::PackId, ::std::string::String>,
        pack_put_url: ::std::result::Result<::std::string::String, ::std::string::String>,
        reason: ::std::result::Result<super::SyncReason, ::std::string::String>,
    }
    impl ::std::default::Default for SyncRequest {
        fn default() -> Self {
            Self {
                full: Err("no value supplied for full".to_string()),
                manifest_id: Err("no value supplied for manifest_id".to_string()),
                manifest_put_url: Err("no value supplied for manifest_put_url".to_string()),
                pack_id: Err("no value supplied for pack_id".to_string()),
                pack_put_url: Err("no value supplied for pack_put_url".to_string()),
                reason: Err("no value supplied for reason".to_string()),
            }
        }
    }
    impl SyncRequest {
        pub fn full<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.full = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for full: {e}"));
            self
        }
        pub fn manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ManifestId>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_id: {e}"));
            self
        }
        pub fn manifest_put_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_put_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_put_url: {e}"));
            self
        }
        pub fn pack_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::PackId>,
            T::Error: ::std::fmt::Display,
        {
            self.pack_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pack_id: {e}"));
            self
        }
        pub fn pack_put_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.pack_put_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for pack_put_url: {e}"));
            self
        }
        pub fn reason<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SyncReason>,
            T::Error: ::std::fmt::Display,
        {
            self.reason = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for reason: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SyncRequest> for super::SyncRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SyncRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                full: value.full?,
                manifest_id: value.manifest_id?,
                manifest_put_url: value.manifest_put_url?,
                pack_id: value.pack_id?,
                pack_put_url: value.pack_put_url?,
                reason: value.reason?,
            })
        }
    }
    impl ::std::convert::From<super::SyncRequest> for SyncRequest {
        fn from(value: super::SyncRequest) -> Self {
            Self {
                full: Ok(value.full),
                manifest_id: Ok(value.manifest_id),
                manifest_put_url: Ok(value.manifest_put_url),
                pack_id: Ok(value.pack_id),
                pack_put_url: Ok(value.pack_put_url),
                reason: Ok(value.reason),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SyncResponse {
        bytes_total: ::std::result::Result<u64, ::std::string::String>,
        bytes_uploaded: ::std::result::Result<u64, ::std::string::String>,
        changed: ::std::result::Result<bool, ::std::string::String>,
        duration_ms: ::std::result::Result<u64, ::std::string::String>,
        files_added: ::std::result::Result<u64, ::std::string::String>,
        files_deleted: ::std::result::Result<u64, ::std::string::String>,
        files_modified: ::std::result::Result<u64, ::std::string::String>,
        files_total: ::std::result::Result<u64, ::std::string::String>,
        manifest_id: ::std::result::Result<super::ManifestId, ::std::string::String>,
        packs_referenced: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for SyncResponse {
        fn default() -> Self {
            Self {
                bytes_total: Err("no value supplied for bytes_total".to_string()),
                bytes_uploaded: Err("no value supplied for bytes_uploaded".to_string()),
                changed: Err("no value supplied for changed".to_string()),
                duration_ms: Err("no value supplied for duration_ms".to_string()),
                files_added: Err("no value supplied for files_added".to_string()),
                files_deleted: Err("no value supplied for files_deleted".to_string()),
                files_modified: Err("no value supplied for files_modified".to_string()),
                files_total: Err("no value supplied for files_total".to_string()),
                manifest_id: Err("no value supplied for manifest_id".to_string()),
                packs_referenced: Err("no value supplied for packs_referenced".to_string()),
            }
        }
    }
    impl SyncResponse {
        pub fn bytes_total<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes_total = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes_total: {e}"));
            self
        }
        pub fn bytes_uploaded<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes_uploaded = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes_uploaded: {e}"));
            self
        }
        pub fn changed<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.changed = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for changed: {e}"));
            self
        }
        pub fn duration_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.duration_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for duration_ms: {e}"));
            self
        }
        pub fn files_added<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.files_added = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files_added: {e}"));
            self
        }
        pub fn files_deleted<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.files_deleted = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files_deleted: {e}"));
            self
        }
        pub fn files_modified<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.files_modified = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files_modified: {e}"));
            self
        }
        pub fn files_total<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.files_total = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files_total: {e}"));
            self
        }
        pub fn manifest_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ManifestId>,
            T::Error: ::std::fmt::Display,
        {
            self.manifest_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for manifest_id: {e}"));
            self
        }
        pub fn packs_referenced<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.packs_referenced = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for packs_referenced: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SyncResponse> for super::SyncResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SyncResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes_total: value.bytes_total?,
                bytes_uploaded: value.bytes_uploaded?,
                changed: value.changed?,
                duration_ms: value.duration_ms?,
                files_added: value.files_added?,
                files_deleted: value.files_deleted?,
                files_modified: value.files_modified?,
                files_total: value.files_total?,
                manifest_id: value.manifest_id?,
                packs_referenced: value.packs_referenced?,
            })
        }
    }
    impl ::std::convert::From<super::SyncResponse> for SyncResponse {
        fn from(value: super::SyncResponse) -> Self {
            Self {
                bytes_total: Ok(value.bytes_total),
                bytes_uploaded: Ok(value.bytes_uploaded),
                changed: Ok(value.changed),
                duration_ms: Ok(value.duration_ms),
                files_added: Ok(value.files_added),
                files_deleted: Ok(value.files_deleted),
                files_modified: Ok(value.files_modified),
                files_total: Ok(value.files_total),
                manifest_id: Ok(value.manifest_id),
                packs_referenced: Ok(value.packs_referenced),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SyncScope {
        exclude:
            ::std::result::Result<::std::vec::Vec<::std::string::String>, ::std::string::String>,
        roots: ::std::result::Result<::std::vec::Vec<::std::string::String>, ::std::string::String>,
    }
    impl ::std::default::Default for SyncScope {
        fn default() -> Self {
            Self {
                exclude: Ok(Default::default()),
                roots: Err("no value supplied for roots".to_string()),
            }
        }
    }
    impl SyncScope {
        pub fn exclude<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.exclude = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for exclude: {e}"));
            self
        }
        pub fn roots<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.roots = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for roots: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SyncScope> for super::SyncScope {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SyncScope,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                exclude: value.exclude?,
                roots: value.roots?,
            })
        }
    }
    impl ::std::convert::From<super::SyncScope> for SyncScope {
        fn from(value: super::SyncScope) -> Self {
            Self {
                exclude: Ok(value.exclude),
                roots: Ok(value.roots),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TerminalInfo {
        ended_at_monotonic_ms: ::std::result::Result<super::MonotonicMs, ::std::string::String>,
        error: ::std::result::Result<::std::option::Option<super::AbiError>, ::std::string::String>,
        exit_code: ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
        outcome: ::std::result::Result<super::Outcome, ::std::string::String>,
        output: ::std::result::Result<
            ::std::option::Option<::serde_json::Value>,
            ::std::string::String,
        >,
        signal: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        usage: ::std::result::Result<super::Usage, ::std::string::String>,
    }
    impl ::std::default::Default for TerminalInfo {
        fn default() -> Self {
            Self {
                ended_at_monotonic_ms: Err(
                    "no value supplied for ended_at_monotonic_ms".to_string()
                ),
                error: Ok(Default::default()),
                exit_code: Ok(Default::default()),
                outcome: Err("no value supplied for outcome".to_string()),
                output: Ok(Default::default()),
                signal: Ok(Default::default()),
                usage: Err("no value supplied for usage".to_string()),
            }
        }
    }
    impl TerminalInfo {
        pub fn ended_at_monotonic_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::MonotonicMs>,
            T::Error: ::std::fmt::Display,
        {
            self.ended_at_monotonic_ms = value.try_into().map_err(|e| {
                format!("error converting supplied value for ended_at_monotonic_ms: {e}")
            });
            self
        }
        pub fn error<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::AbiError>>,
            T::Error: ::std::fmt::Display,
        {
            self.error = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for error: {e}"));
            self
        }
        pub fn exit_code<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<i64>>,
            T::Error: ::std::fmt::Display,
        {
            self.exit_code = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for exit_code: {e}"));
            self
        }
        pub fn outcome<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Outcome>,
            T::Error: ::std::fmt::Display,
        {
            self.outcome = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for outcome: {e}"));
            self
        }
        pub fn output<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::serde_json::Value>>,
            T::Error: ::std::fmt::Display,
        {
            self.output = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output: {e}"));
            self
        }
        pub fn signal<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.signal = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for signal: {e}"));
            self
        }
        pub fn usage<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Usage>,
            T::Error: ::std::fmt::Display,
        {
            self.usage = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for usage: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<TerminalInfo> for super::TerminalInfo {
        type Error = super::error::ConversionError;
        fn try_from(
            value: TerminalInfo,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                ended_at_monotonic_ms: value.ended_at_monotonic_ms?,
                error: value.error?,
                exit_code: value.exit_code?,
                outcome: value.outcome?,
                output: value.output?,
                signal: value.signal?,
                usage: value.usage?,
            })
        }
    }
    impl ::std::convert::From<super::TerminalInfo> for TerminalInfo {
        fn from(value: super::TerminalInfo) -> Self {
            Self {
                ended_at_monotonic_ms: Ok(value.ended_at_monotonic_ms),
                error: Ok(value.error),
                exit_code: Ok(value.exit_code),
                outcome: Ok(value.outcome),
                output: Ok(value.output),
                signal: Ok(value.signal),
                usage: Ok(value.usage),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolManifest {
        tools: ::std::result::Result<::std::vec::Vec<super::ToolSpec>, ::std::string::String>,
        version: ::std::result::Result<super::ToolManifestVersion, ::std::string::String>,
    }
    impl ::std::default::Default for ToolManifest {
        fn default() -> Self {
            Self {
                tools: Err("no value supplied for tools".to_string()),
                version: Err("no value supplied for version".to_string()),
            }
        }
    }
    impl ToolManifest {
        pub fn tools<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::ToolSpec>>,
            T::Error: ::std::fmt::Display,
        {
            self.tools = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tools: {e}"));
            self
        }
        pub fn version<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolManifestVersion>,
            T::Error: ::std::fmt::Display,
        {
            self.version = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for version: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ToolManifest> for super::ToolManifest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolManifest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                tools: value.tools?,
                version: value.version?,
            })
        }
    }
    impl ::std::convert::From<super::ToolManifest> for ToolManifest {
        fn from(value: super::ToolManifest) -> Self {
            Self {
                tools: Ok(value.tools),
                version: Ok(value.version),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolSpec {
        description: ::std::result::Result<::std::string::String, ::std::string::String>,
        input_schema: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::ToolSpecName, ::std::string::String>,
        output_schema: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        streams: ::std::result::Result<
            ::std::option::Option<super::ToolSpecStreams>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for ToolSpec {
        fn default() -> Self {
            Self {
                description: Err("no value supplied for description".to_string()),
                input_schema: Err("no value supplied for input_schema".to_string()),
                name: Err("no value supplied for name".to_string()),
                output_schema: Err("no value supplied for output_schema".to_string()),
                streams: Ok(Default::default()),
            }
        }
    }
    impl ToolSpec {
        pub fn description<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.description = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for description: {e}"));
            self
        }
        pub fn input_schema<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::serde_json::Map<::std::string::String, ::serde_json::Value>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.input_schema = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input_schema: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolSpecName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn output_schema<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::serde_json::Map<::std::string::String, ::serde_json::Value>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.output_schema = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output_schema: {e}"));
            self
        }
        pub fn streams<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ToolSpecStreams>>,
            T::Error: ::std::fmt::Display,
        {
            self.streams = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for streams: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ToolSpec> for super::ToolSpec {
        type Error = super::error::ConversionError;
        fn try_from(value: ToolSpec) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                description: value.description?,
                input_schema: value.input_schema?,
                name: value.name?,
                output_schema: value.output_schema?,
                streams: value.streams?,
            })
        }
    }
    impl ::std::convert::From<super::ToolSpec> for ToolSpec {
        fn from(value: super::ToolSpec) -> Self {
            Self {
                description: Ok(value.description),
                input_schema: Ok(value.input_schema),
                name: Ok(value.name),
                output_schema: Ok(value.output_schema),
                streams: Ok(value.streams),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Usage {
        cpu_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        max_rss_bytes: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        wall_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for Usage {
        fn default() -> Self {
            Self {
                cpu_ms: Ok(Default::default()),
                max_rss_bytes: Ok(Default::default()),
                wall_ms: Err("no value supplied for wall_ms".to_string()),
            }
        }
    }
    impl Usage {
        pub fn cpu_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.cpu_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for cpu_ms: {e}"));
            self
        }
        pub fn max_rss_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.max_rss_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_rss_bytes: {e}"));
            self
        }
        pub fn wall_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.wall_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wall_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Usage> for super::Usage {
        type Error = super::error::ConversionError;
        fn try_from(value: Usage) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                cpu_ms: value.cpu_ms?,
                max_rss_bytes: value.max_rss_bytes?,
                wall_ms: value.wall_ms?,
            })
        }
    }
    impl ::std::convert::From<super::Usage> for Usage {
        fn from(value: super::Usage) -> Self {
            Self {
                cpu_ms: Ok(value.cpu_ms),
                max_rss_bytes: Ok(value.max_rss_bytes),
                wall_ms: Ok(value.wall_ms),
            }
        }
    }
}
