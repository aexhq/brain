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
#[doc = "A digest-sealed executable in the session's default Aex-managed realm."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A digest-sealed executable in the session's default Aex-managed realm.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bundle_digest\","]
#[doc = "    \"kind\","]
#[doc = "    \"required_env\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bundle_digest\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"aex_managed\""]
#[doc = "    },"]
#[doc = "    \"required_env\": {"]
#[doc = "      \"description\": \"Environment-key names only. Secret values never enter the seal.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"pattern\": \"^[A-Za-z_][A-Za-z0-9_]*$\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 64,"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AexManagedToolExecutor {
    pub bundle_digest: Sha256Hex,
    pub kind: ::std::string::String,
    #[doc = "Environment-key names only. Secret values never enter the seal."]
    pub required_env: Vec<AexManagedToolExecutorRequiredEnvItem>,
}
impl AexManagedToolExecutor {
    pub fn builder() -> builder::AexManagedToolExecutor {
        Default::default()
    }
}
#[doc = "`AexManagedToolExecutorRequiredEnvItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z_][A-Za-z0-9_]*$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct AexManagedToolExecutorRequiredEnvItem(::std::string::String);
impl ::std::ops::Deref for AexManagedToolExecutorRequiredEnvItem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<AexManagedToolExecutorRequiredEnvItem> for ::std::string::String {
    fn from(value: AexManagedToolExecutorRequiredEnvItem) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for AexManagedToolExecutorRequiredEnvItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z_][A-Za-z0-9_]*$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z_][A-Za-z0-9_]*$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for AexManagedToolExecutorRequiredEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AexManagedToolExecutorRequiredEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AexManagedToolExecutorRequiredEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for AexManagedToolExecutorRequiredEnvItem {
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "Stable machine-readable code. Brain defines its core codes; a host executor may return its own code without teaching Brain product semantics."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Stable machine-readable code. Brain defines its core codes; a host executor may return its own code without teaching Brain product semantics.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[a-z][a-z0-9_]{0,63}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ApiErrorCode(::std::string::String);
impl ::std::ops::Deref for ApiErrorCode {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ApiErrorCode> for ::std::string::String {
    fn from(value: ApiErrorCode) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ApiErrorCode {
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
impl<'de> ::serde::Deserialize<'de> for ApiErrorCode {
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}
impl ApiErrorResponse {
    pub fn builder() -> builder::ApiErrorResponse {
        Default::default()
    }
}
#[doc = "Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Session lifecycle and current-turn activity are independent axes. Absent provider counters are absent, never zero."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://github.com/aexhq/brain/contracts/session/v1/schemas.json\","]
#[doc = "  \"title\": \"Brain session API v1 types\","]
#[doc = "  \"description\": \"Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Session lifecycle and current-turn activity are independent axes. Absent provider counters are absent, never zero.\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct BrainSessionApiV1Types(pub ::serde_json::Value);
impl ::std::ops::Deref for BrainSessionApiV1Types {
    type Target = ::serde_json::Value;
    fn deref(&self) -> &::serde_json::Value {
        &self.0
    }
}
impl ::std::convert::From<BrainSessionApiV1Types> for ::serde_json::Value {
    fn from(value: BrainSessionApiV1Types) -> Self {
        value.0
    }
}
impl ::std::convert::From<::serde_json::Value> for BrainSessionApiV1Types {
    fn from(value: ::serde_json::Value) -> Self {
        Self(value)
    }
}
#[doc = "Brain-minted id of one durable Tool operation. Managed Hands receive the same operation_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted id of one durable Tool operation. Managed Hands receive the same operation_id.\","]
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
#[doc = "`ChildLimits`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"max_depth\": {"]
#[doc = "      \"default\": 4,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 8.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_descendants\": {"]
#[doc = "      \"default\": 256,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 1024.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"max_direct_children\": {"]
#[doc = "      \"default\": 32,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 128.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ChildLimits {
    #[serde(default = "defaults::default_u64::<i64, 4>")]
    pub max_depth: i64,
    #[serde(default = "defaults::default_u64::<i64, 256>")]
    pub max_descendants: i64,
    #[serde(default = "defaults::default_u64::<i64, 32>")]
    pub max_direct_children: i64,
}
impl ::std::default::Default for ChildLimits {
    fn default() -> Self {
        Self {
            max_depth: defaults::default_u64::<i64, 4>(),
            max_descendants: defaults::default_u64::<i64, 256>(),
            max_direct_children: defaults::default_u64::<i64, 32>(),
        }
    }
}
impl ChildLimits {
    pub fn builder() -> builder::ChildLimits {
        Default::default()
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "Immutable pointer to the exact bounded parent model projection inherited at child admission. It never embeds parent prompt bytes."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Immutable pointer to the exact bounded parent model projection inherited at child admission. It never embeds parent prompt bytes.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"mode\","]
#[doc = "    \"resolved_turns\","]
#[doc = "    \"source_context_generation\","]
#[doc = "    \"source_projection_digest\","]
#[doc = "    \"source_session_id\","]
#[doc = "    \"source_through_sequence\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"last_n\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 4294967295.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"mode\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"all\","]
#[doc = "        \"none\","]
#[doc = "        \"last_n\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"resolved_turns\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"source_context_generation\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"source_projection_digest\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[a-f0-9]{64}$\""]
#[doc = "    },"]
#[doc = "    \"source_session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"source_through_sequence\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ContextFork {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub last_n: ::std::option::Option<::std::num::NonZeroU64>,
    pub mode: ContextForkMode,
    pub resolved_turns: u64,
    pub source_context_generation: u64,
    pub source_projection_digest: ContextForkSourceProjectionDigest,
    pub source_session_id: SessionId,
    pub source_through_sequence: u64,
}
impl ContextFork {
    pub fn builder() -> builder::ContextFork {
        Default::default()
    }
}
#[doc = "`ContextForkMode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"all\","]
#[doc = "    \"none\","]
#[doc = "    \"last_n\""]
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
pub enum ContextForkMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "last_n")]
    LastN,
}
impl ::std::fmt::Display for ContextForkMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::All => f.write_str("all"),
            Self::None => f.write_str("none"),
            Self::LastN => f.write_str("last_n"),
        }
    }
}
impl ::std::str::FromStr for ContextForkMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "all" => Ok(Self::All),
            "none" => Ok(Self::None),
            "last_n" => Ok(Self::LastN),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContextForkMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContextForkMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContextForkMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ContextForkSourceProjectionDigest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[a-f0-9]{64}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ContextForkSourceProjectionDigest(::std::string::String);
impl ::std::ops::Deref for ContextForkSourceProjectionDigest {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ContextForkSourceProjectionDigest> for ::std::string::String {
    fn from(value: ContextForkSourceProjectionDigest) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ContextForkSourceProjectionDigest {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[a-f0-9]{64}$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[a-f0-9]{64}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ContextForkSourceProjectionDigest {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContextForkSourceProjectionDigest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContextForkSourceProjectionDigest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ContextForkSourceProjectionDigest {
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
#[doc = "    \"children\": {"]
#[doc = "      \"$ref\": \"#/$defs/ChildLimits\""]
#[doc = "    },"]
#[doc = "    \"client\": {"]
#[doc = "      \"$ref\": \"#/$defs/CustomerClientConfig\""]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 16,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 1024"]
#[doc = "      },"]
#[doc = "      \"propertyNames\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 64,"]
#[doc = "        \"minLength\": 1"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelConfig\""]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"$ref\": \"#/$defs/NetworkPolicy\""]
#[doc = "    },"]
#[doc = "    \"provider_recovery_retries\": {"]
#[doc = "      \"default\": 1,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 8.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"secrets\": {"]
#[doc = "      \"description\": \"Write-only values for required managed Tool environment names; encrypted in custody.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 128,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 2048"]
#[doc = "      },"]
#[doc = "      \"propertyNames\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"pattern\": \"^[A-Za-z_][A-Za-z0-9_]{0,127}$\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"system_prompt\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 131072"]
#[doc = "    },"]
#[doc = "    \"tool_bundles\": {"]
#[doc = "      \"description\": \"Bounded bundle payloads referenced by tools.items. Never part of the model prefix or journal.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolBundle\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 128"]
#[doc = "    },"]
#[doc = "    \"tools\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolsConfig\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub children: ::std::option::Option<ChildLimits>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub client: ::std::option::Option<CustomerClientConfig>,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<
        CreateSessionRequestMetadataKey,
        CreateSessionRequestMetadataValue,
    >,
    pub model: ModelConfig,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub network: ::std::option::Option<NetworkPolicy>,
    #[serde(default = "defaults::default_u64::<i64, 1>")]
    pub provider_recovery_retries: i64,
    #[doc = "Write-only values for required managed Tool environment names; encrypted in custody."]
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub secrets: ::std::collections::HashMap<
        CreateSessionRequestSecretsKey,
        CreateSessionRequestSecretsValue,
    >,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub system_prompt: ::std::option::Option<CreateSessionRequestSystemPrompt>,
    #[doc = "Bounded bundle payloads referenced by tools.items. Never part of the model prefix or journal."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub tool_bundles: ::std::vec::Vec<ToolBundle>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tools: ::std::option::Option<ToolsConfig>,
}
impl CreateSessionRequest {
    pub fn builder() -> builder::CreateSessionRequest {
        Default::default()
    }
}
#[doc = "`CreateSessionRequestMetadataKey`"]
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
pub struct CreateSessionRequestMetadataKey(::std::string::String);
impl ::std::ops::Deref for CreateSessionRequestMetadataKey {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CreateSessionRequestMetadataKey> for ::std::string::String {
    fn from(value: CreateSessionRequestMetadataKey) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CreateSessionRequestMetadataKey {
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
impl ::std::convert::TryFrom<&str> for CreateSessionRequestMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CreateSessionRequestMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CreateSessionRequestMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CreateSessionRequestMetadataKey {
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
#[doc = "`CreateSessionRequestMetadataValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 1024"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CreateSessionRequestMetadataValue(::std::string::String);
impl ::std::ops::Deref for CreateSessionRequestMetadataValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CreateSessionRequestMetadataValue> for ::std::string::String {
    fn from(value: CreateSessionRequestMetadataValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CreateSessionRequestMetadataValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 1024usize {
            return Err("longer than 1024 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CreateSessionRequestMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CreateSessionRequestMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CreateSessionRequestMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CreateSessionRequestMetadataValue {
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
#[doc = "`CreateSessionRequestSecretsKey`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z_][A-Za-z0-9_]{0,127}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CreateSessionRequestSecretsKey(::std::string::String);
impl ::std::ops::Deref for CreateSessionRequestSecretsKey {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CreateSessionRequestSecretsKey> for ::std::string::String {
    fn from(value: CreateSessionRequestSecretsKey) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CreateSessionRequestSecretsKey {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z_][A-Za-z0-9_]{0,127}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z_][A-Za-z0-9_]{0,127}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CreateSessionRequestSecretsKey {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CreateSessionRequestSecretsKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CreateSessionRequestSecretsKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CreateSessionRequestSecretsKey {
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
#[doc = "`CreateSessionRequestSecretsValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 2048"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CreateSessionRequestSecretsValue(::std::string::String);
impl ::std::ops::Deref for CreateSessionRequestSecretsValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CreateSessionRequestSecretsValue> for ::std::string::String {
    fn from(value: CreateSessionRequestSecretsValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CreateSessionRequestSecretsValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 2048usize {
            return Err("longer than 2048 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CreateSessionRequestSecretsValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CreateSessionRequestSecretsValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CreateSessionRequestSecretsValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CreateSessionRequestSecretsValue {
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
#[doc = "`CreateSessionRequestSystemPrompt`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 131072"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CreateSessionRequestSystemPrompt(::std::string::String);
impl ::std::ops::Deref for CreateSessionRequestSystemPrompt {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CreateSessionRequestSystemPrompt> for ::std::string::String {
    fn from(value: CreateSessionRequestSystemPrompt) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CreateSessionRequestSystemPrompt {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 131072usize {
            return Err("longer than 131072 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CreateSessionRequestSystemPrompt {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CreateSessionRequestSystemPrompt {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CreateSessionRequestSystemPrompt {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CreateSessionRequestSystemPrompt {
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
#[doc = "`CustomerAppToolExecutor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"kind\","]
#[doc = "    \"registration\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"customer_app\""]
#[doc = "    },"]
#[doc = "    \"registration\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[A-Za-z0-9_.:-]{1,128}$\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CustomerAppToolExecutor {
    pub kind: ::std::string::String,
    pub registration: CustomerAppToolExecutorRegistration,
}
impl CustomerAppToolExecutor {
    pub fn builder() -> builder::CustomerAppToolExecutor {
        Default::default()
    }
}
#[doc = "`CustomerAppToolExecutorRegistration`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z0-9_.:-]{1,128}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CustomerAppToolExecutorRegistration(::std::string::String);
impl ::std::ops::Deref for CustomerAppToolExecutorRegistration {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CustomerAppToolExecutorRegistration> for ::std::string::String {
    fn from(value: CustomerAppToolExecutorRegistration) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CustomerAppToolExecutorRegistration {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z0-9_.:-]{1,128}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z0-9_.:-]{1,128}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CustomerAppToolExecutorRegistration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CustomerAppToolExecutorRegistration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CustomerAppToolExecutorRegistration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CustomerAppToolExecutorRegistration {
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
#[doc = "`CustomerClientConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"id\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^[A-Za-z0-9_.:-]{1,128}$\""]
#[doc = "    },"]
#[doc = "    \"submit_retries\": {"]
#[doc = "      \"default\": 1,"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 8.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CustomerClientConfig {
    pub id: CustomerClientConfigId,
    #[serde(default = "defaults::default_u64::<i64, 1>")]
    pub submit_retries: i64,
}
impl CustomerClientConfig {
    pub fn builder() -> builder::CustomerClientConfig {
        Default::default()
    }
}
#[doc = "`CustomerClientConfigId`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z0-9_.:-]{1,128}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CustomerClientConfigId(::std::string::String);
impl ::std::ops::Deref for CustomerClientConfigId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CustomerClientConfigId> for ::std::string::String {
    fn from(value: CustomerClientConfigId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CustomerClientConfigId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z0-9_.:-]{1,128}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z0-9_.:-]{1,128}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CustomerClientConfigId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CustomerClientConfigId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CustomerClientConfigId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CustomerClientConfigId {
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
#[doc = "`EngineToolExecutor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"capability\","]
#[doc = "    \"kind\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"capability\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"pattern\": \"^(brain|aex)\\\\.[A-Za-z0-9_.:-]{1,120}$\""]
#[doc = "    },"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"engine\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct EngineToolExecutor {
    pub capability: EngineToolExecutorCapability,
    pub kind: ::std::string::String,
}
impl EngineToolExecutor {
    pub fn builder() -> builder::EngineToolExecutor {
        Default::default()
    }
}
#[doc = "`EngineToolExecutorCapability`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^(brain|aex)\\\\.[A-Za-z0-9_.:-]{1,120}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct EngineToolExecutorCapability(::std::string::String);
impl ::std::ops::Deref for EngineToolExecutorCapability {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EngineToolExecutorCapability> for ::std::string::String {
    fn from(value: EngineToolExecutorCapability) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EngineToolExecutorCapability {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^(brain|aex)\\.[A-Za-z0-9_.:-]{1,120}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^(brain|aex)\\.[A-Za-z0-9_.:-]{1,120}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for EngineToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EngineToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EngineToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EngineToolExecutorCapability {
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
#[doc = "        \"agent_id\","]
#[doc = "        \"at\","]
#[doc = "        \"attempt_id\","]
#[doc = "        \"provisional\","]
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
#[doc = "        \"attempt_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/ModelAttemptId\""]
#[doc = "        },"]
#[doc = "        \"provisional\": {"]
#[doc = "          \"description\": \"Deltas are provisional until the matching assistant.message wins.\","]
#[doc = "          \"type\": \"boolean\","]
#[doc = "          \"const\": true"]
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
#[doc = "        \"attempt_id\","]
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
#[doc = "        \"attempt_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/ModelAttemptId\""]
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
#[doc = "        \"session_id\","]
#[doc = "        \"through_seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"through_seq\": {"]
#[doc = "          \"description\": \"Strong durable HEAD high-water captured after subscription and reached by every replay page before this proof was emitted. This control event has no SSE id and is never journaled.\","]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"replay.complete\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"logical_operation_id\","]
#[doc = "        \"reason\","]
#[doc = "        \"replacement_attempt_id\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"superseded_attempt_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"logical_operation_id\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 64,"]
#[doc = "          \"minLength\": 1"]
#[doc = "        },"]
#[doc = "        \"reason\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"unknown\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"replacement_attempt_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/ModelAttemptId\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"session_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionId\""]
#[doc = "        },"]
#[doc = "        \"superseded_attempt_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/ModelAttemptId\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"model.attempt_superseded\""]
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
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"storage\","]
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
#[doc = "        \"storage\": {"]
#[doc = "          \"$ref\": \"#/$defs/StorageInfo\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"storage.usage\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"state\","]
#[doc = "        \"turn_state\","]
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
#[doc = "        \"state\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionState\""]
#[doc = "        },"]
#[doc = "        \"turn_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/TurnId\""]
#[doc = "        },"]
#[doc = "        \"turn_phase\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 64,"]
#[doc = "          \"minLength\": 1"]
#[doc = "        },"]
#[doc = "        \"turn_state\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionTurnState\""]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Event {
    #[serde(rename = "turn.started")]
    TurnStarted {
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        turn_id: TurnId,
    },
    #[serde(rename = "assistant.delta")]
    AssistantDelta {
        agent_id: AgentId,
        at: Timestamp,
        attempt_id: ModelAttemptId,
        #[doc = "Deltas are provisional until the matching assistant.message wins."]
        provisional: bool,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        text: ::std::string::String,
        turn_id: TurnId,
    },
    #[serde(rename = "assistant.message")]
    AssistantMessage {
        agent_id: AgentId,
        at: Timestamp,
        attempt_id: ModelAttemptId,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        #[doc = "The complete assistant text of one model round."]
        text: ::std::string::String,
        turn_id: TurnId,
    },
    #[serde(rename = "replay.complete")]
    ReplayComplete {
        session_id: SessionId,
        #[doc = "Strong durable HEAD high-water captured after subscription and reached by every replay page before this proof was emitted. This control event has no SSE id and is never journaled."]
        through_seq: u64,
    },
    #[serde(rename = "model.attempt_superseded")]
    ModelAttemptSuperseded {
        at: Timestamp,
        logical_operation_id: EventLogicalOperationId,
        reason: EventReason,
        replacement_attempt_id: ModelAttemptId,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        superseded_attempt_id: ModelAttemptId,
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
    #[serde(rename = "storage.usage")]
    StorageUsage {
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        storage: StorageInfo,
    },
    #[serde(rename = "session.updated")]
    SessionUpdated {
        at: Timestamp,
        seq: ::std::num::NonZeroU64,
        session_id: SessionId,
        state: SessionState,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_id: ::std::option::Option<TurnId>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        turn_phase: ::std::option::Option<EventTurnPhase>,
        turn_state: SessionTurnState,
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
#[doc = "`EventLogicalOperationId`"]
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
pub struct EventLogicalOperationId(::std::string::String);
impl ::std::ops::Deref for EventLogicalOperationId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EventLogicalOperationId> for ::std::string::String {
    fn from(value: EventLogicalOperationId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EventLogicalOperationId {
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
impl ::std::convert::TryFrom<&str> for EventLogicalOperationId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventLogicalOperationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventLogicalOperationId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EventLogicalOperationId {
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
#[doc = "`EventReason`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"unknown\""]
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
pub enum EventReason {
    #[serde(rename = "unknown")]
    Unknown,
}
impl ::std::fmt::Display for EventReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Unknown => f.write_str("unknown"),
        }
    }
}
impl ::std::str::FromStr for EventReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "unknown" => Ok(Self::Unknown),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EventReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventReason {
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
#[doc = "`EventTurnPhase`"]
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
pub struct EventTurnPhase(::std::string::String);
impl ::std::ops::Deref for EventTurnPhase {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EventTurnPhase> for ::std::string::String {
    fn from(value: EventTurnPhase) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EventTurnPhase {
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
impl ::std::convert::TryFrom<&str> for EventTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EventTurnPhase {
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "Generic trusted server-executor result. A successful response carries its structured Tool output in result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Generic trusted server-executor result. A successful response carries its structured Tool output in result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent.\","]
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
#[doc = "      \"description\": \"Structured successful Tool output. Required when outcome is completed and is_error is false; also attached to turn.completed for complete_turn.\""]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
    #[doc = "Structured successful Tool output. Required when outcome is completed and is_error is false; also attached to turn.completed for complete_turn."]
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
#[doc = "Which agents may call a trusted server capability."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Which agents may call a trusted server capability.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"root\","]
#[doc = "    \"all\""]
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
    #[serde(rename = "all")]
    All,
}
impl ::std::fmt::Display for ExternalToolScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Root => f.write_str("root"),
            Self::All => f.write_str("all"),
        }
    }
}
impl ::std::str::FromStr for ExternalToolScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "root" => Ok(Self::Root),
            "all" => Ok(Self::All),
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
pub struct MessageAccepted {
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
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub content: MessageRequestContent,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "Brain-minted identity of one provisional provider attempt."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted identity of one provisional provider attempt.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^att_[A-Za-z0-9]{20,32}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ModelAttemptId(::std::string::String);
impl ::std::ops::Deref for ModelAttemptId {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelAttemptId> for ::std::string::String {
    fn from(value: ModelAttemptId) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelAttemptId {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^att_[A-Za-z0-9]{20,32}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^att_[A-Za-z0-9]{20,32}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelAttemptId {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelAttemptId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelAttemptId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelAttemptId {
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
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"base_url\": {"]
#[doc = "      \"description\": \"Override the provider endpoint (required for openai_compatible).\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\","]
#[doc = "      \"maxLength\": 2048"]
#[doc = "    },"]
#[doc = "    \"context_window_tokens\": {"]
#[doc = "      \"description\": \"Immutable model context window. Omission seals the conservative neutral default of 32768 tokens; custom model names are never guessed from a mutable catalog.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 2000000.0,"]
#[doc = "      \"minimum\": 8192.0"]
#[doc = "    },"]
#[doc = "    \"max_output_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 131072.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Provider model id, e.g. \\\"claude-sonnet-5\\\" or \\\"gpt-5\\\".\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 128,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"provider\": {"]
#[doc = "      \"$ref\": \"#/$defs/Provider\""]
#[doc = "    },"]
#[doc = "    \"reasoning_effort\": {"]
#[doc = "      \"description\": \"Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it.\","]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[doc = "BYOK. Encrypted per session, never returned, never logged."]
    pub api_key: ModelConfigApiKey,
    #[doc = "Override the provider endpoint (required for openai_compatible)."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub base_url: ::std::option::Option<ModelConfigBaseUrl>,
    #[doc = "Immutable model context window. Omission seals the conservative neutral default of 32768 tokens; custom model names are never guessed from a mutable catalog."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub context_window_tokens: ::std::option::Option<i64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_output_tokens: ::std::option::Option<::std::num::NonZeroU64>,
    #[doc = "Provider model id, e.g. \"claude-sonnet-5\" or \"gpt-5\"."]
    pub name: ModelConfigName,
    pub provider: Provider,
    #[doc = "Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it."]
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
#[doc = "  \"maxLength\": 4096,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
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
#[doc = "Override the provider endpoint (required for openai_compatible)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Override the provider endpoint (required for openai_compatible).\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"format\": \"uri\","]
#[doc = "  \"maxLength\": 2048"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ModelConfigBaseUrl(::std::string::String);
impl ::std::ops::Deref for ModelConfigBaseUrl {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelConfigBaseUrl> for ::std::string::String {
    fn from(value: ModelConfigBaseUrl) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelConfigBaseUrl {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 2048usize {
            return Err("longer than 2048 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelConfigBaseUrl {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelConfigBaseUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelConfigBaseUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelConfigBaseUrl {
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
#[doc = "  \"maxLength\": 128,"]
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
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
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
#[doc = "Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it.\","]
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
#[doc = "    \"context_window_tokens\","]
#[doc = "    \"name\","]
#[doc = "    \"provider\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"base_url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"format\": \"uri\""]
#[doc = "    },"]
#[doc = "    \"context_window_tokens\": {"]
#[doc = "      \"description\": \"Effective immutable context window used for request admission and semantic compaction.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 2000000.0,"]
#[doc = "      \"minimum\": 8192.0"]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
pub struct ModelInfo {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub base_url: ::std::option::Option<::std::string::String>,
    #[doc = "Effective immutable context window used for request admission and semantic compaction."]
    pub context_window_tokens: i64,
    pub name: ::std::string::String,
    pub provider: Provider,
}
impl ModelInfo {
    pub fn builder() -> builder::ModelInfo {
        Default::default()
    }
}
#[doc = "Immutable direct outbound ceiling. Omission means none."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Immutable direct outbound ceiling. Omission means none.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"outbound\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"outbound\": {"]
#[doc = "          \"const\": \"none\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"outbound\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"outbound\": {"]
#[doc = "          \"const\": \"public\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"destinations\","]
#[doc = "        \"outbound\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"destinations\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"oneOf\": ["]
#[doc = "              {"]
#[doc = "                \"type\": \"object\","]
#[doc = "                \"required\": ["]
#[doc = "                  \"host\","]
#[doc = "                  \"ports\","]
#[doc = "                  \"protocol\""]
#[doc = "                ],"]
#[doc = "                \"properties\": {"]
#[doc = "                  \"host\": {"]
#[doc = "                    \"type\": \"string\","]
#[doc = "                    \"maxLength\": 253,"]
#[doc = "                    \"minLength\": 1"]
#[doc = "                  },"]
#[doc = "                  \"ports\": {"]
#[doc = "                    \"type\": \"array\","]
#[doc = "                    \"maxItems\": 1,"]
#[doc = "                    \"minItems\": 1,"]
#[doc = "                    \"prefixItems\": ["]
#[doc = "                      {"]
#[doc = "                        \"const\": 443,"]
#[doc = "                        \"type\": \"integer\""]
#[doc = "                      }"]
#[doc = "                    ]"]
#[doc = "                  },"]
#[doc = "                  \"protocol\": {"]
#[doc = "                    \"const\": \"tls\""]
#[doc = "                  }"]
#[doc = "                },"]
#[doc = "                \"additionalProperties\": false"]
#[doc = "              },"]
#[doc = "              {"]
#[doc = "                \"type\": \"object\","]
#[doc = "                \"required\": ["]
#[doc = "                  \"cidr\","]
#[doc = "                  \"ports\","]
#[doc = "                  \"protocol\""]
#[doc = "                ],"]
#[doc = "                \"properties\": {"]
#[doc = "                  \"cidr\": {"]
#[doc = "                    \"type\": \"string\","]
#[doc = "                    \"maxLength\": 49,"]
#[doc = "                    \"minLength\": 3"]
#[doc = "                  },"]
#[doc = "                  \"ports\": {"]
#[doc = "                    \"type\": \"array\","]
#[doc = "                    \"items\": {"]
#[doc = "                      \"type\": \"integer\","]
#[doc = "                      \"maximum\": 65535.0,"]
#[doc = "                      \"minimum\": 1.0"]
#[doc = "                    },"]
#[doc = "                    \"maxItems\": 32,"]
#[doc = "                    \"minItems\": 1,"]
#[doc = "                    \"uniqueItems\": true"]
#[doc = "                  },"]
#[doc = "                  \"protocol\": {"]
#[doc = "                    \"const\": \"tcp\""]
#[doc = "                  }"]
#[doc = "                },"]
#[doc = "                \"additionalProperties\": false"]
#[doc = "              }"]
#[doc = "            ]"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 128,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        },"]
#[doc = "        \"outbound\": {"]
#[doc = "          \"const\": \"allowlist\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(tag = "outbound", content = "destinations")]
pub enum NetworkPolicy {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "allowlist")]
    Allowlist(::std::vec::Vec<NetworkPolicyDestinationsItem>),
}
impl ::std::convert::From<::std::vec::Vec<NetworkPolicyDestinationsItem>> for NetworkPolicy {
    fn from(value: ::std::vec::Vec<NetworkPolicyDestinationsItem>) -> Self {
        Self::Allowlist(value)
    }
}
#[doc = "`NetworkPolicyDestinationsItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"host\","]
#[doc = "        \"ports\","]
#[doc = "        \"protocol\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"host\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 253,"]
#[doc = "          \"minLength\": 1"]
#[doc = "        },"]
#[doc = "        \"ports\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"maxItems\": 1,"]
#[doc = "          \"minItems\": 1,"]
#[doc = "          \"prefixItems\": ["]
#[doc = "            {"]
#[doc = "              \"const\": 443,"]
#[doc = "              \"type\": \"integer\""]
#[doc = "            }"]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"protocol\": {"]
#[doc = "          \"const\": \"tls\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"cidr\","]
#[doc = "        \"ports\","]
#[doc = "        \"protocol\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"cidr\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 49,"]
#[doc = "          \"minLength\": 3"]
#[doc = "        },"]
#[doc = "        \"ports\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"integer\","]
#[doc = "            \"maximum\": 65535.0,"]
#[doc = "            \"minimum\": 1.0"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 32,"]
#[doc = "          \"minItems\": 1,"]
#[doc = "          \"uniqueItems\": true"]
#[doc = "        },"]
#[doc = "        \"protocol\": {"]
#[doc = "          \"const\": \"tcp\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(tag = "protocol", deny_unknown_fields)]
pub enum NetworkPolicyDestinationsItem {
    #[serde(rename = "tls")]
    Tls {
        host: NetworkPolicyDestinationsItemHost,
        ports: [::serde_json::Value; 1usize],
    },
    #[serde(rename = "tcp")]
    Tcp {
        cidr: NetworkPolicyDestinationsItemCidr,
        ports: Vec<::std::num::NonZeroU64>,
    },
}
#[doc = "`NetworkPolicyDestinationsItemCidr`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 49,"]
#[doc = "  \"minLength\": 3"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct NetworkPolicyDestinationsItemCidr(::std::string::String);
impl ::std::ops::Deref for NetworkPolicyDestinationsItemCidr {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkPolicyDestinationsItemCidr> for ::std::string::String {
    fn from(value: NetworkPolicyDestinationsItemCidr) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkPolicyDestinationsItemCidr {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 49usize {
            return Err("longer than 49 characters".into());
        }
        if value.chars().count() < 3usize {
            return Err("shorter than 3 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for NetworkPolicyDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkPolicyDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkPolicyDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkPolicyDestinationsItemCidr {
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
#[doc = "`NetworkPolicyDestinationsItemHost`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 253,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct NetworkPolicyDestinationsItemHost(::std::string::String);
impl ::std::ops::Deref for NetworkPolicyDestinationsItemHost {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkPolicyDestinationsItemHost> for ::std::string::String {
    fn from(value: NetworkPolicyDestinationsItemHost) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkPolicyDestinationsItemHost {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 253usize {
            return Err("longer than 253 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for NetworkPolicyDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkPolicyDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkPolicyDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkPolicyDestinationsItemHost {
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "    \"depth\","]
#[doc = "    \"id\","]
#[doc = "    \"last_seq\","]
#[doc = "    \"metadata\","]
#[doc = "    \"model\","]
#[doc = "    \"object\","]
#[doc = "    \"root_id\","]
#[doc = "    \"shape\","]
#[doc = "    \"state\","]
#[doc = "    \"storage\","]
#[doc = "    \"turn_state\","]
#[doc = "    \"turns\","]
#[doc = "    \"updated_at\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"context_fork\": {"]
#[doc = "      \"$ref\": \"#/$defs/ContextFork\""]
#[doc = "    },"]
#[doc = "    \"created_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"current_turn\": {"]
#[doc = "      \"$ref\": \"#/$defs/TurnId\""]
#[doc = "    },"]
#[doc = "    \"depth\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 8.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"failure\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionFailure\""]
#[doc = "    },"]
#[doc = "    \"id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"last_message_at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"last_seq\": {"]
#[doc = "      \"description\": \"Authoritative durable journal high-water mark used for tenant discovery and delta folding.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"description\": \"Customer key/value; up to 16 pairs.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 16,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 1024"]
#[doc = "      },"]
#[doc = "      \"propertyNames\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 64,"]
#[doc = "        \"minLength\": 1"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelInfo\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Optional customer-visible task name for a child session.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 128,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"session\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"parent_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    },"]
#[doc = "    \"shape\": {"]
#[doc = "      \"description\": \"Authoritative immutable execution shape inherited by every child. The hosted alpha supports only 1gb.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"1gb\""]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionState\""]
#[doc = "    },"]
#[doc = "    \"storage\": {"]
#[doc = "      \"$ref\": \"#/$defs/StorageInfo\""]
#[doc = "    },"]
#[doc = "    \"turn_phase\": {"]
#[doc = "      \"description\": \"Stable recovery/dispatch phase when a turn is running. Absent while idle.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 64,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"turn_state\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionTurnState\""]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
pub struct Session {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub context_fork: ::std::option::Option<ContextFork>,
    pub created_at: Timestamp,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub current_turn: ::std::option::Option<TurnId>,
    pub depth: i64,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub failure: ::std::option::Option<SessionFailure>,
    pub id: SessionId,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub last_message_at: ::std::option::Option<Timestamp>,
    #[doc = "Authoritative durable journal high-water mark used for tenant discovery and delta folding."]
    pub last_seq: u64,
    #[doc = "Customer key/value; up to 16 pairs."]
    pub metadata: ::std::collections::HashMap<SessionMetadataKey, SessionMetadataValue>,
    pub model: ModelInfo,
    #[doc = "Optional customer-visible task name for a child session."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub name: ::std::option::Option<SessionName>,
    pub object: SessionObject,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub parent_id: ::std::option::Option<SessionId>,
    pub root_id: SessionId,
    #[doc = "Authoritative immutable execution shape inherited by every child. The hosted alpha supports only 1gb."]
    pub shape: ::std::string::String,
    pub state: SessionState,
    pub storage: StorageInfo,
    #[doc = "Stable recovery/dispatch phase when a turn is running. Absent while idle."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub turn_phase: ::std::option::Option<SessionTurnPhase>,
    pub turn_state: SessionTurnState,
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
#[doc = "        \"binding_conflict\","]
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "    \"binding_conflict\","]
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
    #[serde(rename = "binding_conflict")]
    BindingConflict,
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
            Self::BindingConflict => f.write_str("binding_conflict"),
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
            "binding_conflict" => Ok(Self::BindingConflict),
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "`SessionMetadataKey`"]
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
pub struct SessionMetadataKey(::std::string::String);
impl ::std::ops::Deref for SessionMetadataKey {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SessionMetadataKey> for ::std::string::String {
    fn from(value: SessionMetadataKey) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SessionMetadataKey {
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
impl ::std::convert::TryFrom<&str> for SessionMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionMetadataKey {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SessionMetadataKey {
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
#[doc = "`SessionMetadataValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 1024"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SessionMetadataValue(::std::string::String);
impl ::std::ops::Deref for SessionMetadataValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SessionMetadataValue> for ::std::string::String {
    fn from(value: SessionMetadataValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SessionMetadataValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 1024usize {
            return Err("longer than 1024 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SessionMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionMetadataValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SessionMetadataValue {
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
#[doc = "Optional customer-visible task name for a child session."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Optional customer-visible task name for a child session.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SessionName(::std::string::String);
impl ::std::ops::Deref for SessionName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SessionName> for ::std::string::String {
    fn from(value: SessionName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SessionName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SessionName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SessionName {
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
#[doc = "Lifecycle only. Whether a turn is running is reported separately as turn_state."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Lifecycle only. Whether a turn is running is reported separately as turn_state.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"open\","]
#[doc = "    \"ending\","]
#[doc = "    \"ended\","]
#[doc = "    \"deleting\","]
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
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "ending")]
    Ending,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "deleting")]
    Deleting,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "failed")]
    Failed,
}
impl ::std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Open => f.write_str("open"),
            Self::Ending => f.write_str("ending"),
            Self::Ended => f.write_str("ended"),
            Self::Deleting => f.write_str("deleting"),
            Self::Deleted => f.write_str("deleted"),
            Self::Failed => f.write_str("failed"),
        }
    }
}
impl ::std::str::FromStr for SessionState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "open" => Ok(Self::Open),
            "ending" => Ok(Self::Ending),
            "ended" => Ok(Self::Ended),
            "deleting" => Ok(Self::Deleting),
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
#[doc = "Stable recovery/dispatch phase when a turn is running. Absent while idle."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Stable recovery/dispatch phase when a turn is running. Absent while idle.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 64,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SessionTurnPhase(::std::string::String);
impl ::std::ops::Deref for SessionTurnPhase {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SessionTurnPhase> for ::std::string::String {
    fn from(value: SessionTurnPhase) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SessionTurnPhase {
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
impl ::std::convert::TryFrom<&str> for SessionTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionTurnPhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SessionTurnPhase {
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
#[doc = "Current-turn activity, independent from session lifecycle."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Current-turn activity, independent from session lifecycle.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"idle\","]
#[doc = "    \"running\""]
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
pub enum SessionTurnState {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
}
impl ::std::fmt::Display for SessionTurnState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Idle => f.write_str("idle"),
            Self::Running => f.write_str("running"),
        }
    }
}
impl ::std::str::FromStr for SessionTurnState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "idle" => Ok(Self::Idle),
            "running" => Ok(Self::Running),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SessionTurnState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SessionTurnState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SessionTurnState {
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
#[doc = "    \"refusal\","]
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
    #[serde(rename = "refusal")]
    Refusal,
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
            Self::Refusal => f.write_str("refusal"),
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
            "refusal" => Ok(Self::Refusal),
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
#[doc = "    \"session_storage_bytes\","]
#[doc = "    \"upload_reserved_bytes\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"session_storage_bytes\": {"]
#[doc = "      \"description\": \"Durable objects scoped to the session.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"upload_reserved_bytes\": {"]
#[doc = "      \"description\": \"Outstanding staged upload bytes held against the sealed session quota until staging cleanup completes. These bytes are not yet published session objects.\","]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
pub struct StorageInfo {
    #[doc = "Durable objects scoped to the session."]
    pub session_storage_bytes: u64,
    #[doc = "Outstanding staged upload bytes held against the sealed session quota until staging cleanup completes. These bytes are not yet published session objects."]
    pub upload_reserved_bytes: u64,
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
#[doc = "Create-time-only bundle bytes. Brain stages these outside the journal, then discards this representation."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Create-time-only bundle bytes. Brain stages these outside the journal, then discards this representation.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"checksum\","]
#[doc = "    \"content_base64\","]
#[doc = "    \"media_type\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 4194304.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"checksum\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"content_base64\": {"]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 5592408,"]
#[doc = "      \"contentEncoding\": \"base64\""]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"application/javascript+esm\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ToolBundle {
    pub bytes: ::std::num::NonZeroU64,
    pub checksum: Sha256Hex,
    pub content_base64: ToolBundleContentBase64,
    pub media_type: ::std::string::String,
}
impl ToolBundle {
    pub fn builder() -> builder::ToolBundle {
        Default::default()
    }
}
#[doc = "`ToolBundleContentBase64`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"writeOnly\": true,"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 5592408,"]
#[doc = "  \"contentEncoding\": \"base64\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolBundleContentBase64(::std::string::String);
impl ::std::ops::Deref for ToolBundleContentBase64 {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolBundleContentBase64> for ::std::string::String {
    fn from(value: ToolBundleContentBase64) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolBundleContentBase64 {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 5592408usize {
            return Err("longer than 5592408 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolBundleContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolBundleContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolBundleContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolBundleContentBase64 {
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
#[doc = "`ToolConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"definition\","]
#[doc = "    \"executor\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"definition\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolDefinition\""]
#[doc = "    },"]
#[doc = "    \"executor\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolExecutor\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolConfig {
    pub definition: ToolDefinition,
    pub executor: ToolExecutor,
}
impl ToolConfig {
    pub fn builder() -> builder::ToolConfig {
        Default::default()
    }
}
#[doc = "The model-visible half of one Tool. Array order is preserved exactly in the immutable model prefix."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The model-visible half of one Tool. Array order is preserved exactly in the immutable model prefix.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"contract_digest\","]
#[doc = "    \"input_schema\","]
#[doc = "    \"name\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"contract_digest\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"description\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"input_schema\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"minProperties\": 1,"]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"$ref\": \"#/$defs/ToolName\""]
#[doc = "    },"]
#[doc = "    \"output_schema\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"minProperties\": 1,"]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub contract_digest: Sha256Hex,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<ToolDefinitionDescription>,
    pub input_schema: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub name: ToolName,
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub output_schema: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ToolDefinition {
    pub fn builder() -> builder::ToolDefinition {
        Default::default()
    }
}
#[doc = "`ToolDefinitionDescription`"]
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
pub struct ToolDefinitionDescription(::std::string::String);
impl ::std::ops::Deref for ToolDefinitionDescription {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolDefinitionDescription> for ::std::string::String {
    fn from(value: ToolDefinitionDescription) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolDefinitionDescription {
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
impl ::std::convert::TryFrom<&str> for ToolDefinitionDescription {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolDefinitionDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolDefinitionDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolDefinitionDescription {
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
#[doc = "`ToolExecutor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"$ref\": \"#/$defs/AexManagedToolExecutor\""]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"$ref\": \"#/$defs/CustomerAppToolExecutor\""]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"$ref\": \"#/$defs/EngineToolExecutor\""]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum ToolExecutor {
    AexManagedToolExecutor(AexManagedToolExecutor),
    CustomerAppToolExecutor(CustomerAppToolExecutor),
    EngineToolExecutor(EngineToolExecutor),
}
impl ::std::convert::From<AexManagedToolExecutor> for ToolExecutor {
    fn from(value: AexManagedToolExecutor) -> Self {
        Self::AexManagedToolExecutor(value)
    }
}
impl ::std::convert::From<CustomerAppToolExecutor> for ToolExecutor {
    fn from(value: CustomerAppToolExecutor) -> Self {
        Self::CustomerAppToolExecutor(value)
    }
}
impl ::std::convert::From<EngineToolExecutor> for ToolExecutor {
    fn from(value: EngineToolExecutor) -> Self {
        Self::EngineToolExecutor(value)
    }
}
#[doc = "`ToolName`"]
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
pub struct ToolName(::std::string::String);
impl ::std::ops::Deref for ToolName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolName> for ::std::string::String {
    fn from(value: ToolName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolName {
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
impl ::std::convert::TryFrom<&str> for ToolName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolName {
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
#[doc = "    \"items\": {"]
#[doc = "      \"description\": \"The exact ordered native Tool grant. Omitted or empty means no native tools.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolConfig\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 128"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[doc = "The exact ordered native Tool grant. Omitted or empty means no native tools."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub items: ::std::vec::Vec<ToolConfig>,
}
impl ::std::default::Default for ToolsConfig {
    fn default() -> Self {
        Self {
            items: Default::default(),
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
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
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
    pub struct AexManagedToolExecutor {
        bundle_digest: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        kind: ::std::result::Result<::std::string::String, ::std::string::String>,
        required_env: ::std::result::Result<
            Vec<super::AexManagedToolExecutorRequiredEnvItem>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for AexManagedToolExecutor {
        fn default() -> Self {
            Self {
                bundle_digest: Err("no value supplied for bundle_digest".to_string()),
                kind: Err("no value supplied for kind".to_string()),
                required_env: Err("no value supplied for required_env".to_string()),
            }
        }
    }
    impl AexManagedToolExecutor {
        pub fn bundle_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.bundle_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundle_digest: {e}"));
            self
        }
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
            self
        }
        pub fn required_env<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::AexManagedToolExecutorRequiredEnvItem>>,
            T::Error: ::std::fmt::Display,
        {
            self.required_env = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for required_env: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<AexManagedToolExecutor> for super::AexManagedToolExecutor {
        type Error = super::error::ConversionError;
        fn try_from(
            value: AexManagedToolExecutor,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bundle_digest: value.bundle_digest?,
                kind: value.kind?,
                required_env: value.required_env?,
            })
        }
    }
    impl ::std::convert::From<super::AexManagedToolExecutor> for AexManagedToolExecutor {
        fn from(value: super::AexManagedToolExecutor) -> Self {
            Self {
                bundle_digest: Ok(value.bundle_digest),
                kind: Ok(value.kind),
                required_env: Ok(value.required_env),
            }
        }
    }
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
    pub struct ChildLimits {
        max_depth: ::std::result::Result<i64, ::std::string::String>,
        max_descendants: ::std::result::Result<i64, ::std::string::String>,
        max_direct_children: ::std::result::Result<i64, ::std::string::String>,
    }
    impl ::std::default::Default for ChildLimits {
        fn default() -> Self {
            Self {
                max_depth: Ok(super::defaults::default_u64::<i64, 4>()),
                max_descendants: Ok(super::defaults::default_u64::<i64, 256>()),
                max_direct_children: Ok(super::defaults::default_u64::<i64, 32>()),
            }
        }
    }
    impl ChildLimits {
        pub fn max_depth<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_depth = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_depth: {e}"));
            self
        }
        pub fn max_descendants<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_descendants = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_descendants: {e}"));
            self
        }
        pub fn max_direct_children<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_direct_children = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_direct_children: {e}")
            });
            self
        }
    }
    impl ::std::convert::TryFrom<ChildLimits> for super::ChildLimits {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ChildLimits,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                max_depth: value.max_depth?,
                max_descendants: value.max_descendants?,
                max_direct_children: value.max_direct_children?,
            })
        }
    }
    impl ::std::convert::From<super::ChildLimits> for ChildLimits {
        fn from(value: super::ChildLimits) -> Self {
            Self {
                max_depth: Ok(value.max_depth),
                max_descendants: Ok(value.max_descendants),
                max_direct_children: Ok(value.max_direct_children),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ContextFork {
        last_n: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
        mode: ::std::result::Result<super::ContextForkMode, ::std::string::String>,
        resolved_turns: ::std::result::Result<u64, ::std::string::String>,
        source_context_generation: ::std::result::Result<u64, ::std::string::String>,
        source_projection_digest:
            ::std::result::Result<super::ContextForkSourceProjectionDigest, ::std::string::String>,
        source_session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        source_through_sequence: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for ContextFork {
        fn default() -> Self {
            Self {
                last_n: Ok(Default::default()),
                mode: Err("no value supplied for mode".to_string()),
                resolved_turns: Err("no value supplied for resolved_turns".to_string()),
                source_context_generation: Err(
                    "no value supplied for source_context_generation".to_string()
                ),
                source_projection_digest: Err(
                    "no value supplied for source_projection_digest".to_string()
                ),
                source_session_id: Err("no value supplied for source_session_id".to_string()),
                source_through_sequence: Err(
                    "no value supplied for source_through_sequence".to_string()
                ),
            }
        }
    }
    impl ContextFork {
        pub fn last_n<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.last_n = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for last_n: {e}"));
            self
        }
        pub fn mode<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ContextForkMode>,
            T::Error: ::std::fmt::Display,
        {
            self.mode = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for mode: {e}"));
            self
        }
        pub fn resolved_turns<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.resolved_turns = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resolved_turns: {e}"));
            self
        }
        pub fn source_context_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.source_context_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for source_context_generation: {e}")
            });
            self
        }
        pub fn source_projection_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ContextForkSourceProjectionDigest>,
            T::Error: ::std::fmt::Display,
        {
            self.source_projection_digest = value.try_into().map_err(|e| {
                format!("error converting supplied value for source_projection_digest: {e}")
            });
            self
        }
        pub fn source_session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionId>,
            T::Error: ::std::fmt::Display,
        {
            self.source_session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for source_session_id: {e}"));
            self
        }
        pub fn source_through_sequence<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.source_through_sequence = value.try_into().map_err(|e| {
                format!("error converting supplied value for source_through_sequence: {e}")
            });
            self
        }
    }
    impl ::std::convert::TryFrom<ContextFork> for super::ContextFork {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ContextFork,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                last_n: value.last_n?,
                mode: value.mode?,
                resolved_turns: value.resolved_turns?,
                source_context_generation: value.source_context_generation?,
                source_projection_digest: value.source_projection_digest?,
                source_session_id: value.source_session_id?,
                source_through_sequence: value.source_through_sequence?,
            })
        }
    }
    impl ::std::convert::From<super::ContextFork> for ContextFork {
        fn from(value: super::ContextFork) -> Self {
            Self {
                last_n: Ok(value.last_n),
                mode: Ok(value.mode),
                resolved_turns: Ok(value.resolved_turns),
                source_context_generation: Ok(value.source_context_generation),
                source_projection_digest: Ok(value.source_projection_digest),
                source_session_id: Ok(value.source_session_id),
                source_through_sequence: Ok(value.source_through_sequence),
            }
        }
    }
    #[derive(Clone)]
    pub struct CreateSessionRequest {
        children:
            ::std::result::Result<::std::option::Option<super::ChildLimits>, ::std::string::String>,
        client: ::std::result::Result<
            ::std::option::Option<super::CustomerClientConfig>,
            ::std::string::String,
        >,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<
                super::CreateSessionRequestMetadataKey,
                super::CreateSessionRequestMetadataValue,
            >,
            ::std::string::String,
        >,
        model: ::std::result::Result<super::ModelConfig, ::std::string::String>,
        network: ::std::result::Result<
            ::std::option::Option<super::NetworkPolicy>,
            ::std::string::String,
        >,
        provider_recovery_retries: ::std::result::Result<i64, ::std::string::String>,
        secrets: ::std::result::Result<
            ::std::collections::HashMap<
                super::CreateSessionRequestSecretsKey,
                super::CreateSessionRequestSecretsValue,
            >,
            ::std::string::String,
        >,
        system_prompt: ::std::result::Result<
            ::std::option::Option<super::CreateSessionRequestSystemPrompt>,
            ::std::string::String,
        >,
        tool_bundles:
            ::std::result::Result<::std::vec::Vec<super::ToolBundle>, ::std::string::String>,
        tools:
            ::std::result::Result<::std::option::Option<super::ToolsConfig>, ::std::string::String>,
    }
    impl ::std::default::Default for CreateSessionRequest {
        fn default() -> Self {
            Self {
                children: Ok(Default::default()),
                client: Ok(Default::default()),
                metadata: Ok(Default::default()),
                model: Err("no value supplied for model".to_string()),
                network: Ok(Default::default()),
                provider_recovery_retries: Ok(super::defaults::default_u64::<i64, 1>()),
                secrets: Ok(Default::default()),
                system_prompt: Ok(Default::default()),
                tool_bundles: Ok(Default::default()),
                tools: Ok(Default::default()),
            }
        }
    }
    impl CreateSessionRequest {
        pub fn children<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ChildLimits>>,
            T::Error: ::std::fmt::Display,
        {
            self.children = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for children: {e}"));
            self
        }
        pub fn client<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::CustomerClientConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.client = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for client: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        super::CreateSessionRequestMetadataKey,
                        super::CreateSessionRequestMetadataValue,
                    >,
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
        pub fn network<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::NetworkPolicy>>,
            T::Error: ::std::fmt::Display,
        {
            self.network = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for network: {e}"));
            self
        }
        pub fn provider_recovery_retries<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.provider_recovery_retries = value.try_into().map_err(|e| {
                format!("error converting supplied value for provider_recovery_retries: {e}")
            });
            self
        }
        pub fn secrets<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        super::CreateSessionRequestSecretsKey,
                        super::CreateSessionRequestSecretsValue,
                    >,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.secrets = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for secrets: {e}"));
            self
        }
        pub fn system_prompt<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::option::Option<super::CreateSessionRequestSystemPrompt>,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.system_prompt = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for system_prompt: {e}"));
            self
        }
        pub fn tool_bundles<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::ToolBundle>>,
            T::Error: ::std::fmt::Display,
        {
            self.tool_bundles = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tool_bundles: {e}"));
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
                children: value.children?,
                client: value.client?,
                metadata: value.metadata?,
                model: value.model?,
                network: value.network?,
                provider_recovery_retries: value.provider_recovery_retries?,
                secrets: value.secrets?,
                system_prompt: value.system_prompt?,
                tool_bundles: value.tool_bundles?,
                tools: value.tools?,
            })
        }
    }
    impl ::std::convert::From<super::CreateSessionRequest> for CreateSessionRequest {
        fn from(value: super::CreateSessionRequest) -> Self {
            Self {
                children: Ok(value.children),
                client: Ok(value.client),
                metadata: Ok(value.metadata),
                model: Ok(value.model),
                network: Ok(value.network),
                provider_recovery_retries: Ok(value.provider_recovery_retries),
                secrets: Ok(value.secrets),
                system_prompt: Ok(value.system_prompt),
                tool_bundles: Ok(value.tool_bundles),
                tools: Ok(value.tools),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CustomerAppToolExecutor {
        kind: ::std::result::Result<::std::string::String, ::std::string::String>,
        registration: ::std::result::Result<
            super::CustomerAppToolExecutorRegistration,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for CustomerAppToolExecutor {
        fn default() -> Self {
            Self {
                kind: Err("no value supplied for kind".to_string()),
                registration: Err("no value supplied for registration".to_string()),
            }
        }
    }
    impl CustomerAppToolExecutor {
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
            self
        }
        pub fn registration<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::CustomerAppToolExecutorRegistration>,
            T::Error: ::std::fmt::Display,
        {
            self.registration = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for registration: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CustomerAppToolExecutor> for super::CustomerAppToolExecutor {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CustomerAppToolExecutor,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                kind: value.kind?,
                registration: value.registration?,
            })
        }
    }
    impl ::std::convert::From<super::CustomerAppToolExecutor> for CustomerAppToolExecutor {
        fn from(value: super::CustomerAppToolExecutor) -> Self {
            Self {
                kind: Ok(value.kind),
                registration: Ok(value.registration),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CustomerClientConfig {
        id: ::std::result::Result<super::CustomerClientConfigId, ::std::string::String>,
        submit_retries: ::std::result::Result<i64, ::std::string::String>,
    }
    impl ::std::default::Default for CustomerClientConfig {
        fn default() -> Self {
            Self {
                id: Err("no value supplied for id".to_string()),
                submit_retries: Ok(super::defaults::default_u64::<i64, 1>()),
            }
        }
    }
    impl CustomerClientConfig {
        pub fn id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::CustomerClientConfigId>,
            T::Error: ::std::fmt::Display,
        {
            self.id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for id: {e}"));
            self
        }
        pub fn submit_retries<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.submit_retries = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for submit_retries: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CustomerClientConfig> for super::CustomerClientConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CustomerClientConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                id: value.id?,
                submit_retries: value.submit_retries?,
            })
        }
    }
    impl ::std::convert::From<super::CustomerClientConfig> for CustomerClientConfig {
        fn from(value: super::CustomerClientConfig) -> Self {
            Self {
                id: Ok(value.id),
                submit_retries: Ok(value.submit_retries),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EngineToolExecutor {
        capability:
            ::std::result::Result<super::EngineToolExecutorCapability, ::std::string::String>,
        kind: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for EngineToolExecutor {
        fn default() -> Self {
            Self {
                capability: Err("no value supplied for capability".to_string()),
                kind: Err("no value supplied for kind".to_string()),
            }
        }
    }
    impl EngineToolExecutor {
        pub fn capability<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::EngineToolExecutorCapability>,
            T::Error: ::std::fmt::Display,
        {
            self.capability = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capability: {e}"));
            self
        }
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<EngineToolExecutor> for super::EngineToolExecutor {
        type Error = super::error::ConversionError;
        fn try_from(
            value: EngineToolExecutor,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                capability: value.capability?,
                kind: value.kind?,
            })
        }
    }
    impl ::std::convert::From<super::EngineToolExecutor> for EngineToolExecutor {
        fn from(value: super::EngineToolExecutor) -> Self {
            Self {
                capability: Ok(value.capability),
                kind: Ok(value.kind),
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
    pub struct MessageAccepted {
        seq: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        session_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        turn_id: ::std::result::Result<super::TurnId, ::std::string::String>,
    }
    impl ::std::default::Default for MessageAccepted {
        fn default() -> Self {
            Self {
                seq: Err("no value supplied for seq".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                turn_id: Err("no value supplied for turn_id".to_string()),
            }
        }
    }
    impl MessageAccepted {
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
                seq: value.seq?,
                session_id: value.session_id?,
                turn_id: value.turn_id?,
            })
        }
    }
    impl ::std::convert::From<super::MessageAccepted> for MessageAccepted {
        fn from(value: super::MessageAccepted) -> Self {
            Self {
                seq: Ok(value.seq),
                session_id: Ok(value.session_id),
                turn_id: Ok(value.turn_id),
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
    }
    impl ::std::default::Default for MessageRequest {
        fn default() -> Self {
            Self {
                content: Err("no value supplied for content".to_string()),
                metadata: Ok(Default::default()),
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
    }
    impl ::std::convert::TryFrom<MessageRequest> for super::MessageRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: MessageRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                content: value.content?,
                metadata: value.metadata?,
            })
        }
    }
    impl ::std::convert::From<super::MessageRequest> for MessageRequest {
        fn from(value: super::MessageRequest) -> Self {
            Self {
                content: Ok(value.content),
                metadata: Ok(value.metadata),
            }
        }
    }
    #[derive(Clone)]
    pub struct ModelConfig {
        api_key: ::std::result::Result<super::ModelConfigApiKey, ::std::string::String>,
        base_url: ::std::result::Result<
            ::std::option::Option<super::ModelConfigBaseUrl>,
            ::std::string::String,
        >,
        context_window_tokens:
            ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
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
                context_window_tokens: Ok(Default::default()),
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
            T: ::std::convert::TryInto<::std::option::Option<super::ModelConfigBaseUrl>>,
            T::Error: ::std::fmt::Display,
        {
            self.base_url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for base_url: {e}"));
            self
        }
        pub fn context_window_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<i64>>,
            T::Error: ::std::fmt::Display,
        {
            self.context_window_tokens = value.try_into().map_err(|e| {
                format!("error converting supplied value for context_window_tokens: {e}")
            });
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
                context_window_tokens: value.context_window_tokens?,
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
                context_window_tokens: Ok(value.context_window_tokens),
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
        context_window_tokens: ::std::result::Result<i64, ::std::string::String>,
        name: ::std::result::Result<::std::string::String, ::std::string::String>,
        provider: ::std::result::Result<super::Provider, ::std::string::String>,
    }
    impl ::std::default::Default for ModelInfo {
        fn default() -> Self {
            Self {
                base_url: Ok(Default::default()),
                context_window_tokens: Err(
                    "no value supplied for context_window_tokens".to_string()
                ),
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
        pub fn context_window_tokens<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.context_window_tokens = value.try_into().map_err(|e| {
                format!("error converting supplied value for context_window_tokens: {e}")
            });
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
                context_window_tokens: value.context_window_tokens?,
                name: value.name?,
                provider: value.provider?,
            })
        }
    }
    impl ::std::convert::From<super::ModelInfo> for ModelInfo {
        fn from(value: super::ModelInfo) -> Self {
            Self {
                base_url: Ok(value.base_url),
                context_window_tokens: Ok(value.context_window_tokens),
                name: Ok(value.name),
                provider: Ok(value.provider),
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
        context_fork:
            ::std::result::Result<::std::option::Option<super::ContextFork>, ::std::string::String>,
        created_at: ::std::result::Result<super::Timestamp, ::std::string::String>,
        current_turn:
            ::std::result::Result<::std::option::Option<super::TurnId>, ::std::string::String>,
        depth: ::std::result::Result<i64, ::std::string::String>,
        failure: ::std::result::Result<
            ::std::option::Option<super::SessionFailure>,
            ::std::string::String,
        >,
        id: ::std::result::Result<super::SessionId, ::std::string::String>,
        last_message_at:
            ::std::result::Result<::std::option::Option<super::Timestamp>, ::std::string::String>,
        last_seq: ::std::result::Result<u64, ::std::string::String>,
        metadata: ::std::result::Result<
            ::std::collections::HashMap<super::SessionMetadataKey, super::SessionMetadataValue>,
            ::std::string::String,
        >,
        model: ::std::result::Result<super::ModelInfo, ::std::string::String>,
        name:
            ::std::result::Result<::std::option::Option<super::SessionName>, ::std::string::String>,
        object: ::std::result::Result<super::SessionObject, ::std::string::String>,
        parent_id:
            ::std::result::Result<::std::option::Option<super::SessionId>, ::std::string::String>,
        root_id: ::std::result::Result<super::SessionId, ::std::string::String>,
        shape: ::std::result::Result<::std::string::String, ::std::string::String>,
        state: ::std::result::Result<super::SessionState, ::std::string::String>,
        storage: ::std::result::Result<super::StorageInfo, ::std::string::String>,
        turn_phase: ::std::result::Result<
            ::std::option::Option<super::SessionTurnPhase>,
            ::std::string::String,
        >,
        turn_state: ::std::result::Result<super::SessionTurnState, ::std::string::String>,
        turns: ::std::result::Result<u64, ::std::string::String>,
        updated_at: ::std::result::Result<super::Timestamp, ::std::string::String>,
    }
    impl ::std::default::Default for Session {
        fn default() -> Self {
            Self {
                context_fork: Ok(Default::default()),
                created_at: Err("no value supplied for created_at".to_string()),
                current_turn: Ok(Default::default()),
                depth: Err("no value supplied for depth".to_string()),
                failure: Ok(Default::default()),
                id: Err("no value supplied for id".to_string()),
                last_message_at: Ok(Default::default()),
                last_seq: Err("no value supplied for last_seq".to_string()),
                metadata: Err("no value supplied for metadata".to_string()),
                model: Err("no value supplied for model".to_string()),
                name: Ok(Default::default()),
                object: Err("no value supplied for object".to_string()),
                parent_id: Ok(Default::default()),
                root_id: Err("no value supplied for root_id".to_string()),
                shape: Err("no value supplied for shape".to_string()),
                state: Err("no value supplied for state".to_string()),
                storage: Err("no value supplied for storage".to_string()),
                turn_phase: Ok(Default::default()),
                turn_state: Err("no value supplied for turn_state".to_string()),
                turns: Err("no value supplied for turns".to_string()),
                updated_at: Err("no value supplied for updated_at".to_string()),
            }
        }
    }
    impl Session {
        pub fn context_fork<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ContextFork>>,
            T::Error: ::std::fmt::Display,
        {
            self.context_fork = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for context_fork: {e}"));
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
        pub fn depth<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<i64>,
            T::Error: ::std::fmt::Display,
        {
            self.depth = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for depth: {e}"));
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
        pub fn last_seq<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.last_seq = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for last_seq: {e}"));
            self
        }
        pub fn metadata<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        super::SessionMetadataKey,
                        super::SessionMetadataValue,
                    >,
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
        pub fn name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SessionName>>,
            T::Error: ::std::fmt::Display,
        {
            self.name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for name: {e}"));
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
        pub fn parent_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SessionId>>,
            T::Error: ::std::fmt::Display,
        {
            self.parent_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for parent_id: {e}"));
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionId>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn shape<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::string::String>,
            T::Error: ::std::fmt::Display,
        {
            self.shape = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for shape: {e}"));
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
        pub fn turn_phase<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SessionTurnPhase>>,
            T::Error: ::std::fmt::Display,
        {
            self.turn_phase = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turn_phase: {e}"));
            self
        }
        pub fn turn_state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SessionTurnState>,
            T::Error: ::std::fmt::Display,
        {
            self.turn_state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turn_state: {e}"));
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
                context_fork: value.context_fork?,
                created_at: value.created_at?,
                current_turn: value.current_turn?,
                depth: value.depth?,
                failure: value.failure?,
                id: value.id?,
                last_message_at: value.last_message_at?,
                last_seq: value.last_seq?,
                metadata: value.metadata?,
                model: value.model?,
                name: value.name?,
                object: value.object?,
                parent_id: value.parent_id?,
                root_id: value.root_id?,
                shape: value.shape?,
                state: value.state?,
                storage: value.storage?,
                turn_phase: value.turn_phase?,
                turn_state: value.turn_state?,
                turns: value.turns?,
                updated_at: value.updated_at?,
            })
        }
    }
    impl ::std::convert::From<super::Session> for Session {
        fn from(value: super::Session) -> Self {
            Self {
                context_fork: Ok(value.context_fork),
                created_at: Ok(value.created_at),
                current_turn: Ok(value.current_turn),
                depth: Ok(value.depth),
                failure: Ok(value.failure),
                id: Ok(value.id),
                last_message_at: Ok(value.last_message_at),
                last_seq: Ok(value.last_seq),
                metadata: Ok(value.metadata),
                model: Ok(value.model),
                name: Ok(value.name),
                object: Ok(value.object),
                parent_id: Ok(value.parent_id),
                root_id: Ok(value.root_id),
                shape: Ok(value.shape),
                state: Ok(value.state),
                storage: Ok(value.storage),
                turn_phase: Ok(value.turn_phase),
                turn_state: Ok(value.turn_state),
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
        session_storage_bytes: ::std::result::Result<u64, ::std::string::String>,
        upload_reserved_bytes: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for StorageInfo {
        fn default() -> Self {
            Self {
                session_storage_bytes: Err(
                    "no value supplied for session_storage_bytes".to_string()
                ),
                upload_reserved_bytes: Err(
                    "no value supplied for upload_reserved_bytes".to_string()
                ),
            }
        }
    }
    impl StorageInfo {
        pub fn session_storage_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.session_storage_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for session_storage_bytes: {e}")
            });
            self
        }
        pub fn upload_reserved_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.upload_reserved_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for upload_reserved_bytes: {e}")
            });
            self
        }
    }
    impl ::std::convert::TryFrom<StorageInfo> for super::StorageInfo {
        type Error = super::error::ConversionError;
        fn try_from(
            value: StorageInfo,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                session_storage_bytes: value.session_storage_bytes?,
                upload_reserved_bytes: value.upload_reserved_bytes?,
            })
        }
    }
    impl ::std::convert::From<super::StorageInfo> for StorageInfo {
        fn from(value: super::StorageInfo) -> Self {
            Self {
                session_storage_bytes: Ok(value.session_storage_bytes),
                upload_reserved_bytes: Ok(value.upload_reserved_bytes),
            }
        }
    }
    #[derive(Clone)]
    pub struct ToolBundle {
        bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        checksum: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        content_base64:
            ::std::result::Result<super::ToolBundleContentBase64, ::std::string::String>,
        media_type: ::std::result::Result<::std::string::String, ::std::string::String>,
    }
    impl ::std::default::Default for ToolBundle {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                checksum: Err("no value supplied for checksum".to_string()),
                content_base64: Err("no value supplied for content_base64".to_string()),
                media_type: Err("no value supplied for media_type".to_string()),
            }
        }
    }
    impl ToolBundle {
        pub fn bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bytes: {e}"));
            self
        }
        pub fn checksum<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.checksum = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for checksum: {e}"));
            self
        }
        pub fn content_base64<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolBundleContentBase64>,
            T::Error: ::std::fmt::Display,
        {
            self.content_base64 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for content_base64: {e}"));
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
    }
    impl ::std::convert::TryFrom<ToolBundle> for super::ToolBundle {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolBundle,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                checksum: value.checksum?,
                content_base64: value.content_base64?,
                media_type: value.media_type?,
            })
        }
    }
    impl ::std::convert::From<super::ToolBundle> for ToolBundle {
        fn from(value: super::ToolBundle) -> Self {
            Self {
                bytes: Ok(value.bytes),
                checksum: Ok(value.checksum),
                content_base64: Ok(value.content_base64),
                media_type: Ok(value.media_type),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolConfig {
        definition: ::std::result::Result<super::ToolDefinition, ::std::string::String>,
        executor: ::std::result::Result<super::ToolExecutor, ::std::string::String>,
    }
    impl ::std::default::Default for ToolConfig {
        fn default() -> Self {
            Self {
                definition: Err("no value supplied for definition".to_string()),
                executor: Err("no value supplied for executor".to_string()),
            }
        }
    }
    impl ToolConfig {
        pub fn definition<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolDefinition>,
            T::Error: ::std::fmt::Display,
        {
            self.definition = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for definition: {e}"));
            self
        }
        pub fn executor<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ToolExecutor>,
            T::Error: ::std::fmt::Display,
        {
            self.executor = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for executor: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ToolConfig> for super::ToolConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                definition: value.definition?,
                executor: value.executor?,
            })
        }
    }
    impl ::std::convert::From<super::ToolConfig> for ToolConfig {
        fn from(value: super::ToolConfig) -> Self {
            Self {
                definition: Ok(value.definition),
                executor: Ok(value.executor),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolDefinition {
        contract_digest: ::std::result::Result<super::Sha256Hex, ::std::string::String>,
        description: ::std::result::Result<
            ::std::option::Option<super::ToolDefinitionDescription>,
            ::std::string::String,
        >,
        input_schema: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        name: ::std::result::Result<super::ToolName, ::std::string::String>,
        output_schema: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for ToolDefinition {
        fn default() -> Self {
            Self {
                contract_digest: Err("no value supplied for contract_digest".to_string()),
                description: Ok(Default::default()),
                input_schema: Err("no value supplied for input_schema".to_string()),
                name: Err("no value supplied for name".to_string()),
                output_schema: Ok(Default::default()),
            }
        }
    }
    impl ToolDefinition {
        pub fn contract_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Sha256Hex>,
            T::Error: ::std::fmt::Display,
        {
            self.contract_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for contract_digest: {e}"));
            self
        }
        pub fn description<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ToolDefinitionDescription>>,
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
            T: ::std::convert::TryInto<super::ToolName>,
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
    }
    impl ::std::convert::TryFrom<ToolDefinition> for super::ToolDefinition {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolDefinition,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                contract_digest: value.contract_digest?,
                description: value.description?,
                input_schema: value.input_schema?,
                name: value.name?,
                output_schema: value.output_schema?,
            })
        }
    }
    impl ::std::convert::From<super::ToolDefinition> for ToolDefinition {
        fn from(value: super::ToolDefinition) -> Self {
            Self {
                contract_digest: Ok(value.contract_digest),
                description: Ok(value.description),
                input_schema: Ok(value.input_schema),
                name: Ok(value.name),
                output_schema: Ok(value.output_schema),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ToolsConfig {
        items: ::std::result::Result<::std::vec::Vec<super::ToolConfig>, ::std::string::String>,
    }
    impl ::std::default::Default for ToolsConfig {
        fn default() -> Self {
            Self {
                items: Ok(Default::default()),
            }
        }
    }
    impl ToolsConfig {
        pub fn items<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::ToolConfig>>,
            T::Error: ::std::fmt::Display,
        {
            self.items = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for items: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ToolsConfig> for super::ToolsConfig {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ToolsConfig,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                items: value.items?,
            })
        }
    }
    impl ::std::convert::From<super::ToolsConfig> for ToolsConfig {
        fn from(value: super::ToolsConfig) -> Self {
            Self {
                items: Ok(value.items),
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
    pub(super) fn default_u64<T, const V: u64>() -> T
    where
        T: ::std::convert::TryFrom<u64>,
        <T as ::std::convert::TryFrom<u64>>::Error: ::std::fmt::Debug,
    {
        T::try_from(V).unwrap()
    }
}
