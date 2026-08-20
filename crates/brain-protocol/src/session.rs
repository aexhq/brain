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
#[doc = "Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Public state model: session `active | idle | deleted | failed`; hand state is a separate field. Absent provider counters are absent, never zero."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://aex.dev/contracts/session/v1/schemas.json\","]
#[doc = "  \"title\": \"Aex session API v1 types\","]
#[doc = "  \"description\": \"Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Public state model: session `active | idle | deleted | failed`; hand state is a separate field. Absent provider counters are absent, never zero.\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct AexSessionApiV1Types(pub ::serde_json::Value);
impl ::std::ops::Deref for AexSessionApiV1Types {
    type Target = ::serde_json::Value;
    fn deref(&self) -> &::serde_json::Value {
        &self.0
    }
}
impl ::std::convert::From<AexSessionApiV1Types> for ::serde_json::Value {
    fn from(value: AexSessionApiV1Types) -> Self {
        value.0
    }
}
impl ::std::convert::From<::serde_json::Value> for AexSessionApiV1Types {
    fn from(value: ::serde_json::Value) -> Self {
        Self(value)
    }
}
#[doc = "\"root\" for the session's root agent; subagents get brain-minted ids."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"\\\"root\\\" for the session's root agent; subagents get brain-minted ids.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct AgentId(::std::string::String);
impl ::std::ops::Deref for AgentId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<AgentId> for ::std::string::String {
    fn from(value: AgentId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for AgentId {
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
impl ::std::convert::TryFrom<&str> for AgentId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AgentId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AgentId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for AgentId {
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
#[doc = "`ApiError`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"code\","]
#[doc = "    \"message\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"code\": {"]
#[doc = "      \"$ref\": \"#/$defs/ApiErrorCode\""]
#[doc = "    },"]
#[doc = "    \"details\": {"]
#[doc = "      \"description\": \"Machine-readable failure details when available, such as bounded validation issues.\""]
#[doc = "    },"]
#[doc = "    \"message\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"param\": {"]
#[doc = "      \"description\": \"JSON pointer to the offending request field, when applicable.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"request_id\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ApiError {
    pub code: ApiErrorCode,
    #[doc = "Machine-readable failure details when available, such as bounded validation issues."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub details: ::std::option::Option<::serde_json::Value>,
    pub message: ::std::string::String,
    #[doc = "JSON pointer to the offending request field, when applicable."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub param: ::std::option::Option<::std::string::String>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub request_id: ::std::option::Option<::std::string::String>,
}
impl ApiError {
    pub fn builder() -> builder::ApiError {
        Default::default()
    }
}
#[doc = "`ApiErrorCode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"invalid_request\","]
#[doc = "    \"unauthorized\","]
#[doc = "    \"forbidden\","]
#[doc = "    \"not_found\","]
#[doc = "    \"conflict\","]
#[doc = "    \"session_busy\","]
#[doc = "    \"session_deleted\","]
#[doc = "    \"session_failed\","]
#[doc = "    \"cancelled\","]
#[doc = "    \"insufficient_balance\","]
#[doc = "    \"rate_limited\","]
#[doc = "    \"provider_error\","]
#[doc = "    \"output_schema_error\","]
#[doc = "    \"output_refused\","]
#[doc = "    \"output_validation_error\","]
#[doc = "    \"hand_unavailable\","]
#[doc = "    \"too_large\","]
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
pub enum ApiErrorCode {
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "session_busy")]
    SessionBusy,
    #[serde(rename = "session_deleted")]
    SessionDeleted,
    #[serde(rename = "session_failed")]
    SessionFailed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "insufficient_balance")]
    InsufficientBalance,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "provider_error")]
    ProviderError,
    #[serde(rename = "output_schema_error")]
    OutputSchemaError,
    #[serde(rename = "output_refused")]
    OutputRefused,
    #[serde(rename = "output_validation_error")]
    OutputValidationError,
    #[serde(rename = "hand_unavailable")]
    HandUnavailable,
    #[serde(rename = "too_large")]
    TooLarge,
    #[serde(rename = "internal")]
    Internal,
}
impl ::std::fmt::Display for ApiErrorCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InvalidRequest => f.write_str("invalid_request"),
            Self::Unauthorized => f.write_str("unauthorized"),
            Self::Forbidden => f.write_str("forbidden"),
            Self::NotFound => f.write_str("not_found"),
            Self::Conflict => f.write_str("conflict"),
            Self::SessionBusy => f.write_str("session_busy"),
            Self::SessionDeleted => f.write_str("session_deleted"),
            Self::SessionFailed => f.write_str("session_failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::InsufficientBalance => f.write_str("insufficient_balance"),
            Self::RateLimited => f.write_str("rate_limited"),
            Self::ProviderError => f.write_str("provider_error"),
            Self::OutputSchemaError => f.write_str("output_schema_error"),
            Self::OutputRefused => f.write_str("output_refused"),
            Self::OutputValidationError => f.write_str("output_validation_error"),
            Self::HandUnavailable => f.write_str("hand_unavailable"),
            Self::TooLarge => f.write_str("too_large"),
            Self::Internal => f.write_str("internal"),
        }
    }
}
impl ::std::str::FromStr for ApiErrorCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "invalid_request" => Ok(Self::InvalidRequest),
            "unauthorized" => Ok(Self::Unauthorized),
            "forbidden" => Ok(Self::Forbidden),
            "not_found" => Ok(Self::NotFound),
            "conflict" => Ok(Self::Conflict),
            "session_busy" => Ok(Self::SessionBusy),
            "session_deleted" => Ok(Self::SessionDeleted),
            "session_failed" => Ok(Self::SessionFailed),
            "cancelled" => Ok(Self::Cancelled),
            "insufficient_balance" => Ok(Self::InsufficientBalance),
            "rate_limited" => Ok(Self::RateLimited),
            "provider_error" => Ok(Self::ProviderError),
            "output_schema_error" => Ok(Self::OutputSchemaError),
            "output_refused" => Ok(Self::OutputRefused),
            "output_validation_error" => Ok(Self::OutputValidationError),
            "hand_unavailable" => Ok(Self::HandUnavailable),
            "too_large" => Ok(Self::TooLarge),
            "internal" => Ok(Self::Internal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ApiErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ApiErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ApiErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ApiErrorResponse`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"error\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"error\": {"]
#[doc = "      \"$ref\": \"#/$defs/ApiError\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}
impl ApiErrorResponse {
    pub fn builder() -> builder::ApiErrorResponse {
        Default::default()
    }
}
#[doc = "`Artifact`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"created_at\","]
#[doc = "    \"media_type\","]
#[doc = "    \"name\","]
#[doc = "    \"object\","]
#[doc = "    \"session_id\","]
#[doc = "    \"sha256\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"created_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"download_url\": {"]
#[doc = "      \"description\": \"Short-lived; present on GET of a single artifact.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"download_url_expires_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"artifact\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Artifact {
    pub bytes: u64,
    pub created_at: Timestamp,
    #[doc = "Short-lived; present on GET of a single artifact."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub download_url: ::std::option::Option<::std::string::String>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub download_url_expires_at: ::std::option::Option<Timestamp>,
    pub media_type: ::std::string::String,
    pub name: ::std::string::String,
    pub object: ArtifactObject,
    pub session_id: SessionId,
    pub sha256: Sha256Hex,
}
impl Artifact {
    pub fn builder() -> builder::Artifact {
        Default::default()
    }
}
#[doc = "`ArtifactList`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"data\","]
#[doc = "    \"object\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"data\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/Artifact\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"list\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ArtifactList {
    pub data: ::std::vec::Vec<Artifact>,
    pub object: ArtifactListObject,
}
impl ArtifactList {
    pub fn builder() -> builder::ArtifactList {
        Default::default()
    }
}
#[doc = "`ArtifactListObject`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"list\""]
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
pub enum ArtifactListObject {
    #[serde(rename = "list")]
    List,
}
impl ::std::fmt::Display for ArtifactListObject {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::List => f.write_str("list"),
        }
    }
}
impl ::std::str::FromStr for ArtifactListObject {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "list" => Ok(Self::List),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ArtifactListObject {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ArtifactListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ArtifactListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ArtifactObject`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"artifact\""]
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
pub enum ArtifactObject {
    #[serde(rename = "artifact")]
    Artifact,
}
impl ::std::fmt::Display for ArtifactObject {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Artifact => f.write_str("artifact"),
        }
    }
}
impl ::std::str::FromStr for ArtifactObject {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "artifact" => Ok(Self::Artifact),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ArtifactObject {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ArtifactObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ArtifactObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "bash..ls run in the hand; task/todo run in the brain; web_search/web_fetch are managed and billed."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"bash..ls run in the hand; task/todo run in the brain; web_search/web_fetch are managed and billed.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"bash\","]
#[doc = "    \"read\","]
#[doc = "    \"write\","]
#[doc = "    \"edit\","]
#[doc = "    \"glob\","]
#[doc = "    \"grep\","]
#[doc = "    \"ls\","]
#[doc = "    \"task\","]
#[doc = "    \"todo\","]
#[doc = "    \"web_search\","]
#[doc = "    \"web_fetch\""]
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
pub enum BuiltinTool {
    #[serde(rename = "bash")]
    Bash,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "glob")]
    Glob,
    #[serde(rename = "grep")]
    Grep,
    #[serde(rename = "ls")]
    Ls,
    #[serde(rename = "task")]
    Task,
    #[serde(rename = "todo")]
    Todo,
    #[serde(rename = "web_search")]
    WebSearch,
    #[serde(rename = "web_fetch")]
    WebFetch,
}
impl ::std::fmt::Display for BuiltinTool {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Bash => f.write_str("bash"),
            Self::Read => f.write_str("read"),
            Self::Write => f.write_str("write"),
            Self::Edit => f.write_str("edit"),
            Self::Glob => f.write_str("glob"),
            Self::Grep => f.write_str("grep"),
            Self::Ls => f.write_str("ls"),
            Self::Task => f.write_str("task"),
            Self::Todo => f.write_str("todo"),
            Self::WebSearch => f.write_str("web_search"),
            Self::WebFetch => f.write_str("web_fetch"),
        }
    }
}
impl ::std::str::FromStr for BuiltinTool {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "bash" => Ok(Self::Bash),
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "edit" => Ok(Self::Edit),
            "glob" => Ok(Self::Glob),
            "grep" => Ok(Self::Grep),
            "ls" => Ok(Self::Ls),
            "task" => Ok(Self::Task),
            "todo" => Ok(Self::Todo),
            "web_search" => Ok(Self::WebSearch),
            "web_fetch" => Ok(Self::WebFetch),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BuiltinTool {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BuiltinTool {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BuiltinTool {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Brain-minted id of one tool call (equals the ABI operation_id for hand tools)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted id of one tool call (equals the ABI operation_id for hand tools).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CallId(::std::string::String);
impl ::std::ops::Deref for CallId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CallId> for ::std::string::String {
    fn from(value: CallId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CallId {
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
impl ::std::convert::TryFrom<&str> for CallId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CallId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CallId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CallId {
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
#[doc = "`ContentPart`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"text\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"text\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"text\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"path\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"path\": {"]
#[doc = "          \"description\": \"A file already in the workspace; the model is told about it.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"workspace_file\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: ::std::string::String },
    #[serde(rename = "workspace_file")]
    WorkspaceFile {
        #[doc = "A file already in the workspace; the model is told about it."]
        path: ::std::string::String,
    },
}
#[doc = "Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"model\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"files\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/FileInput\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"hand\": {"]
#[doc = "      \"$ref\": \"#/$defs/HandConfig\""]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelConfig\""]
#[doc = "    },"]
#[doc = "    \"system_prompt\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"tools\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolsConfig\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub files: ::std::vec::Vec<FileInput>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub hand: ::std::option::Option<HandConfig>,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    pub model: ModelConfig,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub system_prompt: ::std::option::Option<::std::string::String>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tools: ::std::option::Option<ToolsConfig>,
}
impl CreateSessionRequest {
    pub fn builder() -> builder::CreateSessionRequest {
        Default::default()
    }
}
#[doc = "One journal event, delivered over SSE as `event: <type>` with `id: <seq>` and this object as data. Discriminated by `type`."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One journal event, delivered over SSE as `event: <type>` with `id: <seq>` and this object as data. Discriminated by `type`.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn.started\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"output_id\","]
#[doc = "        \"schema_hash\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"source_seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"output_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/OutputId\""]
#[doc = "        },"]
#[doc = "        \"schema_hash\": {"]
#[doc = "          \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"source_seq\": {"]
#[doc = "          \"description\": \"Last committed session sequence captured for this output request.\","]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"description\": \"Present when the output request included new user input.\","]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"output.started\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"output\","]
#[doc = "        \"output_id\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"output\": {"]
#[doc = "          \"$ref\": \"#/$defs/OutputContent\""]
#[doc = "        },"]
#[doc = "        \"output_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/OutputId\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"output.completed\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"usage\": {"]
#[doc = "          \"description\": \"Aggregate provider counters for the private commit and bounded repair calls. Absent counters remain absent.\","]
#[doc = "          \"$ref\": \"#/$defs/ProviderUsage\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"error\","]
#[doc = "        \"output_id\","]
#[doc = "        \"schema_hash\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"error\": {"]
#[doc = "          \"$ref\": \"#/$defs/ApiError\""]
#[doc = "        },"]
#[doc = "        \"issues\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/OutputValidationIssue\""]
#[doc = "          }"]
#[doc = "        },"]
#[doc = "        \"output_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/OutputId\""]
#[doc = "        },"]
#[doc = "        \"schema_hash\": {"]
#[doc = "          \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"output.failed\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"usage\": {"]
#[doc = "          \"description\": \"Aggregate provider counters for any private commit or repair calls completed before failure.\","]
#[doc = "          \"$ref\": \"#/$defs/ProviderUsage\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"text\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"text\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"assistant.delta\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"text\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"text\": {"]
#[doc = "          \"description\": \"The complete assistant text of one model round.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"assistant.message\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"call_id\","]
#[doc = "        \"detach\","]
#[doc = "        \"input\","]
#[doc = "        \"name\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"call_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/CallId\""]
#[doc = "        },"]
#[doc = "        \"detach\": {"]
#[doc = "          \"type\": \"boolean\""]
#[doc = "        },"]
#[doc = "        \"input\": {},"]
#[doc = "        \"name\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tool.call\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"call_id\","]
#[doc = "        \"offset\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"stream\","]
#[doc = "        \"text\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"call_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/CallId\""]
#[doc = "        },"]
#[doc = "        \"offset\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"stream\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"stdout\","]
#[doc = "            \"stderr\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"text\": {"]
#[doc = "          \"description\": \"Bounded, lossy UTF-8 preview of the bytes from offset.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tool.output\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"call_id\","]
#[doc = "        \"duration_ms\","]
#[doc = "        \"name\","]
#[doc = "        \"outcome\","]
#[doc = "        \"output_preview\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"truncated\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"call_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/CallId\""]
#[doc = "        },"]
#[doc = "        \"duration_ms\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"error\": {"]
#[doc = "          \"description\": \"Present when outcome != completed.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"exit_code\": {"]
#[doc = "          \"type\": ["]
#[doc = "            \"integer\","]
#[doc = "            \"null\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"outcome\": {"]
#[doc = "          \"$ref\": \"#/$defs/ToolOutcome\""]
#[doc = "        },"]
#[doc = "        \"output_preview\": {"]
#[doc = "          \"description\": \"What the model was shown, bounded.\","]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"truncated\": {"]
#[doc = "          \"type\": \"boolean\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tool.result\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"depth\","]
#[doc = "        \"description\","]
#[doc = "        \"parent_agent_id\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"depth\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"maximum\": 3.0,"]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"description\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"parent_agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"agent.spawned\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"outcome\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"outcome\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"completed\","]
#[doc = "            \"failed\","]
#[doc = "            \"cancelled\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"summary\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"agent.finished\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"model\","]
#[doc = "        \"provider\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\","]
#[doc = "        \"usage\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"agent_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentId\""]
#[doc = "        },"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"model\": {"]
#[doc = "          \"type\": \"string\""]
#[doc = "        },"]
#[doc = "        \"provider\": {"]
#[doc = "          \"$ref\": \"#/$defs/Provider\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"model.usage\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"usage\": {"]
#[doc = "          \"$ref\": \"#/$defs/ProviderUsage\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"hand\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"state\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"hand\": {"]
#[doc = "          \"$ref\": \"#/$defs/HandInfo\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"state\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionState\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"session.updated\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"interrupted_calls\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"interrupted_calls\": {"]
#[doc = "          \"description\": \"Calls whose outcome is unknown; they are reported to the model as interrupted and never replayed.\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/CallId\""]
#[doc = "          }"]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"hand.lost\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"workspace_synced_at\": {"]
#[doc = "          \"description\": \"Last successful sync; work after it is lost.\","]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"rounds\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"stop_reason\","]
#[doc = "        \"tool_calls\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"result\": {"]
#[doc = "          \"description\": \"Present when a return_direct external tool completed the turn.\","]
#[doc = "          \"$ref\": \"#/$defs/TurnResult\""]
#[doc = "        },"]
#[doc = "        \"rounds\": {"]
#[doc = "          \"description\": \"Model calls in this turn (root agent).\","]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"stop_reason\": {"]
#[doc = "          \"$ref\": \"#/$defs/StopReason\""]
#[doc = "        },"]
#[doc = "        \"tool_calls\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn.completed\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"error\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"error\": {"]
#[doc = "          \"$ref\": \"#/$defs/ApiError\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn.failed\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Event {
    #[serde(rename = "turn.started")]
    TurnStarted {
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
    },
    #[serde(rename = "output.started")]
    OutputStarted {
        at: Timestamp,
        output_id: OutputId,
        schema_hash: Sha256Hex,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[doc = "Last committed session sequence captured for this output request."]
        source_seq: u64,
        #[doc = "Present when the output request included new user input."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
    },
    #[serde(rename = "output.completed")]
    OutputCompleted {
        at: Timestamp,
        output: OutputContent,
        output_id: OutputId,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
        #[doc = "Aggregate provider counters for the private commit and bounded repair calls. Absent counters remain absent."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        usage: ::std::option::Option<ProviderUsage>,
    },
    #[serde(rename = "output.failed")]
    OutputFailed {
        at: Timestamp,
        error: ApiError,
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        issues: ::std::vec::Vec<OutputValidationIssue>,
        output_id: OutputId,
        schema_hash: Sha256Hex,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
        #[doc = "Aggregate provider counters for any private commit or repair calls completed before failure."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        usage: ::std::option::Option<ProviderUsage>,
    },
    #[serde(rename = "assistant.delta")]
    AssistantDelta {
        agent_id: AgentId,
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        text: ::std::string::String,
        turn_id: TurnId,
    },
    #[serde(rename = "assistant.message")]
    AssistantMessage {
        agent_id: AgentId,
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[doc = "The complete assistant text of one model round."]
        text: ::std::string::String,
        turn_id: TurnId,
    },
    #[serde(rename = "tool.call")]
    ToolCall {
        agent_id: AgentId,
        at: Timestamp,
        call_id: CallId,
        detach: bool,
        input: ::serde_json::Value,
        name: ::std::string::String,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
    },
    #[serde(rename = "tool.output")]
    ToolOutput {
        at: Timestamp,
        call_id: CallId,
        offset: u64,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        stream: EventStream,
        #[doc = "Bounded, lossy UTF-8 preview of the bytes from offset."]
        text: ::std::string::String,
        turn_id: TurnId,
    },
    #[serde(rename = "tool.result")]
    ToolResult {
        agent_id: AgentId,
        at: Timestamp,
        call_id: CallId,
        duration_ms: u64,
        #[doc = "Present when outcome != completed."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        error: ::std::option::Option<::std::string::String>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        exit_code: ::std::option::Option<i64>,
        name: ::std::string::String,
        outcome: ToolOutcome,
        #[doc = "What the model was shown, bounded."]
        output_preview: ::std::string::String,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        truncated: bool,
        turn_id: TurnId,
    },
    #[serde(rename = "agent.spawned")]
    AgentSpawned {
        agent_id: AgentId,
        at: Timestamp,
        depth: ::std::num::NonZeroU64,
        description: ::std::string::String,
        parent_agent_id: AgentId,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
    },
    #[serde(rename = "agent.finished")]
    AgentFinished {
        agent_id: AgentId,
        at: Timestamp,
        outcome: EventOutcome,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        summary: ::std::option::Option<::std::string::String>,
        turn_id: TurnId,
    },
    #[serde(rename = "model.usage")]
    ModelUsage {
        agent_id: AgentId,
        at: Timestamp,
        model: ::std::string::String,
        provider: Provider,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
        usage: ProviderUsage,
    },
    #[serde(rename = "session.updated")]
    SessionUpdated {
        at: Timestamp,
        hand: HandInfo,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        state: SessionState,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
    },
    #[serde(rename = "hand.lost")]
    HandLost {
        at: Timestamp,
        #[doc = "Calls whose outcome is unknown; they are reported to the model as interrupted and never replayed."]
        interrupted_calls: ::std::vec::Vec<CallId>,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
        #[doc = "Last successful sync; work after it is lost."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        workspace_synced_at: ::std::option::Option<Timestamp>,
    },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        at: Timestamp,
        #[doc = "Present when a return_direct external tool completed the turn."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        result: ::std::option::Option<TurnResult>,
        #[doc = "Model calls in this turn (root agent)."]
        rounds: u64,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        stop_reason: StopReason,
        tool_calls: u64,
        turn_id: TurnId,
    },
    #[serde(rename = "turn.failed")]
    TurnFailed {
        at: Timestamp,
        error: ApiError,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
    },
}
#[doc = "`EventOutcome`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"completed\","]
#[doc = "    \"failed\","]
#[doc = "    \"cancelled\""]
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
pub enum EventOutcome {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}
impl ::std::fmt::Display for EventOutcome {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}
impl ::std::str::FromStr for EventOutcome {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EventOutcome {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EventStream`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
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
pub enum EventStream {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "stderr")]
    Stderr,
}
impl ::std::fmt::Display for EventStream {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Stdout => f.write_str("stdout"),
            Self::Stderr => f.write_str("stderr"),
        }
    }
}
impl ::std::str::FromStr for EventStream {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "stdout" => Ok(Self::Stdout),
            "stderr" => Ok(Self::Stderr),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EventStream {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventStream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventStream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Generic Brain-to-host executor request. Repeating a replay_safe call uses the same session_id and call_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Generic Brain-to-host executor request. Repeating a replay_safe call uses the same session_id and call_id.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"agent_id\","]
#[doc = "    \"call_id\","]
#[doc = "    \"context\","]
#[doc = "    \"input\","]
#[doc = "    \"name\","]
#[doc = "    \"session_id\","]
#[doc = "    \"turn_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"agent_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/AgentId\""]
#[doc = "    },"]
#[doc = "    \"call_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/CallId\""]
#[doc = "    },"]
#[doc = "    \"context\": {"]
#[doc = "      \"description\": \"Trusted, journaled message metadata supplied by the host, not model arguments.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"input\": {},"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"turn_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/TurnId\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolCallRequest {
    pub agent_id: AgentId,
    pub call_id: CallId,
    #[doc = "Trusted, journaled message metadata supplied by the host, not model arguments."]
    pub context: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    pub input: ::serde_json::Value,
    pub name: ::std::string::String,
    pub session_id: SessionId,
    pub turn_id: TurnId,
}
impl ExternalToolCallRequest {
    pub fn builder() -> builder::ExternalToolCallRequest {
        Default::default()
    }
}
#[doc = "Generic host executor result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Generic host executor result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"content\","]
#[doc = "    \"disposition\","]
#[doc = "    \"is_error\","]
#[doc = "    \"outcome\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"content\": {"]
#[doc = "      \"description\": \"Bounded result shown to the model and journaled as the tool result.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 98304,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"disposition\": {"]
#[doc = "      \"$ref\": \"#/$defs/ExternalToolDisposition\""]
#[doc = "    },"]
#[doc = "    \"error\": {"]
#[doc = "      \"description\": \"Turn failure attached to turn.failed when disposition is fail_turn.\","]
#[doc = "      \"$ref\": \"#/$defs/ApiError\""]
#[doc = "    },"]
#[doc = "    \"is_error\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"outcome\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolOutcome\""]
#[doc = "    },"]
#[doc = "    \"result\": {"]
#[doc = "      \"description\": \"Client-facing value attached to turn.completed when disposition is complete_turn.\""]
#[doc = "    },"]
#[doc = "    \"result_metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolCallResponse {
    #[doc = "Bounded result shown to the model and journaled as the tool result."]
    pub content: ExternalToolCallResponseContent,
    pub disposition: ExternalToolDisposition,
    #[doc = "Turn failure attached to turn.failed when disposition is fail_turn."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub error: ::std::option::Option<ApiError>,
    pub is_error: bool,
    pub outcome: ToolOutcome,
    #[doc = "Client-facing value attached to turn.completed when disposition is complete_turn."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub result: ::std::option::Option<::serde_json::Value>,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub result_metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ExternalToolCallResponse {
    pub fn builder() -> builder::ExternalToolCallResponse {
        Default::default()
    }
}
#[doc = "Bounded result shown to the model and journaled as the tool result."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Bounded result shown to the model and journaled as the tool result.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 98304,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ExternalToolCallResponseContent(::std::string::String);
impl ::std::ops::Deref for ExternalToolCallResponseContent {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ExternalToolCallResponseContent> for ::std::string::String {
    fn from(value: ExternalToolCallResponseContent) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ExternalToolCallResponseContent {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 98304usize {
            return Err("longer than 98304 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolCallResponseContent {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolCallResponseContent {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolCallResponseContent {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ExternalToolCallResponseContent {
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
#[doc = "continue returns the result to the model. return_direct may complete or fail the turn without another model call."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"continue returns the result to the model. return_direct may complete or fail the turn without another model call.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"continue\","]
#[doc = "    \"return_direct\""]
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
pub enum ExternalToolCompletion {
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "return_direct")]
    ReturnDirect,
}
impl ::std::fmt::Display for ExternalToolCompletion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Continue => f.write_str("continue"),
            Self::ReturnDirect => f.write_str("return_direct"),
        }
    }
}
impl ::std::str::FromStr for ExternalToolCompletion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "continue" => Ok(Self::Continue),
            "return_direct" => Ok(Self::ReturnDirect),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolCompletion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolCompletion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolCompletion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "A model-visible tool executed by the Brain host's configured external executor. The executor address and credentials are host configuration, never session data."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A model-visible tool executed by the Brain host's configured external executor. The executor address and credentials are host configuration, never session data.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"completion\","]
#[doc = "    \"description\","]
#[doc = "    \"effect\","]
#[doc = "    \"input_schema\","]
#[doc = "    \"max_input_bytes\","]
#[doc = "    \"name\","]
#[doc = "    \"scope\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"completion\": {"]
#[doc = "      \"$ref\": \"#/$defs/ExternalToolCompletion\""]
#[doc = "    },"]
#[doc = "    \"description\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"effect\": {"]
#[doc = "      \"$ref\": \"#/$defs/ExternalToolEffect\""]
#[doc = "    },"]
#[doc = "    \"input_schema\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"minProperties\": 1,"]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"max_input_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 98304.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[A-Za-z_][A-Za-z0-9_-]{0,63}$\""]
#[doc = "    },"]
#[doc = "    \"scope\": {"]
#[doc = "      \"$ref\": \"#/$defs/ExternalToolScope\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolConfig {
    pub completion: ExternalToolCompletion,
    pub description: ExternalToolConfigDescription,
    pub effect: ExternalToolEffect,
    pub input_schema: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub max_input_bytes: ::std::num::NonZeroU64,
    pub name: ExternalToolConfigName,
    pub scope: ExternalToolScope,
}
impl ExternalToolConfig {
    pub fn builder() -> builder::ExternalToolConfig {
        Default::default()
    }
}
#[doc = "`ExternalToolConfigDescription`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ExternalToolConfigDescription(::std::string::String);
impl ::std::ops::Deref for ExternalToolConfigDescription {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ExternalToolConfigDescription> for ::std::string::String {
    fn from(value: ExternalToolConfigDescription) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ExternalToolConfigDescription {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolConfigDescription {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolConfigDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolConfigDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ExternalToolConfigDescription {
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
#[doc = "`ExternalToolConfigName`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z_][A-Za-z0-9_-]{0,63}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ExternalToolConfigName(::std::string::String);
impl ::std::ops::Deref for ExternalToolConfigName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ExternalToolConfigName> for ::std::string::String {
    fn from(value: ExternalToolConfigName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ExternalToolConfigName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z_][A-Za-z0-9_-]{0,63}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z_][A-Za-z0-9_-]{0,63}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolConfigName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ExternalToolConfigName {
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
#[doc = "`ExternalToolDisposition`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"continue\","]
#[doc = "    \"complete_turn\","]
#[doc = "    \"fail_turn\""]
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
pub enum ExternalToolDisposition {
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "complete_turn")]
    CompleteTurn,
    #[serde(rename = "fail_turn")]
    FailTurn,
}
impl ::std::fmt::Display for ExternalToolDisposition {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Continue => f.write_str("continue"),
            Self::CompleteTurn => f.write_str("complete_turn"),
            Self::FailTurn => f.write_str("fail_turn"),
        }
    }
}
impl ::std::str::FromStr for ExternalToolDisposition {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "continue" => Ok(Self::Continue),
            "complete_turn" => Ok(Self::CompleteTurn),
            "fail_turn" => Ok(Self::FailTurn),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolDisposition {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolDisposition {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolDisposition {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "replay_safe promises that repeating the same session_id and call_id returns the same logical result."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"replay_safe promises that repeating the same session_id and call_id returns the same logical result.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"opaque\","]
#[doc = "    \"replay_safe\""]
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
pub enum ExternalToolEffect {
    #[serde(rename = "opaque")]
    Opaque,
    #[serde(rename = "replay_safe")]
    ReplaySafe,
}
impl ::std::fmt::Display for ExternalToolEffect {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Opaque => f.write_str("opaque"),
            Self::ReplaySafe => f.write_str("replay_safe"),
        }
    }
}
impl ::std::str::FromStr for ExternalToolEffect {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "opaque" => Ok(Self::Opaque),
            "replay_safe" => Ok(Self::ReplaySafe),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolEffect {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolEffect {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolEffect {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Which agents may call a host-executed tool. root keeps terminal control out of subagents."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Which agents may call a host-executed tool. root keeps terminal control out of subagents.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"root\","]
#[doc = "    \"all_agents\""]
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
pub enum ExternalToolScope {
    #[serde(rename = "root")]
    Root,
    #[serde(rename = "all_agents")]
    AllAgents,
}
impl ::std::fmt::Display for ExternalToolScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Root => f.write_str("root"),
            Self::AllAgents => f.write_str("all_agents"),
        }
    }
}
impl ::std::str::FromStr for ExternalToolScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "root" => Ok(Self::Root),
            "all_agents" => Ok(Self::AllAgents),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExternalToolScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExternalToolScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExternalToolScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`FileEntry`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"kind\","]
#[doc = "    \"path\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"file\","]
#[doc = "        \"dir\","]
#[doc = "        \"symlink\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"modified_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"size\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub kind: FileEntryKind,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub modified_at: ::std::option::Option<Timestamp>,
    pub path: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub sha256: ::std::option::Option<Sha256Hex>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub size: ::std::option::Option<u64>,
}
impl FileEntry {
    pub fn builder() -> builder::FileEntry {
        Default::default()
    }
}
#[doc = "`FileEntryKind`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"file\","]
#[doc = "    \"dir\","]
#[doc = "    \"symlink\""]
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
pub enum FileEntryKind {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "dir")]
    Dir,
    #[serde(rename = "symlink")]
    Symlink,
}
impl ::std::fmt::Display for FileEntryKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::File => f.write_str("file"),
            Self::Dir => f.write_str("dir"),
            Self::Symlink => f.write_str("symlink"),
        }
    }
}
impl ::std::str::FromStr for FileEntryKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "file" => Ok(Self::File),
            "dir" => Ok(Self::Dir),
            "symlink" => Ok(Self::Symlink),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for FileEntryKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FileEntryKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FileEntryKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Small files placed into the workspace at create (limit 1 MiB each). Larger files: PUT /files/{path} after create."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Small files placed into the workspace at create (limit 1 MiB each). Larger files: PUT /files/{path} after create.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"content_base64\","]
#[doc = "    \"path\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"content_base64\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"contentEncoding\": \"base64\""]
#[doc = "    },"]
#[doc = "    \"mode\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 4095.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"description\": \"Relative to /workspace.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileInput {
    pub content_base64: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub mode: ::std::option::Option<i64>,
    #[doc = "Relative to /workspace."]
    pub path: ::std::string::String,
}
impl FileInput {
    pub fn builder() -> builder::FileInput {
        Default::default()
    }
}
#[doc = "`FileList`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"data\","]
#[doc = "    \"object\","]
#[doc = "    \"source\","]
#[doc = "    \"synced_at\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"data\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/FileEntry\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"list\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"source\": {"]
#[doc = "      \"description\": \"hand = live listing from a running hand; manifest = from the last sync (hand released).\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"hand\","]
#[doc = "        \"manifest\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"synced_at\": {"]
#[doc = "      \"description\": \"Time of the manifest this listing reflects; null when the workspace has never synced.\","]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"format\": \"date-time\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct FileList {
    pub data: ::std::vec::Vec<FileEntry>,
    pub object: FileListObject,
    #[doc = "hand = live listing from a running hand; manifest = from the last sync (hand released)."]
    pub source: FileListSource,
    #[doc = "Time of the manifest this listing reflects; null when the workspace has never synced."]
    pub synced_at: ::std::option::Option<::chrono::DateTime<::chrono::offset::Utc>>,
}
impl FileList {
    pub fn builder() -> builder::FileList {
        Default::default()
    }
}
#[doc = "`FileListObject`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"list\""]
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
pub enum FileListObject {
    #[serde(rename = "list")]
    List,
}
impl ::std::fmt::Display for FileListObject {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::List => f.write_str("list"),
        }
    }
}
impl ::std::str::FromStr for FileListObject {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "list" => Ok(Self::List),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for FileListObject {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FileListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FileListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "hand = live listing from a running hand; manifest = from the last sync (hand released)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"hand = live listing from a running hand; manifest = from the last sync (hand released).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"hand\","]
#[doc = "    \"manifest\""]
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
pub enum FileListSource {
    #[serde(rename = "hand")]
    Hand,
    #[serde(rename = "manifest")]
    Manifest,
}
impl ::std::fmt::Display for FileListSource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Hand => f.write_str("hand"),
            Self::Manifest => f.write_str("manifest"),
        }
    }
}
impl ::std::str::FromStr for FileListSource {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "hand" => Ok(Self::Hand),
            "manifest" => Ok(Self::Manifest),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for FileListSource {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FileListSource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FileListSource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`HandConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"enabled\": {"]
#[doc = "      \"description\": \"false = no sandbox; hand tools are unavailable.\","]
#[doc = "      \"default\": true,"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"env\": {"]
#[doc = "      \"description\": \"Environment for the agent's shell. Encrypted per session, never returned.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"max_background_minutes\": {"]
#[doc = "      \"description\": \"Optional cap on how long a background job may keep the hand running after the turn ends. Absent or null = no cap.\","]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"shape\": {"]
#[doc = "      \"$ref\": \"#/$defs/HandShape\""]
#[doc = "    },"]
#[doc = "    \"sync_interval_seconds\": {"]
#[doc = "      \"description\": \"Mid-turn workspace sync period.\","]
#[doc = "      \"default\": 600,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 60.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HandConfig {
    #[doc = "false = no sandbox; hand tools are unavailable."]
    #[serde(default = "defaults::default_bool::<true>")]
    pub enabled: bool,
    #[doc = "Environment for the agent's shell. Encrypted per session, never returned."]
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub env: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    #[doc = "Optional cap on how long a background job may keep the hand running after the turn ends. Absent or null = no cap."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_background_minutes: ::std::option::Option<::std::num::NonZeroU64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub shape: ::std::option::Option<HandShape>,
    #[doc = "Mid-turn workspace sync period."]
    #[serde(default = "defaults::default_u64::<i64, 600>")]
    pub sync_interval_seconds: i64,
}
impl ::std::default::Default for HandConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::default_bool::<true>(),
            env: Default::default(),
            max_background_minutes: Default::default(),
            shape: Default::default(),
            sync_interval_seconds: defaults::default_u64::<i64, 600>(),
        }
    }
}
impl HandConfig {
    pub fn builder() -> builder::HandConfig {
        Default::default()
    }
}
#[doc = "`HandInfo`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"shape\","]
#[doc = "    \"state\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"generation\": {"]
#[doc = "      \"description\": \"How many microVM incarnations this session has had.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"last_sync_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"live_jobs\": {"]
#[doc = "      \"description\": \"Background jobs still running.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"shape\": {"]
#[doc = "      \"$ref\": \"#/$defs/HandShape\""]
#[doc = "    },"]
#[doc = "    \"started_at\": {"]
#[doc = "      \"description\": \"When the current incarnation launched.\","]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"$ref\": \"#/$defs/HandState\""]
#[doc = "    },"]
#[doc = "    \"wall_deadline_at\": {"]
#[doc = "      \"description\": \"When the platform will sync + release this incarnation (8 h after launch).\","]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct HandInfo {
    #[doc = "How many microVM incarnations this session has had."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub generation: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub last_sync_at: ::std::option::Option<Timestamp>,
    #[doc = "Background jobs still running."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub live_jobs: ::std::option::Option<u64>,
    pub shape: HandShape,
    #[doc = "When the current incarnation launched."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub started_at: ::std::option::Option<Timestamp>,
    pub state: HandState,
    #[doc = "When the platform will sync + release this incarnation (8 h after launch)."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub wall_deadline_at: ::std::option::Option<Timestamp>,
}
impl HandInfo {
    pub fn builder() -> builder::HandInfo {
        Default::default()
    }
}
#[doc = "Baseline memory; vCPU = memory/2; bursts to 4x. Default 1gb."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Baseline memory; vCPU = memory/2; bursts to 4x. Default 1gb.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"1gb\","]
#[doc = "    \"2gb\","]
#[doc = "    \"4gb\","]
#[doc = "    \"8gb\""]
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
pub enum HandShape {
    #[serde(rename = "1gb")]
    X1gb,
    #[serde(rename = "2gb")]
    X2gb,
    #[serde(rename = "4gb")]
    X4gb,
    #[serde(rename = "8gb")]
    X8gb,
}
impl ::std::fmt::Display for HandShape {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::X1gb => f.write_str("1gb"),
            Self::X2gb => f.write_str("2gb"),
            Self::X4gb => f.write_str("4gb"),
            Self::X8gb => f.write_str("8gb"),
        }
    }
}
impl ::std::str::FromStr for HandShape {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "1gb" => Ok(Self::X1gb),
            "2gb" => Ok(Self::X2gb),
            "4gb" => Ok(Self::X4gb),
            "8gb" => Ok(Self::X8gb),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for HandShape {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HandShape {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HandShape {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "preparing = microVM launching or restoring; ready = running and connected; suspended = AWS holds RAM+disk after 180 s idle, compute free, ~1 s back; released = VM destroyed, workspace synced to storage, ~3 s back into a fresh VM; lost = the hand died mid-run (in-flight calls reported as interrupted, never replayed)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"preparing = microVM launching or restoring; ready = running and connected; suspended = AWS holds RAM+disk after 180 s idle, compute free, ~1 s back; released = VM destroyed, workspace synced to storage, ~3 s back into a fresh VM; lost = the hand died mid-run (in-flight calls reported as interrupted, never replayed).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"preparing\","]
#[doc = "    \"ready\","]
#[doc = "    \"suspended\","]
#[doc = "    \"released\","]
#[doc = "    \"lost\""]
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
pub enum HandState {
    #[serde(rename = "preparing")]
    Preparing,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "released")]
    Released,
    #[serde(rename = "lost")]
    Lost,
}
impl ::std::fmt::Display for HandState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Preparing => f.write_str("preparing"),
            Self::Ready => f.write_str("ready"),
            Self::Suspended => f.write_str("suspended"),
            Self::Released => f.write_str("released"),
            Self::Lost => f.write_str("lost"),
        }
    }
}
impl ::std::str::FromStr for HandState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "ready" => Ok(Self::Ready),
            "suspended" => Ok(Self::Suspended),
            "released" => Ok(Self::Released),
            "lost" => Ok(Self::Lost),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for HandState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HandState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HandState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "auto probes server/discover and falls back to the legacy adapter (initialize + Mcp-Session-Id)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"auto probes server/discover and falls back to the legacy adapter (initialize + Mcp-Session-Id).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"auto\","]
#[doc = "    \"2026-07\","]
#[doc = "    \"legacy\""]
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
pub enum McpProtocol {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "2026-07")]
    X202607,
    #[serde(rename = "legacy")]
    Legacy,
}
impl ::std::fmt::Display for McpProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Auto => f.write_str("auto"),
            Self::X202607 => f.write_str("2026-07"),
            Self::Legacy => f.write_str("legacy"),
        }
    }
}
impl ::std::str::FromStr for McpProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "auto" => Ok(Self::Auto),
            "2026-07" => Ok(Self::X202607),
            "legacy" => Ok(Self::Legacy),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for McpProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for McpProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for McpProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`McpServerConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"name\","]
#[doc = "    \"url\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"allowed_tools\": {"]
#[doc = "      \"description\": \"Whitelist; default all.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"headers\": {"]
#[doc = "      \"description\": \"Sent on every request (e.g. Authorization). Encrypted per session, never returned.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Prefix for its tools (\\\"name__tool\\\").\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[a-z][a-z0-9_-]{0,31}$\""]
#[doc = "    },"]
#[doc = "    \"protocol\": {"]
#[doc = "      \"$ref\": \"#/$defs/McpProtocol\""]
#[doc = "    },"]
#[doc = "    \"url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    #[doc = "Whitelist; default all."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub allowed_tools: ::std::vec::Vec<::std::string::String>,
    #[doc = "Sent on every request (e.g. Authorization). Encrypted per session, never returned."]
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    #[doc = "Prefix for its tools (\"name__tool\")."]
    pub name: McpServerConfigName,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub protocol: ::std::option::Option<McpProtocol>,
    pub url: ::std::string::String,
}
impl McpServerConfig {
    pub fn builder() -> builder::McpServerConfig {
        Default::default()
    }
}
#[doc = "Prefix for its tools (\"name__tool\")."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Prefix for its tools (\\\"name__tool\\\").\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[a-z][a-z0-9_-]{0,31}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct McpServerConfigName(::std::string::String);
impl ::std::ops::Deref for McpServerConfigName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<McpServerConfigName> for ::std::string::String {
    fn from(value: McpServerConfigName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for McpServerConfigName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[a-z][a-z0-9_-]{0,31}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[a-z][a-z0-9_-]{0,31}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for McpServerConfigName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for McpServerConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for McpServerConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for McpServerConfigName {
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
#[doc = "The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"seq\","]
#[doc = "    \"session_id\","]
#[doc = "    \"turn_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"output_id\": {"]
#[doc = "      \"description\": \"Present when this message requested typed output.\","]
#[doc = "      \"$ref\": \"#/$defs/OutputId\""]
#[doc = "    },"]
#[doc = "    \"schema_hash\": {"]
#[doc = "      \"description\": \"Present when this message requested typed output.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"seq\": {"]
#[doc = "      \"description\": \"Journal sequence of the turn.started event.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"turn_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/TurnId\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct MessageAccepted {
    #[doc = "Present when this message requested typed output."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub output_id: ::std::option::Option<OutputId>,
    #[doc = "Present when this message requested typed output."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub schema_hash: ::std::option::Option<Sha256Hex>,
    #[doc = "Journal sequence of the turn.started event."]
    pub seq: ::std::num::NonZeroU64,
    pub session_id: SessionId,
    pub turn_id: TurnId,
}
impl MessageAccepted {
    pub fn builder() -> builder::MessageAccepted {
        Default::default()
    }
}
#[doc = "`MessageOutput`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"schema\","]
#[doc = "    \"schema_hash\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"retries\": {"]
#[doc = "      \"description\": \"Extra model attempts after the first invalid candidate.\","]
#[doc = "      \"default\": 1,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 2.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"schema\": {"]
#[doc = "      \"$ref\": \"#/$defs/OutputSchema\""]
#[doc = "    },"]
#[doc = "    \"schema_hash\": {"]
#[doc = "      \"description\": \"SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MessageOutput {
    #[doc = "Extra model attempts after the first invalid candidate."]
    #[serde(default = "defaults::default_u64::<i64, 1>")]
    pub retries: i64,
    pub schema: OutputSchema,
    #[doc = "SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model."]
    pub schema_hash: Sha256Hex,
}
impl MessageOutput {
    pub fn builder() -> builder::MessageOutput {
        Default::default()
    }
}
#[doc = "`MessageRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"content\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"content\": {"]
#[doc = "      \"oneOf\": ["]
#[doc = "        {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"minLength\": 1"]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ContentPart\""]
#[doc = "          },"]
#[doc = "          \"minItems\": 1"]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"output\": {"]
#[doc = "      \"description\": \"Optional typed result requested for this turn. It is a per-message operation, not session configuration.\","]
#[doc = "      \"$ref\": \"#/$defs/MessageOutput\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub content: MessageRequestContent,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    #[doc = "Optional typed result requested for this turn. It is a per-message operation, not session configuration."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub output: ::std::option::Option<MessageOutput>,
}
impl MessageRequest {
    pub fn builder() -> builder::MessageRequest {
        Default::default()
    }
}
#[doc = "`MessageRequestContent`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ContentPart\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum MessageRequestContent {
    String(MessageRequestContentString),
    Array(::std::vec::Vec<ContentPart>),
}
impl ::std::convert::From<MessageRequestContentString> for MessageRequestContent {
    fn from(value: MessageRequestContentString) -> Self {
        Self::String(value)
    }
}
impl ::std::convert::From<::std::vec::Vec<ContentPart>> for MessageRequestContent {
    fn from(value: ::std::vec::Vec<ContentPart>) -> Self {
        Self::Array(value)
    }
}
#[doc = "`MessageRequestContentString`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct MessageRequestContentString(::std::string::String);
impl ::std::ops::Deref for MessageRequestContentString {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<MessageRequestContentString> for ::std::string::String {
    fn from(value: MessageRequestContentString) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for MessageRequestContentString {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for MessageRequestContentString {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for MessageRequestContentString {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for MessageRequestContentString {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for MessageRequestContentString {
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
#[doc = "`ModelConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"api_key\","]
#[doc = "    \"name\","]
#[doc = "    \"provider\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"api_key\": {"]
#[doc = "      \"description\": \"BYOK. Encrypted per session, never returned, never logged.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"base_url\": {"]
#[doc = "      \"description\": \"Override the provider endpoint (required for openai_compatible).\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"max_output_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Provider model id, e.g. \\\"claude-sonnet-5\\\" or \\\"gpt-5\\\".\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"provider\": {"]
#[doc = "      \"$ref\": \"#/$defs/Provider\""]
#[doc = "    },"]
#[doc = "    \"reasoning_effort\": {"]
#[doc = "      \"description\": \"Passed through where the provider supports it.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"low\","]
#[doc = "        \"medium\","]
#[doc = "        \"high\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"temperature\": {"]
#[doc = "      \"type\": \"number\","]
#[doc = "      \"maximum\": 2.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[doc = "BYOK. Encrypted per session, never returned, never logged."]
    pub api_key: ModelConfigApiKey,
    #[doc = "Override the provider endpoint (required for openai_compatible)."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub base_url: ::std::option::Option<::std::string::String>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_output_tokens: ::std::option::Option<::std::num::NonZeroU64>,
    #[doc = "Provider model id, e.g. \"claude-sonnet-5\" or \"gpt-5\"."]
    pub name: ModelConfigName,
    pub provider: Provider,
    #[doc = "Passed through where the provider supports it."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub reasoning_effort: ::std::option::Option<ModelConfigReasoningEffort>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub temperature: ::std::option::Option<f64>,
}
impl ModelConfig {
    pub fn builder() -> builder::ModelConfig {
        Default::default()
    }
}
#[doc = "BYOK. Encrypted per session, never returned, never logged."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"BYOK. Encrypted per session, never returned, never logged.\","]
#[doc = "  \"writeOnly\": true,"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ModelConfigApiKey(::std::string::String);
impl ::std::ops::Deref for ModelConfigApiKey {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelConfigApiKey> for ::std::string::String {
    fn from(value: ModelConfigApiKey) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelConfigApiKey {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelConfigApiKey {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelConfigApiKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelConfigApiKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelConfigApiKey {
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
#[doc = "Provider model id, e.g. \"claude-sonnet-5\" or \"gpt-5\"."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Provider model id, e.g. \\\"claude-sonnet-5\\\" or \\\"gpt-5\\\".\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ModelConfigName(::std::string::String);
impl ::std::ops::Deref for ModelConfigName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelConfigName> for ::std::string::String {
    fn from(value: ModelConfigName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelConfigName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelConfigName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelConfigName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelConfigName {
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
#[doc = "Passed through where the provider supports it."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Passed through where the provider supports it.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"low\","]
#[doc = "    \"medium\","]
#[doc = "    \"high\""]
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
pub enum ModelConfigReasoningEffort {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}
impl ::std::fmt::Display for ModelConfigReasoningEffort {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Low => f.write_str("low"),
            Self::Medium => f.write_str("medium"),
            Self::High => f.write_str("high"),
        }
    }
}
impl ::std::str::FromStr for ModelConfigReasoningEffort {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ModelConfigReasoningEffort {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelConfigReasoningEffort {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelConfigReasoningEffort {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "ModelConfig without the key."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"ModelConfig without the key.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"name\","]
#[doc = "    \"provider\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"base_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"provider\": {"]
#[doc = "      \"$ref\": \"#/$defs/Provider\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ModelInfo {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub base_url: ::std::option::Option<::std::string::String>,
    pub name: ::std::string::String,
    pub provider: Provider,
}
impl ModelInfo {
    pub fn builder() -> builder::ModelInfo {
        Default::default()
    }
}
#[doc = "The output request was admitted. Follow the session event stream from seq - 1 until the matching output.completed or output.failed event."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The output request was admitted. Follow the session event stream from seq - 1 until the matching output.completed or output.failed event.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"output_id\","]
#[doc = "    \"schema_hash\","]
#[doc = "    \"seq\","]
#[doc = "    \"session_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"output_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/OutputId\""]
#[doc = "    },"]
#[doc = "    \"schema_hash\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"seq\": {"]
#[doc = "      \"description\": \"Journal sequence of the output.started event.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputAccepted {
    pub output_id: OutputId,
    pub schema_hash: Sha256Hex,
    #[doc = "Journal sequence of the output.started event."]
    pub seq: ::std::num::NonZeroU64,
    pub session_id: SessionId,
}
impl OutputAccepted {
    pub fn builder() -> builder::OutputAccepted {
        Default::default()
    }
}
#[doc = "The only durable assistant content created by the private output commit phase. The schema and repair context are never journaled."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The only durable assistant content created by the private output commit phase. The schema and repair context are never journaled.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"schema_hash\","]
#[doc = "    \"type\","]
#[doc = "    \"value\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"schema_hash\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"output\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"value\": {"]
#[doc = "      \"description\": \"The validated JSON value.\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputContent {
    pub schema_hash: Sha256Hex,
    #[serde(rename = "type")]
    pub type_: OutputContentType,
    #[doc = "The validated JSON value."]
    pub value: ::serde_json::Value,
}
impl OutputContent {
    pub fn builder() -> builder::OutputContent {
        Default::default()
    }
}
#[doc = "`OutputContentType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"output\""]
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
pub enum OutputContentType {
    #[serde(rename = "output")]
    Output,
}
impl ::std::fmt::Display for OutputContentType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Output => f.write_str("output"),
        }
    }
}
impl ::std::str::FromStr for OutputContentType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "output" => Ok(Self::Output),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OutputContentType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OutputContentType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OutputContentType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Correlation id for one output request. It is not a separately managed resource."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Correlation id for one output request. It is not a separately managed resource.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^out_[A-Za-z0-9]{20,32}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OutputId(::std::string::String);
impl ::std::ops::Deref for OutputId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OutputId> for ::std::string::String {
    fn from(value: OutputId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OutputId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^out_[A-Za-z0-9]{20,32}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^out_[A-Za-z0-9]{20,32}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OutputId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OutputId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OutputId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OutputId {
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
#[doc = "`OutputRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"schema\","]
#[doc = "    \"schema_hash\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"input\": {"]
#[doc = "      \"description\": \"Optional real user input. It is journaled and worked normally before the private output commit step.\","]
#[doc = "      \"oneOf\": ["]
#[doc = "        {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"minLength\": 1"]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ContentPart\""]
#[doc = "          },"]
#[doc = "          \"minItems\": 1"]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"schema\": {"]
#[doc = "      \"$ref\": \"#/$defs/OutputSchema\""]
#[doc = "    },"]
#[doc = "    \"schema_hash\": {"]
#[doc = "      \"description\": \"SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model.\","]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputRequest {
    #[doc = "Optional real user input. It is journaled and worked normally before the private output commit step."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub input: ::std::option::Option<OutputRequestInput>,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    pub schema: OutputSchema,
    #[doc = "SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model."]
    pub schema_hash: Sha256Hex,
}
impl OutputRequest {
    pub fn builder() -> builder::OutputRequest {
        Default::default()
    }
}
#[doc = "Optional real user input. It is journaled and worked normally before the private output commit step."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Optional real user input. It is journaled and worked normally before the private output commit step.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ContentPart\""]
#[doc = "      },"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum OutputRequestInput {
    String(OutputRequestInputString),
    Array(::std::vec::Vec<ContentPart>),
}
impl ::std::convert::From<OutputRequestInputString> for OutputRequestInput {
    fn from(value: OutputRequestInputString) -> Self {
        Self::String(value)
    }
}
impl ::std::convert::From<::std::vec::Vec<ContentPart>> for OutputRequestInput {
    fn from(value: ::std::vec::Vec<ContentPart>) -> Self {
        Self::Array(value)
    }
}
#[doc = "`OutputRequestInputString`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OutputRequestInputString(::std::string::String);
impl ::std::ops::Deref for OutputRequestInputString {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OutputRequestInputString> for ::std::string::String {
    fn from(value: OutputRequestInputString) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OutputRequestInputString {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OutputRequestInputString {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OutputRequestInputString {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OutputRequestInputString {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OutputRequestInputString {
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
#[doc = "JSON Schema 2020-12 produced by the SDK. Aex validates it in the trusted host executor; it is never provider-native response-format configuration."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"JSON Schema 2020-12 produced by the SDK. Aex validates it in the trusted host executor; it is never provider-native response-format configuration.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"minProperties\": 1,"]
#[doc = "  \"additionalProperties\": true"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct OutputSchema(pub ::serde_json::Map<::std::string::String, ::serde_json::Value>);
impl ::std::ops::Deref for OutputSchema {
    type Target = ::serde_json::Map<::std::string::String, ::serde_json::Value>;
    fn deref(&self) -> &::serde_json::Map<::std::string::String, ::serde_json::Value> {
        &self.0
    }
}
impl ::std::convert::From<OutputSchema>
    for ::serde_json::Map<::std::string::String, ::serde_json::Value>
{
    fn from(value: OutputSchema) -> Self {
        value.0
    }
}
impl ::std::convert::From<::serde_json::Map<::std::string::String, ::serde_json::Value>>
    for OutputSchema
{
    fn from(value: ::serde_json::Map<::std::string::String, ::serde_json::Value>) -> Self {
        Self(value)
    }
}
#[doc = "`OutputValidationIssue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"message\","]
#[doc = "    \"path\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"keyword\": {"]
#[doc = "      \"description\": \"The failed JSON Schema keyword when available.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"message\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"description\": \"JSON Pointer into the candidate output.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputValidationIssue {
    #[doc = "The failed JSON Schema keyword when available."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub keyword: ::std::option::Option<::std::string::String>,
    pub message: ::std::string::String,
    #[doc = "JSON Pointer into the candidate output."]
    pub path: ::std::string::String,
}
impl OutputValidationIssue {
    pub fn builder() -> builder::OutputValidationIssue {
        Default::default()
    }
}
#[doc = "`PersistRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"name\","]
#[doc = "    \"path\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 255,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"description\": \"Workspace path to persist as a named, downloadable artifact.\","]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PersistRequest {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub media_type: ::std::option::Option<::std::string::String>,
    pub name: PersistRequestName,
    #[doc = "Workspace path to persist as a named, downloadable artifact."]
    pub path: ::std::string::String,
}
impl PersistRequest {
    pub fn builder() -> builder::PersistRequest {
        Default::default()
    }
}
#[doc = "`PersistRequestName`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 255,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct PersistRequestName(::std::string::String);
impl ::std::ops::Deref for PersistRequestName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<PersistRequestName> for ::std::string::String {
    fn from(value: PersistRequestName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for PersistRequestName {
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
impl ::std::convert::TryFrom<&str> for PersistRequestName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PersistRequestName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PersistRequestName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for PersistRequestName {
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
#[doc = "openai and anthropic are certified; the rest are available uncertified."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"openai and anthropic are certified; the rest are available uncertified.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"openai\","]
#[doc = "    \"anthropic\","]
#[doc = "    \"deepseek\","]
#[doc = "    \"moonshot\","]
#[doc = "    \"xai\","]
#[doc = "    \"openai_compatible\""]
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
pub enum Provider {
    #[serde(rename = "openai")]
    Openai,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "deepseek")]
    Deepseek,
    #[serde(rename = "moonshot")]
    Moonshot,
    #[serde(rename = "xai")]
    Xai,
    #[serde(rename = "openai_compatible")]
    OpenaiCompatible,
}
impl ::std::fmt::Display for Provider {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Openai => f.write_str("openai"),
            Self::Anthropic => f.write_str("anthropic"),
            Self::Deepseek => f.write_str("deepseek"),
            Self::Moonshot => f.write_str("moonshot"),
            Self::Xai => f.write_str("xai"),
            Self::OpenaiCompatible => f.write_str("openai_compatible"),
        }
    }
}
impl ::std::str::FromStr for Provider {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "openai" => Ok(Self::Openai),
            "anthropic" => Ok(Self::Anthropic),
            "deepseek" => Ok(Self::Deepseek),
            "moonshot" => Ok(Self::Moonshot),
            "xai" => Ok(Self::Xai),
            "openai_compatible" => Ok(Self::OpenaiCompatible),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Provider {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Provider {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Provider {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Raw provider counters for one model call. A counter the provider did not send is absent here — never reported as 0."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Raw provider counters for one model call. A counter the provider did not send is absent here — never reported as 0.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"cache_creation_input_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"cache_read_input_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"input_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"output_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"reasoning_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct ProviderUsage {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cache_creation_input_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cache_read_input_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub input_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub output_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub reasoning_tokens: ::std::option::Option<u64>,
}
impl ::std::default::Default for ProviderUsage {
    fn default() -> Self {
        Self {
            cache_creation_input_tokens: Default::default(),
            cache_read_input_tokens: Default::default(),
            input_tokens: Default::default(),
            output_tokens: Default::default(),
            reasoning_tokens: Default::default(),
        }
    }
}
impl ProviderUsage {
    pub fn builder() -> builder::ProviderUsage {
        Default::default()
    }
}
#[doc = "`Session`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"created_at\","]
#[doc = "    \"hand\","]
#[doc = "    \"id\","]
#[doc = "    \"metadata\","]
#[doc = "    \"model\","]
#[doc = "    \"object\","]
#[doc = "    \"state\","]
#[doc = "    \"storage\","]
#[doc = "    \"turns\","]
#[doc = "    \"updated_at\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"created_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"current_turn\": {"]
#[doc = "      \"$ref\": \"#/$defs/TurnId\""]
#[doc = "    },"]
#[doc = "    \"failure\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionFailure\""]
#[doc = "    },"]
#[doc = "    \"hand\": {"]
#[doc = "      \"$ref\": \"#/$defs/HandInfo\""]
#[doc = "    },"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"last_message_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"description\": \"Customer key/value; up to 16 pairs.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelInfo\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"session\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionState\""]
#[doc = "    },"]
#[doc = "    \"storage\": {"]
#[doc = "      \"$ref\": \"#/$defs/StorageInfo\""]
#[doc = "    },"]
#[doc = "    \"turns\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"updated_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct Session {
    pub created_at: Timestamp,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub current_turn: ::std::option::Option<TurnId>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub failure: ::std::option::Option<SessionFailure>,
    pub hand: HandInfo,
    pub id: SessionId,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub last_message_at: ::std::option::Option<Timestamp>,
    #[doc = "Customer key/value; up to 16 pairs."]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    pub model: ModelInfo,
    pub object: SessionObject,
    pub state: SessionState,
    pub storage: StorageInfo,
    pub turns: u64,
    pub updated_at: Timestamp,
}
impl Session {
    pub fn builder() -> builder::Session {
        Default::default()
    }
}
#[doc = "`SessionFailure`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"at\","]
#[doc = "    \"code\","]
#[doc = "    \"message\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"code\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"tool_manifest_mismatch\","]
#[doc = "        \"provider_unusable\","]
#[doc = "        \"hand_unavailable\","]
#[doc = "        \"internal\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"message\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SessionFailure {
    pub at: Timestamp,
    pub code: SessionFailureCode,
    pub message: ::std::string::String,
}
impl SessionFailure {
    pub fn builder() -> builder::SessionFailure {
        Default::default()
    }
}
#[doc = "`SessionFailureCode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"tool_manifest_mismatch\","]
#[doc = "    \"provider_unusable\","]
#[doc = "    \"hand_unavailable\","]
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
pub enum SessionFailureCode {
    #[serde(rename = "tool_manifest_mismatch")]
    ToolManifestMismatch,
    #[serde(rename = "provider_unusable")]
    ProviderUnusable,
    #[serde(rename = "hand_unavailable")]
    HandUnavailable,
    #[serde(rename = "internal")]
    Internal,
}
impl ::std::fmt::Display for SessionFailureCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::ToolManifestMismatch => f.write_str("tool_manifest_mismatch"),
            Self::ProviderUnusable => f.write_str("provider_unusable"),
            Self::HandUnavailable => f.write_str("hand_unavailable"),
            Self::Internal => f.write_str("internal"),
        }
    }
}
impl ::std::str::FromStr for SessionFailureCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "tool_manifest_mismatch" => Ok(Self::ToolManifestMismatch),
            "provider_unusable" => Ok(Self::ProviderUnusable),
            "hand_unavailable" => Ok(Self::HandUnavailable),
            "internal" => Ok(Self::Internal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SessionFailureCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionFailureCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionFailureCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`SessionId`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^ses_[A-Za-z0-9]{20,32}$\""]
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
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^ses_[A-Za-z0-9]{20,32}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^ses_[A-Za-z0-9]{20,32}$\"".into());
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
#[doc = "`SessionList`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"data\","]
#[doc = "    \"has_more\","]
#[doc = "    \"object\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"data\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/Session\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"has_more\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"next_cursor\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"list\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct SessionList {
    pub data: ::std::vec::Vec<Session>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub next_cursor: ::std::option::Option<::std::string::String>,
    pub object: SessionListObject,
}
impl SessionList {
    pub fn builder() -> builder::SessionList {
        Default::default()
    }
}
#[doc = "`SessionListObject`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"list\""]
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
pub enum SessionListObject {
    #[serde(rename = "list")]
    List,
}
impl ::std::fmt::Display for SessionListObject {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::List => f.write_str("list"),
        }
    }
}
impl ::std::str::FromStr for SessionListObject {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "list" => Ok(Self::List),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SessionListObject {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionListObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`SessionObject`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"session\""]
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
pub enum SessionObject {
    #[serde(rename = "session")]
    Session,
}
impl ::std::fmt::Display for SessionObject {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Session => f.write_str("session"),
        }
    }
}
impl ::std::str::FromStr for SessionObject {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "session" => Ok(Self::Session),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SessionObject {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionObject {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "active = a turn is running or a background job is live; idle = waiting for the next message (hand may be running, suspended or released underneath); deleted = irreversible; failed = the session cannot continue (see Session.failure)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"active = a turn is running or a background job is live; idle = waiting for the next message (hand may be running, suspended or released underneath); deleted = irreversible; failed = the session cannot continue (see Session.failure).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"active\","]
#[doc = "    \"idle\","]
#[doc = "    \"deleted\","]
#[doc = "    \"failed\""]
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
pub enum SessionState {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "failed")]
    Failed,
}
impl ::std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Active => f.write_str("active"),
            Self::Idle => f.write_str("idle"),
            Self::Deleted => f.write_str("deleted"),
            Self::Failed => f.write_str("failed"),
        }
    }
}
impl ::std::str::FromStr for SessionState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "deleted" => Ok(Self::Deleted),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SessionState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`Sha256Hex`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
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
#[doc = "`StopReason`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"end_turn\","]
#[doc = "    \"max_rounds\","]
#[doc = "    \"cancelled\","]
#[doc = "    \"error\""]
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
pub enum StopReason {
    #[serde(rename = "end_turn")]
    EndTurn,
    #[serde(rename = "max_rounds")]
    MaxRounds,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "error")]
    Error,
}
impl ::std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EndTurn => f.write_str("end_turn"),
            Self::MaxRounds => f.write_str("max_rounds"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Error => f.write_str("error"),
        }
    }
}
impl ::std::str::FromStr for StopReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "end_turn" => Ok(Self::EndTurn),
            "max_rounds" => Ok(Self::MaxRounds),
            "cancelled" => Ok(Self::Cancelled),
            "error" => Ok(Self::Error),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StopReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Billed storage, visible from day one."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Billed storage, visible from day one.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"artifact_bytes\","]
#[doc = "    \"suspended_bytes\","]
#[doc = "    \"workspace_bytes\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"artifact_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"suspended_bytes\": {"]
#[doc = "      \"description\": \"Bytes AWS holds for a suspended hand.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"workspace_bytes\": {"]
#[doc = "      \"description\": \"Synced workspace objects (packs + manifests) in storage.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
pub struct StorageInfo {
    pub artifact_bytes: u64,
    #[doc = "Bytes AWS holds for a suspended hand."]
    pub suspended_bytes: u64,
    #[doc = "Synced workspace objects (packs + manifests) in storage."]
    pub workspace_bytes: u64,
}
impl StorageInfo {
    pub fn builder() -> builder::StorageInfo {
        Default::default()
    }
}
#[doc = "RFC 3339, UTC."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"RFC 3339, UTC.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"format\": \"date-time\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct Timestamp(pub ::chrono::DateTime<::chrono::offset::Utc>);
impl ::std::ops::Deref for Timestamp {
    type Target = ::chrono::DateTime<::chrono::offset::Utc>;
    fn deref(&self) -> &::chrono::DateTime<::chrono::offset::Utc> {
        &self.0
    }
}
impl ::std::convert::From<Timestamp> for ::chrono::DateTime<::chrono::offset::Utc> {
    fn from(value: Timestamp) -> Self {
        value.0
    }
}
impl ::std::convert::From<::chrono::DateTime<::chrono::offset::Utc>> for Timestamp {
    fn from(value: ::chrono::DateTime<::chrono::offset::Utc>) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for Timestamp {
    type Err = <::chrono::DateTime<::chrono::offset::Utc> as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for Timestamp {
    type Error = <::chrono::DateTime<::chrono::offset::Utc> as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for Timestamp {
    type Error = <::chrono::DateTime<::chrono::offset::Utc> as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
#[doc = "`ToolOutcome`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
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
pub enum ToolOutcome {
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
impl ::std::fmt::Display for ToolOutcome {
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
impl ::std::str::FromStr for ToolOutcome {
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
impl ::std::convert::TryFrom<&str> for ToolOutcome {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Sealed at create with the rest of the prefix. Omitted tools default to an empty set."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Sealed at create with the rest of the prefix. Omitted tools default to an empty set.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"builtin\": {"]
#[doc = "      \"description\": \"Built-in tools to enable. Omitted or empty means no built-in tools.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/BuiltinTool\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"external\": {"]
#[doc = "      \"description\": \"Host-executed tools sealed into the model prefix. Hosted Aex reserves its own output tool; direct Brain deployments may compose others.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ExternalToolConfig\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"mcp\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/McpServerConfig\""]
#[doc = "      }"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[doc = "Built-in tools to enable. Omitted or empty means no built-in tools."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub builtin: ::std::option::Option<Vec<BuiltinTool>>,
    #[doc = "Host-executed tools sealed into the model prefix. Hosted Aex reserves its own output tool; direct Brain deployments may compose others."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub external: ::std::vec::Vec<ExternalToolConfig>,
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub mcp: ::std::vec::Vec<McpServerConfig>,
}
impl ::std::default::Default for ToolsConfig {
    fn default() -> Self {
        Self {
            builtin: Default::default(),
            external: Default::default(),
            mcp: Default::default(),
        }
    }
}
impl ToolsConfig {
    pub fn builder() -> builder::ToolsConfig {
        Default::default()
    }
}
#[doc = "`TurnId`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^trn_[A-Za-z0-9]{20,32}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct TurnId(::std::string::String);
impl ::std::ops::Deref for TurnId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<TurnId> for ::std::string::String {
    fn from(value: TurnId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for TurnId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^trn_[A-Za-z0-9]{20,32}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^trn_[A-Za-z0-9]{20,32}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for TurnId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TurnId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TurnId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for TurnId {
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
#[doc = "A replayable client-facing result returned directly by a generic external tool."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A replayable client-facing result returned directly by a generic external tool.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"call_id\","]
#[doc = "    \"name\","]
#[doc = "    \"value\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"call_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/CallId\""]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"type\": \"string\""]
#[doc = "    },"]
#[doc = "    \"value\": {}"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TurnResult {
    pub call_id: CallId,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    pub name: ::std::string::String,
    pub value: ::serde_json::Value,
}
impl TurnResult {
    pub fn builder() -> builder::TurnResult {
        Default::default()
    }
}
#[doc = r" Types for composing complex structures."]
pub mod builder {
    #[derive(Clone, Debug)]
    pub struct ApiError {
        code: ::std::result::Result<super::ApiErrorCode, ::std::string::String>,
        details: ::std::result::Result<
            ::std::option::Option<::serde_json::Value>,
            ::std::string::String,
        >,
        message: ::std::result::Result<::std::string::String, ::std::string::String>,
        param: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        request_id: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for ApiError {
        fn default() -> Self {
            Self {
                code: Err("no value supplied for code".to_string()),
                details: Ok(Default::default()),
                message: Err("no value supplied for message".to_string()),
                param: Ok(Default::default()),
                request_id: Ok(Default::default()),
            }
        }
    }
    impl ApiError {
        pub fn code<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ApiErrorCode>,
            T::Error: ::std::fmt::Display,
        {
            self.code = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for code: {e}"));
            self
        }
        pub fn details<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::serde_json::Value>>,
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
        pub fn param<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.param = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for param: {e}"));
            self
        }
        pub fn request_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.request_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ApiError> for super::ApiError {
        type Error = super::error::ConversionError;
        fn try_from(value: ApiError) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                code: value.code?,
                details: value.details?,
                message: value.message?,
                param: value.param?,
                request_id: value.request_id?,
            })
        }
    }
    impl ::std::convert::From<super::ApiError> for ApiError {
        fn from(value: super::ApiError) -> Self {
            Self {
                code: Ok(value.code),
                details: Ok(value.details),
                message: Ok(value.message),
                param: Ok(value.param),
                request_id: Ok(value.request_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ApiErrorResponse {
        error: ::std::result::Result<super::ApiError, ::std::string::String>,
    }
    impl ::std::default::Default for ApiErrorResponse {
        fn default() -> Self {
            Self {
                error: Err("no value supplied for error".to_string()),
            }
        }
    }
    impl ApiErrorResponse {
        pub fn error<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ApiError>,
            T::Error: ::std::fmt::Display,
        {
            self.error = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for error: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ApiErrorResponse> for super::ApiErrorResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ApiErrorResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                error: value.error?,
            })
        }
    }
    impl ::std::convert::From<super::ApiErrorResponse> for ApiErrorResponse {
        fn from(value: super::ApiErrorResponse) -> Self {
            Self {
                error: Ok(value.error),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Artifact {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        created_at: ::std::result::Result<super::Timestamp, ::std::string::String>,
        download_url: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        download_url_expires_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        media_type: ::std::result::Result<::std::string::String, ::std::string::String>,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        object: ::std::result::Result<super::ArtifactObject, ::std::string::String>,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        sha256: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
    }
    impl ::std::default::Default for Artifact {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                created_at: Err("no value supplied for created_at".to_string()),
                download_url: Ok(Default::default()),
                download_url_expires_at: Ok(Default::default()),
                media_type: Err("no value supplied for media_type".to_string()),
                name: Err("no value supplied for name".to_string()),
                object: Err("no value supplied for object".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                sha256: Err("no value supplied for sha256".to_string()),
            }
        }
    }
    impl Artifact {
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
        pub fn created_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Timestamp>,
            T::Error: ::std::fmt::Display,
        {
            self.created_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for created_at: {e}"));
            self
        }
        pub fn download_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.download_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for download_url: {e}"));
            self
        }
        pub fn download_url_expires_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.download_url_expires_at = value.try_into().map_err(|e| {
                format!("error converting supplied value for download_url_expires_at: {e}")
            });
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
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ArtifactObject>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
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
    impl ::std::convert::TryFrom<Artifact> for super::Artifact {
        type Error = super::error::ConversionError;
        fn try_from(value: Artifact) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                created_at: value.created_at?,
                download_url: value.download_url?,
                download_url_expires_at: value.download_url_expires_at?,
                media_type: value.media_type?,
                name: value.name?,
                object: value.object?,
                session_id: value.session_id?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::Artifact> for Artifact {
        fn from(value: super::Artifact) -> Self {
            Self {
                bytes: Ok(value.bytes),
                created_at: Ok(value.created_at),
                download_url: Ok(value.download_url),
                download_url_expires_at: Ok(value.download_url_expires_at),
                media_type: Ok(value.media_type),
                name: Ok(value.name),
                object: Ok(value.object),
                session_id: Ok(value.session_id),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ArtifactList {
        data: ::std::result::Result<::std::vec::Vec<super::Artifact>, ::std::string::String>,
        object: ::std::result::Result<super::ArtifactListObject, ::std::string::String>,
    }
    impl ::std::default::Default for ArtifactList {
        fn default() -> Self {
            Self {
                data: Err("no value supplied for data".to_string()),
                object: Err("no value supplied for object".to_string()),
            }
        }
    }
    impl ArtifactList {
        pub fn data<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::Artifact>>,
            T::Error: ::std::fmt::Display,
        {
            self.data = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for data: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ArtifactListObject>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ArtifactList> for super::ArtifactList {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ArtifactList,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                data: value.data?,
                object: value.object?,
            })
        }
    }
    impl ::std::convert::From<super::ArtifactList> for ArtifactList {
        fn from(value: super::ArtifactList) -> Self {
            Self {
                data: Ok(value.data),
                object: Ok(value.object),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CreateSessionRequest {
        files: ::std::result::Result<::std::vec::Vec<super::FileInput>, ::std::string::String>,
        hand:
            ::std::result::Result<::std::option::Option<super::HandConfig>, ::std::string::String>,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        model: ::std::result::Result<super::ModelConfig, ::std::string::String>,
        system_prompt: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        tools:
            ::std::result::Result<::std::option::Option<super::ToolsConfig>, ::std::string::String>,
    }
    impl ::std::default::Default for CreateSessionRequest {
        fn default() -> Self {
            Self {
                files: Ok(Default::default()),
                hand: Ok(Default::default()),
                metadata: Ok(Default::default()),
                model: Err("no value supplied for model".to_string()),
                system_prompt: Ok(Default::default()),
                tools: Ok(Default::default()),
            }
        }
    }
    impl CreateSessionRequest {
        pub fn files<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::FileInput>>,
            T::Error: ::std::fmt::Display,
        {
            self.files = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for files: {e}"));
            self
        }
        pub fn hand<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::HandConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.hand = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for hand: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for metadata: {e}"));
            self
        }
        pub fn model<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ModelConfig>,
            T::Error: ::std::fmt::Display,
        {
            self.model = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for model: {e}"));
            self
        }
        pub fn system_prompt<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.system_prompt = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for system_prompt: {e}"));
            self
        }
        pub fn tools<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ToolsConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.tools = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tools: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CreateSessionRequest> for super::CreateSessionRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CreateSessionRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                files: value.files?,
                hand: value.hand?,
                metadata: value.metadata?,
                model: value.model?,
                system_prompt: value.system_prompt?,
                tools: value.tools?,
            })
        }
    }
    impl ::std::convert::From<super::CreateSessionRequest> for CreateSessionRequest {
        fn from(value: super::CreateSessionRequest) -> Self {
            Self {
                files: Ok(value.files),
                hand: Ok(value.hand),
                metadata: Ok(value.metadata),
                model: Ok(value.model),
                system_prompt: Ok(value.system_prompt),
                tools: Ok(value.tools),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ExternalToolCallRequest {
        agent_id: ::std::result::Result<super::AgentId, ::std::string::String>,
        call_id: ::std::result::Result<super::CallId, ::std::string::String>,
        context: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        input: ::std::result::Result<::serde_json::Value, ::std::string::String>,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        turn_id: ::std::result::Result<super::TurnId, ::std::string::String>,
    }
    impl ::std::default::Default for ExternalToolCallRequest {
        fn default() -> Self {
            Self {
                agent_id: Err("no value supplied for agent_id".to_string()),
                call_id: Err("no value supplied for call_id".to_string()),
                context: Err("no value supplied for context".to_string()),
                input: Err("no value supplied for input".to_string()),
                name: Err("no value supplied for name".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                turn_id: Err("no value supplied for turn_id".to_string()),
            }
        }
    }
    impl ExternalToolCallRequest {
        pub fn agent_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::AgentId>,
            T::Error: ::std::fmt::Display,
        {
            self.agent_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for agent_id: {e}"));
            self
        }
        pub fn call_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::CallId>,
            T::Error: ::std::fmt::Display,
        {
            self.call_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for call_id: {e}"));
            self
        }
        pub fn context<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.context = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for context: {e}"));
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
        pub fn turn_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::TurnId>,
            T::Error: ::std::fmt::Display,
        {
            self.turn_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turn_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ExternalToolCallRequest> for super::ExternalToolCallRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ExternalToolCallRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                agent_id: value.agent_id?,
                call_id: value.call_id?,
                context: value.context?,
                input: value.input?,
                name: value.name?,
                session_id: value.session_id?,
                turn_id: value.turn_id?,
            })
        }
    }
    impl ::std::convert::From<super::ExternalToolCallRequest> for ExternalToolCallRequest {
        fn from(value: super::ExternalToolCallRequest) -> Self {
            Self {
                agent_id: Ok(value.agent_id),
                call_id: Ok(value.call_id),
                context: Ok(value.context),
                input: Ok(value.input),
                name: Ok(value.name),
                session_id: Ok(value.session_id),
                turn_id: Ok(value.turn_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ExternalToolCallResponse {
        content:
            ::std::result::Result<super::ExternalToolCallResponseContent, ::std::string::String>,
        disposition: ::std::result::Result<super::ExternalToolDisposition, ::std::string::String>,
        error: ::std::result::Result<::std::option::Option<super::ApiError>, ::std::string::String>,
        is_error: ::std::result::Result<bool, ::std::string::String>,
        outcome: ::std::result::Result<super::ToolOutcome, ::std::string::String>,
        result: ::std::result::Result<
            ::std::option::Option<::serde_json::Value>,
            ::std::string::String,
        >,
        result_metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for ExternalToolCallResponse {
        fn default() -> Self {
            Self {
                content: Err("no value supplied for content".to_string()),
                disposition: Err("no value supplied for disposition".to_string()),
                error: Ok(Default::default()),
                is_error: Err("no value supplied for is_error".to_string()),
                outcome: Err("no value supplied for outcome".to_string()),
                result: Ok(Default::default()),
                result_metadata: Ok(Default::default()),
            }
        }
    }
    impl ExternalToolCallResponse {
        pub fn content<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolCallResponseContent>,
            T::Error: ::std::fmt::Display,
        {
            self.content = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for content: {e}"));
            self
        }
        pub fn disposition<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolDisposition>,
            T::Error: ::std::fmt::Display,
        {
            self.disposition = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for disposition: {e}"));
            self
        }
        pub fn error<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ApiError>>,
            T::Error: ::std::fmt::Display,
        {
            self.error = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for error: {e}"));
            self
        }
        pub fn is_error<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.is_error = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for is_error: {e}"));
            self
        }
        pub fn outcome<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolOutcome>,
            T::Error: ::std::fmt::Display,
        {
            self.outcome = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for outcome: {e}"));
            self
        }
        pub fn result<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::serde_json::Value>>,
            T::Error: ::std::fmt::Display,
        {
            self.result = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for result: {e}"));
            self
        }
        pub fn result_metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.result_metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for result_metadata: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ExternalToolCallResponse> for super::ExternalToolCallResponse {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ExternalToolCallResponse,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                content: value.content?,
                disposition: value.disposition?,
                error: value.error?,
                is_error: value.is_error?,
                outcome: value.outcome?,
                result: value.result?,
                result_metadata: value.result_metadata?,
            })
        }
    }
    impl ::std::convert::From<super::ExternalToolCallResponse> for ExternalToolCallResponse {
        fn from(value: super::ExternalToolCallResponse) -> Self {
            Self {
                content: Ok(value.content),
                disposition: Ok(value.disposition),
                error: Ok(value.error),
                is_error: Ok(value.is_error),
                outcome: Ok(value.outcome),
                result: Ok(value.result),
                result_metadata: Ok(value.result_metadata),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ExternalToolConfig {
        completion: ::std::result::Result<super::ExternalToolCompletion, ::std::string::String>,
        description:
            ::std::result::Result<super::ExternalToolConfigDescription, ::std::string::String>,
        effect: ::std::result::Result<super::ExternalToolEffect, ::std::string::String>,
        input_schema: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        max_input_bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        name: ::std::result::Result<super::ExternalToolConfigName, ::std::string::String>,
        scope: ::std::result::Result<super::ExternalToolScope, ::std::string::String>,
    }
    impl ::std::default::Default for ExternalToolConfig {
        fn default() -> Self {
            Self {
                completion: Err("no value supplied for completion".to_string()),
                description: Err("no value supplied for description".to_string()),
                effect: Err("no value supplied for effect".to_string()),
                input_schema: Err("no value supplied for input_schema".to_string()),
                max_input_bytes: Err("no value supplied for max_input_bytes".to_string()),
                name: Err("no value supplied for name".to_string()),
                scope: Err("no value supplied for scope".to_string()),
            }
        }
    }
    impl ExternalToolConfig {
        pub fn completion<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolCompletion>,
            T::Error: ::std::fmt::Display,
        {
            self.completion = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for completion: {e}"));
            self
        }
        pub fn description<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolConfigDescription>,
            T::Error: ::std::fmt::Display,
        {
            self.description = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for description: {e}"));
            self
        }
        pub fn effect<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolEffect>,
            T::Error: ::std::fmt::Display,
        {
            self.effect = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for effect: {e}"));
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
        pub fn max_input_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_input_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_input_bytes: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolConfigName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn scope<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExternalToolScope>,
            T::Error: ::std::fmt::Display,
        {
            self.scope = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for scope: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ExternalToolConfig> for super::ExternalToolConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ExternalToolConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                completion: value.completion?,
                description: value.description?,
                effect: value.effect?,
                input_schema: value.input_schema?,
                max_input_bytes: value.max_input_bytes?,
                name: value.name?,
                scope: value.scope?,
            })
        }
    }
    impl ::std::convert::From<super::ExternalToolConfig> for ExternalToolConfig {
        fn from(value: super::ExternalToolConfig) -> Self {
            Self {
                completion: Ok(value.completion),
                description: Ok(value.description),
                effect: Ok(value.effect),
                input_schema: Ok(value.input_schema),
                max_input_bytes: Ok(value.max_input_bytes),
                name: Ok(value.name),
                scope: Ok(value.scope),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct FileEntry {
        kind: ::std::result::Result<super::FileEntryKind, ::std::string::String>,
        modified_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
        sha256:
            ::std::result::Result<::std::option::Option<super::Sha256Hex>, ::std::string::String>,
        size: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
    }
    impl ::std::default::Default for FileEntry {
        fn default() -> Self {
            Self {
                kind: Err("no value supplied for kind".to_string()),
                modified_at: Ok(Default::default()),
                path: Err("no value supplied for path".to_string()),
                sha256: Ok(Default::default()),
                size: Ok(Default::default()),
            }
        }
    }
    impl FileEntry {
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileEntryKind>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
            self
        }
        pub fn modified_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.modified_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for modified_at: {e}"));
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
            T: ::std::convert::TryInto<::std::option::Option<super::Sha256Hex>>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
        pub fn size<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.size = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for size: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<FileEntry> for super::FileEntry {
        type Error = super::error::ConversionError;
        fn try_from(
            value: FileEntry,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                kind: value.kind?,
                modified_at: value.modified_at?,
                path: value.path?,
                sha256: value.sha256?,
                size: value.size?,
            })
        }
    }
    impl ::std::convert::From<super::FileEntry> for FileEntry {
        fn from(value: super::FileEntry) -> Self {
            Self {
                kind: Ok(value.kind),
                modified_at: Ok(value.modified_at),
                path: Ok(value.path),
                sha256: Ok(value.sha256),
                size: Ok(value.size),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct FileInput {
        content_base64: ::std::result::Result<::std::string::String, ::std::string::String>,
        mode: ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for FileInput {
        fn default() -> Self {
            Self {
                content_base64: Err("no value supplied for content_base64".to_string()),
                mode: Ok(Default::default()),
                path: Err("no value supplied for path".to_string()),
            }
        }
    }
    impl FileInput {
        pub fn content_base64<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.content_base64 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for content_base64: {e}"));
            self
        }
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
    }
    impl ::std::convert::TryFrom<FileInput> for super::FileInput {
        type Error = super::error::ConversionError;
        fn try_from(
            value: FileInput,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                content_base64: value.content_base64?,
                mode: value.mode?,
                path: value.path?,
            })
        }
    }
    impl ::std::convert::From<super::FileInput> for FileInput {
        fn from(value: super::FileInput) -> Self {
            Self {
                content_base64: Ok(value.content_base64),
                mode: Ok(value.mode),
                path: Ok(value.path),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct FileList {
        data: ::std::result::Result<::std::vec::Vec<super::FileEntry>, ::std::string::String>,
        object: ::std::result::Result<super::FileListObject, ::std::string::String>,
        source: ::std::result::Result<super::FileListSource, ::std::string::String>,
        synced_at: ::std::result::Result<
            ::std::option::Option<::chrono::DateTime<::chrono::offset::Utc>>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for FileList {
        fn default() -> Self {
            Self {
                data: Err("no value supplied for data".to_string()),
                object: Err("no value supplied for object".to_string()),
                source: Err("no value supplied for source".to_string()),
                synced_at: Err("no value supplied for synced_at".to_string()),
            }
        }
    }
    impl FileList {
        pub fn data<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::FileEntry>>,
            T::Error: ::std::fmt::Display,
        {
            self.data = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for data: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileListObject>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn source<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileListSource>,
            T::Error: ::std::fmt::Display,
        {
            self.source = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for source: {e}"));
            self
        }
        pub fn synced_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::option::Option<::chrono::DateTime<::chrono::offset::Utc>>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.synced_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for synced_at: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<FileList> for super::FileList {
        type Error = super::error::ConversionError;
        fn try_from(value: FileList) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                data: value.data?,
                object: value.object?,
                source: value.source?,
                synced_at: value.synced_at?,
            })
        }
    }
    impl ::std::convert::From<super::FileList> for FileList {
        fn from(value: super::FileList) -> Self {
            Self {
                data: Ok(value.data),
                object: Ok(value.object),
                source: Ok(value.source),
                synced_at: Ok(value.synced_at),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HandConfig {
        enabled: ::std::result::Result<bool, ::std::string::String>,
        env: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        max_background_minutes: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
        shape:
            ::std::result::Result<::std::option::Option<super::HandShape>, ::std::string::String>,
        sync_interval_seconds: ::std::result::Result<i64, ::std::string::String>,
    }
    impl ::std::default::Default for HandConfig {
        fn default() -> Self {
            Self {
                enabled: Ok(super::defaults::default_bool::<true>()),
                env: Ok(Default::default()),
                max_background_minutes: Ok(Default::default()),
                shape: Ok(Default::default()),
                sync_interval_seconds: Ok(super::defaults::default_u64::<i64, 600>()),
            }
        }
    }
    impl HandConfig {
        pub fn enabled<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.enabled = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for enabled: {e}"));
            self
        }
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
        pub fn max_background_minutes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.max_background_minutes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_background_minutes: {e}")
            });
            self
        }
        pub fn shape<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::HandShape>>,
            T::Error: ::std::fmt::Display,
        {
            self.shape = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for shape: {e}"));
            self
        }
        pub fn sync_interval_seconds<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.sync_interval_seconds = value.try_into().map_err(|e| {
                format!("error converting supplied value for sync_interval_seconds: {e}")
            });
            self
        }
    }
    impl ::std::convert::TryFrom<HandConfig> for super::HandConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: HandConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                enabled: value.enabled?,
                env: value.env?,
                max_background_minutes: value.max_background_minutes?,
                shape: value.shape?,
                sync_interval_seconds: value.sync_interval_seconds?,
            })
        }
    }
    impl ::std::convert::From<super::HandConfig> for HandConfig {
        fn from(value: super::HandConfig) -> Self {
            Self {
                enabled: Ok(value.enabled),
                env: Ok(value.env),
                max_background_minutes: Ok(value.max_background_minutes),
                shape: Ok(value.shape),
                sync_interval_seconds: Ok(value.sync_interval_seconds),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HandInfo {
        generation: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        last_sync_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        live_jobs: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        shape: ::std::result::Result<super::HandShape, ::std::string::String>,
        started_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        state: ::std::result::Result<super::HandState, ::std::string::String>,
        wall_deadline_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
    }
    impl ::std::default::Default for HandInfo {
        fn default() -> Self {
            Self {
                generation: Ok(Default::default()),
                last_sync_at: Ok(Default::default()),
                live_jobs: Ok(Default::default()),
                shape: Err("no value supplied for shape".to_string()),
                started_at: Ok(Default::default()),
                state: Err("no value supplied for state".to_string()),
                wall_deadline_at: Ok(Default::default()),
            }
        }
    }
    impl HandInfo {
        pub fn generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.generation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation: {e}"));
            self
        }
        pub fn last_sync_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.last_sync_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for last_sync_at: {e}"));
            self
        }
        pub fn live_jobs<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.live_jobs = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for live_jobs: {e}"));
            self
        }
        pub fn shape<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::HandShape>,
            T::Error: ::std::fmt::Display,
        {
            self.shape = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for shape: {e}"));
            self
        }
        pub fn started_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.started_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for started_at: {e}"));
            self
        }
        pub fn state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::HandState>,
            T::Error: ::std::fmt::Display,
        {
            self.state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for state: {e}"));
            self
        }
        pub fn wall_deadline_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.wall_deadline_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wall_deadline_at: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<HandInfo> for super::HandInfo {
        type Error = super::error::ConversionError;
        fn try_from(value: HandInfo) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                generation: value.generation?,
                last_sync_at: value.last_sync_at?,
                live_jobs: value.live_jobs?,
                shape: value.shape?,
                started_at: value.started_at?,
                state: value.state?,
                wall_deadline_at: value.wall_deadline_at?,
            })
        }
    }
    impl ::std::convert::From<super::HandInfo> for HandInfo {
        fn from(value: super::HandInfo) -> Self {
            Self {
                generation: Ok(value.generation),
                last_sync_at: Ok(value.last_sync_at),
                live_jobs: Ok(value.live_jobs),
                shape: Ok(value.shape),
                started_at: Ok(value.started_at),
                state: Ok(value.state),
                wall_deadline_at: Ok(value.wall_deadline_at),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct McpServerConfig {
        allowed_tools:
            ::std::result::Result<::std::vec::Vec<::std::string::String>, ::std::string::String>,
        headers: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::McpServerConfigName, ::std::string::String>,
        protocol:
            ::std::result::Result<::std::option::Option<super::McpProtocol>, ::std::string::String>,
        url: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for McpServerConfig {
        fn default() -> Self {
            Self {
                allowed_tools: Ok(Default::default()),
                headers: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                protocol: Ok(Default::default()),
                url: Err("no value supplied for url".to_string()),
            }
        }
    }
    impl McpServerConfig {
        pub fn allowed_tools<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.allowed_tools = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for allowed_tools: {e}"));
            self
        }
        pub fn headers<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.headers = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for headers: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::McpServerConfigName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn protocol<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::McpProtocol>>,
            T::Error: ::std::fmt::Display,
        {
            self.protocol = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for protocol: {e}"));
            self
        }
        pub fn url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for url: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<McpServerConfig> for super::McpServerConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: McpServerConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                allowed_tools: value.allowed_tools?,
                headers: value.headers?,
                name: value.name?,
                protocol: value.protocol?,
                url: value.url?,
            })
        }
    }
    impl ::std::convert::From<super::McpServerConfig> for McpServerConfig {
        fn from(value: super::McpServerConfig) -> Self {
            Self {
                allowed_tools: Ok(value.allowed_tools),
                headers: Ok(value.headers),
                name: Ok(value.name),
                protocol: Ok(value.protocol),
                url: Ok(value.url),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct MessageAccepted {
        output_id:
            ::std::result::Result<::std::option::Option<super::OutputId>, ::std::string::String>,
        schema_hash:
            ::std::result::Result<::std::option::Option<super::Sha256Hex>, ::std::string::String>,
        seq: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        turn_id: ::std::result::Result<super::TurnId, ::std::string::String>,
    }
    impl ::std::default::Default for MessageAccepted {
        fn default() -> Self {
            Self {
                output_id: Ok(Default::default()),
                schema_hash: Ok(Default::default()),
                seq: Err("no value supplied for seq".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                turn_id: Err("no value supplied for turn_id".to_string()),
            }
        }
    }
    impl MessageAccepted {
        pub fn output_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::OutputId>>,
            T::Error: ::std::fmt::Display,
        {
            self.output_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output_id: {e}"));
            self
        }
        pub fn schema_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Sha256Hex>>,
            T::Error: ::std::fmt::Display,
        {
            self.schema_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema_hash: {e}"));
            self
        }
        pub fn seq<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.seq = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for seq: {e}"));
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
        pub fn turn_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::TurnId>,
            T::Error: ::std::fmt::Display,
        {
            self.turn_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turn_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<MessageAccepted> for super::MessageAccepted {
        type Error = super::error::ConversionError;
        fn try_from(
            value: MessageAccepted,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                output_id: value.output_id?,
                schema_hash: value.schema_hash?,
                seq: value.seq?,
                session_id: value.session_id?,
                turn_id: value.turn_id?,
            })
        }
    }
    impl ::std::convert::From<super::MessageAccepted> for MessageAccepted {
        fn from(value: super::MessageAccepted) -> Self {
            Self {
                output_id: Ok(value.output_id),
                schema_hash: Ok(value.schema_hash),
                seq: Ok(value.seq),
                session_id: Ok(value.session_id),
                turn_id: Ok(value.turn_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct MessageOutput {
        retries: ::std::result::Result<i64, ::std::string::String>,
        schema: ::std::result::Result<super::OutputSchema, ::std::string::String>,
        schema_hash: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
    }
    impl ::std::default::Default for MessageOutput {
        fn default() -> Self {
            Self {
                retries: Ok(super::defaults::default_u64::<i64, 1>()),
                schema: Err("no value supplied for schema".to_string()),
                schema_hash: Err("no value supplied for schema_hash".to_string()),
            }
        }
    }
    impl MessageOutput {
        pub fn retries<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.retries = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for retries: {e}"));
            self
        }
        pub fn schema<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OutputSchema>,
            T::Error: ::std::fmt::Display,
        {
            self.schema = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema: {e}"));
            self
        }
        pub fn schema_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.schema_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema_hash: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<MessageOutput> for super::MessageOutput {
        type Error = super::error::ConversionError;
        fn try_from(
            value: MessageOutput,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                retries: value.retries?,
                schema: value.schema?,
                schema_hash: value.schema_hash?,
            })
        }
    }
    impl ::std::convert::From<super::MessageOutput> for MessageOutput {
        fn from(value: super::MessageOutput) -> Self {
            Self {
                retries: Ok(value.retries),
                schema: Ok(value.schema),
                schema_hash: Ok(value.schema_hash),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct MessageRequest {
        content: ::std::result::Result<super::MessageRequestContent, ::std::string::String>,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        output: ::std::result::Result<
            ::std::option::Option<super::MessageOutput>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for MessageRequest {
        fn default() -> Self {
            Self {
                content: Err("no value supplied for content".to_string()),
                metadata: Ok(Default::default()),
                output: Ok(Default::default()),
            }
        }
    }
    impl MessageRequest {
        pub fn content<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::MessageRequestContent>,
            T::Error: ::std::fmt::Display,
        {
            self.content = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for content: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for metadata: {e}"));
            self
        }
        pub fn output<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::MessageOutput>>,
            T::Error: ::std::fmt::Display,
        {
            self.output = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<MessageRequest> for super::MessageRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: MessageRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                content: value.content?,
                metadata: value.metadata?,
                output: value.output?,
            })
        }
    }
    impl ::std::convert::From<super::MessageRequest> for MessageRequest {
        fn from(value: super::MessageRequest) -> Self {
            Self {
                content: Ok(value.content),
                metadata: Ok(value.metadata),
                output: Ok(value.output),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ModelConfig {
        api_key: ::std::result::Result<super::ModelConfigApiKey, ::std::string::String>,
        base_url: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        max_output_tokens: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::ModelConfigName, ::std::string::String>,
        provider: ::std::result::Result<super::Provider, ::std::string::String>,
        reasoning_effort: ::std::result::Result<
            ::std::option::Option<super::ModelConfigReasoningEffort>,
            ::std::string::String,
        >,
        temperature: ::std::result::Result<::std::option::Option<f64>, ::std::string::String>,
    }
    impl ::std::default::Default for ModelConfig {
        fn default() -> Self {
            Self {
                api_key: Err("no value supplied for api_key".to_string()),
                base_url: Ok(Default::default()),
                max_output_tokens: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                provider: Err("no value supplied for provider".to_string()),
                reasoning_effort: Ok(Default::default()),
                temperature: Ok(Default::default()),
            }
        }
    }
    impl ModelConfig {
        pub fn api_key<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ModelConfigApiKey>,
            T::Error: ::std::fmt::Display,
        {
            self.api_key = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for api_key: {e}"));
            self
        }
        pub fn base_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.base_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for base_url: {e}"));
            self
        }
        pub fn max_output_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.max_output_tokens = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_output_tokens: {e}"));
            self
        }
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ModelConfigName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
            self
        }
        pub fn provider<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Provider>,
            T::Error: ::std::fmt::Display,
        {
            self.provider = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for provider: {e}"));
            self
        }
        pub fn reasoning_effort<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ModelConfigReasoningEffort>>,
            T::Error: ::std::fmt::Display,
        {
            self.reasoning_effort = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for reasoning_effort: {e}"));
            self
        }
        pub fn temperature<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<f64>>,
            T::Error: ::std::fmt::Display,
        {
            self.temperature = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for temperature: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ModelConfig> for super::ModelConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ModelConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                api_key: value.api_key?,
                base_url: value.base_url?,
                max_output_tokens: value.max_output_tokens?,
                name: value.name?,
                provider: value.provider?,
                reasoning_effort: value.reasoning_effort?,
                temperature: value.temperature?,
            })
        }
    }
    impl ::std::convert::From<super::ModelConfig> for ModelConfig {
        fn from(value: super::ModelConfig) -> Self {
            Self {
                api_key: Ok(value.api_key),
                base_url: Ok(value.base_url),
                max_output_tokens: Ok(value.max_output_tokens),
                name: Ok(value.name),
                provider: Ok(value.provider),
                reasoning_effort: Ok(value.reasoning_effort),
                temperature: Ok(value.temperature),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ModelInfo {
        base_url: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        provider: ::std::result::Result<super::Provider, ::std::string::String>,
    }
    impl ::std::default::Default for ModelInfo {
        fn default() -> Self {
            Self {
                base_url: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                provider: Err("no value supplied for provider".to_string()),
            }
        }
    }
    impl ModelInfo {
        pub fn base_url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.base_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for base_url: {e}"));
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
        pub fn provider<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Provider>,
            T::Error: ::std::fmt::Display,
        {
            self.provider = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for provider: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ModelInfo> for super::ModelInfo {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ModelInfo,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                base_url: value.base_url?,
                name: value.name?,
                provider: value.provider?,
            })
        }
    }
    impl ::std::convert::From<super::ModelInfo> for ModelInfo {
        fn from(value: super::ModelInfo) -> Self {
            Self {
                base_url: Ok(value.base_url),
                name: Ok(value.name),
                provider: Ok(value.provider),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputAccepted {
        output_id: ::std::result::Result<super::OutputId, ::std::string::String>,
        schema_hash: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        seq: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
    }
    impl ::std::default::Default for OutputAccepted {
        fn default() -> Self {
            Self {
                output_id: Err("no value supplied for output_id".to_string()),
                schema_hash: Err("no value supplied for schema_hash".to_string()),
                seq: Err("no value supplied for seq".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
            }
        }
    }
    impl OutputAccepted {
        pub fn output_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OutputId>,
            T::Error: ::std::fmt::Display,
        {
            self.output_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output_id: {e}"));
            self
        }
        pub fn schema_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.schema_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema_hash: {e}"));
            self
        }
        pub fn seq<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.seq = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for seq: {e}"));
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
    }
    impl ::std::convert::TryFrom<OutputAccepted> for super::OutputAccepted {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputAccepted,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                output_id: value.output_id?,
                schema_hash: value.schema_hash?,
                seq: value.seq?,
                session_id: value.session_id?,
            })
        }
    }
    impl ::std::convert::From<super::OutputAccepted> for OutputAccepted {
        fn from(value: super::OutputAccepted) -> Self {
            Self {
                output_id: Ok(value.output_id),
                schema_hash: Ok(value.schema_hash),
                seq: Ok(value.seq),
                session_id: Ok(value.session_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputContent {
        schema_hash: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        type_: ::std::result::Result<super::OutputContentType, ::std::string::String>,
        value: ::std::result::Result<::serde_json::Value, ::std::string::String>,
    }
    impl ::std::default::Default for OutputContent {
        fn default() -> Self {
            Self {
                schema_hash: Err("no value supplied for schema_hash".to_string()),
                type_: Err("no value supplied for type_".to_string()),
                value: Err("no value supplied for value".to_string()),
            }
        }
    }
    impl OutputContent {
        pub fn schema_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.schema_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema_hash: {e}"));
            self
        }
        pub fn type_<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OutputContentType>,
            T::Error: ::std::fmt::Display,
        {
            self.type_ = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for type_: {e}"));
            self
        }
        pub fn value<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::serde_json::Value>,
            T::Error: ::std::fmt::Display,
        {
            self.value = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for value: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OutputContent> for super::OutputContent {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputContent,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                schema_hash: value.schema_hash?,
                type_: value.type_?,
                value: value.value?,
            })
        }
    }
    impl ::std::convert::From<super::OutputContent> for OutputContent {
        fn from(value: super::OutputContent) -> Self {
            Self {
                schema_hash: Ok(value.schema_hash),
                type_: Ok(value.type_),
                value: Ok(value.value),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputRequest {
        input: ::std::result::Result<
            ::std::option::Option<super::OutputRequestInput>,
            ::std::string::String,
        >,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        schema: ::std::result::Result<super::OutputSchema, ::std::string::String>,
        schema_hash: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
    }
    impl ::std::default::Default for OutputRequest {
        fn default() -> Self {
            Self {
                input: Ok(Default::default()),
                metadata: Ok(Default::default()),
                schema: Err("no value supplied for schema".to_string()),
                schema_hash: Err("no value supplied for schema_hash".to_string()),
            }
        }
    }
    impl OutputRequest {
        pub fn input<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::OutputRequestInput>>,
            T::Error: ::std::fmt::Display,
        {
            self.input = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for metadata: {e}"));
            self
        }
        pub fn schema<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OutputSchema>,
            T::Error: ::std::fmt::Display,
        {
            self.schema = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema: {e}"));
            self
        }
        pub fn schema_hash<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.schema_hash = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for schema_hash: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OutputRequest> for super::OutputRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                input: value.input?,
                metadata: value.metadata?,
                schema: value.schema?,
                schema_hash: value.schema_hash?,
            })
        }
    }
    impl ::std::convert::From<super::OutputRequest> for OutputRequest {
        fn from(value: super::OutputRequest) -> Self {
            Self {
                input: Ok(value.input),
                metadata: Ok(value.metadata),
                schema: Ok(value.schema),
                schema_hash: Ok(value.schema_hash),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputValidationIssue {
        keyword: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        message: ::std::result::Result<::std::string::String, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for OutputValidationIssue {
        fn default() -> Self {
            Self {
                keyword: Ok(Default::default()),
                message: Err("no value supplied for message".to_string()),
                path: Err("no value supplied for path".to_string()),
            }
        }
    }
    impl OutputValidationIssue {
        pub fn keyword<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.keyword = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for keyword: {e}"));
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
    }
    impl ::std::convert::TryFrom<OutputValidationIssue> for super::OutputValidationIssue {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputValidationIssue,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                keyword: value.keyword?,
                message: value.message?,
                path: value.path?,
            })
        }
    }
    impl ::std::convert::From<super::OutputValidationIssue> for OutputValidationIssue {
        fn from(value: super::OutputValidationIssue) -> Self {
            Self {
                keyword: Ok(value.keyword),
                message: Ok(value.message),
                path: Ok(value.path),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PersistRequest {
        media_type: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::PersistRequestName, ::std::string::String>,
        path: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for PersistRequest {
        fn default() -> Self {
            Self {
                media_type: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                path: Err("no value supplied for path".to_string()),
            }
        }
    }
    impl PersistRequest {
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
            T: ::std::convert::TryInto<super::PersistRequestName>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
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
    }
    impl ::std::convert::TryFrom<PersistRequest> for super::PersistRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PersistRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                media_type: value.media_type?,
                name: value.name?,
                path: value.path?,
            })
        }
    }
    impl ::std::convert::From<super::PersistRequest> for PersistRequest {
        fn from(value: super::PersistRequest) -> Self {
            Self {
                media_type: Ok(value.media_type),
                name: Ok(value.name),
                path: Ok(value.path),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ProviderUsage {
        cache_creation_input_tokens:
            ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        cache_read_input_tokens:
            ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        input_tokens: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        output_tokens: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        reasoning_tokens: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
    }
    impl ::std::default::Default for ProviderUsage {
        fn default() -> Self {
            Self {
                cache_creation_input_tokens: Ok(Default::default()),
                cache_read_input_tokens: Ok(Default::default()),
                input_tokens: Ok(Default::default()),
                output_tokens: Ok(Default::default()),
                reasoning_tokens: Ok(Default::default()),
            }
        }
    }
    impl ProviderUsage {
        pub fn cache_creation_input_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.cache_creation_input_tokens = value.try_into().map_err(|e| {
                format!("error converting supplied value for cache_creation_input_tokens: {e}")
            });
            self
        }
        pub fn cache_read_input_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.cache_read_input_tokens = value.try_into().map_err(|e| {
                format!("error converting supplied value for cache_read_input_tokens: {e}")
            });
            self
        }
        pub fn input_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.input_tokens = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input_tokens: {e}"));
            self
        }
        pub fn output_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.output_tokens = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output_tokens: {e}"));
            self
        }
        pub fn reasoning_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.reasoning_tokens = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for reasoning_tokens: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ProviderUsage> for super::ProviderUsage {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ProviderUsage,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                cache_creation_input_tokens: value.cache_creation_input_tokens?,
                cache_read_input_tokens: value.cache_read_input_tokens?,
                input_tokens: value.input_tokens?,
                output_tokens: value.output_tokens?,
                reasoning_tokens: value.reasoning_tokens?,
            })
        }
    }
    impl ::std::convert::From<super::ProviderUsage> for ProviderUsage {
        fn from(value: super::ProviderUsage) -> Self {
            Self {
                cache_creation_input_tokens: Ok(value.cache_creation_input_tokens),
                cache_read_input_tokens: Ok(value.cache_read_input_tokens),
                input_tokens: Ok(value.input_tokens),
                output_tokens: Ok(value.output_tokens),
                reasoning_tokens: Ok(value.reasoning_tokens),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Session {
        created_at: ::std::result::Result<super::Timestamp, ::std::string::String>,
        current_turn:
            ::std::result::Result<::std::option::Option<super::TurnId>, ::std::string::String>,
        failure: ::std::result::Result<
            ::std::option::Option<super::SessionFailure>,
            ::std::string::String,
        >,
        hand: ::std::result::Result<super::HandInfo, ::std::string::String>,
        id: ::std::result::Result<super::SessionId, ::std::string::String>,
        last_message_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        model: ::std::result::Result<super::ModelInfo, ::std::string::String>,
        object: ::std::result::Result<super::SessionObject, ::std::string::String>,
        state: ::std::result::Result<super::SessionState, ::std::string::String>,
        storage: ::std::result::Result<super::StorageInfo, ::std::string::String>,
        turns: ::std::result::Result<u64, ::std::string::String>,
        updated_at: ::std::result::Result<super::Timestamp, ::std::string::String>,
    }
    impl ::std::default::Default for Session {
        fn default() -> Self {
            Self {
                created_at: Err("no value supplied for created_at".to_string()),
                current_turn: Ok(Default::default()),
                failure: Ok(Default::default()),
                hand: Err("no value supplied for hand".to_string()),
                id: Err("no value supplied for id".to_string()),
                last_message_at: Ok(Default::default()),
                metadata: Err("no value supplied for metadata".to_string()),
                model: Err("no value supplied for model".to_string()),
                object: Err("no value supplied for object".to_string()),
                state: Err("no value supplied for state".to_string()),
                storage: Err("no value supplied for storage".to_string()),
                turns: Err("no value supplied for turns".to_string()),
                updated_at: Err("no value supplied for updated_at".to_string()),
            }
        }
    }
    impl Session {
        pub fn created_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Timestamp>,
            T::Error: ::std::fmt::Display,
        {
            self.created_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for created_at: {e}"));
            self
        }
        pub fn current_turn<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::TurnId>>,
            T::Error: ::std::fmt::Display,
        {
            self.current_turn = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for current_turn: {e}"));
            self
        }
        pub fn failure<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SessionFailure>>,
            T::Error: ::std::fmt::Display,
        {
            self.failure = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for failure: {e}"));
            self
        }
        pub fn hand<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::HandInfo>,
            T::Error: ::std::fmt::Display,
        {
            self.hand = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for hand: {e}"));
            self
        }
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
        pub fn last_message_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Timestamp>>,
            T::Error: ::std::fmt::Display,
        {
            self.last_message_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for last_message_at: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for metadata: {e}"));
            self
        }
        pub fn model<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ModelInfo>,
            T::Error: ::std::fmt::Display,
        {
            self.model = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for model: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionObject>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionState>,
            T::Error: ::std::fmt::Display,
        {
            self.state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for state: {e}"));
            self
        }
        pub fn storage<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::StorageInfo>,
            T::Error: ::std::fmt::Display,
        {
            self.storage = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for storage: {e}"));
            self
        }
        pub fn turns<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.turns = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turns: {e}"));
            self
        }
        pub fn updated_at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Timestamp>,
            T::Error: ::std::fmt::Display,
        {
            self.updated_at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for updated_at: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Session> for super::Session {
        type Error = super::error::ConversionError;
        fn try_from(value: Session) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                created_at: value.created_at?,
                current_turn: value.current_turn?,
                failure: value.failure?,
                hand: value.hand?,
                id: value.id?,
                last_message_at: value.last_message_at?,
                metadata: value.metadata?,
                model: value.model?,
                object: value.object?,
                state: value.state?,
                storage: value.storage?,
                turns: value.turns?,
                updated_at: value.updated_at?,
            })
        }
    }
    impl ::std::convert::From<super::Session> for Session {
        fn from(value: super::Session) -> Self {
            Self {
                created_at: Ok(value.created_at),
                current_turn: Ok(value.current_turn),
                failure: Ok(value.failure),
                hand: Ok(value.hand),
                id: Ok(value.id),
                last_message_at: Ok(value.last_message_at),
                metadata: Ok(value.metadata),
                model: Ok(value.model),
                object: Ok(value.object),
                state: Ok(value.state),
                storage: Ok(value.storage),
                turns: Ok(value.turns),
                updated_at: Ok(value.updated_at),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SessionFailure {
        at: ::std::result::Result<super::Timestamp, ::std::string::String>,
        code: ::std::result::Result<super::SessionFailureCode, ::std::string::String>,
        message: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for SessionFailure {
        fn default() -> Self {
            Self {
                at: Err("no value supplied for at".to_string()),
                code: Err("no value supplied for code".to_string()),
                message: Err("no value supplied for message".to_string()),
            }
        }
    }
    impl SessionFailure {
        pub fn at<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Timestamp>,
            T::Error: ::std::fmt::Display,
        {
            self.at = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for at: {e}"));
            self
        }
        pub fn code<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionFailureCode>,
            T::Error: ::std::fmt::Display,
        {
            self.code = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for code: {e}"));
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
    }
    impl ::std::convert::TryFrom<SessionFailure> for super::SessionFailure {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SessionFailure,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                at: value.at?,
                code: value.code?,
                message: value.message?,
            })
        }
    }
    impl ::std::convert::From<super::SessionFailure> for SessionFailure {
        fn from(value: super::SessionFailure) -> Self {
            Self {
                at: Ok(value.at),
                code: Ok(value.code),
                message: Ok(value.message),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SessionList {
        data: ::std::result::Result<::std::vec::Vec<super::Session>, ::std::string::String>,
        has_more: ::std::result::Result<bool, ::std::string::String>,
        next_cursor: ::std::result::Result<
            ::std::option::Option<::std::string::String>,
            ::std::string::String,
        >,
        object: ::std::result::Result<super::SessionListObject, ::std::string::String>,
    }
    impl ::std::default::Default for SessionList {
        fn default() -> Self {
            Self {
                data: Err("no value supplied for data".to_string()),
                has_more: Err("no value supplied for has_more".to_string()),
                next_cursor: Ok(Default::default()),
                object: Err("no value supplied for object".to_string()),
            }
        }
    }
    impl SessionList {
        pub fn data<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::Session>>,
            T::Error: ::std::fmt::Display,
        {
            self.data = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for data: {e}"));
            self
        }
        pub fn has_more<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.has_more = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for has_more: {e}"));
            self
        }
        pub fn next_cursor<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
            T::Error: ::std::fmt::Display,
        {
            self.next_cursor = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for next_cursor: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionListObject>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SessionList> for super::SessionList {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SessionList,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                data: value.data?,
                has_more: value.has_more?,
                next_cursor: value.next_cursor?,
                object: value.object?,
            })
        }
    }
    impl ::std::convert::From<super::SessionList> for SessionList {
        fn from(value: super::SessionList) -> Self {
            Self {
                data: Ok(value.data),
                has_more: Ok(value.has_more),
                next_cursor: Ok(value.next_cursor),
                object: Ok(value.object),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StorageInfo {
        artifact_bytes: ::std::result::Result<u64, ::std::string::String>,
        suspended_bytes: ::std::result::Result<u64, ::std::string::String>,
        workspace_bytes: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for StorageInfo {
        fn default() -> Self {
            Self {
                artifact_bytes: Err("no value supplied for artifact_bytes".to_string()),
                suspended_bytes: Err("no value supplied for suspended_bytes".to_string()),
                workspace_bytes: Err("no value supplied for workspace_bytes".to_string()),
            }
        }
    }
    impl StorageInfo {
        pub fn artifact_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.artifact_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for artifact_bytes: {e}"));
            self
        }
        pub fn suspended_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.suspended_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for suspended_bytes: {e}"));
            self
        }
        pub fn workspace_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.workspace_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for workspace_bytes: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<StorageInfo> for super::StorageInfo {
        type Error = super::error::ConversionError;
        fn try_from(
            value: StorageInfo,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                artifact_bytes: value.artifact_bytes?,
                suspended_bytes: value.suspended_bytes?,
                workspace_bytes: value.workspace_bytes?,
            })
        }
    }
    impl ::std::convert::From<super::StorageInfo> for StorageInfo {
        fn from(value: super::StorageInfo) -> Self {
            Self {
                artifact_bytes: Ok(value.artifact_bytes),
                suspended_bytes: Ok(value.suspended_bytes),
                workspace_bytes: Ok(value.workspace_bytes),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolsConfig {
        builtin: ::std::result::Result<
            ::std::option::Option<Vec<super::BuiltinTool>>,
            ::std::string::String,
        >,
        external: ::std::result::Result<
            ::std::vec::Vec<super::ExternalToolConfig>,
            ::std::string::String,
        >,
        mcp: ::std::result::Result<::std::vec::Vec<super::McpServerConfig>, ::std::string::String>,
    }
    impl ::std::default::Default for ToolsConfig {
        fn default() -> Self {
            Self {
                builtin: Ok(Default::default()),
                external: Ok(Default::default()),
                mcp: Ok(Default::default()),
            }
        }
    }
    impl ToolsConfig {
        pub fn builtin<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<Vec<super::BuiltinTool>>>,
            T::Error: ::std::fmt::Display,
        {
            self.builtin = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for builtin: {e}"));
            self
        }
        pub fn external<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::ExternalToolConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.external = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for external: {e}"));
            self
        }
        pub fn mcp<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::McpServerConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.mcp = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for mcp: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ToolsConfig> for super::ToolsConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolsConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                builtin: value.builtin?,
                external: value.external?,
                mcp: value.mcp?,
            })
        }
    }
    impl ::std::convert::From<super::ToolsConfig> for ToolsConfig {
        fn from(value: super::ToolsConfig) -> Self {
            Self {
                builtin: Ok(value.builtin),
                external: Ok(value.external),
                mcp: Ok(value.mcp),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TurnResult {
        call_id: ::std::result::Result<super::CallId, ::std::string::String>,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, ::std::string::String>,
            ::std::string::String,
        >,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        value: ::std::result::Result<::serde_json::Value, ::std::string::String>,
    }
    impl ::std::default::Default for TurnResult {
        fn default() -> Self {
            Self {
                call_id: Err("no value supplied for call_id".to_string()),
                metadata: Ok(Default::default()),
                name: Err("no value supplied for name".to_string()),
                value: Err("no value supplied for value".to_string()),
            }
        }
    }
    impl TurnResult {
        pub fn call_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::CallId>,
            T::Error: ::std::fmt::Display,
        {
            self.call_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for call_id: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<::std::string::String, ::std::string::String>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.metadata = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for metadata: {e}"));
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
        pub fn value<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::serde_json::Value>,
            T::Error: ::std::fmt::Display,
        {
            self.value = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for value: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<TurnResult> for super::TurnResult {
        type Error = super::error::ConversionError;
        fn try_from(
            value: TurnResult,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                call_id: value.call_id?,
                metadata: value.metadata?,
                name: value.name?,
                value: value.value?,
            })
        }
    }
    impl ::std::convert::From<super::TurnResult> for TurnResult {
        fn from(value: super::TurnResult) -> Self {
            Self {
                call_id: Ok(value.call_id),
                metadata: Ok(value.metadata),
                name: Ok(value.name),
                value: Ok(value.value),
            }
        }
    }
}
#[doc = r" Generation of default values for serde."]
pub mod defaults {
    pub(super) fn default_bool<const V: bool>() -> bool {
        V
    }
    pub(super) fn default_u64<T, const V: u64>() -> T
    where
        T: ::std::convert::TryFrom<u64>,
        <T as ::std::convert::TryFrom<u64>>::Error: ::std::fmt::Debug,
    {
        T::try_from(V).unwrap()
    }
}
