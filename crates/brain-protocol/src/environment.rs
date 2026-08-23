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
#[doc = "`AcknowledgeTerminalRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"operation\","]
#[doc = "    \"terminal_digest\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    },"]
#[doc = "    \"terminal_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgeTerminalRequest {
    pub operation: OperationRef,
    pub terminal_digest: Digest,
}
#[doc = "`Acknowledgement`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"acknowledged\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"acknowledged\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    pub acknowledged: bool,
}
#[doc = "`ArtifactTarget`"]
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
pub enum ArtifactTarget {
    #[serde(rename = "linux-amd64")]
    LinuxAmd64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
}
impl ::std::fmt::Display for ArtifactTarget {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::LinuxAmd64 => f.write_str("linux-amd64"),
            Self::LinuxArm64 => f.write_str("linux-arm64"),
        }
    }
}
impl ::std::str::FromStr for ArtifactTarget {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "linux-amd64" => Ok(Self::LinuxAmd64),
            "linux-arm64" => Ok(Self::LinuxArm64),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ArtifactTarget {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ArtifactTarget {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ArtifactTarget {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "The single current, transport-neutral Brain to Environment receipt contract. The canonical schema digest is the compatibility identity; the wire carries no protocol version."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://github.com/aexhq/brain/contracts/environment/contract.json\","]
#[doc = "  \"title\": \"BrainEnvironmentContract\","]
#[doc = "  \"description\": \"The single current, transport-neutral Brain to Environment receipt contract. The canonical schema digest is the compatibility identity; the wire carries no protocol version.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"contract\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"contract\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"methods\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"methods\": {"]
#[doc = "          \"const\": ["]
#[doc = "            \"resolve_binding\","]
#[doc = "            \"submit\","]
#[doc = "            \"observe\","]
#[doc = "            \"cancel\","]
#[doc = "            \"acknowledge_terminal\""]
#[doc = "          ]"]
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
pub struct BrainEnvironmentContract {
    pub contract: BrainEnvironmentContractContract,
}
#[doc = "`BrainEnvironmentContractContract`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"methods\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"methods\": {"]
#[doc = "      \"const\": ["]
#[doc = "        \"resolve_binding\","]
#[doc = "        \"submit\","]
#[doc = "        \"observe\","]
#[doc = "        \"cancel\","]
#[doc = "        \"acknowledge_terminal\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BrainEnvironmentContractContract {
    pub methods: ::serde_json::Value,
}
#[doc = "`BundleDescriptor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bundle_digest\","]
#[doc = "    \"bytes\","]
#[doc = "    \"contract_digest\","]
#[doc = "    \"environment_name\","]
#[doc = "    \"execute_path\","]
#[doc = "    \"object\","]
#[doc = "    \"required_env\","]
#[doc = "    \"target\","]
#[doc = "    \"tool_name\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bundle_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"contract_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"description\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 4096"]
#[doc = "    },"]
#[doc = "    \"environment_name\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"execute_path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"pattern\": \"^/[^\\\\u0000]+$\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"$ref\": \"#/definitions/ObjectReference\""]
#[doc = "    },"]
#[doc = "    \"required_env\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/Identifier\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 128,"]
#[doc = "      \"uniqueItems\": true"]
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
#[doc = "      \"$ref\": \"#/definitions/ArtifactTarget\""]
#[doc = "    },"]
#[doc = "    \"tool_name\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BundleDescriptor {
    pub bundle_digest: Digest,
    pub bytes: ::std::num::NonZeroU64,
    pub contract_digest: Digest,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<BundleDescriptorDescription>,
    pub environment_name: Identifier,
    pub execute_path: BundleDescriptorExecutePath,
    pub object: ObjectReference,
    pub required_env: Vec<Identifier>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub setup_path: ::std::option::Option<BundleDescriptorSetupPath>,
    pub target: ArtifactTarget,
    pub tool_name: Identifier,
}
#[doc = "`BundleDescriptorDescription`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct BundleDescriptorDescription(::std::string::String);
impl ::std::ops::Deref for BundleDescriptorDescription {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BundleDescriptorDescription> for ::std::string::String {
    fn from(value: BundleDescriptorDescription) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BundleDescriptorDescription {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for BundleDescriptorDescription {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleDescriptorDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleDescriptorDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BundleDescriptorDescription {
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
#[doc = "`BundleDescriptorExecutePath`"]
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
pub struct BundleDescriptorExecutePath(::std::string::String);
impl ::std::ops::Deref for BundleDescriptorExecutePath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BundleDescriptorExecutePath> for ::std::string::String {
    fn from(value: BundleDescriptorExecutePath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BundleDescriptorExecutePath {
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
impl ::std::convert::TryFrom<&str> for BundleDescriptorExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleDescriptorExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleDescriptorExecutePath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BundleDescriptorExecutePath {
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
#[doc = "`BundleDescriptorSetupPath`"]
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
pub struct BundleDescriptorSetupPath(::std::string::String);
impl ::std::ops::Deref for BundleDescriptorSetupPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BundleDescriptorSetupPath> for ::std::string::String {
    fn from(value: BundleDescriptorSetupPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BundleDescriptorSetupPath {
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
impl ::std::convert::TryFrom<&str> for BundleDescriptorSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleDescriptorSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleDescriptorSetupPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BundleDescriptorSetupPath {
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
#[doc = "Short-lived, one-purpose fetch authority supplied only at preparation time; it is not part of the persisted sealed binding."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Short-lived, one-purpose fetch authority supplied only at preparation time; it is not part of the persisted sealed binding.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bundle_digest\","]
#[doc = "    \"expires_at_ms\","]
#[doc = "    \"headers\","]
#[doc = "    \"max_bytes\","]
#[doc = "    \"url\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bundle_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"expires_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"headers\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 16,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 4096"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"max_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 8192,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BundleFetch {
    pub bundle_digest: Digest,
    pub expires_at_ms: ::std::num::NonZeroU64,
    pub headers: ::std::collections::HashMap<::std::string::String, BundleFetchHeadersValue>,
    pub max_bytes: ::std::num::NonZeroU64,
    pub url: BundleFetchUrl,
}
#[doc = "`BundleFetchHeadersValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct BundleFetchHeadersValue(::std::string::String);
impl ::std::ops::Deref for BundleFetchHeadersValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BundleFetchHeadersValue> for ::std::string::String {
    fn from(value: BundleFetchHeadersValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BundleFetchHeadersValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for BundleFetchHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleFetchHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleFetchHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BundleFetchHeadersValue {
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
#[doc = "`BundleFetchUrl`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 8192,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct BundleFetchUrl(::std::string::String);
impl ::std::ops::Deref for BundleFetchUrl {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<BundleFetchUrl> for ::std::string::String {
    fn from(value: BundleFetchUrl) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for BundleFetchUrl {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 8192usize {
            return Err("longer than 8192 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for BundleFetchUrl {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleFetchUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleFetchUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for BundleFetchUrl {
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
#[doc = "`CancelRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"operation\","]
#[doc = "    \"reason\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    },"]
#[doc = "    \"reason\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 512,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub operation: OperationRef,
    pub reason: CancelRequestReason,
}
#[doc = "`CancelRequestReason`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 512,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CancelRequestReason(::std::string::String);
impl ::std::ops::Deref for CancelRequestReason {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CancelRequestReason> for ::std::string::String {
    fn from(value: CancelRequestReason) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CancelRequestReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 512usize {
            return Err("longer than 512 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for CancelRequestReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CancelRequestReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CancelRequestReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CancelRequestReason {
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
#[doc = "`CancellationReceipt`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"accepted\","]
#[doc = "    \"observation\","]
#[doc = "    \"operation\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"accepted\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"observation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationObservation\""]
#[doc = "    },"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CancellationReceipt {
    pub accepted: bool,
    pub observation: OperationObservation,
    pub operation: OperationRef,
}
#[doc = "`CreateSandboxRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"generation_intent\","]
#[doc = "    \"network\","]
#[doc = "    \"resource_class\","]
#[doc = "    \"resources\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"generation_intent\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"$ref\": \"#/definitions/NetworkCeiling\""]
#[doc = "    },"]
#[doc = "    \"resource_class\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"resources\": {"]
#[doc = "      \"$ref\": \"#/definitions/ResourceCeiling\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CreateSandboxRequest {
    pub generation_intent: Identifier,
    pub network: NetworkCeiling,
    pub resource_class: Identifier,
    pub resources: ResourceCeiling,
    pub target: SandboxTarget,
}
#[doc = "`Digest`"]
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
pub struct Digest(::std::string::String);
impl ::std::ops::Deref for Digest {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<Digest> for ::std::string::String {
    fn from(value: Digest) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for Digest {
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
impl ::std::convert::TryFrom<&str> for Digest {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Digest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Digest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for Digest {
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
#[doc = "`EnvironmentCapability`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"execution\","]
#[doc = "    \"session_preparation\","]
#[doc = "    \"sandbox_files\","]
#[doc = "    \"sandbox_control\""]
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
pub enum EnvironmentCapability {
    #[serde(rename = "execution")]
    Execution,
    #[serde(rename = "session_preparation")]
    SessionPreparation,
    #[serde(rename = "sandbox_files")]
    SandboxFiles,
    #[serde(rename = "sandbox_control")]
    SandboxControl,
}
impl ::std::fmt::Display for EnvironmentCapability {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Execution => f.write_str("execution"),
            Self::SessionPreparation => f.write_str("session_preparation"),
            Self::SandboxFiles => f.write_str("sandbox_files"),
            Self::SandboxControl => f.write_str("sandbox_control"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentCapability {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "execution" => Ok(Self::Execution),
            "session_preparation" => Ok(Self::SessionPreparation),
            "sandbox_files" => Ok(Self::SandboxFiles),
            "sandbox_control" => Ok(Self::SandboxControl),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentCapability {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentError`"]
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
#[doc = "      \"$ref\": \"#/definitions/EnvironmentErrorCode\""]
#[doc = "    },"]
#[doc = "    \"details\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"additionalProperties\": true"]
#[doc = "    },"]
#[doc = "    \"message\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"retryable\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentError {
    pub code: EnvironmentErrorCode,
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub details: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub message: EnvironmentErrorMessage,
    pub retryable: bool,
}
#[doc = "`EnvironmentErrorCode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"binding_conflict\","]
#[doc = "    \"capability_unavailable\","]
#[doc = "    \"operation_conflict\","]
#[doc = "    \"operation_unknown\","]
#[doc = "    \"sandbox_not_materialized\","]
#[doc = "    \"sandbox_gone\","]
#[doc = "    \"file_not_found\","]
#[doc = "    \"generation_conflict\","]
#[doc = "    \"invalid_request\","]
#[doc = "    \"resource_exhausted\","]
#[doc = "    \"temporarily_unavailable\""]
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
pub enum EnvironmentErrorCode {
    #[serde(rename = "binding_conflict")]
    BindingConflict,
    #[serde(rename = "capability_unavailable")]
    CapabilityUnavailable,
    #[serde(rename = "operation_conflict")]
    OperationConflict,
    #[serde(rename = "operation_unknown")]
    OperationUnknown,
    #[serde(rename = "sandbox_not_materialized")]
    SandboxNotMaterialized,
    #[serde(rename = "sandbox_gone")]
    SandboxGone,
    #[serde(rename = "file_not_found")]
    FileNotFound,
    #[serde(rename = "generation_conflict")]
    GenerationConflict,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "resource_exhausted")]
    ResourceExhausted,
    #[serde(rename = "temporarily_unavailable")]
    TemporarilyUnavailable,
}
impl ::std::fmt::Display for EnvironmentErrorCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::BindingConflict => f.write_str("binding_conflict"),
            Self::CapabilityUnavailable => f.write_str("capability_unavailable"),
            Self::OperationConflict => f.write_str("operation_conflict"),
            Self::OperationUnknown => f.write_str("operation_unknown"),
            Self::SandboxNotMaterialized => f.write_str("sandbox_not_materialized"),
            Self::SandboxGone => f.write_str("sandbox_gone"),
            Self::FileNotFound => f.write_str("file_not_found"),
            Self::GenerationConflict => f.write_str("generation_conflict"),
            Self::InvalidRequest => f.write_str("invalid_request"),
            Self::ResourceExhausted => f.write_str("resource_exhausted"),
            Self::TemporarilyUnavailable => f.write_str("temporarily_unavailable"),
        }
    }
}
impl ::std::str::FromStr for EnvironmentErrorCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "binding_conflict" => Ok(Self::BindingConflict),
            "capability_unavailable" => Ok(Self::CapabilityUnavailable),
            "operation_conflict" => Ok(Self::OperationConflict),
            "operation_unknown" => Ok(Self::OperationUnknown),
            "sandbox_not_materialized" => Ok(Self::SandboxNotMaterialized),
            "sandbox_gone" => Ok(Self::SandboxGone),
            "file_not_found" => Ok(Self::FileNotFound),
            "generation_conflict" => Ok(Self::GenerationConflict),
            "invalid_request" => Ok(Self::InvalidRequest),
            "resource_exhausted" => Ok(Self::ResourceExhausted),
            "temporarily_unavailable" => Ok(Self::TemporarilyUnavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EnvironmentErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`EnvironmentErrorMessage`"]
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
pub struct EnvironmentErrorMessage(::std::string::String);
impl ::std::ops::Deref for EnvironmentErrorMessage {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<EnvironmentErrorMessage> for ::std::string::String {
    fn from(value: EnvironmentErrorMessage) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for EnvironmentErrorMessage {
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
impl ::std::convert::TryFrom<&str> for EnvironmentErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EnvironmentErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EnvironmentErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for EnvironmentErrorMessage {
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
#[doc = "`FileEntry`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"kind\","]
#[doc = "    \"modified_at_ms\","]
#[doc = "    \"path\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"kind\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"file\","]
#[doc = "        \"directory\","]
#[doc = "        \"symlink\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"modified_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/Digest\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub bytes: u64,
    pub kind: FileEntryKind,
    pub modified_at_ms: u64,
    pub path: FileEntryPath,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub sha256: ::std::option::Option<Digest>,
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
#[doc = "    \"directory\","]
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
    #[serde(rename = "directory")]
    Directory,
    #[serde(rename = "symlink")]
    Symlink,
}
impl ::std::fmt::Display for FileEntryKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::File => f.write_str("file"),
            Self::Directory => f.write_str("directory"),
            Self::Symlink => f.write_str("symlink"),
        }
    }
}
impl ::std::str::FromStr for FileEntryKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "file" => Ok(Self::File),
            "directory" => Ok(Self::Directory),
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
#[doc = "`FileEntryPath`"]
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
pub struct FileEntryPath(::std::string::String);
impl ::std::ops::Deref for FileEntryPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<FileEntryPath> for ::std::string::String {
    fn from(value: FileEntryPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for FileEntryPath {
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
impl ::std::convert::TryFrom<&str> for FileEntryPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FileEntryPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FileEntryPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for FileEntryPath {
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
#[doc = "`Identifier`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128,"]
#[doc = "  \"minLength\": 1,"]
#[doc = "  \"pattern\": \"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct Identifier(::std::string::String);
impl ::std::ops::Deref for Identifier {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<Identifier> for ::std::string::String {
    fn from(value: Identifier) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for Identifier {
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
impl ::std::convert::TryFrom<&str> for Identifier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Identifier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Identifier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for Identifier {
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
#[doc = "`NetworkCeiling`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"const\": \"none\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"const\": \"public\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"destinations\","]
#[doc = "        \"kind\""]
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
#[doc = "                    \"maxLength\": 64,"]
#[doc = "                    \"minLength\": 1"]
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
#[doc = "          \"minItems\": 1,"]
#[doc = "          \"uniqueItems\": true"]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
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
#[serde(tag = "kind", content = "destinations")]
pub enum NetworkCeiling {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "allowlist")]
    Allowlist(Vec<NetworkCeilingDestinationsItem>),
}
impl ::std::convert::From<Vec<NetworkCeilingDestinationsItem>> for NetworkCeiling {
    fn from(value: Vec<NetworkCeilingDestinationsItem>) -> Self {
        Self::Allowlist(value)
    }
}
#[doc = "`NetworkCeilingDestinationsItem`"]
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
#[doc = "          \"maxLength\": 64,"]
#[doc = "          \"minLength\": 1"]
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
pub enum NetworkCeilingDestinationsItem {
    #[serde(rename = "tls")]
    Tls {
        host: NetworkCeilingDestinationsItemHost,
        ports: [::serde_json::Value; 1usize],
    },
    #[serde(rename = "tcp")]
    Tcp {
        cidr: NetworkCeilingDestinationsItemCidr,
        ports: Vec<::std::num::NonZeroU64>,
    },
}
#[doc = "`NetworkCeilingDestinationsItemCidr`"]
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
pub struct NetworkCeilingDestinationsItemCidr(::std::string::String);
impl ::std::ops::Deref for NetworkCeilingDestinationsItemCidr {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkCeilingDestinationsItemCidr> for ::std::string::String {
    fn from(value: NetworkCeilingDestinationsItemCidr) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkCeilingDestinationsItemCidr {
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
impl ::std::convert::TryFrom<&str> for NetworkCeilingDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkCeilingDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkCeilingDestinationsItemCidr {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkCeilingDestinationsItemCidr {
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
#[doc = "`NetworkCeilingDestinationsItemHost`"]
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
pub struct NetworkCeilingDestinationsItemHost(::std::string::String);
impl ::std::ops::Deref for NetworkCeilingDestinationsItemHost {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<NetworkCeilingDestinationsItemHost> for ::std::string::String {
    fn from(value: NetworkCeilingDestinationsItemHost) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for NetworkCeilingDestinationsItemHost {
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
impl ::std::convert::TryFrom<&str> for NetworkCeilingDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkCeilingDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkCeilingDestinationsItemHost {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for NetworkCeilingDestinationsItemHost {
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
#[doc = "`ObjectReference`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bytes\","]
#[doc = "    \"object_id\","]
#[doc = "    \"sha256\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"media_type\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 255"]
#[doc = "    },"]
#[doc = "    \"object_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"sha256\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ObjectReference {
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub media_type: ::std::option::Option<ObjectReferenceMediaType>,
    pub object_id: Identifier,
    pub sha256: Digest,
}
#[doc = "`ObjectReferenceMediaType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 255"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ObjectReferenceMediaType(::std::string::String);
impl ::std::ops::Deref for ObjectReferenceMediaType {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ObjectReferenceMediaType> for ::std::string::String {
    fn from(value: ObjectReferenceMediaType) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ObjectReferenceMediaType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 255usize {
            return Err("longer than 255 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ObjectReferenceMediaType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ObjectReferenceMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ObjectReferenceMediaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ObjectReferenceMediaType {
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
#[doc = "Short-lived, one-purpose transfer capability minted by Brain-owned storage. transfer_id identifies the reservation/capability; object_id is the immutable source or pending destination identity. GET is valid only for import and PUT only for export; Environments never infer an object-store key. Export returns ObjectReference.object_id exactly equal to this sealed object_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Short-lived, one-purpose transfer capability minted by Brain-owned storage. transfer_id identifies the reservation/capability; object_id is the immutable source or pending destination identity. GET is valid only for import and PUT only for export; Environments never infer an object-store key. Export returns ObjectReference.object_id exactly equal to this sealed object_id.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"expires_at_ms\","]
#[doc = "    \"headers\","]
#[doc = "    \"max_bytes\","]
#[doc = "    \"method\","]
#[doc = "    \"object_id\","]
#[doc = "    \"transfer_id\","]
#[doc = "    \"url\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"expires_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"headers\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 16,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 4096"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"max_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"method\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"GET\","]
#[doc = "        \"PUT\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"object_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"transfer_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"url\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 8192,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ObjectTransferAuthority {
    pub expires_at_ms: ::std::num::NonZeroU64,
    pub headers:
        ::std::collections::HashMap<::std::string::String, ObjectTransferAuthorityHeadersValue>,
    pub max_bytes: ::std::num::NonZeroU64,
    pub method: ObjectTransferAuthorityMethod,
    pub object_id: Identifier,
    pub transfer_id: Identifier,
    pub url: ObjectTransferAuthorityUrl,
}
#[doc = "`ObjectTransferAuthorityHeadersValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ObjectTransferAuthorityHeadersValue(::std::string::String);
impl ::std::ops::Deref for ObjectTransferAuthorityHeadersValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ObjectTransferAuthorityHeadersValue> for ::std::string::String {
    fn from(value: ObjectTransferAuthorityHeadersValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ObjectTransferAuthorityHeadersValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ObjectTransferAuthorityHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ObjectTransferAuthorityHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ObjectTransferAuthorityHeadersValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ObjectTransferAuthorityHeadersValue {
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
#[doc = "`ObjectTransferAuthorityMethod`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"GET\","]
#[doc = "    \"PUT\""]
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
pub enum ObjectTransferAuthorityMethod {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "PUT")]
    Put,
}
impl ::std::fmt::Display for ObjectTransferAuthorityMethod {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Get => f.write_str("GET"),
            Self::Put => f.write_str("PUT"),
        }
    }
}
impl ::std::str::FromStr for ObjectTransferAuthorityMethod {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "GET" => Ok(Self::Get),
            "PUT" => Ok(Self::Put),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ObjectTransferAuthorityMethod {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ObjectTransferAuthorityMethod {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ObjectTransferAuthorityMethod {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ObjectTransferAuthorityUrl`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 8192,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ObjectTransferAuthorityUrl(::std::string::String);
impl ::std::ops::Deref for ObjectTransferAuthorityUrl {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ObjectTransferAuthorityUrl> for ::std::string::String {
    fn from(value: ObjectTransferAuthorityUrl) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ObjectTransferAuthorityUrl {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 8192usize {
            return Err("longer than 8192 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ObjectTransferAuthorityUrl {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ObjectTransferAuthorityUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ObjectTransferAuthorityUrl {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ObjectTransferAuthorityUrl {
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
#[doc = "`ObserveRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"cursor\","]
#[doc = "    \"operation\","]
#[doc = "    \"wait_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"cursor\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 256"]
#[doc = "    },"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    },"]
#[doc = "    \"wait_ms\": {"]
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
pub struct ObserveRequest {
    pub cursor: ObserveRequestCursor,
    pub operation: OperationRef,
    pub wait_ms: u64,
}
#[doc = "`ObserveRequestCursor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ObserveRequestCursor(::std::string::String);
impl ::std::ops::Deref for ObserveRequestCursor {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ObserveRequestCursor> for ::std::string::String {
    fn from(value: ObserveRequestCursor) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ObserveRequestCursor {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ObserveRequestCursor {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ObserveRequestCursor {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ObserveRequestCursor {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ObserveRequestCursor {
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
#[doc = "`OperationEnvelope`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"binding_ref\","]
#[doc = "    \"caller_id\","]
#[doc = "    \"capability\","]
#[doc = "    \"deadline_at_ms\","]
#[doc = "    \"fence\","]
#[doc = "    \"input\","]
#[doc = "    \"network\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"phase\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"resources\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\","]
#[doc = "    \"trace\","]
#[doc = "    \"turn_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"caller_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"capability\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"deadline_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"fence\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"generation\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 128"]
#[doc = "    },"]
#[doc = "    \"input\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationInput\""]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"$ref\": \"#/definitions/NetworkCeiling\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"phase\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"setup\","]
#[doc = "        \"execute\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"resources\": {"]
#[doc = "      \"$ref\": \"#/definitions/ResourceCeiling\""]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"target_ref\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 256"]
#[doc = "    },"]
#[doc = "    \"trace\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"maxProperties\": 16,"]
#[doc = "      \"additionalProperties\": {"]
#[doc = "        \"type\": \"string\","]
#[doc = "        \"maxLength\": 256"]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"turn_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperationEnvelope {
    pub binding_ref: Identifier,
    pub caller_id: Identifier,
    pub capability: Identifier,
    pub deadline_at_ms: ::std::num::NonZeroU64,
    pub fence: ::std::num::NonZeroU64,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub generation: ::std::option::Option<OperationEnvelopeGeneration>,
    pub input: OperationInput,
    pub network: NetworkCeiling,
    pub operation_id: Identifier,
    pub phase: OperationEnvelopePhase,
    pub request_digest: Digest,
    pub resources: ResourceCeiling,
    pub root_id: Identifier,
    pub session_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub target_ref: ::std::option::Option<OperationEnvelopeTargetRef>,
    pub trace: ::std::collections::HashMap<::std::string::String, OperationEnvelopeTraceValue>,
    pub turn_id: Identifier,
}
#[doc = "`OperationEnvelopeGeneration`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OperationEnvelopeGeneration(::std::string::String);
impl ::std::ops::Deref for OperationEnvelopeGeneration {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OperationEnvelopeGeneration> for ::std::string::String {
    fn from(value: OperationEnvelopeGeneration) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OperationEnvelopeGeneration {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OperationEnvelopeGeneration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationEnvelopeGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationEnvelopeGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OperationEnvelopeGeneration {
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
#[doc = "`OperationEnvelopePhase`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"setup\","]
#[doc = "    \"execute\""]
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
pub enum OperationEnvelopePhase {
    #[serde(rename = "setup")]
    Setup,
    #[serde(rename = "execute")]
    Execute,
}
impl ::std::fmt::Display for OperationEnvelopePhase {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Setup => f.write_str("setup"),
            Self::Execute => f.write_str("execute"),
        }
    }
}
impl ::std::str::FromStr for OperationEnvelopePhase {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "setup" => Ok(Self::Setup),
            "execute" => Ok(Self::Execute),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OperationEnvelopePhase {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationEnvelopePhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationEnvelopePhase {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`OperationEnvelopeTargetRef`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OperationEnvelopeTargetRef(::std::string::String);
impl ::std::ops::Deref for OperationEnvelopeTargetRef {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OperationEnvelopeTargetRef> for ::std::string::String {
    fn from(value: OperationEnvelopeTargetRef) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OperationEnvelopeTargetRef {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OperationEnvelopeTargetRef {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationEnvelopeTargetRef {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationEnvelopeTargetRef {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OperationEnvelopeTargetRef {
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
#[doc = "`OperationEnvelopeTraceValue`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OperationEnvelopeTraceValue(::std::string::String);
impl ::std::ops::Deref for OperationEnvelopeTraceValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OperationEnvelopeTraceValue> for ::std::string::String {
    fn from(value: OperationEnvelopeTraceValue) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OperationEnvelopeTraceValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OperationEnvelopeTraceValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationEnvelopeTraceValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationEnvelopeTraceValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OperationEnvelopeTraceValue {
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
#[doc = "Canonical JSON Tool arguments only. Brain rejects serialized input above 192 KiB before submit. Large data is referenced by storage key, URL, or sandbox path and transferred through typed streaming authorities, never embedded as a managed Tool argument."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Canonical JSON Tool arguments only. Brain rejects serialized input above 192 KiB before submit. Large data is referenced by storage key, URL, or sandbox path and transferred through typed streaming authorities, never embedded as a managed Tool argument.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"kind\","]
#[doc = "    \"value\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"kind\": {"]
#[doc = "      \"const\": \"inline\""]
#[doc = "    },"]
#[doc = "    \"value\": {}"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperationInput {
    pub kind: ::serde_json::Value,
    pub value: ::serde_json::Value,
}
#[doc = "`OperationObservation`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"next_cursor\","]
#[doc = "    \"operation\","]
#[doc = "    \"output\","]
#[doc = "    \"state\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"next_cursor\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 256"]
#[doc = "    },"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    },"]
#[doc = "    \"output\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/OutputChunk\""]
#[doc = "      }"]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationState\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/TargetReceipt\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"terminal\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/TerminalResult\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperationObservation {
    pub next_cursor: OperationObservationNextCursor,
    pub operation: OperationRef,
    pub output: ::std::vec::Vec<OutputChunk>,
    pub state: OperationState,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub target: ::std::option::Option<TargetReceipt>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub terminal: ::std::option::Option<TerminalResult>,
}
#[doc = "`OperationObservationNextCursor`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OperationObservationNextCursor(::std::string::String);
impl ::std::ops::Deref for OperationObservationNextCursor {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OperationObservationNextCursor> for ::std::string::String {
    fn from(value: OperationObservationNextCursor) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OperationObservationNextCursor {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OperationObservationNextCursor {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationObservationNextCursor {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationObservationNextCursor {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OperationObservationNextCursor {
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
#[doc = "Durable execution locator carrying both the opaque receipt and the exact rooted target authority required to observe, cancel, acknowledge, and reconcile target loss."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Durable execution locator carrying both the opaque receipt and the exact rooted target authority required to observe, cancel, acknowledge, and reconcile target loss.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"generation\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"receipt_ref\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"target\","]
#[doc = "    \"target_ref\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"receipt_ref\": {"]
#[doc = "      \"description\": \"Opaque Environment-issued locator for the accepted physical execution. Brain journals it before observe/cancel/ack; it complements the Environment binding/preparation/target registry and never encodes product routing policy.\","]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"description\": \"Exact rooted logical target accepted for this execution. Control and acknowledgement calls carry it back so Environment can reconcile its root-keyed target registry without a reverse index or scan.\","]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    },"]
#[doc = "    \"target_ref\": {"]
#[doc = "      \"description\": \"Opaque physical target locator paired with generation. It never replaces the rooted logical target.\","]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OperationRef {
    pub generation: Identifier,
    pub operation_id: Identifier,
    #[doc = "Opaque Environment-issued locator for the accepted physical execution. Brain journals it before observe/cancel/ack; it complements the Environment binding/preparation/target registry and never encodes product routing policy."]
    pub receipt_ref: Identifier,
    pub request_digest: Digest,
    #[doc = "Exact rooted logical target accepted for this execution. Control and acknowledgement calls carry it back so Environment can reconcile its root-keyed target registry without a reverse index or scan."]
    pub target: SandboxTarget,
    #[doc = "Opaque physical target locator paired with generation. It never replaces the rooted logical target."]
    pub target_ref: Identifier,
}
#[doc = "`OperationState`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"accepted\","]
#[doc = "    \"running\","]
#[doc = "    \"terminal\","]
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
pub enum OperationState {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "unknown")]
    Unknown,
}
impl ::std::fmt::Display for OperationState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Accepted => f.write_str("accepted"),
            Self::Running => f.write_str("running"),
            Self::Terminal => f.write_str("terminal"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}
impl ::std::str::FromStr for OperationState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "running" => Ok(Self::Running),
            "terminal" => Ok(Self::Terminal),
            "unknown" => Ok(Self::Unknown),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OperationState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperationState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperationState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "One bounded output observation emitted by a Environment. Brain treats it as provisional until the terminal receipt is durably journaled."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One bounded output observation emitted by a Environment. Brain treats it as provisional until the terminal receipt is durably journaled.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"offset\","]
#[doc = "    \"stream\","]
#[doc = "    \"text\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"offset\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"stream\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"stdout\","]
#[doc = "        \"stderr\","]
#[doc = "        \"progress\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"text\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct OutputChunk {
    pub offset: u64,
    pub stream: OutputChunkStream,
    pub text: OutputChunkText,
}
#[doc = "`OutputChunkStream`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"stdout\","]
#[doc = "    \"stderr\","]
#[doc = "    \"progress\""]
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
pub enum OutputChunkStream {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "stderr")]
    Stderr,
    #[serde(rename = "progress")]
    Progress,
}
impl ::std::fmt::Display for OutputChunkStream {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Stdout => f.write_str("stdout"),
            Self::Stderr => f.write_str("stderr"),
            Self::Progress => f.write_str("progress"),
        }
    }
}
impl ::std::str::FromStr for OutputChunkStream {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "stdout" => Ok(Self::Stdout),
            "stderr" => Ok(Self::Stderr),
            "progress" => Ok(Self::Progress),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OutputChunkStream {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OutputChunkStream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OutputChunkStream {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`OutputChunkText`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct OutputChunkText(::std::string::String);
impl ::std::ops::Deref for OutputChunkText {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<OutputChunkText> for ::std::string::String {
    fn from(value: OutputChunkText) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for OutputChunkText {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for OutputChunkText {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OutputChunkText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OutputChunkText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for OutputChunkText {
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
#[doc = "`PrepareSessionRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"bindings\","]
#[doc = "    \"bundles\","]
#[doc = "    \"network\","]
#[doc = "    \"resources\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"bindings\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/PreparedBindingBundles\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"bundles\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/BundleFetch\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"$ref\": \"#/definitions/NetworkCeiling\""]
#[doc = "    },"]
#[doc = "    \"resources\": {"]
#[doc = "      \"$ref\": \"#/definitions/ResourceCeiling\""]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"secret_capability\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/SecretCapability\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PrepareSessionRequest {
    pub bindings: Vec<PreparedBindingBundles>,
    pub bundles: Vec<BundleFetch>,
    pub network: NetworkCeiling,
    pub resources: ResourceCeiling,
    pub root_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub secret_capability: ::std::option::Option<SecretCapability>,
    pub session_id: Identifier,
}
#[doc = "`PreparedBindingBundles`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"binding_ref\","]
#[doc = "    \"bundle_digests\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"bundle_digests\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/Digest\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PreparedBindingBundles {
    pub binding_ref: Identifier,
    pub bundle_digests: Vec<Digest>,
}
#[doc = "`PreparedSession`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"preparation_ref\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"preparation_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PreparedSession {
    pub preparation_ref: Identifier,
}
#[doc = "`RecoveryClass`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"retained\","]
#[doc = "    \"connection_scoped\""]
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
pub enum RecoveryClass {
    #[serde(rename = "retained")]
    Retained,
    #[serde(rename = "connection_scoped")]
    ConnectionScoped,
}
impl ::std::fmt::Display for RecoveryClass {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Retained => f.write_str("retained"),
            Self::ConnectionScoped => f.write_str("connection_scoped"),
        }
    }
}
impl ::std::str::FromStr for RecoveryClass {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "retained" => Ok(Self::Retained),
            "connection_scoped" => Ok(Self::ConnectionScoped),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RecoveryClass {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RecoveryClass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RecoveryClass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ResolvedBinding`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"binding_ref\","]
#[doc = "    \"capabilities\","]
#[doc = "    \"environment_id\","]
#[doc = "    \"limits\","]
#[doc = "    \"recovery\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"capabilities\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/EnvironmentCapability\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"environment_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"limits\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"max_inline_input_bytes\","]
#[doc = "        \"max_inline_result_bytes\","]
#[doc = "        \"max_wait_ms\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"max_inline_input_bytes\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"max_inline_result_bytes\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"max_wait_ms\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 0.0"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    \"recovery\": {"]
#[doc = "      \"$ref\": \"#/definitions/RecoveryClass\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ResolvedBinding {
    pub binding_ref: Identifier,
    pub capabilities: Vec<EnvironmentCapability>,
    pub environment_id: Identifier,
    pub limits: ResolvedBindingLimits,
    pub recovery: RecoveryClass,
}
#[doc = "`ResolvedBindingLimits`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"max_inline_input_bytes\","]
#[doc = "    \"max_inline_result_bytes\","]
#[doc = "    \"max_wait_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"max_inline_input_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"max_inline_result_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"max_wait_ms\": {"]
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
pub struct ResolvedBindingLimits {
    pub max_inline_input_bytes: ::std::num::NonZeroU64,
    pub max_inline_result_bytes: ::std::num::NonZeroU64,
    pub max_wait_ms: u64,
}
#[doc = "`ResourceCeiling`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"max_output_bytes\","]
#[doc = "    \"timeout_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"max_output_bytes\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"timeout_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ResourceCeiling {
    pub max_output_bytes: ::std::num::NonZeroU64,
    pub timeout_ms: ::std::num::NonZeroU64,
}
#[doc = "Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"direction\","]
#[doc = "    \"expected_generation\","]
#[doc = "    \"object\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"overwrite\","]
#[doc = "    \"path\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"target\","]
#[doc = "    \"transfer\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"direction\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"import\","]
#[doc = "        \"export\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"expected_generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/ObjectReference\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"overwrite\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    },"]
#[doc = "    \"transfer\": {"]
#[doc = "      \"$ref\": \"#/definitions/ObjectTransferAuthority\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SandboxCopyRequest {
    pub direction: SandboxCopyRequestDirection,
    pub expected_generation: Identifier,
    pub object: ::std::option::Option<ObjectReference>,
    pub operation_id: Identifier,
    pub overwrite: bool,
    pub path: SandboxCopyRequestPath,
    pub request_digest: Digest,
    pub target: SandboxTarget,
    pub transfer: ObjectTransferAuthority,
}
#[doc = "`SandboxCopyRequestDirection`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"import\","]
#[doc = "    \"export\""]
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
pub enum SandboxCopyRequestDirection {
    #[serde(rename = "import")]
    Import,
    #[serde(rename = "export")]
    Export,
}
impl ::std::fmt::Display for SandboxCopyRequestDirection {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Import => f.write_str("import"),
            Self::Export => f.write_str("export"),
        }
    }
}
impl ::std::str::FromStr for SandboxCopyRequestDirection {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "import" => Ok(Self::Import),
            "export" => Ok(Self::Export),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SandboxCopyRequestDirection {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxCopyRequestDirection {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxCopyRequestDirection {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`SandboxCopyRequestPath`"]
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
pub struct SandboxCopyRequestPath(::std::string::String);
impl ::std::ops::Deref for SandboxCopyRequestPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxCopyRequestPath> for ::std::string::String {
    fn from(value: SandboxCopyRequestPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxCopyRequestPath {
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
impl ::std::convert::TryFrom<&str> for SandboxCopyRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxCopyRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxCopyRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxCopyRequestPath {
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
#[doc = "Import returns object=null. Export returns the uploaded object identity so Brain can verify and durably publish it."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Import returns object=null. Export returns the uploaded object identity so Brain can verify and durably publish it.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"file\","]
#[doc = "    \"object\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"replayed\","]
#[doc = "    \"request_digest\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"file\": {"]
#[doc = "      \"$ref\": \"#/definitions/FileEntry\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/ObjectReference\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"replayed\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxCopyResult {
    pub file: FileEntry,
    pub object: ::std::option::Option<ObjectReference>,
    pub operation_id: Identifier,
    pub replayed: bool,
    pub request_digest: Digest,
}
#[doc = "Execute with /bin/bash -lc in the selected additional sandbox. Environment secrets are never accepted from model input; declared server-tool env is delivered through SecretCapability."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Execute with /bin/bash -lc in the selected additional sandbox. Environment secrets are never accepted from model input; declared server-tool env is delivered through SecretCapability.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"command\","]
#[doc = "    \"interactive\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"command\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 131072,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"cwd\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 4096"]
#[doc = "    },"]
#[doc = "    \"interactive\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxExecInput {
    pub command: SandboxExecInputCommand,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cwd: ::std::option::Option<SandboxExecInputCwd>,
    pub interactive: bool,
}
#[doc = "`SandboxExecInputCommand`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 131072,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SandboxExecInputCommand(::std::string::String);
impl ::std::ops::Deref for SandboxExecInputCommand {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxExecInputCommand> for ::std::string::String {
    fn from(value: SandboxExecInputCommand) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxExecInputCommand {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 131072usize {
            return Err("longer than 131072 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SandboxExecInputCommand {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxExecInputCommand {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxExecInputCommand {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxExecInputCommand {
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
#[doc = "`SandboxExecInputCwd`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SandboxExecInputCwd(::std::string::String);
impl ::std::ops::Deref for SandboxExecInputCwd {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxExecInputCwd> for ::std::string::String {
    fn from(value: SandboxExecInputCwd) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxExecInputCwd {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SandboxExecInputCwd {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxExecInputCwd {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxExecInputCwd {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxExecInputCwd {
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
#[doc = "`SandboxExecutionRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"execution_id\","]
#[doc = "    \"expected_generation\","]
#[doc = "    \"input\","]
#[doc = "    \"network\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"resources\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"execution_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"expected_generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"input\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxExecInput\""]
#[doc = "    },"]
#[doc = "    \"network\": {"]
#[doc = "      \"$ref\": \"#/definitions/NetworkCeiling\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"resources\": {"]
#[doc = "      \"$ref\": \"#/definitions/ResourceCeiling\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxExecutionRequest {
    pub execution_id: Identifier,
    pub expected_generation: Identifier,
    pub input: SandboxExecInput,
    pub network: NetworkCeiling,
    pub request_digest: Digest,
    pub resources: ResourceCeiling,
    pub target: SandboxTarget,
}
#[doc = "`SandboxFileRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"expected_generation\","]
#[doc = "    \"path\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"expected_generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxFileRequest {
    pub expected_generation: Identifier,
    pub path: SandboxFileRequestPath,
    pub target: SandboxTarget,
}
#[doc = "`SandboxFileRequestPath`"]
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
pub struct SandboxFileRequestPath(::std::string::String);
impl ::std::ops::Deref for SandboxFileRequestPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxFileRequestPath> for ::std::string::String {
    fn from(value: SandboxFileRequestPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxFileRequestPath {
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
impl ::std::convert::TryFrom<&str> for SandboxFileRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxFileRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxFileRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxFileRequestPath {
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
#[doc = "Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"expected_generation\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"overwrite\","]
#[doc = "    \"path\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"source\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"expected_generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"overwrite\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"path\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096,"]
#[doc = "      \"minLength\": 1"]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"source\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxFileWriteSource\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SandboxFileWriteRequest {
    pub expected_generation: Identifier,
    pub operation_id: Identifier,
    pub overwrite: bool,
    pub path: SandboxFileWriteRequestPath,
    pub request_digest: Digest,
    pub source: SandboxFileWriteSource,
    pub target: SandboxTarget,
}
#[doc = "`SandboxFileWriteRequestPath`"]
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
pub struct SandboxFileWriteRequestPath(::std::string::String);
impl ::std::ops::Deref for SandboxFileWriteRequestPath {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxFileWriteRequestPath> for ::std::string::String {
    fn from(value: SandboxFileWriteRequestPath) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxFileWriteRequestPath {
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
impl ::std::convert::TryFrom<&str> for SandboxFileWriteRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxFileWriteRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxFileWriteRequestPath {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxFileWriteRequestPath {
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
#[doc = "`SandboxFileWriteResult`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"file\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"replayed\","]
#[doc = "    \"request_digest\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"file\": {"]
#[doc = "      \"$ref\": \"#/definitions/FileEntry\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"replayed\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxFileWriteResult {
    pub file: FileEntry,
    pub operation_id: Identifier,
    pub replayed: bool,
    pub request_digest: Digest,
}
#[doc = "Inline content is standard padded base64 and capped at 1 MiB decoded. Larger writes carry an opaque object identity plus a one-purpose GET authority."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Inline content is standard padded base64 and capped at 1 MiB decoded. Larger writes carry an opaque object identity plus a one-purpose GET authority.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"content_base64\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"content_base64\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"maxLength\": 1398108"]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"const\": \"inline\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"fetch\","]
#[doc = "        \"kind\","]
#[doc = "        \"object\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"fetch\": {"]
#[doc = "          \"$ref\": \"#/definitions/ObjectTransferAuthority\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"const\": \"object\""]
#[doc = "        },"]
#[doc = "        \"object\": {"]
#[doc = "          \"$ref\": \"#/definitions/ObjectReference\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum SandboxFileWriteSource {
    #[serde(rename = "inline")]
    Inline {
        content_base64: SandboxFileWriteSourceContentBase64,
    },
    #[serde(rename = "object")]
    Object {
        fetch: ObjectTransferAuthority,
        object: ObjectReference,
    },
}
#[doc = "`SandboxFileWriteSourceContentBase64`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 1398108"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SandboxFileWriteSourceContentBase64(::std::string::String);
impl ::std::ops::Deref for SandboxFileWriteSourceContentBase64 {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxFileWriteSourceContentBase64> for ::std::string::String {
    fn from(value: SandboxFileWriteSourceContentBase64) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxFileWriteSourceContentBase64 {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 1398108usize {
            return Err("longer than 1398108 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SandboxFileWriteSourceContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxFileWriteSourceContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxFileWriteSourceContentBase64 {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxFileWriteSourceContentBase64 {
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
#[doc = "`SandboxState`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"never_materialized\","]
#[doc = "    \"creating\","]
#[doc = "    \"running\","]
#[doc = "    \"suspended\","]
#[doc = "    \"gone\","]
#[doc = "    \"terminated\""]
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
pub enum SandboxState {
    #[serde(rename = "never_materialized")]
    NeverMaterialized,
    #[serde(rename = "creating")]
    Creating,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "gone")]
    Gone,
    #[serde(rename = "terminated")]
    Terminated,
}
impl ::std::fmt::Display for SandboxState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NeverMaterialized => f.write_str("never_materialized"),
            Self::Creating => f.write_str("creating"),
            Self::Running => f.write_str("running"),
            Self::Suspended => f.write_str("suspended"),
            Self::Gone => f.write_str("gone"),
            Self::Terminated => f.write_str("terminated"),
        }
    }
}
impl ::std::str::FromStr for SandboxState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "never_materialized" => Ok(Self::NeverMaterialized),
            "creating" => Ok(Self::Creating),
            "running" => Ok(Self::Running),
            "suspended" => Ok(Self::Suspended),
            "gone" => Ok(Self::Gone),
            "terminated" => Ok(Self::Terminated),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SandboxState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`SandboxStatus`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"expires_at_ms\","]
#[doc = "    \"state\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"changed_at_ms\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"expires_at_ms\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"generation\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 128"]
#[doc = "    },"]
#[doc = "    \"reason\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"string\","]
#[doc = "        \"null\""]
#[doc = "      ],"]
#[doc = "      \"maxLength\": 512"]
#[doc = "    },"]
#[doc = "    \"state\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxState\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    },"]
#[doc = "    \"target_ref\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/Identifier\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxStatus {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub changed_at_ms: ::std::option::Option<u64>,
    pub expires_at_ms: ::std::option::Option<::std::num::NonZeroU64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub generation: ::std::option::Option<SandboxStatusGeneration>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub reason: ::std::option::Option<SandboxStatusReason>,
    pub state: SandboxState,
    pub target: SandboxTarget,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub target_ref: ::std::option::Option<Identifier>,
}
#[doc = "`SandboxStatusGeneration`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SandboxStatusGeneration(::std::string::String);
impl ::std::ops::Deref for SandboxStatusGeneration {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxStatusGeneration> for ::std::string::String {
    fn from(value: SandboxStatusGeneration) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxStatusGeneration {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SandboxStatusGeneration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxStatusGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxStatusGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxStatusGeneration {
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
#[doc = "`SandboxStatusReason`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 512"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SandboxStatusReason(::std::string::String);
impl ::std::ops::Deref for SandboxStatusReason {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SandboxStatusReason> for ::std::string::String {
    fn from(value: SandboxStatusReason) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for SandboxStatusReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 512usize {
            return Err("longer than 512 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for SandboxStatusReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SandboxStatusReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SandboxStatusReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for SandboxStatusReason {
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
#[doc = "`SandboxTarget`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"binding_ref\","]
#[doc = "    \"kind\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"kind\": {"]
#[doc = "      \"$ref\": \"#/definitions/TargetKind\""]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"sandbox_id\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/Identifier\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SandboxTarget {
    pub binding_ref: Identifier,
    pub kind: TargetKind,
    pub root_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub sandbox_id: ::std::option::Option<Identifier>,
    pub session_id: Identifier,
}
#[doc = "`SealedBinding`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"binding_id\","]
#[doc = "    \"capability\","]
#[doc = "    \"contract_digest\","]
#[doc = "    \"environment_name\","]
#[doc = "    \"implementation_identity\","]
#[doc = "    \"policy_digest\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"bundle\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/BundleDescriptor\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"capability\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"contract_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"environment_name\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"implementation_identity\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"policy_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"required_capabilities\": {"]
#[doc = "      \"default\": ["]
#[doc = "        \"execution\""]
#[doc = "      ],"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/EnvironmentCapability\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SealedBinding {
    pub binding_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub bundle: ::std::option::Option<BundleDescriptor>,
    pub capability: Identifier,
    pub contract_digest: Digest,
    pub environment_name: Identifier,
    pub implementation_identity: Digest,
    pub policy_digest: Digest,
    #[serde(default = "defaults::sealed_binding_required_capabilities")]
    pub required_capabilities: Vec<EnvironmentCapability>,
    pub root_id: Identifier,
    pub session_id: Identifier,
}
#[doc = "Opaque, short-lived, one-redemption authority for one session and one physical target generation. The Environment may keep redeemed values only in supervisor memory and inject each binding's declared subset at child spawn. Brain may mint a replacement capability for the same surviving generation after a Environment control-process crash. Secret values never enter this contract, binding registry, journal, receipt or argv."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Opaque, short-lived, one-redemption authority for one session and one physical target generation. The Environment may keep redeemed values only in supervisor memory and inject each binding's declared subset at child spawn. Brain may mint a replacement capability for the same surviving generation after a Environment control-process crash. Secret values never enter this contract, binding registry, journal, receipt or argv.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"capability_ref\","]
#[doc = "    \"env_names\","]
#[doc = "    \"expires_at_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"capability_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"env_names\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/Identifier\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 128,"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"expires_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SecretCapability {
    pub capability_ref: Identifier,
    pub env_names: Vec<Identifier>,
    pub expires_at_ms: ::std::num::NonZeroU64,
}
#[doc = "`SecretDeliveryRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"capability_ref\","]
#[doc = "    \"environment_id\","]
#[doc = "    \"generation_intent\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"capability_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"environment_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"generation_intent\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"root_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SecretDeliveryRequest {
    pub capability_ref: Identifier,
    pub environment_id: Identifier,
    pub generation_intent: Identifier,
    pub root_id: Identifier,
    pub session_id: Identifier,
    pub target: SandboxTarget,
}
#[doc = "`SubmitReceipt`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"observation\","]
#[doc = "    \"operation\","]
#[doc = "    \"replayed\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"observation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationObservation\""]
#[doc = "    },"]
#[doc = "    \"operation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationRef\""]
#[doc = "    },"]
#[doc = "    \"replayed\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SubmitReceipt {
    pub observation: OperationObservation,
    pub operation: OperationRef,
    pub replayed: bool,
}
#[doc = "`SubmitRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"envelope\","]
#[doc = "    \"wait_up_to_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"envelope\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationEnvelope\""]
#[doc = "    },"]
#[doc = "    \"wait_up_to_ms\": {"]
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
pub struct SubmitRequest {
    pub envelope: OperationEnvelope,
    pub wait_up_to_ms: u64,
}
#[doc = "`TargetKind`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"default\","]
#[doc = "    \"additional\""]
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
pub enum TargetKind {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "additional")]
    Additional,
}
impl ::std::fmt::Display for TargetKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Default => f.write_str("default"),
            Self::Additional => f.write_str("additional"),
        }
    }
}
impl ::std::str::FromStr for TargetKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "default" => Ok(Self::Default),
            "additional" => Ok(Self::Additional),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TargetKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TargetKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TargetKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Environment-issued continuity locator for a materialized target. Brain journals and projects the newest receipt, then supplies target_ref and generation on later operations."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Environment-issued continuity locator for a materialized target. Brain journals and projects the newest receipt, then supplies target_ref and generation on later operations.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"expires_at_ms\","]
#[doc = "    \"generation\","]
#[doc = "    \"target_ref\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"expires_at_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"target_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TargetReceipt {
    pub expires_at_ms: ::std::num::NonZeroU64,
    pub generation: Identifier,
    pub target_ref: Identifier,
}
#[doc = "`TerminalOutcome`"]
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
pub enum TerminalOutcome {
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
impl ::std::fmt::Display for TerminalOutcome {
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
impl ::std::str::FromStr for TerminalOutcome {
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
impl ::std::convert::TryFrom<&str> for TerminalOutcome {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TerminalOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TerminalOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`TerminalResult`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"is_error\","]
#[doc = "    \"outcome\","]
#[doc = "    \"terminal_digest\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"duration_ms\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"exit_code\": {"]
#[doc = "      \"type\": ["]
#[doc = "        \"integer\","]
#[doc = "        \"null\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"inline\": {"]
#[doc = "      \"description\": \"Inline JSON result. Its RFC 8785 encoding must be at most 94208 bytes; larger data is returned by object/storage key/path.\""]
#[doc = "    },"]
#[doc = "    \"is_error\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"object\": {"]
#[doc = "      \"anyOf\": ["]
#[doc = "        {"]
#[doc = "          \"$ref\": \"#/definitions/ObjectReference\""]
#[doc = "        },"]
#[doc = "        {"]
#[doc = "          \"type\": \"null\""]
#[doc = "        }"]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"outcome\": {"]
#[doc = "      \"$ref\": \"#/definitions/TerminalOutcome\""]
#[doc = "    },"]
#[doc = "    \"terminal_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TerminalResult {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub duration_ms: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub exit_code: ::std::option::Option<i64>,
    #[doc = "Inline JSON result. Its RFC 8785 encoding must be at most 94208 bytes; larger data is returned by object/storage key/path."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub inline: ::std::option::Option<::serde_json::Value>,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub object: ::std::option::Option<ObjectReference>,
    pub outcome: TerminalOutcome,
    pub terminal_digest: Digest,
}
#[doc = "Exact stdin-effect receipt plus the current bounded observation of the referenced interactive execution. Poll requests return accepted=false and still provide the observation."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Exact stdin-effect receipt plus the current bounded observation of the referenced interactive execution. Poll requests return accepted=false and still provide the observation.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"accepted\","]
#[doc = "    \"observation\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"replayed\","]
#[doc = "    \"request_digest\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"accepted\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"observation\": {"]
#[doc = "      \"$ref\": \"#/definitions/OperationObservation\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"replayed\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WriteStdinReceipt {
    pub accepted: bool,
    pub observation: OperationObservation,
    pub operation_id: Identifier,
    pub replayed: bool,
    pub request_digest: Digest,
}
#[doc = "One idempotent stdin append/EOF/poll. Empty text with eof=false is a pure poll. UTF-8 payload bytes are additionally capped at 4096 so the Environment can perform one PIPE_BUF-bounded write; larger input must be split into separately identified requests."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One idempotent stdin append/EOF/poll. Empty text with eof=false is a pure poll. UTF-8 payload bytes are additionally capped at 4096 so the Environment can perform one PIPE_BUF-bounded write; larger input must be split into separately identified requests.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"eof\","]
#[doc = "    \"execution_id\","]
#[doc = "    \"expected_generation\","]
#[doc = "    \"operation_id\","]
#[doc = "    \"request_digest\","]
#[doc = "    \"target\","]
#[doc = "    \"text\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"eof\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"execution_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"expected_generation\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"operation_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"$ref\": \"#/definitions/SandboxTarget\""]
#[doc = "    },"]
#[doc = "    \"text\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 4096"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct WriteStdinRequest {
    pub eof: bool,
    pub execution_id: Identifier,
    pub expected_generation: Identifier,
    pub operation_id: Identifier,
    pub request_digest: Digest,
    pub target: SandboxTarget,
    pub text: WriteStdinRequestText,
}
#[doc = "`WriteStdinRequestText`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 4096"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct WriteStdinRequestText(::std::string::String);
impl ::std::ops::Deref for WriteStdinRequestText {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<WriteStdinRequestText> for ::std::string::String {
    fn from(value: WriteStdinRequestText) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for WriteStdinRequestText {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 4096usize {
            return Err("longer than 4096 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for WriteStdinRequestText {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WriteStdinRequestText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WriteStdinRequestText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for WriteStdinRequestText {
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
#[doc = r" Generation of default values for serde."]
pub mod defaults {
    pub(super) fn sealed_binding_required_capabilities() -> Vec<super::EnvironmentCapability> {
        vec![super::EnvironmentCapability::Execution]
    }
}
