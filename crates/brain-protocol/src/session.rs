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
#[doc = "One precompiled Agentloop component and immutable JSON configuration. Brain verifies the component digest and canonical world before sealing it; no server-side source compilation exists."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One precompiled Agentloop component and immutable JSON configuration. Brain verifies the component digest and canonical world before sealing it; no server-side source compilation exists.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"component_base64\","]
#[doc = "    \"component_digest\","]
#[doc = "    \"world\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"component_base64\": {"]
#[doc = "      \"description\": \"The precompiled Wasm component, base64 (32 MiB decoded maximum). Create-time-only and staged outside the journal.\","]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 44739244"]
#[doc = "    },"]
#[doc = "    \"component_digest\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"config\": {"]
#[doc = "      \"description\": \"Immutable package configuration passed to every activation.\","]
#[doc = "      \"default\": {},"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"world\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"aex:agentloop/agentloop@1.0.0\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AgentloopConfig {
    #[doc = "The precompiled Wasm component, base64 (32 MiB decoded maximum). Create-time-only and staged outside the journal."]
    pub component_base64: AgentloopConfigComponentBase64,
    pub component_digest: Sha256Hex,
    #[doc = "Immutable package configuration passed to every activation."]
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub config: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub world: ::std::string::String,
}
#[doc = "The precompiled Wasm component, base64 (32 MiB decoded maximum). Create-time-only and staged outside the journal."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The precompiled Wasm component, base64 (32 MiB decoded maximum). Create-time-only and staged outside the journal.\","]
#[doc = "  \"writeOnly\": true,"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 44739244"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct AgentloopConfigComponentBase64(::std::string::String);
impl ::std::ops::Deref for AgentloopConfigComponentBase64 {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<AgentloopConfigComponentBase64> for ::std::string::String {
    fn from(value: AgentloopConfigComponentBase64) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for AgentloopConfigComponentBase64 {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 44739244usize {
            return Err("longer than 44739244 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for AgentloopConfigComponentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AgentloopConfigComponentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AgentloopConfigComponentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for AgentloopConfigComponentBase64 {
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
#[doc = "The sealed agentloop identity of a session."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The sealed agentloop identity of a session.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"component_digest\","]
#[doc = "    \"world\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"component_digest\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"config\": {"]
#[doc = "      \"default\": {},"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"world\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"aex:agentloop/agentloop@1.0.0\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AgentloopInfo {
    pub component_digest: Sha256Hex,
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub config: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub world: ::std::string::String,
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
#[doc = "Brain-minted id of one durable Tool operation. Managed Environments receive the same operation_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain-minted id of one durable Tool operation. Managed Environments receive the same operation_id.\","]
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
#[doc = "    \"agentloop\","]
#[doc = "    \"model\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"agentloop\": {"]
#[doc = "      \"$ref\": \"#/$defs/AgentloopConfig\""]
#[doc = "    },"]
#[doc = "    \"children\": {"]
#[doc = "      \"$ref\": \"#/$defs/ChildLimits\""]
#[doc = "    },"]
#[doc = "    \"client\": {"]
#[doc = "      \"$ref\": \"#/$defs/CustomerClientConfig\""]
#[doc = "    },"]
#[doc = "    \"environments\": {"]
#[doc = "      \"$ref\": \"#/$defs/EnvironmentsConfig\""]
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
#[doc = "    \"tool_artifact_layers\": {"]
#[doc = "      \"description\": \"Create-time-only content-addressed artifact-layer bytes referenced by tool_bundles.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolArtifactLayer\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 256"]
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
    pub agentloop: AgentloopConfig,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub children: ::std::option::Option<ChildLimits>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub client: ::std::option::Option<CustomerClientConfig>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub environments: ::std::option::Option<EnvironmentsConfig>,
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
    #[doc = "Create-time-only content-addressed artifact-layer bytes referenced by tool_bundles."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub tool_artifact_layers: ::std::vec::Vec<ToolArtifactLayer>,
    #[doc = "Bounded bundle payloads referenced by tools.items. Never part of the model prefix or journal."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub tool_bundles: ::std::vec::Vec<ToolBundle>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tools: ::std::option::Option<ToolsConfig>,
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
#[doc = "`EnvironmentConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"configuration\","]
#[doc = "    \"extension\","]
#[doc = "    \"profile\","]
#[doc = "    \"protocol\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"configuration\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"extension\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 256,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"profile\": {"]
#[doc = "      \"$ref\": \"#/$defs/EnvironmentProfile\""]
#[doc = "    },"]
#[doc = "    \"protocol\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"const\": \"environment/v1\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConfig {
    pub configuration: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub extension: EnvironmentConfigExtension,
    pub profile: EnvironmentProfile,
    pub protocol: ::std::string::String,
}
#[doc = "`EnvironmentConfigExtension`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct EnvironmentConfigExtension(::std::string::String);
impl ::std::ops::Deref for EnvironmentConfigExtension {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EnvironmentConfigExtension> for ::std::string::String {
    fn from(value: EnvironmentConfigExtension) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EnvironmentConfigExtension {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentConfigExtension {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentConfigExtension {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentConfigExtension {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EnvironmentConfigExtension {
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
#[doc = "`EnvironmentName`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^[A-Za-z][A-Za-z0-9_-]{0,63}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct EnvironmentName(::std::string::String);
impl ::std::ops::Deref for EnvironmentName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EnvironmentName> for ::std::string::String {
    fn from(value: EnvironmentName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EnvironmentName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z][A-Za-z0-9_-]{0,63}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z][A-Za-z0-9_-]{0,63}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EnvironmentName {
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
#[doc = "`EnvironmentProfile`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"kind\","]
#[doc = "    \"network\","]
#[doc = "    \"recovery\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"computer\","]
#[doc = "        \"callbacks\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"none\","]
#[doc = "        \"allowlist\","]
#[doc = "        \"unrestricted\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"platform\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"linux-amd64\","]
#[doc = "        \"linux-arm64\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"recovery\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"retained\","]
#[doc = "        \"connection\","]
#[doc = "        \"replay_safe\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProfile {
    pub kind: EnvironmentProfileKind,
    pub network: EnvironmentProfileNetwork,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub platform: ::std::option::Option<EnvironmentProfilePlatform>,
    pub recovery: EnvironmentProfileRecovery,
}
#[doc = "`EnvironmentProfileKind`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"computer\","]
#[doc = "    \"callbacks\""]
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
pub enum EnvironmentProfileKind {
    #[serde(rename = "computer")]
    Computer,
    #[serde(rename = "callbacks")]
    Callbacks,
}
impl ::std::fmt::Display for EnvironmentProfileKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Computer => f.write_str("computer"),
            Self::Callbacks => f.write_str("callbacks"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentProfileKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "computer" => Ok(Self::Computer),
            "callbacks" => Ok(Self::Callbacks),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentProfileKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentProfileKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentProfileKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentProfileNetwork`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"none\","]
#[doc = "    \"allowlist\","]
#[doc = "    \"unrestricted\""]
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
pub enum EnvironmentProfileNetwork {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "allowlist")]
    Allowlist,
    #[serde(rename = "unrestricted")]
    Unrestricted,
}
impl ::std::fmt::Display for EnvironmentProfileNetwork {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("none"),
            Self::Allowlist => f.write_str("allowlist"),
            Self::Unrestricted => f.write_str("unrestricted"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentProfileNetwork {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "none" => Ok(Self::None),
            "allowlist" => Ok(Self::Allowlist),
            "unrestricted" => Ok(Self::Unrestricted),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentProfileNetwork {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentProfileNetwork {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentProfileNetwork {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentProfilePlatform`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"linux-amd64\","]
#[doc = "    \"linux-arm64\""]
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
pub enum EnvironmentProfilePlatform {
    #[serde(rename = "linux-amd64")]
    LinuxAmd64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
}
impl ::std::fmt::Display for EnvironmentProfilePlatform {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::LinuxAmd64 => f.write_str("linux-amd64"),
            Self::LinuxArm64 => f.write_str("linux-arm64"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentProfilePlatform {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "linux-amd64" => Ok(Self::LinuxAmd64),
            "linux-arm64" => Ok(Self::LinuxArm64),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentProfilePlatform {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentProfilePlatform {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentProfilePlatform {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentProfileRecovery`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"retained\","]
#[doc = "    \"connection\","]
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
pub enum EnvironmentProfileRecovery {
    #[serde(rename = "retained")]
    Retained,
    #[serde(rename = "connection")]
    Connection,
    #[serde(rename = "replay_safe")]
    ReplaySafe,
}
impl ::std::fmt::Display for EnvironmentProfileRecovery {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Retained => f.write_str("retained"),
            Self::Connection => f.write_str("connection"),
            Self::ReplaySafe => f.write_str("replay_safe"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentProfileRecovery {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "retained" => Ok(Self::Retained),
            "connection" => Ok(Self::Connection),
            "replay_safe" => Ok(Self::ReplaySafe),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentProfileRecovery {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentProfileRecovery {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentProfileRecovery {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentsConfig`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"maxProperties\": 32,"]
#[doc = "  \"additionalProperties\": {"]
#[doc = "    \"$ref\": \"#/$defs/EnvironmentConfig\""]
#[doc = "  },"]
#[doc = "  \"propertyNames\": {"]
#[doc = "    \"$ref\": \"#/$defs/EnvironmentName\""]
#[doc = "  }"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct EnvironmentsConfig(pub ::std::collections::HashMap<EnvironmentName, EnvironmentConfig>);
impl ::std::ops::Deref for EnvironmentsConfig {
    type Target = ::std::collections::HashMap<EnvironmentName, EnvironmentConfig>;
    fn deref(&self) -> &::std::collections::HashMap<EnvironmentName, EnvironmentConfig> {
        &self.0
    }
}
impl ::std::convert::From<EnvironmentsConfig>
    for ::std::collections::HashMap<EnvironmentName, EnvironmentConfig>
{
    fn from(value: EnvironmentsConfig) -> Self {
        value.0
    }
}
impl ::std::convert::From<::std::collections::HashMap<EnvironmentName, EnvironmentConfig>>
    for EnvironmentsConfig
{
    fn from(value: ::std::collections::HashMap<EnvironmentName, EnvironmentConfig>) -> Self {
        Self(value)
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
#[doc = "            \"environment.lost\""]
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
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"data\","]
#[doc = "        \"name\","]
#[doc = "        \"seq\","]
#[doc = "        \"session_id\","]
#[doc = "        \"turn_id\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"data\": {"]
#[doc = "          \"description\": \"The loop-authored event payload, journaled as a loop `event` entry before it is delivered.\","]
#[doc = "          \"type\": \"object\","]
#[doc = "          \"additionalProperties\": true"]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"description\": \"Loop-chosen event name.\","]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 128,"]
#[doc = "          \"minLength\": 1,"]
#[doc = "          \"pattern\": \"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$\""]
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
#[doc = "            \"loop.event\""]
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
    #[serde(rename = "environment.lost")]
    EnvironmentLost {
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
    #[serde(rename = "loop.event")]
    LoopEvent {
        at: Timestamp,
        #[doc = "The loop-authored event payload, journaled as a loop `event` entry before it is delivered."]
        data: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
        #[doc = "Loop-chosen event name."]
        name: EventName,
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
#[doc = "Loop-chosen event name."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Loop-chosen event name.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128,"]
#[doc = "  \"minLength\": 1,"]
#[doc = "  \"pattern\": \"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct EventName(::std::string::String);
impl ::std::ops::Deref for EventName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EventName> for ::std::string::String {
    fn from(value: EventName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EventName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for EventName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EventName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EventName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EventName {
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
#[doc = "`NetworkDestination`"]
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
pub enum NetworkDestination {
    #[serde(rename = "tls")]
    Tls {
        host: NetworkDestinationHost,
        ports: [::serde_json::Value; 1usize],
    },
    #[serde(rename = "tcp")]
    Tcp {
        cidr: NetworkDestinationCidr,
        ports: Vec<::std::num::NonZeroU64>,
    },
}
#[doc = "`NetworkDestinationCidr`"]
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
pub struct NetworkDestinationCidr(::std::string::String);
impl ::std::ops::Deref for NetworkDestinationCidr {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkDestinationCidr> for ::std::string::String {
    fn from(value: NetworkDestinationCidr) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkDestinationCidr {
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
impl ::std::convert::TryFrom<&str> for NetworkDestinationCidr {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkDestinationCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkDestinationCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkDestinationCidr {
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
#[doc = "`NetworkDestinationHost`"]
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
pub struct NetworkDestinationHost(::std::string::String);
impl ::std::ops::Deref for NetworkDestinationHost {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkDestinationHost> for ::std::string::String {
    fn from(value: NetworkDestinationHost) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkDestinationHost {
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
impl ::std::convert::TryFrom<&str> for NetworkDestinationHost {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkDestinationHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkDestinationHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkDestinationHost {
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
#[doc = "        \"deny\": {"]
#[doc = "          \"description\": \"Hosts the session explicitly refuses (exact, or \\\"*.suffix\\\"). Subtracted from the merged allowlist at create; incompatible with outbound \\\"public\\\" (nothing enforces a deny off the gateway path).\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"maxLength\": 253,"]
#[doc = "            \"minLength\": 1"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 128"]
#[doc = "        },"]
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
#[doc = "        \"deny\": {"]
#[doc = "          \"description\": \"Hosts the session explicitly refuses (exact, or \\\"*.suffix\\\"). Subtracted from the merged allowlist at create; incompatible with outbound \\\"public\\\" (nothing enforces a deny off the gateway path).\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"maxLength\": 253,"]
#[doc = "            \"minLength\": 1"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 128"]
#[doc = "        },"]
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
#[doc = "        \"deny\": {"]
#[doc = "          \"description\": \"Hosts the session explicitly refuses (exact, or \\\"*.suffix\\\"). Subtracted from the merged allowlist at create; incompatible with outbound \\\"public\\\" (nothing enforces a deny off the gateway path).\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"maxLength\": 253,"]
#[doc = "            \"minLength\": 1"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 128"]
#[doc = "        },"]
#[doc = "        \"destinations\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/NetworkDestination\""]
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
#[serde(tag = "outbound", deny_unknown_fields)]
pub enum NetworkPolicy {
    #[serde(rename = "none")]
    None {
        #[doc = "Hosts the session explicitly refuses (exact, or \"*.suffix\"). Subtracted from the merged allowlist at create; incompatible with outbound \"public\" (nothing enforces a deny off the gateway path)."]
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        deny: ::std::vec::Vec<NetworkPolicyDenyItem>,
    },
    #[serde(rename = "public")]
    Public {
        #[doc = "Hosts the session explicitly refuses (exact, or \"*.suffix\"). Subtracted from the merged allowlist at create; incompatible with outbound \"public\" (nothing enforces a deny off the gateway path)."]
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        deny: ::std::vec::Vec<NetworkPolicyDenyItem>,
    },
    #[serde(rename = "allowlist")]
    Allowlist {
        #[doc = "Hosts the session explicitly refuses (exact, or \"*.suffix\"). Subtracted from the merged allowlist at create; incompatible with outbound \"public\" (nothing enforces a deny off the gateway path)."]
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        deny: ::std::vec::Vec<NetworkPolicyDenyItem>,
        destinations: ::std::vec::Vec<NetworkDestination>,
    },
}
#[doc = "`NetworkPolicyDenyItem`"]
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
pub struct NetworkPolicyDenyItem(::std::string::String);
impl ::std::ops::Deref for NetworkPolicyDenyItem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkPolicyDenyItem> for ::std::string::String {
    fn from(value: NetworkPolicyDenyItem) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkPolicyDenyItem {
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
impl ::std::convert::TryFrom<&str> for NetworkPolicyDenyItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkPolicyDenyItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkPolicyDenyItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkPolicyDenyItem {
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
#[doc = "    \"agentloop\": {"]
#[doc = "      \"$ref\": \"#/$defs/AgentloopInfo\""]
#[doc = "    },"]
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
    pub agentloop: ::std::option::Option<AgentloopInfo>,
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
#[doc = "        \"environment_unavailable\","]
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
#[doc = "    \"environment_unavailable\","]
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
    #[serde(rename = "environment_unavailable")]
    EnvironmentUnavailable,
    #[serde(rename = "internal")]
    Internal,
}
impl ::std::fmt::Display for SessionFailureCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::BindingConflict => f.write_str("binding_conflict"),
            Self::ProviderUnusable => f.write_str("provider_unusable"),
            Self::EnvironmentUnavailable => f.write_str("environment_unavailable"),
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
            "environment_unavailable" => Ok(Self::EnvironmentUnavailable),
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
#[doc = "Create-time-only immutable artifact-layer bytes. Brain stages these outside the journal, then discards this representation."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Create-time-only immutable artifact-layer bytes. Brain stages these outside the journal, then discards this representation.\","]
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
#[doc = "      \"maximum\": 67108864.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"checksum\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"content_base64\": {"]
#[doc = "      \"writeOnly\": true,"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 89478488,"]
#[doc = "      \"contentEncoding\": \"base64\""]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"application/javascript+esm\","]
#[doc = "        \"application/x-xz\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactLayer {
    pub bytes: ::std::num::NonZeroU64,
    pub checksum: Sha256Hex,
    pub content_base64: ToolArtifactLayerContentBase64,
    pub media_type: ToolArtifactLayerMediaType,
}
#[doc = "`ToolArtifactLayerContentBase64`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"writeOnly\": true,"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 89478488,"]
#[doc = "  \"contentEncoding\": \"base64\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolArtifactLayerContentBase64(::std::string::String);
impl ::std::ops::Deref for ToolArtifactLayerContentBase64 {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolArtifactLayerContentBase64> for ::std::string::String {
    fn from(value: ToolArtifactLayerContentBase64) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolArtifactLayerContentBase64 {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 89478488usize {
            return Err("longer than 89478488 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolArtifactLayerContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolArtifactLayerContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolArtifactLayerContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolArtifactLayerContentBase64 {
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
#[doc = "`ToolArtifactLayerMediaType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"application/javascript+esm\","]
#[doc = "    \"application/x-xz\""]
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
pub enum ToolArtifactLayerMediaType {
    #[serde(rename = "application/javascript+esm")]
    ApplicationJavascriptEsm,
    #[serde(rename = "application/x-xz")]
    ApplicationXXz,
}
impl ::std::fmt::Display for ToolArtifactLayerMediaType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::ApplicationJavascriptEsm => f.write_str("application/javascript+esm"),
            Self::ApplicationXXz => f.write_str("application/x-xz"),
        }
    }
}
impl ::std::str::FromStr for ToolArtifactLayerMediaType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "application/javascript+esm" => Ok(Self::ApplicationJavascriptEsm),
            "application/x-xz" => Ok(Self::ApplicationXXz),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolArtifactLayerMediaType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolArtifactLayerMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolArtifactLayerMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ToolArtifactLayerRef`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"checksum\","]
#[doc = "    \"media_type\","]
#[doc = "    \"mount_path\","]
#[doc = "    \"unpack\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 67108864.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"checksum\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"application/javascript+esm\","]
#[doc = "        \"application/x-xz\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"mount_path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"pattern\": \"^/[A-Za-z0-9._/-]+$\""]
#[doc = "    },"]
#[doc = "    \"unpack\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"file\","]
#[doc = "        \"tar.xz\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactLayerRef {
    pub bytes: ::std::num::NonZeroU64,
    pub checksum: Sha256Hex,
    pub media_type: ToolArtifactLayerRefMediaType,
    pub mount_path: ToolArtifactLayerRefMountPath,
    pub unpack: ToolArtifactLayerRefUnpack,
}
#[doc = "`ToolArtifactLayerRefMediaType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"application/javascript+esm\","]
#[doc = "    \"application/x-xz\""]
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
pub enum ToolArtifactLayerRefMediaType {
    #[serde(rename = "application/javascript+esm")]
    ApplicationJavascriptEsm,
    #[serde(rename = "application/x-xz")]
    ApplicationXXz,
}
impl ::std::fmt::Display for ToolArtifactLayerRefMediaType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::ApplicationJavascriptEsm => f.write_str("application/javascript+esm"),
            Self::ApplicationXXz => f.write_str("application/x-xz"),
        }
    }
}
impl ::std::str::FromStr for ToolArtifactLayerRefMediaType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "application/javascript+esm" => Ok(Self::ApplicationJavascriptEsm),
            "application/x-xz" => Ok(Self::ApplicationXXz),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolArtifactLayerRefMediaType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolArtifactLayerRefMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolArtifactLayerRefMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ToolArtifactLayerRefMountPath`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096,"]
#[doc = "  \"pattern\": \"^/[A-Za-z0-9._/-]+$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolArtifactLayerRefMountPath(::std::string::String);
impl ::std::ops::Deref for ToolArtifactLayerRefMountPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolArtifactLayerRefMountPath> for ::std::string::String {
    fn from(value: ToolArtifactLayerRefMountPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolArtifactLayerRefMountPath {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^/[A-Za-z0-9._/-]+$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^/[A-Za-z0-9._/-]+$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolArtifactLayerRefMountPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolArtifactLayerRefMountPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolArtifactLayerRefMountPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolArtifactLayerRefMountPath {
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
#[doc = "`ToolArtifactLayerRefUnpack`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"file\","]
#[doc = "    \"tar.xz\""]
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
pub enum ToolArtifactLayerRefUnpack {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "tar.xz")]
    TarXz,
}
impl ::std::fmt::Display for ToolArtifactLayerRefUnpack {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::File => f.write_str("file"),
            Self::TarXz => f.write_str("tar.xz"),
        }
    }
}
impl ::std::str::FromStr for ToolArtifactLayerRefUnpack {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "file" => Ok(Self::File),
            "tar.xz" => Ok(Self::TarXz),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolArtifactLayerRefUnpack {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolArtifactLayerRefUnpack {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolArtifactLayerRefUnpack {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "A canonical computer-profile manifest plus create-time-only immutable runtime and code layers."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A canonical computer-profile manifest plus create-time-only immutable runtime and code layers.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"checksum\","]
#[doc = "    \"execute_path\","]
#[doc = "    \"layers\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"maximum\": 100663296.0,"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"checksum\": {"]
#[doc = "      \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "    },"]
#[doc = "    \"execute_path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"pattern\": \"^/[^\\\\u0000]+$\""]
#[doc = "    },"]
#[doc = "    \"layers\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolArtifactLayerRef\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 16,"]
#[doc = "      \"minItems\": 1"]
#[doc = "    },"]
#[doc = "    \"setup_path\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"pattern\": \"^/[^\\\\u0000]+$\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"linux-amd64\","]
#[doc = "        \"linux-arm64\""]
#[doc = "      ]"]
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
    pub execute_path: ToolBundleExecutePath,
    pub layers: ::std::vec::Vec<ToolArtifactLayerRef>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub setup_path: ::std::option::Option<ToolBundleSetupPath>,
    pub target: ToolBundleTarget,
}
#[doc = "`ToolBundleExecutePath`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096,"]
#[doc = "  \"pattern\": \"^/[^\\\\u0000]+$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolBundleExecutePath(::std::string::String);
impl ::std::ops::Deref for ToolBundleExecutePath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolBundleExecutePath> for ::std::string::String {
    fn from(value: ToolBundleExecutePath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolBundleExecutePath {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^/[^\\u0000]+$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^/[^\\u0000]+$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolBundleExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolBundleExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolBundleExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolBundleExecutePath {
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
#[doc = "`ToolBundleSetupPath`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096,"]
#[doc = "  \"pattern\": \"^/[^\\\\u0000]+$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolBundleSetupPath(::std::string::String);
impl ::std::ops::Deref for ToolBundleSetupPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolBundleSetupPath> for ::std::string::String {
    fn from(value: ToolBundleSetupPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolBundleSetupPath {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^/[^\\u0000]+$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^/[^\\u0000]+$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolBundleSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolBundleSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolBundleSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolBundleSetupPath {
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
#[doc = "`ToolBundleTarget`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"linux-amd64\","]
#[doc = "    \"linux-arm64\""]
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
pub enum ToolBundleTarget {
    #[serde(rename = "linux-amd64")]
    LinuxAmd64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
}
impl ::std::fmt::Display for ToolBundleTarget {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::LinuxAmd64 => f.write_str("linux-amd64"),
            Self::LinuxArm64 => f.write_str("linux-arm64"),
        }
    }
}
impl ::std::str::FromStr for ToolBundleTarget {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "linux-amd64" => Ok(Self::LinuxAmd64),
            "linux-arm64" => Ok(Self::LinuxArm64),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolBundleTarget {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolBundleTarget {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolBundleTarget {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"description\": \"The tool's declared outbound needs. Merged at create: effective allowlist = (union of tool declarations and session allows) minus session denies. Product-specific infrastructure denials belong to the hosting composition. Declaration and merge only - no per-tool runtime isolation is claimed.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"destinations\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"destinations\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/NetworkDestination\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 32,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
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
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub network: ::std::option::Option<ToolConfigNetwork>,
}
#[doc = "The tool's declared outbound needs. Merged at create: effective allowlist = (union of tool declarations and session allows) minus session denies. Product-specific infrastructure denials belong to the hosting composition. Declaration and merge only - no per-tool runtime isolation is claimed."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The tool's declared outbound needs. Merged at create: effective allowlist = (union of tool declarations and session allows) minus session denies. Product-specific infrastructure denials belong to the hosting composition. Declaration and merge only - no per-tool runtime isolation is claimed.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"destinations\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"destinations\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/NetworkDestination\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 32,"]
#[doc = "      \"minItems\": 1"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolConfigNetwork {
    pub destinations: ::std::vec::Vec<NetworkDestination>,
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
#[doc = "      \"description\": \"An explicitly bound tool executed by one declared logical environment.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"environment\","]
#[doc = "        \"kind\","]
#[doc = "        \"requirements\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"artifact_digest\": {"]
#[doc = "          \"$ref\": \"#/$defs/Sha256Hex\""]
#[doc = "        },"]
#[doc = "        \"callback_registration\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"pattern\": \"^[A-Za-z0-9_.:-]{1,128}$\""]
#[doc = "        },"]
#[doc = "        \"environment\": {"]
#[doc = "          \"$ref\": \"#/$defs/EnvironmentName\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"const\": \"environment\""]
#[doc = "        },"]
#[doc = "        \"requirements\": {"]
#[doc = "          \"$ref\": \"#/$defs/ToolRequirements\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"capability\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"capability\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"pattern\": \"^brain\\\\.[A-Za-z0-9_.:-]{1,120}$\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"const\": \"engine\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ToolExecutor {
    #[doc = "An explicitly bound tool executed by one declared logical environment."]
    #[serde(rename = "environment")]
    Environment {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        artifact_digest: ::std::option::Option<Sha256Hex>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        callback_registration: ::std::option::Option<ToolExecutorCallbackRegistration>,
        environment: EnvironmentName,
        requirements: ToolRequirements,
    },
    #[serde(rename = "engine")]
    Engine { capability: ToolExecutorCapability },
}
#[doc = "`ToolExecutorCallbackRegistration`"]
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
pub struct ToolExecutorCallbackRegistration(::std::string::String);
impl ::std::ops::Deref for ToolExecutorCallbackRegistration {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolExecutorCallbackRegistration> for ::std::string::String {
    fn from(value: ToolExecutorCallbackRegistration) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolExecutorCallbackRegistration {
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
impl ::std::convert::TryFrom<&str> for ToolExecutorCallbackRegistration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolExecutorCallbackRegistration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolExecutorCallbackRegistration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolExecutorCallbackRegistration {
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
#[doc = "`ToolExecutorCapability`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"pattern\": \"^brain\\\\.[A-Za-z0-9_.:-]{1,120}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolExecutorCapability(::std::string::String);
impl ::std::ops::Deref for ToolExecutorCapability {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolExecutorCapability> for ::std::string::String {
    fn from(value: ToolExecutorCapability) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolExecutorCapability {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^brain\\.[A-Za-z0-9_.:-]{1,120}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^brain\\.[A-Za-z0-9_.:-]{1,120}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolExecutorCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolExecutorCapability {
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
#[doc = "`ToolRequirements`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"env\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"pattern\": \"^[A-Za-z_][A-Za-z0-9_]*$\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 64,"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/NetworkDestination\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 32"]
#[doc = "    },"]
#[doc = "    \"processes\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"recovery\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"retained\","]
#[doc = "        \"connection\","]
#[doc = "        \"replay_safe\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"streaming\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"workspace\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolRequirements {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub env: ::std::option::Option<Vec<ToolRequirementsEnvItem>>,
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub network: ::std::vec::Vec<NetworkDestination>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub processes: ::std::option::Option<bool>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub recovery: ::std::option::Option<ToolRequirementsRecovery>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub streaming: ::std::option::Option<bool>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub workspace: ::std::option::Option<bool>,
}
impl ::std::default::Default for ToolRequirements {
    fn default() -> Self {
        Self {
            env: Default::default(),
            network: Default::default(),
            processes: Default::default(),
            recovery: Default::default(),
            streaming: Default::default(),
            workspace: Default::default(),
        }
    }
}
#[doc = "`ToolRequirementsEnvItem`"]
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
pub struct ToolRequirementsEnvItem(::std::string::String);
impl ::std::ops::Deref for ToolRequirementsEnvItem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolRequirementsEnvItem> for ::std::string::String {
    fn from(value: ToolRequirementsEnvItem) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolRequirementsEnvItem {
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
impl ::std::convert::TryFrom<&str> for ToolRequirementsEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolRequirementsEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolRequirementsEnvItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolRequirementsEnvItem {
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
#[doc = "`ToolRequirementsRecovery`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"retained\","]
#[doc = "    \"connection\","]
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
pub enum ToolRequirementsRecovery {
    #[serde(rename = "retained")]
    Retained,
    #[serde(rename = "connection")]
    Connection,
    #[serde(rename = "replay_safe")]
    ReplaySafe,
}
impl ::std::fmt::Display for ToolRequirementsRecovery {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Retained => f.write_str("retained"),
            Self::Connection => f.write_str("connection"),
            Self::ReplaySafe => f.write_str("replay_safe"),
        }
    }
}
impl ::std::str::FromStr for ToolRequirementsRecovery {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "retained" => Ok(Self::Retained),
            "connection" => Ok(Self::Connection),
            "replay_safe" => Ok(Self::ReplaySafe),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolRequirementsRecovery {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolRequirementsRecovery {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolRequirementsRecovery {
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
